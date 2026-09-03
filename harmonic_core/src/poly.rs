//! Polyphonic MIDI engine over [`Voice`]. Fixed voice array, no allocation,
//! no locks — safe to drive straight from an audio callback.
//!
//! Handles voice allocation & stealing, the two envelopes, unison, pitch bend,
//! the shared LFO settings, and note→pitch — so it is unit-testable without a
//! plugin host. Stereo out.

use crate::character::CharParams;
use crate::env::Adsr;
use crate::filter::FilterMode;
use crate::lfo::LfoShape;
use crate::trig::exp2;
use crate::voice::Voice;

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

    // modulation
    bend_ratio: f64, // pitch-bend, 2^(st/12)
    lfo_rate: f64,
    lfo_shape: LfoShape,
    lfo_to_rolloff: f64,
    lfo_to_pitch: f64,

    counter: u64,
}

impl<const VOICES: usize> PolySynth<VOICES> {
    pub fn new(sample_rate: f64) -> Self {
        PolySynth {
            voices: core::array::from_fn(|_| PolyVoice::new(sample_rate)),
            sample_rate,
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
            bend_ratio: 1.0,
            lfo_rate: 5.0,
            lfo_shape: LfoShape::Sine,
            lfo_to_rolloff: 0.0,
            lfo_to_pitch: 0.0,
            counter: 0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        *self = PolySynth::new(sample_rate);
    }

    pub fn set_rolloff(&mut self, r: f64) {
        self.rolloff = r;
        for v in &mut self.voices {
            v.core.set_rolloff(r);
        }
    }

    pub fn set_gain(&mut self, g: f64) {
        self.gain = if g < 0.0 { 0.0 } else { g };
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

    /// Unison: `count` (1..8) detuned + stereo-spread voices per note.
    pub fn set_unison(&mut self, count: u32, detune_cents: f64, spread: f64) {
        self.unison_count = count.clamp(1, MAX_UNISON);
        self.unison_detune = detune_cents;
        self.unison_spread = spread.clamp(0.0, 1.0);
    }

    /// Pitch bend in semitones (applied to every sounding voice, smoothed).
    pub fn set_pitch_bend(&mut self, semitones: f64) {
        self.bend_ratio = exp2(semitones / 12.0);
        for v in &mut self.voices {
            v.core.set_pitch_bend(self.bend_ratio);
        }
    }

    /// Shared LFO: rate, shape, and routing depth to brightness (`to_rolloff`,
    /// ±) and vibrato (`to_pitch_cents`).
    pub fn set_lfo(&mut self, rate_hz: f64, shape: LfoShape, to_rolloff: f64, to_pitch_cents: f64) {
        self.lfo_rate = rate_hz;
        self.lfo_shape = shape;
        self.lfo_to_rolloff = to_rolloff;
        self.lfo_to_pitch = to_pitch_cents;
        for v in &mut self.voices {
            v.core.set_lfo(rate_hz, shape);
            v.core.set_lfo_targets(to_rolloff, to_pitch_cents);
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

    /// MIDI note-on. Stacks `unison_count` detuned, stereo-spread voices.
    pub fn note_on(&mut self, note: u8, velocity: f32) {
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
        let hz = midi_to_hz(note as f32) * exp2(detune_cents / 1200.0);

        let v = &mut self.voices[vi];
        v.core.set_frequency(hz);
        v.core.set_rolloff(self.rolloff);
        v.core.set_gain(1.0);
        v.core.set_pan(pan);
        v.core.set_free_running(self.free_running);
        v.core.set_pitch_bend(self.bend_ratio);
        v.core.set_character(self.character);
        v.core.set_fm(self.fm_ratio, self.fm_index);
        v.core.set_feedback(self.feedback);
        v.core.set_filter_mode(self.filter_mode);
        v.core.set_filter_resonance(self.filter_res);
        v.core.set_filter_cutoff(self.filter_cutoff);
        v.core.set_lfo(self.lfo_rate, self.lfo_shape);
        v.core
            .set_lfo_targets(self.lfo_to_rolloff, self.lfo_to_pitch);
        v.core.reset();

        // decorrelate stacked unison voices
        if count > 1 {
            let frac = idx as f64 / count as f64;
            if !self.free_running {
                v.core.set_start_phase(frac);
            }
            v.core.set_lfo_phase(frac);
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

    /// Render one stereo sample `[left, right]`.
    #[inline]
    pub fn render_sample(&mut self) -> [f32; 2] {
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
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    let x = if x > 3.0 {
        3.0
    } else if x < -3.0 {
        -3.0
    } else {
        x
    };
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

#[inline(always)]
fn clamp01(x: f32) -> f32 {
    if x < 0.0 {
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
        s.set_unison(4, 15.0, 0.9);
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
    fn pitch_bend_shifts_all_voices() {
        let mut s: PolySynth<8> = PolySynth::new(48_000.0);
        s.set_gain(1.0);
        s.set_unison(3, 10.0, 0.5);
        s.note_on(60, 1.0);
        s.set_pitch_bend(2.0); // +2 semitones
        assert!(peak(&mut s, 24_000) <= 1.5);
        s.set_pitch_bend(-12.0); // −1 octave
        assert!(peak(&mut s, 24_000) <= 1.5);
    }

    #[test]
    fn lfo_modulation_stays_bounded() {
        let mut s: PolySynth<8> = PolySynth::new(48_000.0);
        s.set_gain(1.0);
        s.set_lfo(6.0, LfoShape::Triangle, 0.35, 30.0);
        s.note_on(52, 1.0);
        assert!(peak(&mut s, 96_000) <= 1.5);
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
    fn soft_clip_is_gentle_and_bounded() {
        assert!((soft_clip(0.0)).abs() < 1e-9);
        assert!((soft_clip(0.1) - 0.1).abs() < 2e-3);
        assert!(soft_clip(1000.0) <= 1.0);
        assert!(soft_clip(-1000.0) >= -1.0);
    }
}
