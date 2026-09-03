//! A single synthesiser voice: band-limited oscillator, FM + feedback, a
//! per-voice LFO, the character stage, the SVF, pitch bend, equal-power pan,
//! and a per-sample de-click ramp. Stereo out.
//!
//! `#[repr(C)]` and fixed-size — the C side owns the storage, this crate never
//! allocates. See [`crate::ffi`].

use crate::character::{CharParams, Character};
use crate::filter::{FilterMode, Svf};
use crate::kernel::{geometric_partials, geometric_peak};
use crate::lfo::{Lfo, LfoShape};
use crate::trig::{exp2, floor_f64, sin_cos_turns_fast, sin_turns};
use crate::{validate_sample_rate, SampleRateStatus};

/// Hard ceiling on partial count (error budget + `r^n` loop length).
pub const MAX_PARTIALS: u32 = 2048;

/// Length of the note-on de-click fade, in samples.
const DECLICK_LEN: u16 = 16;

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

    hq: bool, // 2× oversample the oscillator + character stage

    character: Character,
    filter: Svf,
}

impl Voice {
    pub const ROLLOFF_MIN: f64 = 1.0e-3;
    pub const ROLLOFF_MAX: f64 = 0.9995;

    /// Latency, in samples, added by [`Voice::set_hq`] `true` (the character
    /// decimation FIR). Zero when HQ is off.
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
            hq: false,
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
        self.phase = turns - floor_f64(turns);
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

    /// LFO routing: `to_rolloff` adds to `r` (± brightness), `to_pitch_cents`
    /// is vibrato depth.
    #[inline]
    pub fn set_lfo_targets(&mut self, to_rolloff: f64, to_pitch_cents: f64) {
        self.lfo_to_rolloff = clamp(to_rolloff, -0.9, 0.9);
        self.lfo_to_pitch = clamp(to_pitch_cents, -1200.0, 1200.0);
    }

    #[inline]
    pub fn set_lfo_phase(&mut self, turns: f64) {
        self.lfo.set_phase(turns);
    }

    // ---- character / filter ----

    #[inline]
    pub fn set_character(&mut self, p: CharParams) {
        self.character.set_params(p);
    }

    /// HQ mode: 2×-oversample the oscillator + character stage and decimate,
    /// so the nonlinear stages do not alias. Adds [`Voice::HQ_LATENCY`]
    /// samples of latency; `false` is bit-identical to the previous behaviour.
    #[inline]
    pub fn set_hq(&mut self, hq: bool) {
        self.hq = hq;
    }

    #[inline]
    pub fn set_filter_mode(&mut self, mode: FilterMode) {
        self.filter.set_mode(mode);
    }

    #[inline]
    pub fn set_filter_cutoff(&mut self, hz: f64) {
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
        self.bend_z = self.bend;
        self.pan_z = self.pan;
        self.character.reset();
        self.filter.reset();
        self.lfo.retrigger();
    }

    #[inline]
    pub fn max_partials(&self) -> u32 {
        let f = if self.freq_z > 1.0 { self.freq_z } else { 1.0 };
        ((self.sample_rate / (2.0 * f)) as u32).max(1).min(MAX_PARTIALS)
    }

    #[inline]
    pub fn current_frequency(&self) -> f64 {
        self.freq_z * self.bend_z
    }

    /// Render one stereo sample `[left, right]`. No allocation, no locks, no
    /// panic path.
    #[inline]
    pub fn render_sample(&mut self) -> [f32; 2] {
        // ---- parameter smoothing ----
        self.freq_z += (1.0 - self.smooth_coeff) * (self.freq - self.freq_z);
        self.rolloff_z += (1.0 - self.smooth_coeff) * (self.rolloff - self.rolloff_z);
        self.bend_z += (1.0 - self.smooth_coeff) * (self.bend - self.bend_z);
        self.pan_z += (1.0 - self.pan_smooth) * (self.pan - self.pan_z);

        // ---- LFO ----
        let m = self.lfo.tick() as f64; // −1..1
        let pitch_mod = if self.lfo_to_pitch != 0.0 {
            exp2(self.lfo_to_pitch * m / 1200.0)
        } else {
            1.0
        };
        let f_eff = self.freq_z * self.bend_z * pitch_mod;
        let roll_eff = clamp(
            self.rolloff_z + self.lfo_to_rolloff * m,
            Self::ROLLOFF_MIN,
            Self::ROLLOFF_MAX,
        );

        // Nyquist partial clamp tracks the effective (bent + vibrato) pitch.
        let n = {
            let f = if f_eff > 1.0 { f_eff } else { 1.0 };
            ((self.sample_rate / (2.0 * f)) as u32).max(1).min(MAX_PARTIALS)
        };

        // ---- oscillator ----
        let peak = geometric_peak(roll_eff, n);
        let fb = self.feedback * self.last_osc;
        let step = f_eff / self.sample_rate; // turns per output sample

        let shaped = if self.hq {
            // 2× oversample: evaluate the analytic oscillator on-grid and at the
            // half-step, run both through the character stage, decimate.
            let fm_step = self.fm_ratio * step;
            let (fm_lo, fm_hi) = if self.fm_index > 0.0 {
                (
                    self.fm_index * sin_turns(self.fm_phase),
                    self.fm_index * sin_turns(self.fm_phase + 0.5 * fm_step),
                )
            } else {
                (0.0, 0.0)
            };
            let osc_lo = geometric_partials(self.phase + fm_lo + fb, roll_eff, n) / peak;
            let osc_hi =
                geometric_partials(self.phase + 0.5 * step + fm_hi + fb, roll_eff, n) / peak;
            self.last_osc = osc_lo;
            self.character
                .process_hq(osc_lo as f32, osc_hi as f32)
        } else {
            let fm_term = if self.fm_index > 0.0 {
                self.fm_index * sin_turns(self.fm_phase)
            } else {
                0.0
            };
            let osc = geometric_partials(self.phase + fm_term + fb, roll_eff, n) / peak;
            self.last_osc = osc;
            self.character.process(osc as f32)
        };

        // ---- filter ----
        let filtered = self.filter.process(shaped);

        // ---- de-click ----
        let dg = if self.declick > 0 {
            self.declick -= 1;
            (DECLICK_LEN - self.declick) as f32 / DECLICK_LEN as f32
        } else {
            1.0
        };
        let mono = filtered * self.gain as f32 * dg;

        // ---- advance phases ----
        self.fm_phase += self.fm_ratio * step;
        if self.fm_phase >= 1.0 || self.fm_phase <= -1.0 {
            self.fm_phase -= floor_f64(self.fm_phase);
        }
        self.phase += step;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // ---- equal-power pan ---- (a modulator → fast trig is plenty)
        // angle θ ∈ [0, π/2]; in turns that is (pan·0.5 + 0.5)·0.25
        let (sin_p, cos_p) = sin_cos_turns_fast((self.pan_z * 0.5 + 0.5) * 0.25);
        [mono * cos_p as f32, mono * sin_p as f32]
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
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
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
    fn pitch_bend_and_lfo_stay_finite() {
        let mut v = Voice::new(48_000.0);
        v.set_gain(1.0);
        v.set_frequency(220.0);
        v.set_pitch_bend((2.0_f64).powf(2.0 / 12.0));
        v.set_lfo(6.0, LfoShape::Sine);
        v.set_lfo_targets(0.3, 25.0);
        v.reset();
        assert!(mono_peak(&mut v, 96_000) <= 1.5);
    }
}
