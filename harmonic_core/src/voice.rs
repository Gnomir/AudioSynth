// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 Mironov Mykhailo Viktorovych.
// A commercial licence without the AGPL obligations is available —
// see LICENSE-COMMERCIAL.md at the repo root.

//! A single synthesiser voice: band-limited oscillator, FM + feedback, a
//! per-voice LFO, the character stage, the SVF, pitch bend, equal-power pan,
//! and a per-sample de-click ramp. Stereo out.
//!
//! `#[repr(C)]` and fixed-size — the C side owns the storage, this crate never
//! allocates. See [`crate::ffi`].

use crate::character::{CharParams, Character};
use crate::filter::{FilterMode, Svf};
use crate::kernel::{
    geometric_hump_peak, geometric_hump_pre, geometric_partials_pre, geometric_partials_pre_frac,
    geometric_peak_pre, powi_pos,
};
use crate::lfo::{Lfo, LfoMode, LfoShape};
use crate::trig::{exp2, floor_f64, sin_cos_turns_fast, sin_turns, sin_turns_fast, wrap01};
use crate::{validate_sample_rate, SampleRateStatus};

/// Hard ceiling on partial count (error budget + `r^n` loop length).
pub const MAX_PARTIALS: u32 = 2048;

/// Length of the note-on de-click fade, in samples.
const DECLICK_LEN: u16 = 16;

/// Oscillator waveform.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Waveform {
    /// `Σ rᵏ·cos(2πkp)` — the closed-form additive core (impulse-train → its
    /// geometric-rolloff "darker" versions). The default.
    Geometric = 0,
    /// Band-limited sawtooth — naïve ramp + PolyBLEP step correction.
    Saw = 1,
    /// Band-limited triangle — naïve triangle + PolyBLAMP corner correction.
    Triangle = 2,
}

impl Waveform {
    /// For the C ABI. Unknown values fall back to `Geometric`.
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Waveform::Saw,
            2 => Waveform::Triangle,
            _ => Waveform::Geometric,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Voice {
    sample_rate: f64,

    phase: f64,        // carrier phase, turns [0,1)
    freq: f64,         // target Hz
    freq_z: f64,       // smoothed Hz
    rolloff: f64,      // target r
    rolloff_z: f64,    // smoothed r
    gain: f64,
    smooth_coeff: f64, // one-pole coeff for freq / rolloff / bend

    bend: f64,   // pitch-bend ratio, target (1.0 = no bend)
    bend_z: f64, // smoothed

    pan: f64,        // −1..1, target
    pan_z: f64,      // smoothed
    pan_smooth: f64, // one-pole coeff, ~10 ms
    // cached equal-power gains for the last `pan_z` (it stops changing once
    // smoothing converges, so this hits every sample of a steady pan)
    pan_cache_z: f64,
    pan_sin: f64,
    pan_cos: f64,

    free_running: bool, // true = phase survives note-on (analog-style)
    declick: u16,       // samples left in the note-on fade

    // FM
    fm_phase: f64,
    fm_ratio: f64,
    fm_index: f64,
    feedback: f64,
    last_osc: f64,

    // per-voice LFO
    lfo: Lfo,
    lfo_to_rolloff: f64, // added to r, ±
    lfo_to_pitch: f64,   // cents of vibrato
    lfo_to_cutoff: f64,  // octaves on the filter cutoff, ±
    lfo_to_fm: f64,      // added to the FM index, ±
    filter_cutoff: f64,  // last cutoff set by the host — the base for lfo_to_cutoff

    // per-voice slow free-running drift, for a "breathing" unison image: a
    // very-low-frequency sine added to the oscillator phase. Never retriggered
    // — its whole job is to keep stacked voices from locking together.
    drift_phase: f64,
    drift_inc: f64,   // turns per sample
    drift_depth: f64, // peak phase excursion, turns

    hq: bool, // 2× oversample the oscillator + character stage

    // cached geometric normalisation: `r^{n+1}` and the peak for the last
    // `(roll_eff, n)`. Both `powi_pos` calls are skipped while a note holds a
    // steady brightness and pitch.
    geom_r: f64,
    geom_n: u32,
    geom_rn1: f64,
    geom_peak: f64,

    // Alternative waveform (Saw / Triangle are stateless PolyBLEP / PolyBLAMP).
    waveform: Waveform,

    // User ceiling on the partial count — the "Partials" control. The integer
    // part caps the count at `min(partial_limit, ⌊fs / 2f_eff⌋, MAX_PARTIALS)`;
    // the fractional part fades the *next* partial in continuously, so the
    // control is smooth across its whole 1.0..2048.0 range, not stepped. Flat
    // cost (the closed form is O(log n) regardless), never aliases (the count
    // is always ≤ the Nyquist clamp, and the fade is suppressed when Nyquist
    // is the binding constraint). Default `2048.0` = no effect. Ignored by
    // Saw / Triangle (they are PolyBLEP, not the additive sum).
    partial_limit: u32,
    partial_frac: f32,

    // Per-note brightness expression (MPE timbre / CC74, poly & channel
    // pressure). Added to the smoothed `rolloff` before the oscillator, on top
    // of any LFO brightness routing, and clamped to the same range. Set per
    // note by the host from `NoteEvent::PolyBrightness` / `PolyPressure` /
    // `MidiChannelPressure`. `expr_bright` is the target; `expr_bright_z` is the
    // one-pole-smoothed value actually applied (MPE slides are dense and can
    // step hard). `0.0` — the default — is bit-identical to no expression: the
    // `roll_eff` term below then reduces to `self.rolloff_z` verbatim.
    expr_bright: f64,
    expr_bright_z: f64,

    // "Formant" — a second closed-form spectral term (`geometric_hump_pre`)
    // adding a controllable resonant bump to the mid-partials. `formant` is the
    // target `[0, 1]`; `formant_z` is one-pole smoothed. `0.0` (default) = the
    // hump is disabled and the oscillator path is bit-identical. `hump_*` is the
    // derived `(a, b, h, aⁿ⁺¹, bⁿ⁺¹, peak)` for the current `(formant_z, n)`,
    // recomputed only when either changes (the hump path is opt-in, not the
    // clean fast path, so a plain cache is enough).
    formant: f64,
    formant_z: f64,
    hump_f: f64,
    hump_n: u32,
    hump: Hump,

    character: Character,
    filter: Svf,
}

/// Derived resonant-hump parameters for [`Voice::geom_osc`]. `h == 0` means the
/// hump is off and the oscillator takes its plain, bit-identical path.
#[derive(Clone, Copy)]
struct Hump {
    h: f64,   // depth
    a: f64,   // slow rolloff of the difference term (peak side)
    b: f64,   // fast rolloff
    an1: f64, // a^{n+1}
    bn1: f64, // b^{n+1}
    peak: f64, // Σaᵏ − Σbᵏ, for renormalisation
}

impl Hump {
    const OFF: Hump = Hump { h: 0.0, a: 0.0, b: 0.0, an1: 0.0, bn1: 0.0, peak: 0.0 };
}

/// Output of [`Voice::tick_modulation`] — the effective, per-sample
/// pitch/brightness/partial-count/feedback/FM terms shared by both render
/// paths, computed once from the (possibly LFO-modulated) parameters.
struct Modulation {
    roll_eff: f64,
    n: u32,
    frac: f64, // fractional weight of the (n+1)-th partial, 0 unless the "Partials" ceiling binds
    fb: f64,
    step: f64,
    drift: f64,
    fm_index: f64,
    fm_term: f64,
    m: f64, // raw LFO output (0 if nothing is routed) — for lfo_to_cutoff
    hump: Hump,
}

impl Voice {
    pub const ROLLOFF_MIN: f64 = 1.0e-3;
    pub const ROLLOFF_MAX: f64 = 0.9995;

    /// Latency, in samples, added by [`Voice::set_hq`] `true` (the character
    /// decimation FIR) when a `Voice` is driven standalone. Zero when HQ is
    /// off. **Not what the plugin reports**: driven through [`crate::poly::PolySynth`]
    /// (the real signal path — every shipped plugin build), the unified HQ
    /// bus's own master decimator supersedes this per-voice figure, at
    /// [`crate::poly::PolySynth::HQ_LATENCY`] (16, not this constant's 3).
    /// The two exist for different callers and are independently asserted
    /// equal to their own values in `poly.rs`'s tests — not a stray
    /// duplicate to consolidate.
    pub const HQ_LATENCY: usize = crate::character::HQ_LATENCY;

    /// Create a voice. Non-finite / out-of-range `sample_rate` is clamped —
    /// see [`Voice::new_checked`] to also learn what happened.
    pub fn new(sample_rate: f64) -> Self {
        Self::new_checked(sample_rate).0
    }

    /// Like [`Voice::new`], and also reports whether `sample_rate` was accepted
    /// as given, clamped to `[8000, 768000]`, or (if non-finite) defaulted.
    pub fn new_checked(sample_rate: f64) -> (Self, SampleRateStatus) {
        let (sr, status) = validate_sample_rate(sample_rate);
        let dt = 1.0 / sr;
        let smooth_coeff = 1.0 - dt / (0.005 + dt); // ~5 ms
        let pan_smooth = 1.0 - dt / (0.010 + dt); // ~10 ms

        let v = Voice {
            sample_rate: sr,
            phase: 0.0,
            freq: 110.0,
            freq_z: 110.0,
            rolloff: 0.9,
            rolloff_z: 0.9,
            gain: 0.5,
            smooth_coeff,
            bend: 1.0,
            bend_z: 1.0,
            pan: 0.0,
            pan_z: 0.0,
            pan_smooth,
            pan_cache_z: f64::NAN, // force a compute on the first sample
            pan_sin: 0.0,
            pan_cos: 0.0,
            free_running: false,
            declick: 0,
            fm_phase: 0.0,
            fm_ratio: 1.0,
            fm_index: 0.0,
            feedback: 0.0,
            last_osc: 0.0,
            lfo: Lfo::new(),
            lfo_to_rolloff: 0.0,
            lfo_to_pitch: 0.0,
            lfo_to_cutoff: 0.0,
            lfo_to_fm: 0.0,
            filter_cutoff: 0.4 * sr, // matches Svf::new's default target
            drift_phase: 0.0,
            drift_inc: 0.0,
            drift_depth: 0.0,
            hq: false,
            geom_r: f64::NAN, // force a compute on the first sample
            geom_n: 0,
            geom_rn1: 0.0,
            geom_peak: 1.0,
            waveform: Waveform::Geometric,
            partial_limit: MAX_PARTIALS,
            partial_frac: 0.0,
            expr_bright: 0.0,
            expr_bright_z: 0.0,
            formant: 0.0,
            formant_z: 0.0,
            hump_f: 0.0,
            hump_n: 0,
            hump: Hump::OFF,
            character: Character::new(),
            filter: Svf::new(sr),
        };
        (v, status)
    }

    /// The (validated) sample rate this voice runs at.
    #[inline]
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    // ---- pitch ----

    /// Fundamental in Hz, clamped to `[1, fs/2)`.
    #[inline]
    pub fn set_frequency(&mut self, hz: f64) {
        self.freq = clamp(hz, 1.0, 0.5 * self.sample_rate - 1.0);
    }

    /// Pitch-bend ratio (`2^(semitones/12)`). Smoothed.
    #[inline]
    pub fn set_pitch_bend(&mut self, ratio: f64) {
        self.bend = clamp(ratio, 0.03125, 32.0);
    }

    /// Force the carrier phase (turns). Used to spread unison voices.
    #[inline]
    pub fn set_start_phase(&mut self, turns: f64) {
        self.phase = wrap01(turns);
    }

    // ---- timbre ----

    #[inline]
    pub fn set_rolloff(&mut self, r: f64) {
        self.rolloff = clamp(r, Self::ROLLOFF_MIN, Self::ROLLOFF_MAX);
    }

    #[inline]
    pub fn set_gain(&mut self, g: f64) {
        self.gain = clamp(g, 0.0, 8.0);
    }

    #[inline]
    pub fn set_pan(&mut self, pan: f64) {
        self.pan = clamp(pan, -1.0, 1.0);
    }

    /// `true` = carrier & FM phase are *not* reset on note-on (they keep
    /// running, classic analog); `false` = reset + a 16-sample de-click fade.
    #[inline]
    pub fn set_free_running(&mut self, free: bool) {
        self.free_running = free;
    }

    // ---- FM ----

    #[inline]
    pub fn set_fm(&mut self, ratio: f64, index: f64) {
        self.fm_ratio = clamp(ratio, 0.0, 64.0);
        self.fm_index = clamp(index, 0.0, 8.0);
    }

    #[inline]
    pub fn set_feedback(&mut self, fb: f64) {
        self.feedback = clamp(fb, 0.0, 0.9);
    }

    // ---- LFO ----

    #[inline]
    pub fn set_lfo(&mut self, rate_hz: f64, shape: LfoShape) {
        self.lfo.set_rate(rate_hz, self.sample_rate);
        self.lfo.set_shape(shape);
    }

    /// Key-sync mode: `Retrigger` (phase snaps to 0 on note-on) or `FreeRun`
    /// (phase runs across notes).
    #[inline]
    pub fn set_lfo_mode(&mut self, mode: LfoMode) {
        self.lfo.set_mode(mode);
    }

    /// LFO routing. `to_rolloff` adds to `r` (± brightness, `[−0.9, 0.9]`),
    /// `to_pitch_cents` is vibrato depth (`[−1200, 1200]`), `to_cutoff_oct`
    /// shifts the filter cutoff (`[−8, 8]` octaves), `to_fm` adds to the FM
    /// index (`[−8, 8]`). A target of `0` is not applied at all.
    #[inline]
    pub fn set_lfo_targets(
        &mut self,
        to_rolloff: f64,
        to_pitch_cents: f64,
        to_cutoff_oct: f64,
        to_fm: f64,
    ) {
        self.lfo_to_rolloff = clamp(to_rolloff, -0.9, 0.9);
        self.lfo_to_pitch = clamp(to_pitch_cents, -1200.0, 1200.0);
        self.lfo_to_cutoff = clamp(to_cutoff_oct, -8.0, 8.0);
        self.lfo_to_fm = clamp(to_fm, -8.0, 8.0);
    }

    #[inline]
    pub fn set_lfo_phase(&mut self, turns: f64) {
        self.lfo.set_phase(turns);
    }

    // ---- unison drift ----

    /// Slow free-running phase drift, for a "breathing" unison stack.
    /// `rate_hz` clamped to `[0, 5]`; `depth_turns` (peak phase excursion)
    /// clamped to `[0, 0.25]`. Both `0` disables it (no cost per sample).
    /// Give each stacked voice a different [`Voice::set_unison_drift_phase`]
    /// (and ideally a slightly different rate) so they decorrelate.
    #[inline]
    pub fn set_unison_drift(&mut self, rate_hz: f64, depth_turns: f64) {
        let r = clamp(rate_hz, 0.0, 5.0);
        self.drift_inc = r / self.sample_rate;
        self.drift_depth = clamp(depth_turns, 0.0, 0.25);
    }

    #[inline]
    pub fn set_unison_drift_phase(&mut self, turns: f64) {
        self.drift_phase = wrap01(turns);
    }

    // ---- character / filter ----

    #[inline]
    pub fn set_character(&mut self, p: CharParams) {
        self.character.set_params(p);
    }

    /// HQ mode: 2×-oversample the oscillator + character stage and decimate,
    /// so the nonlinear stages do not alias. Adds [`Voice::HQ_LATENCY`]
    /// samples of latency; `false` is bit-identical to the previous behaviour.
    /// Only applies to [`Waveform::Geometric`] (`Saw` / `Triangle` are
    /// PolyBLEP / PolyBLAMP, already band-limited).
    #[inline]
    pub fn set_hq(&mut self, hq: bool) {
        self.hq = hq;
    }

    /// Reconfigure the filter for [`Voice::render_hq_subsamples`] — `PolySynth`'s
    /// unified HQ bus, which runs the whole oversampled tract (oscillator →
    /// `Character` → `Svf`) at `2×` the base rate and decimates once for the
    /// whole mix rather than once per voice. Independent of [`Voice::set_hq`]:
    /// that flag governs `render_sample`'s own, unrelated per-voice HQ path
    /// (still filters once, at `1×`), which stays unaffected — so a standalone
    /// `Voice` / the C ABI keeps today's HQ behaviour exactly. Only `PolySynth`
    /// should call this (`pub(crate)` — it is not part of the public API).
    #[inline]
    pub(crate) fn set_hq_bus_active(&mut self, active: bool) {
        let sr = if active { 2.0 * self.sample_rate } else { self.sample_rate };
        self.filter.set_sample_rate(sr);
    }

    /// Oscillator waveform. `Saw` / `Triangle` are PolyBLEP / PolyBLAMP —
    /// fixed `1/k` / `1/k²` spectra, so they ignore the rolloff/brightness
    /// control and the HQ path.
    #[inline]
    pub fn set_waveform(&mut self, w: Waveform) {
        self.waveform = w;
    }

    /// Upper bound on the number of partials in the geometric oscillator,
    /// **fractional**, clamped to `[1.0, MAX_PARTIALS as f32]`. The integer
    /// part is the hard cap; the fractional part fades the next partial in
    /// continuously, so the control is smooth across its whole range, not
    /// stepped. The effective count is still capped at the Nyquist limit
    /// `⌊fs / 2f_eff⌋`, so lowering this only ever darkens the tone — never
    /// aliases (and the fractional fade is dropped when Nyquist binds). Cost
    /// is flat in the partial count (the closed form is `O(log n)` either
    /// way), so this is a free brightness axis distinct from `rolloff`:
    /// `rolloff` tilts the spectrum, this truncates it. Default
    /// `MAX_PARTIALS as f32` = no effect. `Saw` / `Triangle` ignore it.
    #[inline]
    pub fn set_partial_limit(&mut self, limit: f32) {
        // NaN → the default (no limit). `f32::clamp` would return NaN here
        // (it only rejects NaN *bounds*), which then poisons `partial_frac`.
        let limit = if limit.is_nan() {
            MAX_PARTIALS as f32
        } else {
            limit.clamp(1.0, MAX_PARTIALS as f32)
        };
        self.partial_limit = limit as u32; // floor (limit ≥ 1.0)
        self.partial_frac = limit - self.partial_limit as f32;
    }

    /// Per-note brightness expression offset — added to the smoothed `rolloff`
    /// before the oscillator, on top of any LFO brightness routing, then the
    /// sum is clamped to `[ROLLOFF_MIN, ROLLOFF_MAX]`. `r_offset` itself is
    /// clamped to `[−0.9, 0.9]` (the same bound as `lfo_to_rolloff`) and
    /// one-pole smoothed. `0.0` — the default — is bit-identical to no
    /// expression. The host sets this per sounding note from MPE timbre /
    /// CC74 and poly/channel pressure (see [`crate::PolySynth::set_note_brightness`]).
    /// `Saw` / `Triangle` ignore it (fixed spectra).
    #[inline]
    pub fn set_expr_brightness(&mut self, r_offset: f64) {
        self.expr_bright = clamp(r_offset, -0.9, 0.9);
    }

    /// "Formant" — a resonant bump added to the mid-partials via a second
    /// closed-form term ([`crate::kernel::geometric_hump_pre`]). `[0, 1]`;
    /// `0.0` (the default) disables it and the oscillator path is bit-identical.
    /// Higher values raise the bump's centre partial (~1.5 … ~25.5, with
    /// `formant²`) and its depth. Still `Θ(log n)`. One-pole smoothed. Ignored
    /// by `Saw` / `Triangle`.
    #[inline]
    pub fn set_formant(&mut self, f: f64) {
        self.formant = clamp(f, 0.0, 1.0);
    }

    #[inline]
    pub fn set_filter_mode(&mut self, mode: FilterMode) {
        self.filter.set_mode(mode);
    }

    #[inline]
    pub fn set_filter_cutoff(&mut self, hz: f64) {
        self.filter_cutoff = hz; // remembered as the base for `lfo_to_cutoff`
        self.filter.set_cutoff(hz);
    }

    #[inline]
    pub fn set_filter_resonance(&mut self, r: f64) {
        self.filter.set_resonance(r);
    }

    // ---- lifecycle ----

    /// Re-arm for a note. In free-running mode the phase is left alone; in
    /// reset mode the phase snaps to 0 and a short fade masks the edge.
    #[inline]
    pub fn reset(&mut self) {
        if !self.free_running {
            self.phase = 0.0;
            self.fm_phase = 0.0;
            self.last_osc = 0.0;
            self.declick = DECLICK_LEN;
        }
        self.freq_z = self.freq;
        self.rolloff_z = self.rolloff;
        self.expr_bright_z = self.expr_bright;
        self.formant_z = self.formant;
        self.bend_z = self.bend;
        self.pan_z = self.pan;
        self.character.reset();
        self.filter.reset();
        self.lfo.retrigger();
    }

    /// The geometric oscillator sum at phase `ph`, normalised by `peak`. Uses
    /// the integer closed form when `frac == 0` (the common case: default
    /// setting, or the Nyquist clamp binding) so that path stays bit-identical;
    /// otherwise fades the `(n+1)`-th partial in at weight `frac`. When
    /// `hump.h != 0` a second closed-form term (`geometric_hump_pre`) adds a
    /// resonant mid-spectrum bump, and the whole thing is renormalised by the
    /// combined peak. `hump.h == 0` (the default) takes the plain `… / peak`
    /// path unchanged — bit-identical.
    #[inline]
    fn geom_osc(ph: f64, r: f64, n: u32, rn1: f64, frac: f64, peak: f64, hump: &Hump) -> f64 {
        let base = if frac != 0.0 {
            geometric_partials_pre_frac(ph, r, n, rn1, frac)
        } else {
            geometric_partials_pre(ph, r, n, rn1)
        };
        if hump.h == 0.0 {
            base / peak
        } else {
            (base + hump.h * geometric_hump_pre(ph, hump.a, hump.b, n, hump.an1, hump.bn1))
                / (peak + hump.h * hump.peak)
        }
    }

    /// One band-limited sawtooth sample via **PolyBLEP** (Välimäki & Huovilainen
    /// 2007): the naïve ramp `2p−1` with a 2-sample polynomial step correction
    /// at the wrap. Stateless, no trig, flat to DC, unconditionally stable.
    /// `dt` is the phase increment per sample (`f_eff / fs`). Output ≈ `[−1, 1]`.
    #[inline]
    fn polyblep_saw(phase: f64, dt: f64) -> f64 {
        let p = phase - floor_f64(phase); // → [0, 1)
        (2.0 * p - 1.0) - poly_blep(p, dt)
    }

    /// One band-limited triangle sample via **PolyBLAMP** (the integral of the
    /// PolyBLEP step): the naïve triangle with a cubic corner correction at each
    /// of its two slope discontinuities. Stateless, no integrators, no DC
    /// blockers — flat within ~0.1 dB down to 20 Hz. Output ≈ `[−1, 1]`.
    #[inline]
    fn polyblamp_triangle(phase: f64, dt: f64) -> f64 {
        let p = phase - floor_f64(phase);
        // naïve triangle in [−1, 1]: +1 at phase 0, −1 at ½, slope ±4 per turn
        let mut y = 1.0 - 4.0 * fabs(p - 0.5);
        // corner at p = 0 (a minimum → slope steps by +8·dt/sample)
        y += 4.0 * dt * poly_blamp(p, dt);
        // corner at p = ½ (a maximum → slope steps by −8·dt/sample)
        let ph = if p < 0.5 { p + 0.5 } else { p - 0.5 };
        y -= 4.0 * dt * poly_blamp(ph, dt);
        y
    }

    /// `(r^{n+1}, peak)` for the geometric oscillator, keyed on `(r, n)`. The
    /// two `powi_pos` evaluations run only when brightness or the Nyquist
    /// partial count actually changes — a steady note pays two `f64` compares.
    /// Bit-identical to `geometric_peak(r, n)` / the `powi_pos(r, n+1)` inside
    /// `geometric_partials`.
    #[inline]
    fn geom_norm(&mut self, r: f64, n: u32) -> (f64, f64) {
        if r != self.geom_r || n != self.geom_n {
            self.geom_r = r;
            self.geom_n = n;
            self.geom_rn1 = powi_pos(r, n + 1);
            self.geom_peak = geometric_peak_pre(r, n, powi_pos(r, n));
        }
        (self.geom_rn1, self.geom_peak)
    }

    /// Derived [`Hump`] for `(formant, n)`, cached. `formant == 0` → [`Hump::OFF`]
    /// (oscillator path stays bit-identical). Otherwise the bump's centre partial
    /// rises with `formant²` (~1.5 … ~25.5) and its depth linearly; `a`/`b` are
    /// two rolloffs with a fixed decay ratio (`b = a³` in log terms), so their
    /// difference `aᵏ − bᵏ` peaks near that centre.
    #[inline]
    fn hump_for(&mut self, formant: f64, n: u32) -> Hump {
        if formant == 0.0 {
            return Hump::OFF;
        }
        if formant != self.hump_f || n != self.hump_n {
            self.hump_f = formant;
            self.hump_n = n;

            let kc = 1.5 + 24.0 * formant * formant;
            // a = 2^{-1/kc}, b = a² = 2^{-2/kc}. Then bᵏ = a^{2k}, so the bump
            // weight is aᵏ(1 − aᵏ), which peaks (value 0.25) exactly where
            // aᵏ = ½, i.e. at partial k = kc.
            let a = clamp(exp2(-1.0 / kc), 0.05, 0.9995);
            let b = clamp(a * a, 0.02, a - 1.0e-4);
            let an = powi_pos(a, n);
            let bn = powi_pos(b, n);
            self.hump = Hump {
                h: 1.3 * formant,
                a,
                b,
                an1: an * a,
                bn1: bn * b,
                peak: geometric_hump_peak(a, b, n, an, bn),
            };
        }
        self.hump
    }

    /// The partial count in effect right now: the Nyquist clamp
    /// `⌊fs / 2f⌋`, further capped by [`Voice::set_partial_limit`].
    #[inline]
    pub fn max_partials(&self) -> u32 {
        let f = if self.freq_z > 1.0 { self.freq_z } else { 1.0 };
        ((self.sample_rate / (2.0 * f)) as u32)
            .max(1)
            .min(MAX_PARTIALS)
            .min(self.partial_limit)
    }

    #[inline]
    pub fn current_frequency(&self) -> f64 {
        self.freq_z * self.bend_z
    }

    /// The filter's current smoothed cutoff in Hz, with the filter envelope and
    /// LFO→cutoff already folded in (they are pushed onto the `Svf` per sample
    /// from `PolySynth` / here). For the editor's live filter-response curve.
    #[inline]
    pub fn current_cutoff(&self) -> f64 {
        self.filter.cutoff()
    }

    /// The effective geometric rolloff `r` right now — smoothed base plus
    /// LFO→brightness plus per-note expression, exactly as [`Self::tick_modulation`]
    /// computes it. For the editor's live partial comb; not on the render path.
    #[inline]
    pub fn current_rolloff(&self) -> f64 {
        if self.lfo_to_rolloff != 0.0 || self.expr_bright_z != 0.0 {
            clamp(
                self.rolloff_z + self.lfo_to_rolloff * self.lfo.value() as f64 + self.expr_bright_z,
                Self::ROLLOFF_MIN,
                Self::ROLLOFF_MAX,
            )
        } else {
            self.rolloff_z
        }
    }

    /// Render one stereo sample `[left, right]`. No allocation, no locks, no
    /// panic path.
    #[inline]
    pub fn render_sample(&mut self) -> [f32; 2] {
        let mo = self.tick_modulation();

        let shaped = if self.waveform == Waveform::Geometric && self.hq {
            // 2× oversample: analytic oscillator on-grid + at the half-step,
            // both through the character stage, then decimate.
            let (rn1, peak) = self.geom_norm(mo.roll_eff, mo.n);
            let fm_hi = if mo.fm_index > 0.0 {
                mo.fm_index * sin_turns(self.fm_phase + 0.5 * self.fm_ratio * mo.step)
            } else {
                0.0
            };
            let osc_lo = Self::geom_osc(
                self.phase + mo.fm_term + mo.fb + mo.drift,
                mo.roll_eff,
                mo.n,
                rn1,
                mo.frac,
                peak,
                &mo.hump,
            );
            let osc_hi = Self::geom_osc(
                self.phase + 0.5 * mo.step + fm_hi + mo.fb + mo.drift,
                mo.roll_eff,
                mo.n,
                rn1,
                mo.frac,
                peak,
                &mo.hump,
            );
            self.last_osc = osc_lo;
            self.character.process_hq(osc_lo as f32, osc_hi as f32)
        } else {
            let osc = match self.waveform {
                Waveform::Geometric => {
                    let (rn1, peak) = self.geom_norm(mo.roll_eff, mo.n);
                    Self::geom_osc(
                        self.phase + mo.fm_term + mo.fb + mo.drift,
                        mo.roll_eff,
                        mo.n,
                        rn1,
                        mo.frac,
                        peak,
                        &mo.hump,
                    )
                }
                Waveform::Saw => {
                    Self::polyblep_saw(self.phase + mo.fm_term + mo.fb + mo.drift, mo.step)
                }
                Waveform::Triangle => {
                    Self::polyblamp_triangle(self.phase + mo.fm_term + mo.fb + mo.drift, mo.step)
                }
            };
            self.last_osc = osc;
            self.character.process(osc as f32)
        };

        // ---- filter ----
        if self.lfo_to_cutoff != 0.0 {
            self.filter
                .set_cutoff(self.filter_cutoff * exp2(self.lfo_to_cutoff * mo.m));
        }
        let filtered = self.filter.process(shaped);

        // ---- de-click ----
        let dg = if self.declick > 0 {
            self.declick -= 1;
            (DECLICK_LEN - self.declick) as f32 / DECLICK_LEN as f32
        } else {
            1.0
        };
        let mono = filtered * self.gain as f32 * dg;

        self.advance_phase_and_pan(mo.step);
        [mono * self.pan_cos as f32, mono * self.pan_sin as f32]
    }

    /// Two 2×-rate subsamples for `PolySynth`'s unified HQ bus: oscillator →
    /// `Character` → `Svf` all run twice per output sample, at `2×` the base
    /// rate, but *without* per-voice decimation — the caller sums many
    /// voices' pairs and decimates once for the whole stereo mix
    /// (`docs/04_DSP_COMPONENTS.md` §1.7). `Saw`/`Triangle` (already
    /// band-limited PolyBLEP/PolyBLAMP, stateless) are not themselves
    /// oversampled — their single sample is held for both subsamples
    /// (zero-order hold; harmless, since it adds no new energy for the
    /// master decimator to need to remove).
    ///
    /// Crate-private: only `PolySynth` should call this, and only after
    /// [`Voice::set_hq_bus_active`] has configured the filter for the `2×`
    /// rate. A standalone `Voice` / the C ABI keeps using [`Voice::
    /// render_sample`]'s own, unrelated per-voice HQ path via [`Voice::set_hq`].
    #[inline]
    pub(crate) fn render_hq_subsamples(&mut self) -> ([f32; 2], [f32; 2]) {
        let mo = self.tick_modulation();

        let (osc_lo, osc_hi) = match self.waveform {
            Waveform::Geometric => {
                let (rn1, peak) = self.geom_norm(mo.roll_eff, mo.n);
                let fm_hi = if mo.fm_index > 0.0 {
                    mo.fm_index * sin_turns(self.fm_phase + 0.5 * self.fm_ratio * mo.step)
                } else {
                    0.0
                };
                let lo = Self::geom_osc(
                    self.phase + mo.fm_term + mo.fb + mo.drift,
                    mo.roll_eff,
                    mo.n,
                    rn1,
                    mo.frac,
                    peak,
                    &mo.hump,
                );
                let hi = Self::geom_osc(
                    self.phase + 0.5 * mo.step + fm_hi + mo.fb + mo.drift,
                    mo.roll_eff,
                    mo.n,
                    rn1,
                    mo.frac,
                    peak,
                    &mo.hump,
                );
                (lo, hi)
            }
            Waveform::Saw | Waveform::Triangle => {
                let phase = self.phase + mo.fm_term + mo.fb + mo.drift;
                let x = if self.waveform == Waveform::Saw {
                    Self::polyblep_saw(phase, mo.step)
                } else {
                    Self::polyblamp_triangle(phase, mo.step)
                };
                (x, x)
            }
        };
        self.last_osc = osc_lo;

        // ---- filter (twice — the Svf must already be at `2×`, see above) ----
        if self.lfo_to_cutoff != 0.0 {
            self.filter
                .set_cutoff(self.filter_cutoff * exp2(self.lfo_to_cutoff * mo.m));
        }
        let (c_lo, c_hi) = self.character.process_hq_pair(osc_lo as f32, osc_hi as f32);
        let f_lo = self.filter.process(c_lo);
        let f_hi = self.filter.process(c_hi);

        // ---- de-click (once per output sample, applied to both subsamples) ----
        let dg = if self.declick > 0 {
            self.declick -= 1;
            (DECLICK_LEN - self.declick) as f32 / DECLICK_LEN as f32
        } else {
            1.0
        };
        let g = self.gain as f32 * dg;
        let mono_lo = f_lo * g;
        let mono_hi = f_hi * g;

        self.advance_phase_and_pan(mo.step);
        (
            [mono_lo * self.pan_cos as f32, mono_lo * self.pan_sin as f32],
            [mono_hi * self.pan_cos as f32, mono_hi * self.pan_sin as f32],
        )
    }

    /// Shared per-sample parameter smoothing + LFO tick, computing everything
    /// both [`Voice::render_sample`] and [`Voice::render_hq_subsamples`] need
    /// before evaluating the oscillator. Advances `self.lfo` / `self.drift_phase`
    /// — call exactly once per *output* sample (not per 2× subsample), so
    /// modulation runs at the base rate in both render paths.
    #[inline]
    fn tick_modulation(&mut self) -> Modulation {
        // ---- parameter smoothing ----
        // Each one-pole snaps to its target once the gap is `< SNAP` — an
        // inaudibly small residual (`1e-11` in Hz / r / pan / cents units). Two
        // reasons: (1) `pan_z`/`formant_z`/`expr_bright_z` decay geometrically
        // toward *zero* when their control is centred / off, and without the
        // snap they crawl through the entire subnormal range (`< 2.2e-308`) —
        // ~0.3 s of ~100×-slower denormal arithmetic on x86 without FTZ, once
        // per centred note; (2) it also stops the pointless per-sample multiply
        // once a value has converged. `SNAP` is far below any level a smoother
        // reaches inside the verification render, so `VERIFY_HASH` is unchanged.
        const SNAP: f64 = 1.0e-11;
        #[inline(always)]
        fn smooth(z: &mut f64, target: f64, one_minus_coeff: f64) {
            let d = target - *z;
            if fabs(d) < SNAP {
                *z = target;
            } else {
                *z += one_minus_coeff * d;
            }
        }
        let a = 1.0 - self.smooth_coeff;
        smooth(&mut self.freq_z, self.freq, a);
        smooth(&mut self.rolloff_z, self.rolloff, a);
        // Per-note brightness expression / "Formant" depth, same time constant.
        // Both are exactly 0.0 while unused, so `roll_eff` below is unchanged.
        smooth(&mut self.expr_bright_z, self.expr_bright, a);
        smooth(&mut self.formant_z, self.formant, a);
        smooth(&mut self.bend_z, self.bend, a);
        smooth(&mut self.pan_z, self.pan, 1.0 - self.pan_smooth);

        // ---- LFO ----
        // Fast path: when no LFO target is routed the modulator output is
        // unused, so the LFO is not ticked at all. Every routing term below
        // then reduces to an exact identity, so the output is bit-identical.
        // The phase is left frozen; it retriggers (or, in FreeRun mode, does
        // not) on the next note-on.
        let lfo_routed = self.lfo_to_rolloff != 0.0
            || self.lfo_to_pitch != 0.0
            || self.lfo_to_cutoff != 0.0
            || self.lfo_to_fm != 0.0;
        let m = if lfo_routed { self.lfo.tick() as f64 } else { 0.0 };

        let f_eff = if self.lfo_to_pitch != 0.0 {
            self.freq_z * self.bend_z * exp2(self.lfo_to_pitch * m / 1200.0)
        } else {
            self.freq_z * self.bend_z
        };
        let roll_eff = if self.lfo_to_rolloff != 0.0 || self.expr_bright_z != 0.0 {
            clamp(
                self.rolloff_z + self.lfo_to_rolloff * m + self.expr_bright_z,
                Self::ROLLOFF_MIN,
                Self::ROLLOFF_MAX,
            )
        } else {
            self.rolloff_z
        };

        // Nyquist partial clamp tracks the effective (bent + vibrato) pitch,
        // then the user's `partial_limit` ceiling (default MAX_PARTIALS = none).
        // Because the cap is applied *after* the Nyquist clamp, a lower limit
        // only ever removes real harmonics — the result stays exactly
        // band-limited, never aliased. `frac` fades the (n+1)-th partial in,
        // but only when the user's ceiling is what binds: if Nyquist is the
        // limit, fading in partial n+1 would put energy above it, so drop it.
        let nyq = {
            let f = if f_eff > 1.0 { f_eff } else { 1.0 };
            ((self.sample_rate / (2.0 * f)) as u32).max(1).min(MAX_PARTIALS)
        };
        let n = nyq.min(self.partial_limit);
        let frac = if self.partial_limit < nyq {
            self.partial_frac as f64
        } else {
            0.0
        };
        let hump = self.hump_for(self.formant_z, n);

        // ---- oscillator terms ----
        let fb = self.feedback * self.last_osc;
        let step = f_eff / self.sample_rate; // turns per output sample
        // Slow free-running phase drift (unison "breathing"). Folded into the
        // same phase offset as FM and feedback.
        let drift = if self.drift_depth != 0.0 {
            self.drift_phase += self.drift_inc;
            if self.drift_phase >= 1.0 {
                self.drift_phase -= 1.0;
            }
            self.drift_depth * sin_turns_fast(self.drift_phase)
        } else {
            0.0
        };
        // FM index with the LFO routing folded in (clamped at 0 — a negative
        // effective index is just "off").
        let fm_index = if self.lfo_to_fm != 0.0 {
            let i = self.fm_index + self.lfo_to_fm * m;
            if i > 0.0 {
                i
            } else {
                0.0
            }
        } else {
            self.fm_index
        };
        let fm_term = if fm_index > 0.0 {
            fm_index * sin_turns(self.fm_phase)
        } else {
            0.0
        };

        Modulation { roll_eff, n, frac, fb, step, drift, fm_index, fm_term, m, hump }
    }

    /// Advance the carrier/FM phase accumulators by `step` and refresh the
    /// cached equal-power pan gains if `pan_z` moved. Shared tail of both
    /// render paths — called once per output sample, after the oscillator/
    /// filter/de-click work is done (the pan gains are then applied by the
    /// caller, which knows whether it has one mono sample or a lo/hi pair).
    #[inline]
    fn advance_phase_and_pan(&mut self, step: f64) {
        // `fm_ratio` (`≥ 0`) and `step` (`> 0`) are both non-negative, so
        // `fm_phase` only ever increases — the `≤ −1.0` half of the old test
        // was dead. `fm_ratio·step` can be large (up to ~2048), so `floor_f64`,
        // not a bare `-= 1.0`.
        self.fm_phase += self.fm_ratio * step;
        if self.fm_phase >= 1.0 {
            self.fm_phase -= floor_f64(self.fm_phase);
        }
        self.phase += step;
        // `step` is `f_eff / fs`; `f_eff` (bend ×32, vibrato ×2, `freq` up to
        // `fs/2`) can exceed `fs`, so `step` can exceed `1.0` — a bare `-= 1.0`
        // would not fully wrap and the accumulator would run away over a long
        // held note. `floor_f64` is exact for `|x| < 2⁶³`, and for `step < 1`
        // this is bit-identical to `-= 1.0` (`⌊x⌋ == 1` on `[1, 2)`).
        if self.phase >= 1.0 {
            self.phase -= floor_f64(self.phase);
        }

        // ---- equal-power pan ---- (a modulator → fast trig is plenty)
        // angle θ ∈ [0, π/2]; in turns that is (pan·0.5 + 0.5)·0.25. `pan_z`
        // stops moving once the ~10 ms smoother converges, so cache the gains
        // and skip the trig on every steady sample (bit-identical: same input).
        if self.pan_z != self.pan_cache_z {
            let (s, c) = sin_cos_turns_fast((self.pan_z * 0.5 + 0.5) * 0.25);
            self.pan_sin = s;
            self.pan_cos = c;
            self.pan_cache_z = self.pan_z;
        }
    }

    /// The carrier phase accumulator, in turns. Test-only — for asserting the
    /// wrap in [`Self::advance_phase_and_pan`] keeps it in `[0, 1)` even when
    /// `step` exceeds `1.0`.
    #[cfg(test)]
    #[inline]
    pub fn carrier_phase_for_test(&self) -> f64 {
        self.phase
    }

    /// The pan and formant one-pole smoother states. Test-only — for asserting
    /// the `SNAP` in [`Self::tick_modulation`] pins a converged smoother to
    /// *exactly* its target instead of letting it crawl through the subnormal
    /// range for the rest of the note.
    #[cfg(test)]
    #[inline]
    pub fn smoother_states_for_test(&self) -> (f64, f64) {
        (self.pan_z, self.formant_z)
    }

    /// Fill `left` / `right` with rendered samples (up to the shorter length).
    #[inline]
    pub fn render_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        for i in 0..n {
            let [l, r] = self.render_sample();
            left[i] = l;
            right[i] = r;
        }
    }
}

#[inline(always)]
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    // NaN compares false against everything, so a plain `<`/`>` chain lets it
    // fall through to the `else` arm unchanged — every one of this fn's ~14
    // call sites (freq, gain, pan, feedback, FM, LFO routing depths, drift...)
    // would silently latch NaN into voice state and propagate it into audio
    // forever, from any hostile/buggy caller of the public setters (this is
    // the C-ABI's argument-clamping helper, not just an internal convenience
    // — `harmonic_voice_set_*` hands caller-supplied f64s straight to it).
    // Default to `lo`: for every current call site that's a silent/inaudible
    // value (freq floor, 0 gain, 0 feedback/FM, hard-left pan), never a loud
    // or surprising one.
    if x.is_nan() || x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

#[inline(always)]
fn fabs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & 0x7fff_ffff_ffff_ffff)
}

/// PolyBLEP: the residual to *subtract* from a naïve waveform to band-limit a
/// unit downward step at the phase wrap. `t` is the phase in `[0, 1)`, `dt` the
/// per-sample increment. Two-sample (2nd-order) kernel; zero outside `±dt`.
#[inline]
fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0 // 2x − x² − 1
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0 // x² + 2x + 1
    } else {
        0.0
    }
}

/// PolyBLAMP: `∫ poly_blep` — the correction for a unit *slope* step (a corner),
/// scaled so the caller multiplies by `Δslope_per_sample`. Cubic, ±`dt` support.
#[inline]
fn poly_blamp(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt - 1.0;
        -1.0 / 3.0 * x * x * x
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt + 1.0;
        1.0 / 3.0 * x * x * x
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono_peak(v: &mut Voice, samples: usize) -> f32 {
        let mut peak = 0.0_f32;
        for _ in 0..samples {
            let [l, r] = v.render_sample();
            assert!(l.is_finite() && r.is_finite());
            peak = peak.max(l.abs()).max(r.abs());
        }
        peak
    }

    #[test]
    fn output_stays_bounded_across_the_range() {
        let mut v = Voice::new(48_000.0);
        v.set_gain(1.0);
        for &f in &[20.0, 55.0, 220.0, 440.0, 2000.0, 12_000.0] {
            v.set_frequency(f);
            v.reset();
            let p = mono_peak(&mut v, 48_000);
            assert!(p <= 1.5 && p > 0.05, "f={f} peak={p}");
        }
    }

    #[test]
    fn partial_limit_caps_but_nyquist_still_wins() {
        let mut v = Voice::new(48_000.0);
        v.set_frequency(110.0);
        for _ in 0..4000 {
            v.render_sample();
        } // freq_z fully settled
        let nyq = v.max_partials(); // Nyquist-only (limit is default MAX)
        assert!((150..260).contains(&nyq), "unexpected Nyquist count {nyq}");

        v.set_partial_limit(50.7); // below Nyquist → binds (integer part)
        assert_eq!(v.max_partials(), 50);

        v.set_partial_limit((nyq + 1000) as f32); // above Nyquist → moot (also clamped to MAX_PARTIALS)
        assert_eq!(v.max_partials(), nyq);

        v.set_partial_limit(0.0); // out of range → clamped to 1
        assert_eq!(v.max_partials(), 1);

        v.set_frequency(15_000.0); // Nyquist gives 1 partial here
        for _ in 0..4000 {
            v.render_sample();
        }
        v.set_partial_limit(200.5);
        assert_eq!(v.max_partials(), 1);
    }

    #[test]
    fn partial_frac_fades_in_the_next_partial() {
        // With the ceiling at n.frac, the (n+1)-th harmonic should be present
        // at roughly `frac` of its full amplitude — a continuous sweep, not a
        // step. And when Nyquist is what binds, the fraction must be dropped
        // (fading in a partial above Nyquist would alias).
        let fs = 48_000.0;
        let f0 = 220.0; // Nyquist allows ~109 partials — the ceiling binds
        let mag7 = |limit: f32| -> f64 {
            let mut v = Voice::new(fs);
            v.set_frequency(f0);
            v.set_rolloff(0.995);
            v.set_gain(1.0);
            v.set_partial_limit(limit);
            v.reset();
            let mut buf = vec![0.0_f32; 1 << 15];
            let mut scr = vec![0.0_f32; 1 << 15];
            v.render_block(&mut buf, &mut scr);
            let w = core::f64::consts::TAU * (f0 * 7.0) / fs;
            let (mut re, mut im) = (0.0_f64, 0.0);
            for (n, &s) in buf.iter().enumerate() {
                re += s as f64 * (w * n as f64).cos();
                im -= s as f64 * (w * n as f64).sin();
            }
            (re * re + im * im).sqrt() / buf.len() as f64
        };
        let none = mag7(6.0); // 7th partial absent
        let half = mag7(6.5); // ~half in
        let full = mag7(7.0); // fully in
        assert!(none < full * 0.05, "7th partial present at limit 6.0: {none:e}");
        assert!(half > full * 0.25 && half < full * 0.75, "fade not ~half: {half:e} vs {full:e}");
    }

    #[test]
    fn expr_brightness_tilts_the_spectrum_and_zero_is_inert() {
        let fs = 48_000.0;
        let f0 = 180.0;
        // Spectral *tilt*: the 8th partial relative to the fundamental. An
        // absolute partial magnitude is confounded by the peak normalisation
        // (as r→1 the whole spectrum is rescaled), the ratio is not.
        let bin = |buf: &[f32], f: f64| -> f64 {
            let w = core::f64::consts::TAU * f / fs;
            let (mut re, mut im) = (0.0_f64, 0.0);
            for (n, &s) in buf.iter().enumerate() {
                re += s as f64 * (w * n as f64).cos();
                im -= s as f64 * (w * n as f64).sin();
            }
            (re * re + im * im).sqrt() / buf.len() as f64
        };
        let tilt = |offset: f64| -> f64 {
            let mut v = Voice::new(fs);
            v.set_frequency(f0);
            v.set_rolloff(0.7); // mid tilt — headroom both ways
            v.set_gain(1.0);
            v.set_expr_brightness(offset);
            v.reset();
            let mut buf = vec![0.0_f32; 1 << 15];
            let mut scr = vec![0.0_f32; 1 << 15];
            v.render_block(&mut buf, &mut scr);
            bin(&buf, f0 * 8.0) / bin(&buf, f0)
        };
        let dark = tilt(-0.35);
        let flat = tilt(0.0);
        let bright = tilt(0.3);
        assert!(bright > flat * 2.0, "positive expr did not brighten the tilt: {flat:e} -> {bright:e}");
        assert!(dark < flat * 0.5, "negative expr did not darken the tilt: {flat:e} -> {dark:e}");

        // expr 0.0 must be *exactly* the no-expression render, bit for bit.
        let mut a = Voice::new(fs);
        a.set_frequency(f0);
        a.set_rolloff(0.7);
        a.reset();
        let mut b = a;
        b.set_expr_brightness(0.0);
        for _ in 0..3000 {
            assert_eq!(a.render_sample(), b.render_sample(), "expr 0.0 perturbed the output");
        }
    }

    #[test]
    fn formant_adds_a_movable_mid_spectrum_bump_and_zero_is_inert() {
        let fs = 48_000.0;
        let f0 = 150.0;
        // magnitude of the k-th partial once the smoothed formant has settled
        let partial = |formant: f64, k: f64| -> f64 {
            let mut v = Voice::new(fs);
            v.set_frequency(f0);
            v.set_rolloff(0.4); // steep natural rolloff → the mids are near-silent
            v.set_gain(1.0);
            v.set_formant(formant);
            v.reset();
            let mut buf = vec![0.0_f32; 1 << 15];
            let mut scr = vec![0.0_f32; 1 << 15];
            v.render_block(&mut buf, &mut scr);
            for x in &buf {
                assert!(x.is_finite() && x.abs() <= 1.5, "formant {formant}: unbounded {x}");
            }
            let w = core::f64::consts::TAU * (f0 * k) / fs;
            let (mut re, mut im) = (0.0_f64, 0.0);
            for (n, &s) in buf.iter().enumerate() {
                re += s as f64 * (w * n as f64).cos();
                im -= s as f64 * (w * n as f64).sin();
            }
            (re * re + im * im).sqrt() / buf.len() as f64
        };

        // 1. A bump appears: at rolloff 0.4 the 6th partial is ~0.4^6 ≈ 4e-3 of
        //    the fundamental; a mid formant lifts it by well over an order of
        //    magnitude.
        let p6_off = partial(0.0, 6.0);
        let p6_on = partial(0.42, 6.0); // K ≈ 1.5 + 24·0.42² ≈ 5.7
        assert!(p6_on > p6_off * 8.0, "formant did not bump the 6th partial: {p6_off:e} -> {p6_on:e}");

        // 2. The bump moves up with the control. Two views, both robust to the
        //    peak renormalisation: a low formant favours a low partial over a
        //    high one, a high formant the reverse.
        let lo_at4 = partial(0.30, 4.0); // K ≈ 3.7
        let lo_at15 = partial(0.30, 15.0);
        let hi_at4 = partial(0.78, 4.0); // K ≈ 16
        let hi_at15 = partial(0.78, 15.0);
        assert!(
            lo_at4 > lo_at15 * 2.0,
            "low formant should sit on partial 4, not 15: {lo_at4:e} vs {lo_at15:e}"
        );
        assert!(
            hi_at15 > hi_at4,
            "high formant should shift energy up to partial 15: {hi_at4:e} vs {hi_at15:e}"
        );
        assert!(
            hi_at15 > lo_at15,
            "raising the formant should lift partial 15: {lo_at15:e} -> {hi_at15:e}"
        );

        // 3. formant 0.0 must be *exactly* the no-formant render, bit for bit.
        let mut c = Voice::new(fs);
        c.set_frequency(f0);
        c.set_rolloff(0.85);
        c.reset();
        let mut d = c;
        d.set_formant(0.0);
        for _ in 0..3000 {
            assert_eq!(c.render_sample(), d.render_sample(), "formant 0.0 perturbed the output");
        }
    }

    #[test]
    fn equal_power_pan_splits_correctly() {
        let mut v = Voice::new(48_000.0);
        v.set_gain(1.0);
        v.set_frequency(220.0);

        v.set_pan(-1.0);
        v.reset();
        for _ in 0..600 {
            v.render_sample();
        } // let pan_z settle
        let mut lsum = 0.0f32;
        let mut rsum = 0.0f32;
        for _ in 0..4000 {
            let [l, r] = v.render_sample();
            lsum += l.abs();
            rsum += r.abs();
        }
        assert!(lsum > rsum * 20.0, "hard left leaked: L={lsum} R={rsum}");

        v.set_pan(0.0);
        v.reset();
        for _ in 0..600 {
            v.render_sample();
        }
        let (mut ls, mut rs) = (0.0f32, 0.0f32);
        for _ in 0..4000 {
            let [l, r] = v.render_sample();
            ls += l.abs();
            rs += r.abs();
        }
        assert!((ls - rs).abs() / ls.max(rs) < 0.05, "center not balanced: {ls} {rs}");
    }

    #[test]
    fn free_running_phase_survives_note_on() {
        let mut v = Voice::new(48_000.0);
        v.set_frequency(300.0);
        v.set_free_running(true);
        v.reset();
        for _ in 0..1234 {
            v.render_sample();
        }
        let p_before = v.phase;
        v.reset(); // note-on again
        assert!(
            (v.phase - p_before).abs() < 1e-9,
            "free-running phase was reset: {p_before} -> {}",
            v.phase
        );

        v.set_free_running(false);
        v.reset();
        assert_eq!(v.phase, 0.0, "reset mode did not zero the phase");
    }

    #[test]
    fn declick_ramps_in_from_near_zero() {
        let mut v = Voice::new(48_000.0);
        v.set_gain(1.0);
        v.set_frequency(400.0);
        v.reset();
        let first = {
            let [l, _] = v.render_sample();
            l.abs()
        };
        let mut later = 0.0f32;
        for _ in 0..64 {
            let [l, _] = v.render_sample();
            later = later.max(l.abs());
        }
        assert!(first < later, "no de-click ramp: first={first} later={later}");
    }

    #[test]
    fn polyblep_saw_and_triangle_are_bounded_and_shaped() {
        let sr = 48_000.0;
        for wf in [Waveform::Saw, Waveform::Triangle] {
            for &f0 in &[55.0_f64, 220.0, 3000.0] {
                let mut v = Voice::new(sr);
                v.set_frequency(f0);
                v.set_gain(1.0);
                v.set_waveform(wf);
                v.reset();

                for _ in 0..4000 {
                    v.render_sample();
                }
                let mut buf = [0.0_f32; 8192];
                for s in buf.iter_mut() {
                    let [l, _] = v.render_sample();
                    assert!(
                        l.is_finite() && l.abs() <= 1.6,
                        "{:?} out of range: {l}",
                        wf as u32
                    );
                    *s = l;
                }

                // single-bin DFT magnitude at f
                let mag = |f: f64| {
                    let w = core::f64::consts::TAU * f / sr;
                    let (mut re, mut im) = (0.0, 0.0);
                    for (k, &x) in buf.iter().enumerate() {
                        re += x as f64 * (w * k as f64).cos();
                        im -= x as f64 * (w * k as f64).sin();
                    }
                    (re * re + im * im).sqrt() / buf.len() as f64
                };

                let fund = mag(f0);
                let h2 = mag(2.0 * f0);
                let h3 = mag(3.0 * f0);
                let above = mag(sr * 0.49);

                assert!(fund > 0.04, "{wf:?} f0={f0}: weak fundamental {fund:.4}");
                assert!(h3 < fund, "{wf:?} f0={f0}: harmonics not rolling off");
                assert!(
                    above < fund * 0.02,
                    "{wf:?} f0={f0}: energy above Nyquist {above:.5}"
                );
                if wf == Waveform::Triangle {
                    // triangle: even harmonics essentially absent, odd ≈ 1/k²
                    assert!(
                        h2 < fund * 0.15,
                        "triangle f0={f0}: 2nd harmonic too strong: {h2:.4}"
                    );
                    let h3_ratio = h3 / fund;
                    assert!(
                        (0.06..0.22).contains(&h3_ratio),
                        "triangle f0={f0}: h3/h1 = {h3_ratio:.3}, expected ≈ 1/9"
                    );
                }
            }
        }
    }

    #[test]
    fn polyblep_waves_are_flat_into_the_sub_bass() {
        // PolyBLEP / PolyBLAMP have no high-pass anywhere, so the fundamental
        // must sit at its ideal level right down to ~20 Hz.
        let sr = 48_000.0;
        let n = 1 << 15;
        let mag = |buf: &[f64], f: f64| {
            let w = core::f64::consts::TAU * f / sr;
            let (mut re, mut im) = (0.0, 0.0);
            for (k, &x) in buf.iter().enumerate() {
                re += x * (w * k as f64).cos();
                im -= x * (w * k as f64).sin();
            }
            (re * re + im * im).sqrt() / buf.len() as f64
        };

        for wf in [Waveform::Saw, Waveform::Triangle] {
            // ideal fundamental amplitude
            let ideal = if wf == Waveform::Saw {
                2.0 / core::f64::consts::PI
            } else {
                8.0 / (core::f64::consts::PI * core::f64::consts::PI)
            };
            for &f0 in &[27.5_f64, 55.0, 220.0] {
                let mut v = Voice::new(sr);
                v.set_frequency(f0);
                v.set_gain(1.0);
                v.set_pan(0.0);
                v.set_waveform(wf);
                v.set_free_running(true);
                v.reset();
                for _ in 0..2000 {
                    v.render_sample();
                }
                let buf: Vec<f64> = (0..n)
                    .map(|_| v.render_sample()[0] as f64 * core::f64::consts::SQRT_2)
                    .collect();
                // single-bin magnitude → amplitude/2, so compare to ideal/2
                let db = 20.0 * (mag(&buf, f0) / (ideal * 0.5)).log10();
                assert!(
                    db.abs() < 0.5,
                    "{wf:?} f0={f0}: fundamental {db:+.2} dB off ideal — a high-pass crept in"
                );
            }
        }
    }

    #[test]
    fn unrouted_lfo_does_not_affect_output() {
        // Fast path: when both LFO depths are zero the LFO is not ticked. Prove
        // that is transparent — a voice with an LFO configured at some rate but
        // routed to nothing renders bit-identically to one with no LFO at all.
        let mk = |rate: Option<f64>| {
            let mut v = Voice::new(48_000.0);
            v.set_frequency(196.0);
            v.set_gain(0.9);
            v.set_pan(-0.3);
            v.set_rolloff(0.93);
            if let Some(hz) = rate {
                v.set_lfo(hz, LfoShape::Sine);
            }
            v.set_lfo_targets(0.0, 0.0, 0.0, 0.0);
            v.reset();
            v
        };
        let mut a = mk(None);
        let mut b = mk(Some(6.3));
        for i in 0..20_000 {
            assert_eq!(a.render_sample(), b.render_sample(), "diverged at sample {i}");
        }
    }

    #[test]
    fn lfo_to_cutoff_and_fm_stay_bounded() {
        // Route the LFO to every target at once on a filtered + FM voice. A
        // resonant SVF swept fast overshoots unity (that is real, not a bug —
        // see `filter::resonance_lifts_the_corner`), so the bound is loose;
        // `mono_peak` also asserts every sample is finite.
        let mut v = Voice::new(48_000.0);
        v.set_frequency(110.0);
        v.set_gain(1.0);
        v.set_fm(2.0, 0.5);
        v.set_filter_mode(FilterMode::Low);
        v.set_filter_cutoff(3_000.0);
        v.set_filter_resonance(0.5);
        v.set_lfo(7.0, LfoShape::Triangle);
        v.set_lfo_targets(0.5, 40.0, 2.0, 3.0); // rolloff, cents, ±2 oct, ±3 index
        v.reset();
        assert!(mono_peak(&mut v, 96_000) <= 2.5);
    }

    #[test]
    fn free_run_lfo_phase_survives_note_on() {
        // Two identical free-running-oscillator voices, deep vibrato. Run both
        // ~⅔ of an LFO cycle, then note-on. The Retrigger voice snaps its LFO
        // to 0; the FreeRun voice keeps going — so their outputs must differ.
        let mk = |mode: LfoMode| {
            let mut v = Voice::new(48_000.0);
            v.set_frequency(330.0);
            v.set_gain(1.0);
            v.set_free_running(true); // osc phase itself doesn't reset
            v.set_lfo(5.0, LfoShape::Sine);
            v.set_lfo_targets(0.0, 200.0, 0.0, 0.0); // strong vibrato
            v.set_lfo_mode(mode);
            v.reset();
            for _ in 0..6500 {
                v.render_sample();
            }
            v.reset(); // note-on mid LFO cycle
            v
        };
        let mut retrig = mk(LfoMode::Retrigger);
        let mut freerun = mk(LfoMode::FreeRun);
        let mut max_diff = 0.0_f32;
        for _ in 0..2000 {
            let [lr, _] = retrig.render_sample();
            let [lf, _] = freerun.render_sample();
            max_diff = max_diff.max((lr - lf).abs());
        }
        assert!(max_diff > 0.05, "FreeRun LFO restarted like Retrigger ({max_diff})");
    }

    #[test]
    fn geom_and_pan_caches_track_changing_params() {
        // The per-(r,n) and per-pan caches must invalidate when the target
        // moves. Compare a voice whose brightness + pan are swept against a
        // fresh voice started directly at each end state and let it settle.
        let mut swept = Voice::new(48_000.0);
        swept.set_frequency(220.0);
        swept.set_gain(1.0);
        swept.reset();
        for _ in 0..2000 {
            swept.render_sample();
        }
        swept.set_rolloff(0.7);
        swept.set_pan(0.6);
        for _ in 0..6000 {
            swept.render_sample();
        } // let both smoothers converge

        let mut fresh = Voice::new(48_000.0);
        fresh.set_frequency(220.0);
        fresh.set_gain(1.0);
        fresh.set_rolloff(0.7);
        fresh.set_pan(0.6);
        fresh.set_free_running(true); // no phase reset, no de-click ramp
        fresh.reset();
        // align oscillator phase to `swept` so the tones line up
        fresh.set_start_phase(swept.phase);
        fresh.fm_phase = swept.fm_phase;

        let mut max_diff = 0.0_f32;
        for _ in 0..4000 {
            let [la, ra] = swept.render_sample();
            let [lb, rb] = fresh.render_sample();
            max_diff = max_diff.max((la - lb).abs()).max((ra - rb).abs());
        }
        assert!(max_diff < 1e-4, "cache did not converge to the direct value: {max_diff}");
    }

    #[test]
    fn pitch_bend_and_lfo_stay_finite() {
        let mut v = Voice::new(48_000.0);
        v.set_gain(1.0);
        v.set_frequency(220.0);
        v.set_pitch_bend((2.0_f64).powf(2.0 / 12.0));
        v.set_lfo(6.0, LfoShape::Sine);
        v.set_lfo_targets(0.3, 25.0, 0.0, 0.0);
        v.reset();
        assert!(mono_peak(&mut v, 96_000) <= 1.5);
    }

    /// Long-run numerical drift of the carrier and FM phase accumulators
    /// (`phase += step` with a per-period wrap). `#[ignore]` — heavy; run with
    ///
    /// ```text
    /// cargo test --release -p harmonic_core -- --ignored --nocapture drift
    /// DRIFT_SAMPLES=1000000000 cargo test --release -- --ignored --nocapture drift
    /// ```
    ///
    /// The reference is a Kahan-compensated, exactly-wrapped accumulator, so it
    /// is accurate to ~1e-16 turns/step and the assertion measures the real
    /// drift of the production accumulator, not reference noise.
    #[test]
    #[ignore]
    fn phase_accumulators_do_not_drift() {
        let sr = 48_000.0;
        let f0 = 220.0;
        let n: u64 = std::env::var("DRIFT_SAMPLES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(200_000_000);

        let mut v = Voice::new(sr);
        v.set_frequency(f0);
        v.set_gain(1.0);
        v.set_fm(3.0, 0.25); // drive the fm_phase accumulator + its floor-wrap
        v.reset();
        for _ in 0..50_000 {
            v.render_sample(); // lock freq_z to freq exactly
        }

        let step = v.freq_z * v.bend_z / sr;
        let fm_step = v.fm_ratio * step;

        // Kahan-compensated references, wrapped exactly to [0, 1+step).
        let mut rp = v.phase;
        let mut rpc = 0.0_f64;
        let mut rf = v.fm_phase;
        let mut rfc = 0.0_f64;
        let kahan = |acc: &mut f64, comp: &mut f64, inc: f64| {
            let y = inc - *comp;
            let t = *acc + y;
            *comp = (t - *acc) - y;
            *acc = t;
            if *acc >= 1.0 {
                *acc -= 1.0;
            }
        };
        let circ = |a: f64, b: f64| {
            let d = (a - b).abs();
            if d > 0.5 {
                1.0 - d
            } else {
                d
            }
        };

        let (mut max_dp, mut max_df) = (0.0_f64, 0.0_f64);
        for _ in 0..n {
            v.render_sample();
            kahan(&mut rp, &mut rpc, step);
            kahan(&mut rf, &mut rfc, fm_step);
            max_dp = max_dp.max(circ(v.phase, rp));
            max_df = max_df.max(circ(v.fm_phase, rf));
        }

        let hours = n as f64 / sr / 3600.0;
        // The naive accumulator's error grows linearly (a tiny per-add rounding
        // bias), so the *frequency* error is what matters — it is N-independent.
        // A phase deviation of `d` turns over `n` samples = an effective
        // frequency error of `d/n · sr` Hz.
        let carrier_ppm = max_dp / n as f64 * sr / f0 * 1.0e6;
        let fm_ppm = max_df / n as f64 * sr / (v.fm_ratio * f0) * 1.0e6;
        eprintln!(
            "drift over {n} samples ({hours:.2} h @ {sr} Hz):\n  \
             carrier phase max dev {max_dp:.3e} turns  →  {carrier_ppm:.3e} ppm frequency error\n  \
             FM phase      max dev {max_df:.3e} turns  →  {fm_ppm:.3e} ppm"
        );
        // Bound the *rate*, not the absolute deviation. 1e-3 ppm ≈ 2e-6 cents —
        // some nine orders of magnitude above what is measured, but any real
        // regression (e.g. dropping the per-period wrap) blows straight past it.
        assert!(carrier_ppm < 1.0e-3, "carrier frequency drift {carrier_ppm:e} ppm");
        assert!(fm_ppm < 1.0e-3, "fm frequency drift {fm_ppm:e} ppm");
    }

    #[test]
    fn a_converged_smoother_snaps_to_its_target_instead_of_crawling_subnormals() {
        // A one-pole `z += a·(target − z)` approaches a centred target (pan 0,
        // formant off) geometrically but never reaches it — left alone it
        // spends the rest of the note grinding `z` down through the whole
        // subnormal range (`|z| < 2.2e-308`), ~100× slower per multiply on x86
        // without hardware flush-to-zero. `tick_modulation`'s `SNAP` pins it to
        // the target once the residual is inaudible. Assert that pin actually
        // happens, within a musically short window, and holds.
        let mut v = Voice::new(48_000.0);
        v.set_gain(1.0);
        v.set_frequency(220.0);
        v.set_pan(0.7);
        v.set_formant(0.6);
        v.reset(); // smoothers start pinned at 0.7 / 0.6

        v.set_pan(0.0); // now a decaying gap toward zero
        v.set_formant(0.0);

        let mut snapped_at = None;
        for i in 0..48_000 {
            v.render_sample();
            if v.smoother_states_for_test() == (0.0, 0.0) {
                snapped_at = Some(i);
                break;
            }
        }
        let n = snapped_at.expect("pan/formant smoothers never snapped to exactly 0.0");
        // 5 ms (formant) / 10 ms (pan) time constants → a few hundred ms to
        // decay past `SNAP`; 0.5 s is comfortable headroom and a bare
        // `z += a·d` with no snap would never trip this at all.
        assert!(n < 24_000, "snap took {n} samples (> 0.5 s)");

        for _ in 0..9_600 {
            v.render_sample();
            assert_eq!(
                v.smoother_states_for_test(),
                (0.0, 0.0),
                "a snapped smoother drifted back off its target"
            );
        }
    }
}
