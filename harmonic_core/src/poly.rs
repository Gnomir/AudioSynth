//! Polyphonic MIDI engine over [`Voice`]. Fixed voice array, no allocation,
//! no locks — safe to drive straight from an audio callback.
//!
//! Handles voice allocation & stealing, the two envelopes, unison, pitch bend,
//! the shared LFO settings, and note→pitch — so it is unit-testable without a
//! plugin host. Stereo out.

use crate::character::CharParams;
use crate::env::Adsr;
use crate::filter::FilterMode;
use crate::lfo::{LfoMode, LfoShape};
use crate::trig::exp2;
use crate::tuning::Tuning;
use crate::voice::{Voice, Waveform};

/// Max unison voices stacked on one MIDI note.
pub const MAX_UNISON: u32 = 8;

/// Equal-tempered MIDI note (may be fractional) → frequency Hz. A4 = 440.
#[inline]
pub fn midi_to_hz(note: f32) -> f64 {
    440.0 * exp2((note as f64 - 69.0) / 12.0)
}

#[derive(Clone, Copy)]
struct PolyVoice {
    core: Voice,
    amp: Adsr,
    filt_env: Adsr,
    note: u8,
    velocity: f32,
    age: u64,
}

impl PolyVoice {
    fn new(sample_rate: f64) -> Self {
        PolyVoice {
            core: Voice::new(sample_rate),
            amp: Adsr::new(),
            filt_env: Adsr::new(),
            note: 0,
            velocity: 0.0,
            age: 0,
        }
    }
}

/// Master decimator for the unified HQ bus (`docs/04_DSP_COMPONENTS.md` §1.7).
/// Decimates the whole stereo mix `2× → 1×` exactly once, instead of one
/// decimator per voice. One stereo instance lives on `PolySynth`; `Character`'s
/// own (unrelated) per-voice decimator backs the standalone `Voice` / C-ABI
/// HQ path.
///
/// 65-tap linear-phase half-band FIR (windowed-sinc, Kaiser β for an ~85 dB
/// design target, giving margin over the 80 dB spec). Only every second tap
/// around the centre is non-zero (the half-band property), so 17 unique
/// coefficients cover all 65. Measured (design script, not checked in):
/// passband flat to `< 0.33 dB` up to `0.9×` the output Nyquist; reaches
/// `−80` dB by `1.166×` the output Nyquist (worse only in that narrow
/// transition sliver — a 27-tap filter cannot deliver `−80` dB with any
/// usable transition width). Group delay `(65−1)/2 = 32` samples at
/// `2×` = **16 samples at `1×`**, exactly (unlike a 27-tap filter, whose
/// group delay does not divide evenly by 2).
struct HqBusDecimator {
    delay_l: [f32; Self::LEN],
    delay_r: [f32; Self::LEN],
}

impl HqBusDecimator {
    const LEN: usize = 65;
    const CENTER: usize = (Self::LEN - 1) / 2; // 32

    /// Samples of `1×`-rate latency this stage adds.
    const LATENCY: usize = Self::CENTER / 2; // 16, exact

    // Unique half-band coefficients: HB[0] is the centre tap, HB[k] (k>=1) is
    // the tap at offset ±(2k−1) from the centre. All even offsets are exactly
    // zero by the half-band construction and are skipped, not stored.
    const HB: [f32; 17] = [
        5.000_054e-1,
        3.170_889_4e-1,
        -1.024_807_2e-1,
        5.778_425e-2,
        -3.756_863_6e-2,
        2.573_189e-2,
        -1.790_901_2e-2,
        1.242_478_7e-2,
        -8.485_105e-3,
        5.647_418e-3,
        -3.628_984e-3,
        2.228_206_7e-3,
        -1.290_215_1e-3,
        6.914_371_8e-4,
        -3.326_000_3e-4,
        1.352_821_7e-4,
        -3.966_612e-5,
    ];

    fn new() -> Self {
        HqBusDecimator { delay_l: [0.0; Self::LEN], delay_r: [0.0; Self::LEN] }
    }

    fn reset(&mut self) {
        self.delay_l = [0.0; Self::LEN];
        self.delay_r = [0.0; Self::LEN];
    }

    #[inline]
    fn push_and_decimate(delay: &mut [f32; Self::LEN], a: f32, b: f32) -> f32 {
        delay.copy_within(2.., 0); // shift left by 2 (drop the two oldest)
        delay[Self::LEN - 2] = a;
        delay[Self::LEN - 1] = b;
        let mut acc = Self::HB[0] * delay[Self::CENTER];
        let mut k = 1usize;
        while k < Self::HB.len() {
            let off = 2 * k - 1;
            acc += Self::HB[k] * (delay[Self::CENTER - off] + delay[Self::CENTER + off]);
            k += 1;
        }
        acc
    }

    /// Push one `2×`-rate stereo pair, return one decimated `1×` stereo frame.
    #[inline]
    fn process(&mut self, lo: [f32; 2], hi: [f32; 2]) -> [f32; 2] {
        [
            Self::push_and_decimate(&mut self.delay_l, lo[0], hi[0]),
            Self::push_and_decimate(&mut self.delay_r, lo[1], hi[1]),
        ]
    }
}

/// Polyphonic synth with `VOICES` voices.
pub struct PolySynth<const VOICES: usize> {
    voices: [PolyVoice; VOICES],
    sample_rate: f64,
    rolloff: f64,
    gain: f64,

    amp_a: f64,
    amp_d: f64,
    amp_s: f64,
    amp_r: f64,

    character: CharParams,
    fm_ratio: f64,
    fm_index: f64,
    feedback: f64,
    free_running: bool,

    // filter
    filter_mode: FilterMode,
    filter_cutoff: f64,
    filter_res: f64,
    filter_env: f64,
    fenv_a: f64,
    fenv_d: f64,
    fenv_s: f64,
    fenv_r: f64,

    // unison
    unison_count: u32,
    unison_detune: f64, // cents, spread ±
    unison_spread: f64, // 0..1 stereo
    unison_drift: f64,  // 0..1 → per-voice slow phase-drift depth (breathing)

    // modulation
    bend_ratio: f64, // pitch-bend, 2^(st/12)
    lfo_rate: f64,
    lfo_shape: LfoShape,
    lfo_mode: LfoMode,
    lfo_to_rolloff: f64,
    lfo_to_pitch: f64,
    lfo_to_cutoff: f64,
    lfo_to_fm: f64,

    hq: bool,
    hq_decim: HqBusDecimator,
    waveform: Waveform,
    partial_limit: f32,
    formant: f64,

    // Note→frequency map. `tuning_default` is a cheap gate: while it is `true`
    // (the default), `note_hz` short-circuits to `midi_to_hz`, so every render
    // of a 12-TET / A=440 patch is bit-for-bit what it was before tuning
    // existed. Any `set_tuning` clears it; `set_tuning_equal` restores it.
    tuning: Tuning,
    tuning_default: bool,

    // Per-note brightness expression (MPE timbre / CC74, poly & channel
    // pressure). `bright_depth` is how far full expression tilts `rolloff`;
    // `note_bright[k]` is the per-key component the host combined from that
    // note's MPE timbre + poly pressure; `chan_bright` is the channel-pressure
    // component shared by every sounding note. The effective per-voice offset
    // is `bright_depth · clamp(note_bright[note] + chan_bright, −1, 1)`.
    // `bright_depth == 0.0` (the default) leaves every voice at `expr = 0.0`,
    // i.e. bit-identical to no expression.
    bright_depth: f64,
    chan_bright: f32,
    note_bright: [f32; 128],

    counter: u64,
}

impl<const VOICES: usize> PolySynth<VOICES> {
    /// Latency, in samples, added by [`PolySynth::set_hq`] `true` — the
    /// unified HQ bus's single master decimator. Independent of (and, when
    /// driven through `PolySynth`, supersedes) [`crate::Voice::HQ_LATENCY`],
    /// which only applies to a standalone `Voice`.
    pub const HQ_LATENCY: usize = HqBusDecimator::LATENCY;

    /// Create. Out-of-range / non-finite `sample_rate` is clamped — see
    /// [`PolySynth::new_checked`] to also learn what happened.
    pub fn new(sample_rate: f64) -> Self {
        Self::new_checked(sample_rate).0
    }

    /// Like [`PolySynth::new`], and reports whether `sample_rate` was accepted,
    /// clamped to `[8000, 768000]`, or (if non-finite) defaulted.
    pub fn new_checked(sample_rate: f64) -> (Self, crate::SampleRateStatus) {
        let (sr, status) = crate::validate_sample_rate(sample_rate);
        let s = PolySynth {
            voices: core::array::from_fn(|_| PolyVoice::new(sr)),
            sample_rate: sr,
            rolloff: 0.5,
            gain: 0.3,
            amp_a: 0.005,
            amp_d: 0.001,
            amp_s: 1.0,
            amp_r: 0.18,
            character: CharParams::CLEAN,
            fm_ratio: 1.0,
            fm_index: 0.0,
            feedback: 0.0,
            free_running: false,
            filter_mode: FilterMode::Bypass,
            filter_cutoff: 12_000.0,
            filter_res: 0.0,
            filter_env: 0.0,
            fenv_a: 0.005,
            fenv_d: 0.20,
            fenv_s: 0.0,
            fenv_r: 0.30,
            unison_count: 1,
            unison_detune: 12.0,
            unison_spread: 0.6,
            unison_drift: 0.0,
            bend_ratio: 1.0,
            lfo_rate: 5.0,
            lfo_shape: LfoShape::Sine,
            lfo_mode: LfoMode::Retrigger,
            lfo_to_rolloff: 0.0,
            lfo_to_pitch: 0.0,
            lfo_to_cutoff: 0.0,
            lfo_to_fm: 0.0,
            hq: false,
            hq_decim: HqBusDecimator::new(),
            waveform: Waveform::Geometric,
            partial_limit: crate::voice::MAX_PARTIALS as f32,
            formant: 0.0,
            tuning: Tuning::EQUAL_440,
            tuning_default: true,
            bright_depth: 0.0,
            chan_bright: 0.0,
            note_bright: [0.0; 128],
            counter: 0,
        };
        (s, status)
    }

    /// Rebuild at a new sample rate (host `initialize`). Silences all voices
    /// and reports whether the rate was accepted, clamped, or defaulted — the
    /// host should stop feeding audio (or pick a supported rate) on anything
    /// but [`crate::SampleRateStatus::Ok`].
    pub fn set_sample_rate(&mut self, sample_rate: f64) -> crate::SampleRateStatus {
        let (s, status) = PolySynth::new_checked(sample_rate);
        *self = s;
        status
    }

    /// The (validated) sample rate this synth runs at.
    #[inline]
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    pub fn set_rolloff(&mut self, r: f64) {
        self.rolloff = r;
        for v in &mut self.voices {
            v.core.set_rolloff(r);
        }
    }

    pub fn set_gain(&mut self, g: f64) {
        // `NaN < 0.0` is false, so a bare comparison lets NaN straight through
        // — this is the master gain, so a latched NaN here would poison the
        // *entire* mix. `+∞` is just as bad: `mix * ∞` is `NaN` on any sample
        // where the summed mix is exactly `0.0`. And a huge *finite* `f64`
        // (e.g. `1e300`) overflows to `f32::INFINITY` in the `as f32` cast at
        // the mix, with the same result. Clamp to `[0, 64]` (+36 dB — far past
        // any real master; `soft_clip` bounds the output anyway).
        self.gain = if g.is_nan() || g < 0.0 {
            0.0
        } else if g > 64.0 {
            64.0
        } else {
            g
        };
    }

    pub fn set_character(&mut self, p: CharParams) {
        self.character = p;
        for v in &mut self.voices {
            v.core.set_character(p);
        }
    }

    pub fn set_fm(&mut self, ratio: f64, index: f64) {
        self.fm_ratio = ratio;
        self.fm_index = index;
        for v in &mut self.voices {
            v.core.set_fm(ratio, index);
        }
    }

    pub fn set_feedback(&mut self, fb: f64) {
        self.feedback = fb;
        for v in &mut self.voices {
            v.core.set_feedback(fb);
        }
    }

    /// `true` = analog-style free-running phase (no reset on note-on).
    pub fn set_free_running(&mut self, free: bool) {
        self.free_running = free;
        for v in &mut self.voices {
            v.core.set_free_running(free);
        }
    }

    /// Unified HQ bus (`docs/04_DSP_COMPONENTS.md` §1.7): every voice's
    /// oscillator → `Character` → `Svf` runs at `2×` the base rate, all voices
    /// sum at `2×`, the master saturator runs at `2×`, and the whole mix is
    /// decimated back to `1×` **once** — instead of each voice decimating
    /// independently. Adds [`PolySynth::HQ_LATENCY`] samples of latency
    /// (constant; the host reports it once and re-syncs when this toggles).
    /// `false` is bit-identical to leaving HQ off.
    ///
    /// **No-op when `hq` already matches the current state.** The unconditional
    /// `hq_decim.reset()` below wipes the master decimator's 65-tap history, so
    /// a caller that re-asserts the same value every block (as a plugin's
    /// `process` naturally does) would otherwise zero the delay line at every
    /// block boundary — a block-rate settling transient in the HQ output.
    pub fn set_hq(&mut self, hq: bool) {
        if hq == self.hq {
            return;
        }
        self.hq = hq;
        for v in &mut self.voices {
            // Kept in sync for anyone introspecting a voice directly, but
            // `render_sample`'s own internal HQ branch (gated on this flag)
            // is never reached while `PolySynth` drives the voice — the unified
            // bus below calls `render_hq_subsamples` instead.
            v.core.set_hq(hq);
            // Reconfigures the per-voice `Svf` for the `2×`/`1×` rate the bus
            // will actually drive it at.
            v.core.set_hq_bus_active(hq);
        }
        self.hq_decim.reset();
    }

    /// Oscillator waveform (Geometric / Saw / Triangle). See [`Voice::set_waveform`].
    pub fn set_waveform(&mut self, w: Waveform) {
        self.waveform = w;
        for v in &mut self.voices {
            v.core.set_waveform(w);
        }
    }

    /// Upper bound on the geometric oscillator's partial count, fractional,
    /// `[1.0, 2048.0]`. Lowering it darkens the tone without aliasing and at a
    /// flat cost; the fractional part gives a smooth (not stepped) sweep. See
    /// [`Voice::set_partial_limit`].
    pub fn set_partial_limit(&mut self, limit: f32) {
        self.partial_limit = if limit.is_nan() {
            crate::voice::MAX_PARTIALS as f32
        } else {
            limit.clamp(1.0, crate::voice::MAX_PARTIALS as f32)
        };
        for v in &mut self.voices {
            v.core.set_partial_limit(self.partial_limit);
        }
    }

    /// "Formant" — a resonant bump in the mid-partials of the geometric
    /// oscillator, `[0, 1]`. `0.0` (default) disables it (bit-identical). See
    /// [`Voice::set_formant`].
    pub fn set_formant(&mut self, f: f64) {
        self.formant = if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) };
        for v in &mut self.voices {
            v.core.set_formant(self.formant);
        }
    }

    /// Set the note→frequency map (microtuning). The scale takes effect on the
    /// **next** note-on; notes already sounding keep their pitch. Passing
    /// [`Tuning::EQUAL_440`] is equivalent to [`set_tuning_equal`] — it restores
    /// the bit-identical default path.
    ///
    /// [`set_tuning_equal`]: PolySynth::set_tuning_equal
    pub fn set_tuning(&mut self, tuning: Tuning) {
        self.tuning_default = tuning.is_equal_440();
        self.tuning = tuning;
    }

    /// Restore 12-tone equal temperament, A4 = 440 Hz — the default, and the
    /// engine's bit-identical fast path.
    pub fn set_tuning_equal(&mut self) {
        self.tuning = Tuning::EQUAL_440;
        self.tuning_default = true;
    }

    /// Frequency for a MIDI note under the current tuning. While the tuning is
    /// the default this is exactly [`crate::midi_to_hz`] (bit-for-bit).
    #[inline]
    fn note_hz(&self, note: u8) -> f64 {
        if self.tuning_default {
            midi_to_hz(note as f32)
        } else {
            self.tuning.hz(note)
        }
    }

    /// Depth of per-note brightness expression, in `rolloff` units at full
    /// deflection (a musical value is `~0.3`). `0.0` disables it entirely —
    /// every voice stays at `expr = 0.0`, bit-identical to no expression. See
    /// [`Voice::set_expr_brightness`].
    pub fn set_brightness_depth(&mut self, depth: f64) {
        self.bright_depth = if depth.is_nan() {
            0.0
        } else {
            depth.clamp(-0.9, 0.9)
        };
        self.refresh_expr();
    }

    /// Per-note brightness expression for `note`, from the host's combined MPE
    /// timbre / CC74 + poly-pressure value (`raw`, nominally `[-1, 1]`). Fans
    /// out to every voice currently playing that note. Out-of-range `note`
    /// (e.g. CLAP's wildcard key, which arrives as `255`) is ignored.
    pub fn set_note_brightness(&mut self, note: u8, raw: f32) {
        let idx = note as usize;
        if idx >= 128 {
            return;
        }
        self.note_bright[idx] = raw;
        let off = self.expr_offset(raw);
        for v in &mut self.voices {
            if v.note == note && v.amp.is_active() {
                v.core.set_expr_brightness(off);
            }
        }
    }

    /// Channel-pressure contribution to brightness expression (mono aftertouch,
    /// or MPE channel pressure), `raw` nominally `[-1, 1]`. Shared by every
    /// sounding note and summed with each note's own
    /// [`PolySynth::set_note_brightness`] value.
    pub fn set_channel_brightness(&mut self, raw: f32) {
        self.chan_bright = if raw.is_nan() { 0.0 } else { raw };
        self.refresh_expr();
    }

    /// Effective per-voice `rolloff` offset for a given per-key raw value,
    /// folding in the shared channel-pressure term and the depth.
    #[inline]
    fn expr_offset(&self, note_raw: f32) -> f64 {
        let sum = note_raw + self.chan_bright;
        let clamped = if sum.is_nan() {
            0.0
        } else {
            sum.clamp(-1.0, 1.0)
        };
        self.bright_depth * clamped as f64
    }

    /// Re-push the brightness expression offset to every active voice — after a
    /// depth or channel-pressure change, which affect all notes at once.
    fn refresh_expr(&mut self) {
        let depth = self.bright_depth;
        let chan = self.chan_bright;
        let nb = self.note_bright;
        for v in &mut self.voices {
            if v.amp.is_active() {
                let note_raw = nb.get(v.note as usize).copied().unwrap_or(0.0);
                let sum = note_raw + chan;
                let clamped = if sum.is_nan() { 0.0 } else { sum.clamp(-1.0, 1.0) };
                v.core.set_expr_brightness(depth * clamped as f64);
            }
        }
    }

    /// Unison: `count` (1..8) detuned + stereo-spread voices per note, plus
    /// `drift` (0..1) — a slow independent per-voice phase drift so the stacked
    /// image *breathes* instead of sitting still. `drift` applies from the next
    /// note-on.
    pub fn set_unison(&mut self, count: u32, detune_cents: f64, spread: f64, drift: f64) {
        // `f64::clamp` returns NaN when `self` is NaN (it only rejects NaN
        // bounds). A NaN `spread` then flows into the pan gain; a NaN `drift`
        // disables drift (`NaN > 0.0` is false) but is still stored — reject
        // all three at the door. `detune_cents` is bounded generously (±4 oct)
        // so a huge value can't drive `exp2` to `∞` before the freq clamp.
        self.unison_count = count.clamp(1, MAX_UNISON);
        self.unison_detune = nan_clamp(detune_cents, -4800.0, 4800.0);
        self.unison_spread = nan_clamp(spread, 0.0, 1.0);
        self.unison_drift = nan_clamp(drift, 0.0, 1.0);
    }

    /// Pitch bend in semitones (applied to every sounding voice, smoothed).
    pub fn set_pitch_bend(&mut self, semitones: f64) {
        self.bend_ratio = exp2(semitones / 12.0);
        for v in &mut self.voices {
            v.core.set_pitch_bend(self.bend_ratio);
        }
    }

    /// Shared LFO: rate, shape, key-sync `mode`, and routing depth to
    /// brightness (`to_rolloff`, ±), vibrato (`to_pitch_cents`), filter cutoff
    /// (`to_cutoff_oct`, ±) and FM index (`to_fm`, ±).
    #[allow(clippy::too_many_arguments)]
    pub fn set_lfo(
        &mut self,
        rate_hz: f64,
        shape: LfoShape,
        mode: LfoMode,
        to_rolloff: f64,
        to_pitch_cents: f64,
        to_cutoff_oct: f64,
        to_fm: f64,
    ) {
        self.lfo_rate = rate_hz;
        self.lfo_shape = shape;
        self.lfo_mode = mode;
        self.lfo_to_rolloff = to_rolloff;
        self.lfo_to_pitch = to_pitch_cents;
        self.lfo_to_cutoff = to_cutoff_oct;
        self.lfo_to_fm = to_fm;
        for v in &mut self.voices {
            v.core.set_lfo(rate_hz, shape);
            v.core.set_lfo_mode(mode);
            v.core.set_lfo_targets(to_rolloff, to_pitch_cents, to_cutoff_oct, to_fm);
        }
    }

    pub fn set_envelope(&mut self, attack_s: f64, release_s: f64) {
        self.set_amp_adsr(attack_s, 0.0005, 1.0, release_s);
    }

    pub fn set_amp_adsr(&mut self, attack_s: f64, decay_s: f64, sustain: f64, release_s: f64) {
        self.amp_a = attack_s;
        self.amp_d = decay_s;
        self.amp_s = sustain;
        self.amp_r = release_s;
        for v in &mut self.voices {
            if v.amp.is_active() {
                v.amp.set(self.sample_rate, attack_s, decay_s, sustain, release_s);
            }
        }
    }

    pub fn set_filter(
        &mut self,
        mode: FilterMode,
        cutoff_hz: f64,
        resonance: f64,
        env_octaves: f64,
    ) {
        self.filter_mode = mode;
        self.filter_cutoff = cutoff_hz;
        self.filter_res = resonance;
        self.filter_env = env_octaves;
        for v in &mut self.voices {
            v.core.set_filter_mode(mode);
            v.core.set_filter_resonance(resonance);
            if env_octaves == 0.0 {
                v.core.set_filter_cutoff(cutoff_hz);
            }
        }
    }

    pub fn set_filter_envelope(
        &mut self,
        attack_s: f64,
        decay_s: f64,
        sustain: f64,
        release_s: f64,
    ) {
        self.fenv_a = attack_s;
        self.fenv_d = decay_s;
        self.fenv_s = sustain;
        self.fenv_r = release_s;
        for v in &mut self.voices {
            if v.filt_env.is_active() {
                v.filt_env
                    .set(self.sample_rate, attack_s, decay_s, sustain, release_s);
            }
        }
    }

    /// `true` iff `note` sounds something under the current tuning. Always
    /// `true` on the default (12-TET) path; a Scala `.kbm` keyboard map can
    /// mark keys "dead" (see [`Tuning::is_mapped`]), and [`note_on`] ignores
    /// those. A host GUI can call this to grey out dead keys.
    ///
    /// [`note_on`]: PolySynth::note_on
    #[inline]
    pub fn note_is_mapped(&self, note: u8) -> bool {
        self.tuning_default || self.tuning.is_mapped(note)
    }

    /// MIDI note-on. Stacks `unison_count` detuned, stereo-spread voices.
    pub fn note_on(&mut self, note: u8, velocity: f32) {
        // A dead key under a `.kbm` keyboard map sounds nothing. Guarded by
        // `tuning_default` so the 12-TET path is untouched, bit-for-bit.
        if !self.tuning_default && !self.tuning.is_mapped(note) {
            return;
        }
        // A fresh press starts from neutral per-note expression; the host
        // re-sends MPE timbre / pressure right after note-on if it has any.
        if let Some(slot) = self.note_bright.get_mut(note as usize) {
            *slot = 0.0;
        }
        let n = self.unison_count.clamp(1, MAX_UNISON);
        // 1/√n unison make-up gain (no `f32::sqrt` in no_std)
        const INV_SQRT: [f32; 9] = [
            1.0,
            1.0,
            core::f32::consts::FRAC_1_SQRT_2,
            0.577_350_3,
            0.5,
            0.447_213_6,
            0.408_248_3,
            0.377_964_5,
            0.353_553_4,
        ];
        let vscale = INV_SQRT[n as usize];
        for i in 0..n {
            let (det_cents, pan) = if n == 1 {
                (0.0, 0.0)
            } else {
                let x = 2.0 * i as f64 / (n as f64 - 1.0) - 1.0; // −1..1
                (self.unison_detune * x, self.unison_spread * x)
            };
            self.trigger_one(note, velocity * vscale, det_cents, pan, i, n);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn trigger_one(
        &mut self,
        note: u8,
        velocity: f32,
        detune_cents: f64,
        pan: f64,
        idx: u32,
        count: u32,
    ) {
        let vi = self.pick_voice();
        let sr = self.sample_rate;
        let hz = self.note_hz(note) * exp2(detune_cents / 1200.0);
        let expr_off =
            self.expr_offset(self.note_bright.get(note as usize).copied().unwrap_or(0.0));

        let v = &mut self.voices[vi];
        v.core.set_frequency(hz);
        v.core.set_rolloff(self.rolloff);
        v.core.set_gain(1.0);
        v.core.set_pan(pan);
        v.core.set_free_running(self.free_running);
        v.core.set_hq(self.hq);
        v.core.set_waveform(self.waveform);
        v.core.set_partial_limit(self.partial_limit);
        v.core.set_formant(self.formant);
        v.core.set_expr_brightness(expr_off);
        v.core.set_pitch_bend(self.bend_ratio);
        v.core.set_character(self.character);
        v.core.set_fm(self.fm_ratio, self.fm_index);
        v.core.set_feedback(self.feedback);
        v.core.set_filter_mode(self.filter_mode);
        v.core.set_filter_resonance(self.filter_res);
        v.core.set_filter_cutoff(self.filter_cutoff);
        v.core.set_lfo(self.lfo_rate, self.lfo_shape);
        v.core.set_lfo_mode(self.lfo_mode);
        v.core.set_lfo_targets(
            self.lfo_to_rolloff,
            self.lfo_to_pitch,
            self.lfo_to_cutoff,
            self.lfo_to_fm,
        );
        v.core.reset();

        // decorrelate stacked unison voices
        if count > 1 {
            let frac = idx as f64 / count as f64;
            if !self.free_running {
                v.core.set_start_phase(frac);
            }
            v.core.set_lfo_phase(frac);

            // slow phase drift: a per-voice rate around ~0.12 Hz (so the
            // stack never re-locks), depth scaled by the `drift` control,
            // start phase golden-ratio-spread so voices breathe out of sync.
            if self.unison_drift > 0.0 {
                let rate = 0.12 * (0.55 + 0.9 * frac);
                v.core.set_unison_drift(rate, self.unison_drift * 0.05);
                v.core.set_unison_drift_phase(idx as f64 * 0.618_034);
            } else {
                v.core.set_unison_drift(0.0, 0.0);
            }
        } else {
            v.core.set_unison_drift(0.0, 0.0);
        }

        v.amp.set(sr, self.amp_a, self.amp_d, self.amp_s, self.amp_r);
        v.amp.trigger();
        v.filt_env
            .set(sr, self.fenv_a, self.fenv_d, self.fenv_s, self.fenv_r);
        v.filt_env.trigger();

        v.note = note;
        v.velocity = clamp01(velocity).max(0.02);
        v.age = self.counter;
        self.counter += 1;
    }

    pub fn note_off(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.note == note && v.amp.is_active() && !v.amp.is_releasing() {
                v.amp.release();
                v.filt_env.release();
            }
        }
    }

    pub fn choke(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.note == note {
                v.amp.choke();
                v.filt_env.choke();
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.amp.release();
            v.filt_env.release();
        }
    }

    pub fn reset(&mut self) {
        for v in &mut self.voices {
            v.amp.choke();
            v.filt_env.choke();
        }
        self.counter = 0;
        self.chan_bright = 0.0;
        self.note_bright = [0.0; 128];
    }

    fn pick_voice(&self) -> usize {
        for (i, v) in self.voices.iter().enumerate() {
            if !v.amp.is_active() {
                return i;
            }
        }
        let mut best: Option<(usize, u64)> = None;
        for (i, v) in self.voices.iter().enumerate() {
            if v.amp.is_releasing() && best.is_none_or(|(_, a)| v.age < a) {
                best = Some((i, v.age));
            }
        }
        if let Some((i, _)) = best {
            return i;
        }
        let mut oldest = 0usize;
        let mut oldest_age = u64::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            if v.age < oldest_age {
                oldest_age = v.age;
                oldest = i;
            }
        }
        oldest
    }

    pub fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.amp.is_active()).count()
    }

    /// Fundamental frequency, in Hz, of the lowest note currently sounding (any
    /// non-idle voice), under the current tuning — or `0.0` if silent. For a
    /// spectrum display; not on the render path.
    pub fn lowest_sounding_hz(&self) -> f64 {
        let mut lo = f64::INFINITY;
        for v in &self.voices {
            if v.amp.is_active() {
                let hz = self.note_hz(v.note);
                if hz < lo {
                    lo = hz;
                }
            }
        }
        if lo.is_finite() {
            lo
        } else {
            0.0
        }
    }

    /// Live filter cutoff, in Hz, of the lowest sounding voice — the one whose
    /// fundamental [`Self::lowest_sounding_hz`] reports — with its filter
    /// envelope and LFO→cutoff folded in. `0.0` when silent. The editor draws
    /// the filter-response curve from this, so an envelope / LFO sweep animates
    /// instead of the curve sitting at the resting parameter value. Not on the
    /// render path.
    pub fn representative_cutoff(&self) -> f64 {
        let mut lo = f64::INFINITY;
        let mut cutoff = 0.0;
        for v in &self.voices {
            if v.amp.is_active() {
                let hz = self.note_hz(v.note);
                if hz < lo {
                    lo = hz;
                    cutoff = v.core.current_cutoff();
                }
            }
        }
        cutoff
    }

    /// Effective geometric rolloff `r` of the lowest sounding voice — smoothed
    /// brightness with LFO→brightness and per-note expression folded in. `0.0`
    /// when silent. The editor tilts the partial comb from this so an
    /// LFO→brightness sweep animates. Not on the render path.
    pub fn representative_rolloff(&self) -> f64 {
        let mut lo = f64::INFINITY;
        let mut r = 0.0;
        for v in &self.voices {
            if v.amp.is_active() {
                let hz = self.note_hz(v.note);
                if hz < lo {
                    lo = hz;
                    r = v.core.current_rolloff();
                }
            }
        }
        r
    }

    /// Render one stereo sample `[left, right]`.
    #[inline]
    pub fn render_sample(&mut self) -> [f32; 2] {
        if self.hq {
            return self.render_sample_hq_bus();
        }
        let mut ml = 0.0_f32;
        let mut mr = 0.0_f32;
        let env_mod = self.filter_env != 0.0;
        for v in &mut self.voices {
            if !v.amp.is_active() {
                continue;
            }
            let ae = v.amp.tick();
            let fe = v.filt_env.tick();
            if env_mod {
                let oct = self.filter_env * fe as f64;
                v.core.set_filter_cutoff(self.filter_cutoff * exp2(oct));
            }
            let [l, r] = v.core.render_sample();
            ml += l * ae * v.velocity;
            mr += r * ae * v.velocity;
        }
        [
            soft_clip(ml * self.gain as f32),
            soft_clip(mr * self.gain as f32),
        ]
    }

    /// The unified HQ bus: sum every voice's `2×`-rate subsample pair,
    /// master-saturate both, decimate once. See [`PolySynth::set_hq`].
    #[inline]
    fn render_sample_hq_bus(&mut self) -> [f32; 2] {
        let mut lo = [0.0_f32; 2];
        let mut hi = [0.0_f32; 2];
        let env_mod = self.filter_env != 0.0;
        for v in &mut self.voices {
            if !v.amp.is_active() {
                continue;
            }
            let ae = v.amp.tick();
            let fe = v.filt_env.tick();
            if env_mod {
                let oct = self.filter_env * fe as f64;
                v.core.set_filter_cutoff(self.filter_cutoff * exp2(oct));
            }
            let (vl, vh) = v.core.render_hq_subsamples();
            let g = ae * v.velocity;
            lo[0] += vl[0] * g;
            lo[1] += vl[1] * g;
            hi[0] += vh[0] * g;
            hi[1] += vh[1] * g;
        }
        let gain = self.gain as f32;
        let clipped_lo = [soft_clip(lo[0] * gain), soft_clip(lo[1] * gain)];
        let clipped_hi = [soft_clip(hi[0] * gain), soft_clip(hi[1] * gain)];
        self.hq_decim.process(clipped_lo, clipped_hi)
    }

    /// Render a stereo block into `left` / `right` (up to the shorter length).
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

/// Smooth ℝ → (−1, 1) saturator (Padé approximation of `tanh`).
///
/// A last-resort NaN backstop for the master mix: `NaN` folds to `0.0` rather
/// than through (`NaN > 3.0` and `NaN < -3.0` are both false, so a bare clamp
/// chain would pass it). The per-setter clamps upstream are the real defence —
/// this only stops one regressed setter from poisoning the whole output
/// silently. Finite inputs are unaffected (bit-identical).
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    let x = if x.is_nan() {
        0.0
    } else if x > 3.0 {
        3.0
    } else if x < -3.0 {
        -3.0
    } else {
        x
    };
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// NaN-safe `f64` clamp: `NaN` (and `-∞`) resolve to `lo`, `+∞` to `hi`.
/// `f64::clamp` instead returns `NaN` unchanged when `self` is `NaN`.
#[inline(always)]
fn nan_clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x.is_nan() || x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

#[inline(always)]
fn clamp01(x: f32) -> f32 {
    // See env::clamp01 — same NaN-passthrough fix. This particular call site
    // (`note_on`'s velocity) happens to be masked today by a chained
    // `.max(0.02)` (`f32::max` returns the non-NaN operand), but fixing the
    // shared helper directly is the robust choice, not relying on that.
    if x.is_nan() || x < 0.0 {
        0.0
    } else if x > 1.0 {
        1.0
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::FilterMode;

    fn peak(s: &mut PolySynth<8>, samples: usize) -> f32 {
        let mut p = 0.0_f32;
        for _ in 0..samples {
            let [l, r] = s.render_sample();
            assert!(l.is_finite() && r.is_finite());
            p = p.max(l.abs()).max(r.abs());
        }
        p
    }

    #[test]
    fn midi_pitch_reference() {
        assert!((midi_to_hz(69.0) - 440.0).abs() < 0.05);
        assert!((midi_to_hz(60.0) - 261.6256).abs() < 0.1);
        assert!((midi_to_hz(33.0) - 55.0).abs() < 0.02);
    }

    #[test]
    fn note_produces_bounded_sound_then_silence() {
        let mut s: PolySynth<8> = PolySynth::new(48_000.0);
        s.set_gain(1.0);
        s.set_envelope(0.005, 0.05);
        s.note_on(60, 1.0);
        assert!(peak(&mut s, 4800) > 0.05);

        s.note_off(60);
        for _ in 0..24_000 {
            s.render_sample();
        }
        assert_eq!(s.active_voice_count(), 0);
        let tail: f32 = (0..1000)
            .map(|_| {
                let [l, r] = s.render_sample();
                l.abs().max(r.abs())
            })
            .fold(0.0, f32::max);
        assert!(tail < 1e-4, "tail not silent: {tail}");
    }

    #[test]
    fn lowest_sounding_hz_tracks_the_bottom_note() {
        let mut s: PolySynth<8> = PolySynth::new(48_000.0);
        s.set_envelope(0.005, 0.05);
        assert_eq!(s.lowest_sounding_hz(), 0.0); // silent

        s.note_on(69, 1.0); // A4
        s.render_sample();
        assert!((s.lowest_sounding_hz() - 440.0).abs() < 0.1);

        s.note_on(57, 1.0); // A3 — now the lowest
        s.render_sample();
        assert!((s.lowest_sounding_hz() - 220.0).abs() < 0.1);

        s.note_on(76, 1.0); // higher, doesn't change the floor
        s.render_sample();
        assert!((s.lowest_sounding_hz() - 220.0).abs() < 0.1);

        s.all_notes_off();
        for _ in 0..48_000 {
            s.render_sample();
        }
        assert_eq!(s.lowest_sounding_hz(), 0.0);

        // it follows the tuning: A2 (MIDI 45) at A4 = 432 → 432·2⁻² = 108 Hz
        s.set_tuning(crate::Tuning::equal(12, 432.0, 69));
        s.note_on(45, 1.0);
        s.render_sample();
        assert!((s.lowest_sounding_hz() - 108.0).abs() < 0.2);
    }

    #[test]
    fn a_dead_key_under_a_kbm_map_sounds_nothing() {
        let sr = 48_000.0;
        let mut s: PolySynth<4> = PolySynth::new(sr);
        s.set_envelope(0.005, 0.1);

        // 12-EDO scale; a 2-key pattern where every other key is dead, anchored
        // on a live key (C4 = 60) so the map isn't rejected.
        let chromatic: [f64; 12] = core::array::from_fn(|k| k as f64 * 100.0);
        let map: [i8; 2] = [0, -1];
        s.set_tuning(crate::Tuning::from_kbm(
            &chromatic, 1200.0, &map, 2, 60, 1200.0, 60, 261.625_565,
        ));

        assert!(s.note_is_mapped(60) && !s.note_is_mapped(61));

        // a dead key: note_on is a no-op, the synth stays silent
        s.note_on(61, 1.0);
        for _ in 0..64 {
            s.render_sample();
        }
        assert_eq!(s.lowest_sounding_hz(), 0.0, "a dead key started a voice");

        // a live key still plays
        s.note_on(60, 1.0);
        s.render_sample();
        assert!(s.lowest_sounding_hz() > 0.0);

        // default path: every key is mapped again
        s.set_tuning_equal();
        assert!(s.note_is_mapped(61));
    }

    #[test]
    fn representative_cutoff_tracks_the_filter_envelope() {
        let sr = 48_000.0;
        let mut s: PolySynth<4> = PolySynth::new(sr);
        s.set_envelope(0.01, 0.2);
        assert_eq!(s.representative_cutoff(), 0.0, "silent → 0");

        // low-pass, cutoff 500 Hz, +4 octaves of filter envelope at the peak
        s.set_filter(FilterMode::Low, 500.0, 0.2, 4.0);
        s.set_filter_envelope(0.05, 0.30, 0.0, 0.10);
        s.note_on(57, 1.0); // A3
        for _ in 0..64 {
            s.render_sample();
        }
        let c0 = s.representative_cutoff();
        for _ in 0..2_000 {
            s.render_sample(); // ~40 ms — climbing toward the attack peak
        }
        let c1 = s.representative_cutoff();
        assert!(
            c1 > c0 * 1.5 && c1 > 1_500.0 && c1.is_finite() && c1 < 30_000.0,
            "cutoff did not rise with the envelope: {c0} -> {c1}"
        );

        s.all_notes_off();
        for _ in 0..(sr as usize) {
            s.render_sample();
        }
        assert_eq!(s.representative_cutoff(), 0.0, "silent again → 0");
    }

    #[test]
    fn representative_rolloff_moves_with_lfo_to_brightness() {
        let sr = 48_000.0;
        let mut s: PolySynth<4> = PolySynth::new(sr);
        s.set_envelope(0.005, 0.1);
        assert_eq!(s.representative_rolloff(), 0.0, "silent → 0");

        // steady brightness, no LFO → representative rolloff settles to the base
        s.set_rolloff(0.6);
        s.note_on(57, 1.0);
        for _ in 0..4_000 {
            s.render_sample();
        }
        let base = s.representative_rolloff();
        assert!((base - 0.6).abs() < 0.02, "base rolloff off: {base}");

        // a slow, deep LFO→brightness must swing it well away from the base
        s.set_lfo(2.0, LfoShape::Sine, LfoMode::FreeRun, 0.3, 0.0, 0.0, 0.0);
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for _ in 0..(sr as usize / 2) {
            s.render_sample();
            let r = s.representative_rolloff();
            lo = lo.min(r);
            hi = hi.max(r);
        }
        assert!(hi - lo > 0.2, "LFO→brightness did not move the rolloff: {lo}..{hi}");
        assert!(lo >= Voice::ROLLOFF_MIN && hi <= Voice::ROLLOFF_MAX, "out of range: {lo}..{hi}");

        s.all_notes_off();
        for _ in 0..(sr as usize) {
            s.render_sample();
        }
        assert_eq!(s.representative_rolloff(), 0.0, "silent again → 0");
    }

    #[test]
    fn voice_stealing_never_panics_or_clips() {
        let mut s: PolySynth<4> = PolySynth::new(44_100.0);
        s.set_gain(1.0);
        for n in 0..40u8 {
            s.note_on(40 + n, 0.9);
            for _ in 0..64 {
                let [l, r] = s.render_sample();
                assert!(l.is_finite() && r.is_finite() && l.abs() <= 1.001 && r.abs() <= 1.001);
            }
        }
        assert!(s.active_voice_count() <= 4);
    }

    #[test]
    fn unison_stacks_voices_and_spreads_stereo() {
        let mut s: PolySynth<8> = PolySynth::new(48_000.0);
        s.set_gain(1.0);
        s.set_unison(4, 15.0, 0.9, 0.0);
        s.note_on(57, 1.0);
        assert_eq!(s.active_voice_count(), 4, "unison did not stack 4 voices");

        // let pans settle, then confirm the image is wide: for a mono/collapsed
        // image L == R so the difference signal is silent; a wide unison image
        // has real energy in (L − R).
        for _ in 0..1200 {
            s.render_sample();
        }
        let (mut sum_e, mut diff_e) = (0.0f64, 0.0f64);
        for _ in 0..8000 {
            let [l, r] = s.render_sample();
            assert!(l.abs() <= 1.001 && r.abs() <= 1.001);
            let s2 = (l + r) as f64;
            let d2 = (l - r) as f64;
            sum_e += s2 * s2;
            diff_e += d2 * d2;
        }
        let width = diff_e / sum_e.max(1e-12);
        assert!(width > 0.05, "unison stereo image collapsed (width {width:.4})");

        s.note_off(57);
        for _ in 0..40_000 {
            s.render_sample();
        }
        assert_eq!(s.active_voice_count(), 0, "unison voices not all released");
    }

    #[test]
    fn unison_drift_makes_the_image_breathe() {
        // Per-window stereo width `(L−R)²/(L+R)²`. Detune is kept tiny here so
        // the baseline (no drift) is a *static* phase-decorrelated comb — its
        // width barely moves. Turning on drift must set it in motion.
        let window_widths = |drift: f64| -> Vec<f64> {
            let mut s: PolySynth<8> = PolySynth::new(48_000.0);
            s.set_gain(1.0);
            s.set_unison(6, 0.0, 0.9, drift); // 0 detune → a static comb baseline
            s.note_on(45, 1.0);
            for _ in 0..4000 {
                s.render_sample();
            }
            (0..20)
                .map(|_| {
                    // side / (mid + side) energy — a stable [0,1] width measure
                    let (mut mid, mut side) = (0.0f64, 0.0f64);
                    for _ in 0..24_000 {
                        // 0.5 s windows
                        let [l, r] = s.render_sample();
                        let m = (l + r) as f64 * 0.5;
                        let d = (l - r) as f64 * 0.5;
                        mid += m * m;
                        side += d * d;
                    }
                    side / (mid + side).max(1e-12)
                })
                .collect()
        };

        let range = |w: &[f64]| {
            let (mn, mx) = w
                .iter()
                .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
            mx - mn
        };
        let mean = |w: &[f64]| w.iter().sum::<f64>() / w.len() as f64;

        let still = window_widths(0.0);
        let breathing = window_widths(0.7);

        assert!(range(&still) < 0.06, "static unison width wandered: {:.4}", range(&still));
        assert!(
            range(&breathing) > range(&still) * 3.0 && range(&breathing) > 0.05,
            "drift did not modulate the width: still range {:.4} vs breathing range {:.4}",
            range(&still),
            range(&breathing)
        );
        assert!(mean(&breathing) > 0.05, "breathing unison collapsed: {:.4}", mean(&breathing));
    }

    #[test]
    fn pitch_bend_shifts_all_voices() {
        let mut s: PolySynth<8> = PolySynth::new(48_000.0);
        s.set_gain(1.0);
        s.set_unison(3, 10.0, 0.5, 0.4);
        s.note_on(60, 1.0);
        s.set_pitch_bend(2.0); // +2 semitones
        assert!(peak(&mut s, 24_000) <= 1.5);
        s.set_pitch_bend(-12.0); // −1 octave
        assert!(peak(&mut s, 24_000) <= 1.5);
    }

    #[test]
    fn hq_mode_stays_bounded_and_adds_latency() {
        let mut s: PolySynth<8> = PolySynth::new(48_000.0);
        s.set_gain(1.0);
        s.set_rolloff(0.95);
        s.set_character(CharParams {
            drive: 0.7,
            fold: 0.6,
            ..CharParams::CLEAN
        });
        s.set_hq(true);
        s.note_on(64, 1.0);
        assert!(peak(&mut s, 48_000) <= 1.5);
    }

    #[test]
    fn zero_latency_hq_off_and_exactly_16_samples_hq_on() {
        // The compile-time promise.
        assert_eq!(PolySynth::<8>::HQ_LATENCY, 16, "HQ bus latency is not exactly 16");
        assert_eq!(crate::Voice::HQ_LATENCY, 3, "standalone Voice HQ latency is not 3");

        let cfg = |s: &mut PolySynth<8>| {
            s.set_gain(0.9);
            s.set_rolloff(0.9);
            s.set_free_running(true);
            s.set_amp_adsr(0.002, 0.02, 1.0, 0.05);
        };

        // HQ off is bit-identical whether or not HQ was ever toggled — proves
        // turning it off fully removes the path, no residual delay line.
        let render = |touch_hq: bool| -> Vec<u32> {
            let mut s: PolySynth<8> = PolySynth::new(48_000.0);
            cfg(&mut s);
            if touch_hq {
                s.set_hq(true);
                s.set_hq(false);
            }
            s.note_on(57, 1.0);
            (0..2_000).map(|_| s.render_sample()[0].to_bits()).collect()
        };
        assert_eq!(render(false), render(true), "toggling HQ off left a delay / path change");

        // HQ on shifts the output by exactly HQ_LATENCY samples: cross-correlate
        // a steady tone rendered both ways and the lag that lines them up is 16.
        let steady = |hq: bool| -> Vec<f64> {
            let mut s: PolySynth<8> = PolySynth::new(48_000.0);
            cfg(&mut s);
            s.set_hq(hq);
            s.note_on(57, 1.0);
            for _ in 0..3_000 {
                s.render_sample(); // let the amp env + filters settle
            }
            (0..2_048).map(|_| s.render_sample()[0] as f64).collect()
        };
        let off = steady(false);
        let on = steady(true);
        let (mut best_lag, mut best) = (0usize, f64::MIN);
        for lag in 0..40 {
            let c: f64 = (0..off.len() - 40).map(|i| off[i] * on[i + lag]).sum();
            if c > best {
                best = c;
                best_lag = lag;
            }
        }
        assert_eq!(best_lag, PolySynth::<8>::HQ_LATENCY, "measured HQ delay {best_lag}, want 16");
    }

    #[test]
    fn render_is_block_size_independent_bit_for_bit() {
        // "Freeze == realtime == yesterday's render": a scripted pass must hash
        // the same whether the host hands us the whole thing at once, 512-frame
        // blocks, 64-frame blocks, or one sample at a time. Events land at the
        // same *absolute* frame regardless of where the block boundaries fall.
        const N: usize = 3_000;
        type Evt = (usize, fn(&mut PolySynth<8>));
        let events: [Evt; 6] = [
            (0, |s| s.note_on(45, 0.9)),
            (411, |s| s.note_on(52, 0.6)),
            (900, |s| s.set_pitch_bend(2.0)),
            (1_337, |s| s.note_on(59, 1.0)),
            (1_800, |s| s.note_off(45)),
            (2_222, |s| s.note_off(52)),
        ];

        let configure = |s: &mut PolySynth<8>| {
            s.set_gain(0.8);
            s.set_rolloff(0.9);
            s.set_character(CharParams { drive: 0.5, fold: 0.3, ..CharParams::CLEAN });
            s.set_fm(1.5, 0.4);
            s.set_unison(3, 10.0, 0.6, 0.5);
            s.set_amp_adsr(0.004, 0.06, 0.5, 0.1);
            s.set_filter(FilterMode::Low, 1_800.0, 0.6, 2.0);
            s.set_lfo(6.0, LfoShape::Sine, LfoMode::FreeRun, 0.2, 12.0, 1.0, 0.3);
        };

        // block-rendered with a given chunk size, events applied at boundaries
        let render_blocked = |chunk: usize| -> Vec<(u32, u32)> {
            let mut s: PolySynth<8> = PolySynth::new(48_000.0);
            configure(&mut s);
            let mut out = Vec::with_capacity(N);
            let (mut l, mut r) = (vec![0.0f32; chunk], vec![0.0f32; chunk]);
            let mut done = 0;
            while done < N {
                for &(at, f) in &events {
                    if at == done {
                        f(&mut s);
                    }
                }
                // …but events strictly inside this block still have to fire at
                // their exact frame, so split the block at every event.
                let mut next_evt = N;
                for &(at, _) in &events {
                    if at > done && at < next_evt {
                        next_evt = at;
                    }
                }
                let this = chunk.min(next_evt - done).min(N - done);
                s.render_block(&mut l[..this], &mut r[..this]);
                for i in 0..this {
                    out.push((l[i].to_bits(), r[i].to_bits()));
                }
                done += this;
            }
            out
        };

        // sample-by-sample reference
        let mut sref: PolySynth<8> = PolySynth::new(48_000.0);
        configure(&mut sref);
        let mut reference = Vec::with_capacity(N);
        for i in 0..N {
            for &(at, f) in &events {
                if at == i {
                    f(&mut sref);
                }
            }
            let [l, r] = sref.render_sample();
            reference.push((l.to_bits(), r.to_bits()));
        }

        for chunk in [7, 64, 256, 512, N] {
            assert_eq!(
                render_blocked(chunk),
                reference,
                "render_block(chunk = {chunk}) diverged from the render_sample loop"
            );
        }
        // and the reference actually made sound
        assert!(reference.iter().any(|&(l, _)| f32::from_bits(l).abs() > 0.02));
    }

    /// `|X(f)|` of `x` at absolute frequency `f` Hz, normalised by `N` — a
    /// single-bin Goertzel-style DFT (no FFT dependency), matching
    /// `tests/spectrum.rs::dft_bin_mag`.
    fn dft_bin_mag(x: &[f32], fs: f64, f: f64) -> f64 {
        let w = core::f64::consts::TAU * f / fs;
        let (mut re, mut im) = (0.0_f64, 0.0_f64);
        for (n, &s) in x.iter().enumerate() {
            let ph = w * n as f64;
            re += s as f64 * ph.cos();
            im -= s as f64 * ph.sin();
        }
        let nrm = x.len() as f64;
        ((re * re + im * im).sqrt() / nrm) * 2.0
    }

    #[test]
    fn hq_bus_master_clip_stays_under_75db_alias_floor() {
        // With HQ on and the master driven ~6 dB into soft_clip,
        // energy at frequencies that are *not* a harmonic of the fundamental
        // (i.e. could only be there via aliasing, since a single near-pure
        // tone through a symmetric saturator produces nothing else) must sit
        // <= -75 dB below the fundamental.
        let sr = 48_000.0;
        let target_f0 = 733.0_f64; // arbitrary, no simple rational relation to `sr`
        let mut s: PolySynth<4> = PolySynth::new(sr);
        s.set_rolloff(0.001); // near-pure sine: harmonics below are (almost) all from the clip, not the osc
        s.set_gain(6.0); // pumps the pre-clip peak well past 1.0 (~6 dB+ overload)
        s.set_hq(true);
        s.set_envelope(0.0005, 0.05);
        let note = (12.0 * (target_f0 / 440.0).log2() + 69.0).round() as u8;
        s.note_on(note, 1.0);
        let f0 = midi_to_hz(note as f32); // the note's actual quantised frequency

        // A long, unwindowed capture: the single-bin DTFT probe below needs
        // enough samples that a strong harmonic's own sidelobe leakage has
        // decayed well past -75 dB by the time it reaches a probe frequency
        // (rectangular-window sidelobes fall off only as ~1/Δf) — otherwise
        // "alias energy" readings are actually leakage from the fundamental,
        // not aliasing. ~1M samples keeps that leakage below -90 dB at the
        // exclusion margin used below.
        let n = 1 << 20;
        let mut buf = vec![0.0_f32; n];
        for x in buf.iter_mut() {
            *x = s.render_sample()[0];
        }

        let peak_mag = buf.iter().fold(0.0_f32, |m, &x| m.max(x.abs()));
        assert!(peak_mag.is_finite() && peak_mag > 0.5, "clip never engaged: peak={peak_mag}");

        let e_fund = dft_bin_mag(&buf, sr, f0);
        assert!(e_fund > 1e-3, "fundamental missing: {e_fund:e}");

        // sanity: odd harmonics of a symmetrically-clipped sine must be present
        let e_h3 = dft_bin_mag(&buf, sr, f0 * 3.0);
        assert!(e_h3 > 1e-4, "3rd harmonic (expected from clipping) missing: {e_h3:e}");

        // probe a spread of frequencies that are *not* close to any harmonic
        // of f0, up to just under the output Nyquist.
        let mut worst_db = f64::NEG_INFINITY;
        let mut f = f0 * 1.37; // deliberately off-harmonic offset
        while f < sr * 0.48 {
            // skip anything that landed suspiciously close to a real harmonic
            let nearest_harmonic = (f / f0).round() * f0;
            if (f - nearest_harmonic).abs() > f0 * 0.15 {
                let e = dft_bin_mag(&buf, sr, f);
                let db = 20.0 * (e / e_fund).log10();
                if db > worst_db {
                    worst_db = db;
                }
            }
            f += f0 * 1.9; // step by a non-harmonic-related amount
        }
        assert!(
            worst_db <= -75.0,
            "off-harmonic (alias) energy only {worst_db:.1} dB below the fundamental"
        );
    }

    #[test]
    fn lfo_modulation_stays_bounded() {
        let mut s: PolySynth<8> = PolySynth::new(48_000.0);
        s.set_gain(1.0);
        // every routing target at once, plus a filter + FM to actually hit
        s.set_fm(2.0, 0.4);
        s.set_filter(FilterMode::Low, 4_000.0, 0.7, 0.0);
        s.set_lfo(6.0, LfoShape::Triangle, LfoMode::Retrigger, 0.35, 30.0, 3.0, 2.5);
        s.note_on(52, 1.0);
        assert!(peak(&mut s, 96_000) <= 1.5);
    }

    #[test]
    fn extreme_cutoff_modulation_never_destabilises_the_filter() {
        // Stack every cutoff modulator at its extreme at once: base cutoff near
        // Nyquist, filter envelope swinging +6 oct, LFO→cutoff at its ±8 oct
        // clamp, and resonance near the low end (the SVF's `k` is largest
        // there — the one regime where a hypothetically unclamped
        // `tan_turns_fast` past its 0.25-turn pole, which returns a *negative*
        // g, could destabilise `1 + g·(g+k)`). Every write to the filter's
        // cutoff goes through `Svf::set_cutoff`, which clamps to
        // `[20, 0.45·fs]` before it ever reaches `recompute_g`, so this should
        // never come close — but it's the weakest-margin composite, so it's
        // worth locking in.
        let mut s: PolySynth<8> = PolySynth::new(48_000.0);
        s.set_gain(1.0);
        s.set_filter(FilterMode::Low, 12_000.0, 0.02, 6.0);
        s.set_filter_envelope(0.0005, 0.02, 1.0, 0.01);
        s.set_lfo(11.0, LfoShape::Saw, LfoMode::FreeRun, 0.0, 0.0, 8.0, 0.0);
        s.note_on(96, 1.0);
        assert!(peak(&mut s, 48_000) < 20.0, "blew up under extreme cutoff modulation");
    }

    #[test]
    fn filter_envelope_is_independent_of_amp_envelope() {
        let sr = 48_000.0;
        let mut s: PolySynth<8> = PolySynth::new(sr);
        s.set_gain(1.0);
        s.set_rolloff(0.97);
        s.set_amp_adsr(0.002, 0.001, 1.0, 2.0);
        s.set_filter(FilterMode::Low, 250.0, 0.6, 4.0);
        s.set_filter_envelope(0.003, 0.12, 0.0, 0.1);

        let hf = |s: &mut PolySynth<8>, skip: usize, take: usize| -> f64 {
            for _ in 0..skip {
                s.render_sample();
            }
            let mut acc = 0.0;
            let mut prev = 0.0;
            for _ in 0..take {
                let x = s.render_sample()[0] as f64;
                acc += (x - prev).abs();
                prev = x;
            }
            acc / take as f64
        };

        s.note_on(52, 1.0);
        let open = hf(&mut s, 200, 4000);
        let closed = hf(&mut s, (sr * 0.35) as usize, 4000);
        assert_eq!(s.active_voice_count(), 1, "amp env died early");
        assert!(open > closed * 1.5, "filter env not independent: {open:.5} vs {closed:.5}");
    }

    #[test]
    fn set_partial_limit_darkens_the_whole_synth() {
        let sr = 48_000.0;
        let note = 40_u8; // ~82.4 Hz → the 24th partial (~1977 Hz) is well
        let hi_partial = 24.0; // above a 6-partial cap and loud at rolloff 0.97

        // |X(f)| of a rendered mono block at absolute frequency `f`.
        let bin_mag = |buf: &[f32], f: f64| -> f64 {
            let w = core::f64::consts::TAU * f / sr;
            let (mut re, mut im) = (0.0_f64, 0.0);
            for (n, &s) in buf.iter().enumerate() {
                re += s as f64 * (w * n as f64).cos();
                im -= s as f64 * (w * n as f64).sin();
            }
            (re * re + im * im).sqrt() / buf.len() as f64
        };
        let render = |limit: Option<f32>, set_before_note: bool| -> Vec<f32> {
            let mut s: PolySynth<8> = PolySynth::new(sr);
            s.set_gain(1.0);
            s.set_rolloff(0.97);
            if set_before_note {
                if let Some(l) = limit {
                    s.set_partial_limit(l);
                }
                s.note_on(note, 1.0);
            } else {
                s.note_on(note, 1.0);
                for _ in 0..400 {
                    s.render_sample();
                }
                if let Some(l) = limit {
                    s.set_partial_limit(l); // fan-out to the held voice
                }
            }
            for _ in 0..400 {
                s.render_sample();
            }
            (0..16384).map(|_| s.render_sample()[0]).collect()
        };

        let f_hi = midi_to_hz(note as f32) * hi_partial;

        // held note, limit applied mid-flight
        let full = bin_mag(&render(None, true), f_hi);
        let capped = bin_mag(&render(Some(6.0), false), f_hi);
        assert!(full > 5.0e-4, "24th partial missing at full range: {full:e}");
        assert!(
            capped < full * 0.05,
            "partial limit did not remove the 24th partial from a held note: {capped:e} vs {full:e}"
        );

        // note triggered after the limit is set (exercises `trigger_one`)
        let capped_fresh = bin_mag(&render(Some(6.0), true), f_hi);
        assert!(
            capped_fresh < full * 0.05,
            "a fresh note ignored the partial limit: {capped_fresh:e}"
        );
    }

    #[test]
    fn per_note_brightness_addresses_one_key_and_leaves_the_others_alone() {
        // The additive-unique claim, as a mechanism test: expression aimed at
        // one key changes only the voice(s) playing that key. A shared
        // per-voice filter cannot do this.
        let sr = 48_000.0;
        let bin = |buf: &[f32], f: f64| -> f64 {
            let w = core::f64::consts::TAU * f / sr;
            let (mut re, mut im) = (0.0_f64, 0.0);
            for (n, &s) in buf.iter().enumerate() {
                re += s as f64 * (w * n as f64).cos();
                im -= s as f64 * (w * n as f64).sin();
            }
            (re * re + im * im).sqrt() / buf.len() as f64
        };
        // Spectral tilt (8th partial / fundamental) of `note`, optionally with
        // a brightness value routed to key `bright_key` after the note sounds.
        // The ratio, not an absolute magnitude, so peak-normalisation rescaling
        // doesn't confound it.
        let hf_of = |note: u8, bright: Option<(u8, f32)>| -> f64 {
            let mut s: PolySynth<8> = PolySynth::new(sr);
            s.set_gain(1.0);
            s.set_rolloff(0.7);
            s.set_brightness_depth(0.4);
            s.note_on(note, 1.0);
            for _ in 0..400 {
                s.render_sample();
            }
            if let Some((k, b)) = bright {
                s.set_note_brightness(k, b);
            }
            for _ in 0..2500 {
                s.render_sample();
            }
            let buf: Vec<f32> = (0..16384).map(|_| s.render_sample()[0]).collect();
            let f0 = midi_to_hz(note as f32);
            bin(&buf, f0 * 8.0) / bin(&buf, f0)
        };

        let a = 41_u8;
        let b = 60_u8;
        let a_flat = hf_of(a, None);
        let a_bright = hf_of(a, Some((a, 1.0)));
        let b_flat = hf_of(b, None);
        let b_when_a_bright = hf_of(b, Some((a, 1.0))); // aimed at a's key while b plays

        assert!(
            a_bright > a_flat * 3.0,
            "brightness on key {a} didn't lift its own 8th partial: {a_flat:e} -> {a_bright:e}"
        );
        assert!(
            (b_when_a_bright - b_flat).abs() < b_flat * 0.02 + 1e-9,
            "key {b}'s spectrum moved when the expression was aimed at key {a}: \
             {b_flat:e} -> {b_when_a_bright:e}"
        );
    }

    #[test]
    fn brightness_depth_zero_leaves_the_synth_bit_identical() {
        let mut plain: PolySynth<4> = PolySynth::new(48_000.0);
        plain.set_gain(1.0);
        plain.note_on(57, 1.0);

        let mut expr: PolySynth<4> = PolySynth::new(48_000.0);
        expr.set_gain(1.0);
        expr.set_brightness_depth(0.0); // the disabling value
        expr.note_on(57, 1.0);
        expr.set_note_brightness(57, 1.0); // …so these must be no-ops
        expr.set_channel_brightness(0.8);

        for i in 0..8000 {
            assert_eq!(
                plain.render_sample(),
                expr.render_sample(),
                "brightness depth 0.0 changed the output at sample {i}"
            );
        }
    }

    #[test]
    fn formant_fans_out_to_every_voice_and_zero_is_bit_identical() {
        let sr = 48_000.0;
        let bin = |buf: &[f32], f: f64| -> f64 {
            let w = core::f64::consts::TAU * f / sr;
            let (mut re, mut im) = (0.0_f64, 0.0);
            for (n, &s) in buf.iter().enumerate() {
                re += s as f64 * (w * n as f64).cos();
                im -= s as f64 * (w * n as f64).sin();
            }
            (re * re + im * im).sqrt() / buf.len() as f64
        };
        let render = |formant: f64, before_note: bool| -> Vec<f32> {
            let mut s: PolySynth<8> = PolySynth::new(sr);
            s.set_gain(1.0);
            s.set_rolloff(0.4);
            if before_note {
                s.set_formant(formant);
                s.note_on(45, 1.0);
            } else {
                s.note_on(45, 1.0);
                for _ in 0..400 {
                    s.render_sample();
                }
                s.set_formant(formant); // fan-out to the held voice
            }
            for _ in 0..1200 {
                s.render_sample();
            }
            (0..16384).map(|_| s.render_sample()[0]).collect()
        };
        let f6 = midi_to_hz(45.0) * 6.0;
        let flat = bin(&render(0.0, true), f6);
        let held = bin(&render(0.42, false), f6); // applied to a sounding note
        let fresh = bin(&render(0.42, true), f6); // applied before note-on
        assert!(held > flat * 6.0, "formant didn't bump a held voice: {flat:e} -> {held:e}");
        assert!(fresh > flat * 6.0, "a fresh note ignored the formant: {fresh:e}");

        // formant 0.0 → bit-for-bit unchanged
        let mut a: PolySynth<4> = PolySynth::new(sr);
        a.set_gain(1.0);
        a.note_on(57, 1.0);
        let mut b: PolySynth<4> = PolySynth::new(sr);
        b.set_gain(1.0);
        b.set_formant(0.0);
        b.note_on(57, 1.0);
        for i in 0..8000 {
            assert_eq!(a.render_sample(), b.render_sample(), "formant 0.0 changed the output at {i}");
        }
    }

    #[test]
    fn channel_brightness_moves_every_sounding_note() {
        let sr = 48_000.0;
        let bin = |buf: &[f32], f: f64| -> f64 {
            let w = core::f64::consts::TAU * f / sr;
            let (mut re, mut im) = (0.0_f64, 0.0);
            for (n, &s) in buf.iter().enumerate() {
                re += s as f64 * (w * n as f64).cos();
                im -= s as f64 * (w * n as f64).sin();
            }
            (re * re + im * im).sqrt() / buf.len() as f64
        };
        let render = |chan: Option<f32>| -> Vec<f32> {
            let mut s: PolySynth<8> = PolySynth::new(sr);
            s.set_gain(1.0);
            s.set_rolloff(0.7);
            s.set_brightness_depth(0.4);
            s.note_on(45, 1.0);
            for _ in 0..400 {
                s.render_sample();
            }
            if let Some(c) = chan {
                s.set_channel_brightness(c);
            }
            for _ in 0..2500 {
                s.render_sample();
            }
            (0..16384).map(|_| s.render_sample()[0]).collect()
        };
        let f0 = midi_to_hz(45.0);
        let tilt = |buf: &[f32]| bin(buf, f0 * 8.0) / bin(buf, f0);
        let flat = tilt(&render(None));
        let pressed = tilt(&render(Some(1.0)));
        assert!(pressed > flat * 3.0, "channel pressure didn't brighten: {flat:e} -> {pressed:e}");
    }

    #[test]
    fn wildcard_note_brightness_is_ignored_not_a_panic() {
        let mut s: PolySynth<4> = PolySynth::new(48_000.0);
        s.set_gain(1.0);
        s.set_brightness_depth(0.4);
        s.note_on(60, 1.0);
        s.set_note_brightness(255, 1.0); // CLAP wildcard key → out of range
        s.set_note_brightness(200, -1.0);
        for _ in 0..2000 {
            let [l, r] = s.render_sample();
            assert!(l.is_finite() && r.is_finite());
        }
    }

    #[test]
    fn soft_clip_is_gentle_and_bounded() {
        assert!((soft_clip(0.0)).abs() < 1e-9);
        assert!((soft_clip(0.1) - 0.1).abs() < 2e-3);
        assert!(soft_clip(1000.0) <= 1.0);
        assert!(soft_clip(-1000.0) >= -1.0);
    }

    #[test]
    fn soft_clip_joins_the_clamp_smoothly() {
        // `poly::soft_clip` is a textual duplicate of `character::tanh_pade`
        // (same Padé rational, same ±3 clamp) — kept as two separate copies
        // deliberately (master limiter vs. per-voice saturator are different
        // roles that may want to diverge later), which means a future edit
        // to just one of them (e.g. "raise the master clip's clamp to ±4")
        // would silently reintroduce the exact C¹-kink the ±3 clamp prevents,
        // and nothing in this file would catch it — `tanh_pade_joins_the_clamp_
        // smoothly` in character.rs only exercises the other copy. Mirrors
        // that test here so both copies are independently regression-tested.
        let h = 1.0e-4_f32;
        let slope_in = (soft_clip(3.0) - soft_clip(3.0 - h)) / h;
        let slope_out = (soft_clip(3.0 + h) - soft_clip(3.0)) / h;
        assert!(slope_in.abs() < 2.0e-3, "slope into the clamp = {slope_in}");
        assert!(slope_out.abs() < 1.0e-6, "slope past the clamp = {slope_out}");
        for i in 0..5000 {
            let x = i as f32 * 0.01;
            let y = soft_clip(x);
            assert!(y <= 1.0 + 1.0e-6 && soft_clip(-x) >= -1.0 - 1.0e-6, "overshoot at x={x}: {y}");
        }
    }

    #[test]
    fn clamps_reject_nan_instead_of_latching_it() {
        // The shared `clamp`/`clamp01` helpers used by every public setter
        // across `voice.rs`/`env.rs`/`poly.rs` must not let NaN fall straight
        // through (`NaN < lo` and `NaN > hi` are both false in IEEE-754). A
        // hostile or buggy C-ABI caller passing NaN to
        // e.g. `harmonic_voice_set_frequency` would have latched NaN into
        // voice state and propagated it into every rendered sample from then
        // on. Exercise it at the PolySynth level: NaN gain/velocity must not
        // poison the mix.
        let mut s: PolySynth<4> = PolySynth::new(48_000.0);
        s.set_gain(f32::NAN as f64);
        s.note_on(60, f32::NAN);
        for _ in 0..4800 {
            let [l, r] = s.render_sample();
            assert!(l.is_finite() && r.is_finite(), "NaN input latched into the output");
        }
    }

    #[test]
    fn re_asserting_hq_every_block_is_a_no_op_not_a_decimator_reset() {
        // `set_hq(x)` when `x` is already the current state must not touch the
        // master decimator's delay line: a plugin's `process` re-asserts the
        // HQ param every block, and an unconditional `hq_decim.reset()` there
        // would wipe the FIR history at every block boundary — a block-rate
        // settling transient in the HQ output.
        let cfg = |s: &mut PolySynth<8>| {
            s.set_gain(0.9);
            s.set_rolloff(0.9);
            s.set_free_running(true);
            s.set_amp_adsr(0.002, 0.02, 1.0, 0.05);
        };
        let mut once: PolySynth<8> = PolySynth::new(48_000.0);
        cfg(&mut once);
        once.set_hq(true);
        once.note_on(57, 1.0);

        let mut per_block: PolySynth<8> = PolySynth::new(48_000.0);
        cfg(&mut per_block);
        per_block.set_hq(true);
        per_block.note_on(57, 1.0);

        for i in 0..12_000 {
            if i % 128 == 0 {
                per_block.set_hq(true); // the redundant per-block re-assert
            }
            assert_eq!(
                once.render_sample()[0].to_bits(),
                per_block.render_sample()[0].to_bits(),
                "redundant set_hq perturbed the HQ output at sample {i}"
            );
        }
    }

    #[test]
    fn carrier_phase_stays_wrapped_when_step_exceeds_one() {
        // `step = f_eff / fs` can exceed 1.0 (bend ×32, vibrato ×2, freq up to
        // fs/2). A bare `phase -= 1.0` cannot wrap that, so the accumulator
        // would grow without bound over a held note and lose precision. Drive
        // one voice with `step ~ 3.8` and check the phase stays in `[0, 1)`
        // every sample, and the output stays finite/bounded.
        let mut v = Voice::new(48_000.0);
        v.set_gain(1.0);
        v.set_frequency(23_000.0); // just under Nyquist
        v.set_pitch_bend(4.0); // ×4
        v.set_lfo(6.0, LfoShape::Sine);
        v.set_lfo_targets(0.0, 1200.0, 0.0, 0.0); // ±1 octave vibrato
        v.reset();
        for i in 0..200_000 {
            let [l, r] = v.render_sample();
            assert!(l.is_finite() && r.is_finite() && l.abs() <= 4.0 && r.abs() <= 4.0);
            assert!(
                (0.0..1.0).contains(&v.carrier_phase_for_test()),
                "carrier phase escaped [0,1) at sample {i}: {}",
                v.carrier_phase_for_test()
            );
        }
    }
}
