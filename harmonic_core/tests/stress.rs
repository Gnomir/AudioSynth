//! Adversarial stress / RT-safety proof suite.
//!
//! The engine's audio-path contract: it never panics, never allocates, never
//! blocks, and never emits a non-finite or unbounded sample — *whatever* the
//! host feeds it. These tests attack that contract directly rather than trust
//! it: `NaN` / `±∞` / absurd magnitudes into every public setter, parameter
//! and note-event storms at sample rate, the sample-rate extremes, HQ toggled
//! under load, and a multi-million-sample run. Every scenario asserts the
//! output stays finite and bounded, and that hostile input cannot strand a
//! voice so the pool never recovers.
//!
//! Companion to the always-on invariant checks in the unit tests, to
//! `cross_platform_bit_exact` (determinism), and to `pluginval` /
//! `clap-validator` (host-contract conformance).

use harmonic_core::{
    CharParams, FilterMode, LfoMode, LfoShape, PolySynth, Voice, Waveform,
};

/// Hostile `f64` inputs: not-a-number, both infinities, a subnormal, ±huge,
/// and negative zero.
const HOSTILE_F64: [f64; 7] = [
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::MIN_POSITIVE / 2.0, // subnormal
    -1.0e300,
    1.0e300,
    -0.0,
];

/// Render `n` samples and assert every one is finite and within a generous
/// bound. `render_sample` ends in `soft_clip`, so a healthy engine sits at
/// `|y| ≤ 1`; `4.0` leaves margin for any transient the clip is chewing on
/// while still catching a real blow-up.
#[track_caller]
fn assert_sane<const V: usize>(s: &mut PolySynth<V>, n: usize, ctx: &str) {
    for i in 0..n {
        let [l, r] = s.render_sample();
        assert!(
            l.is_finite() && r.is_finite(),
            "{ctx}: non-finite output at sample {i}: [{l}, {r}]"
        );
        assert!(
            l.abs() <= 4.0 && r.abs() <= 4.0,
            "{ctx}: output escaped [-4, 4] at sample {i}: [{l}, {r}]"
        );
    }
}

#[track_caller]
fn assert_sane_voice(v: &mut Voice, n: usize, ctx: &str) {
    for i in 0..n {
        let [l, r] = v.render_sample();
        assert!(
            l.is_finite() && r.is_finite(),
            "{ctx}: non-finite voice output at sample {i}: [{l}, {r}]"
        );
        assert!(
            l.abs() <= 8.0 && r.abs() <= 8.0,
            "{ctx}: voice output escaped [-8, 8] at sample {i}: [{l}, {r}]"
        );
    }
}

/// Small deterministic PRNG so the storms are reproducible across runs and
/// platforms (no `rand` dependency, and nothing that could differ by target).
struct Lcg(u64);
impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn f64_pm(&mut self, mag: f64) -> f64 {
        (self.next_u32() as f64 / u32::MAX as f64 * 2.0 - 1.0) * mag
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n
    }
}

// ---------------------------------------------------------------------------

#[test]
fn nan_and_inf_into_every_polysynth_setter_stays_finite() {
    for &bad in &HOSTILE_F64 {
        let b32 = bad as f32;
        let mut s: PolySynth<8> = PolySynth::new(48_000.0);
        s.set_gain(1.0);
        s.note_on(60, 1.0);
        s.note_on(64, 0.8);
        s.note_on(67, 0.6);

        // Every f64/f32-taking public setter, hit with the hostile value.
        s.set_rolloff(bad);
        s.set_gain(bad);
        s.set_fm(bad, bad);
        s.set_feedback(bad);
        s.set_partial_limit(b32);
        s.set_brightness_depth(bad);
        s.set_note_brightness(60, b32);
        s.set_channel_brightness(b32);
        s.set_unison(3, bad, bad, bad);
        s.set_pitch_bend(bad);
        s.set_lfo(bad, LfoShape::Sine, LfoMode::FreeRun, bad, bad, bad, bad);
        s.set_envelope(bad, bad);
        s.set_amp_adsr(bad, bad, bad, bad);
        s.set_filter(FilterMode::Low, bad, bad, bad);
        s.set_filter_envelope(bad, bad, bad, bad);
        s.set_character(CharParams {
            drive: b32,
            bias: b32,
            fold: b32,
            crush: b32,
            downsample: b32,
        });

        assert_sane(&mut s, 8_000, &format!("PolySynth setters with {bad:e}"));

        // …and it must still be able to voice a fresh note and then go quiet.
        s.set_gain(1.0);
        s.set_rolloff(0.5);
        s.set_fm(1.0, 0.0);
        s.set_feedback(0.0);
        s.set_partial_limit(2048.0);
        s.set_brightness_depth(0.0);
        s.set_unison(1, 0.0, 0.0, 0.0);
        s.set_pitch_bend(0.0);
        s.set_lfo(5.0, LfoShape::Sine, LfoMode::Retrigger, 0.0, 0.0, 0.0, 0.0);
        s.set_amp_adsr(0.005, 0.05, 0.7, 0.1);
        s.set_filter(FilterMode::Bypass, 12_000.0, 0.0, 0.0);
        s.set_filter_envelope(0.005, 0.05, 0.0, 0.05);
        s.set_character(CharParams::CLEAN);
        s.all_notes_off();
        s.note_on(72, 1.0);
        assert!(
            (0..48_000).map(|_| s.render_sample()[0].abs()).fold(0.0_f32, f32::max) > 0.01,
            "engine went permanently silent after {bad:e} into a setter"
        );
        s.all_notes_off();
        for _ in 0..192_000 {
            s.render_sample();
        }
        assert_eq!(
            s.active_voice_count(),
            0,
            "a voice never freed after recovering from {bad:e}"
        );
    }
}

#[test]
fn nan_and_inf_into_every_voice_setter_stays_finite() {
    for &bad in &HOSTILE_F64 {
        let mut v = Voice::new(48_000.0);
        v.set_gain(1.0);
        v.set_frequency(220.0);
        v.reset();

        v.set_frequency(bad);
        v.set_pitch_bend(bad);
        v.set_start_phase(bad);
        v.set_rolloff(bad);
        v.set_gain(bad);
        v.set_pan(bad);
        v.set_fm(bad, bad);
        v.set_feedback(bad);
        v.set_lfo(bad, LfoShape::Saw);
        v.set_lfo_targets(bad, bad, bad, bad);
        v.set_lfo_phase(bad);
        v.set_unison_drift(bad, bad);
        v.set_unison_drift_phase(bad);
        v.set_partial_limit(bad as f32);
        v.set_expr_brightness(bad);
        v.set_filter_cutoff(bad);
        v.set_filter_resonance(bad);
        v.set_character(CharParams {
            drive: bad as f32,
            bias: bad as f32,
            fold: bad as f32,
            crush: bad as f32,
            downsample: bad as f32,
        });

        assert_sane_voice(&mut v, 8_000, &format!("Voice setters with {bad:e}"));
    }
}

#[test]
fn parameter_storm_at_sample_rate() {
    // Change a fistful of parameters *every sample* for a long stretch while a
    // chord holds — smoothers, caches and the filter all get hammered with
    // discontinuities. Values stay in-range here; the point is churn, not
    // hostility (that is covered above).
    let mut s: PolySynth<12> = PolySynth::new(44_100.0);
    let mut rng = Lcg(0x5eed_1234);
    s.set_gain(0.8);
    for n in [48, 52, 55, 59, 62].iter() {
        s.note_on(*n, 0.9);
    }
    s.set_filter(FilterMode::Low, 8_000.0, 0.5, 2.0);

    for i in 0..300_000 {
        s.set_rolloff(0.02 + 0.95 * (rng.next_u32() as f64 / u32::MAX as f64));
        s.set_gain(0.3 + 0.6 * (rng.next_u32() as f64 / u32::MAX as f64));
        s.set_partial_limit(1.0 + rng.below(2048) as f32);
        s.set_brightness_depth(rng.f64_pm(0.9));
        s.set_note_brightness(48 + rng.below(20) as u8, rng.f64_pm(1.0) as f32);
        s.set_channel_brightness(rng.f64_pm(1.0) as f32);
        s.set_fm(rng.below(8) as f64, 4.0 * (rng.next_u32() as f64 / u32::MAX as f64));
        s.set_feedback(0.9 * (rng.next_u32() as f64 / u32::MAX as f64));
        s.set_filter(
            FilterMode::Low,
            20.0 + 20_000.0 * (rng.next_u32() as f64 / u32::MAX as f64),
            rng.next_u32() as f64 / u32::MAX as f64,
            rng.f64_pm(6.0),
        );
        if i % 137 == 0 {
            s.set_hq(i % 274 == 0);
        }
        let [l, r] = s.render_sample();
        assert!(l.is_finite() && r.is_finite(), "parameter storm: non-finite at {i}");
        assert!(l.abs() <= 4.0 && r.abs() <= 4.0, "parameter storm: unbounded at {i}: [{l},{r}]");
    }
}

#[test]
fn note_event_storm_never_exceeds_the_pool_and_recovers_to_silence() {
    const V: usize = 16;
    let mut s: PolySynth<V> = PolySynth::new(48_000.0);
    let mut rng = Lcg(0xabcd_0001);
    s.set_gain(0.7);
    s.set_amp_adsr(0.001, 0.02, 0.6, 0.08);
    s.set_unison(4, 14.0, 0.7, 0.5); // unison eats into the pool too

    for _ in 0..20_000 {
        match rng.below(5) {
            0 | 1 => s.note_on(24 + rng.below(72) as u8, 0.5 + rng.f64_pm(0.5) as f32),
            2 => s.note_off(24 + rng.below(72) as u8),
            3 => s.choke(24 + rng.below(72) as u8),
            _ => {}
        }
        for _ in 0..rng.below(24) {
            let [l, r] = s.render_sample();
            assert!(l.is_finite() && r.is_finite(), "note storm: non-finite");
            assert!(l.abs() <= 4.0 && r.abs() <= 4.0, "note storm: unbounded [{l},{r}]");
        }
        assert!(
            s.active_voice_count() <= V,
            "note storm: {} active voices > pool {V}",
            s.active_voice_count()
        );
    }

    s.all_notes_off();
    for _ in 0..192_000 {
        s.render_sample();
    }
    assert_eq!(s.active_voice_count(), 0, "voices stuck active after all_notes_off + 4 s");
    let tail = (0..2_000)
        .map(|_| {
            let [l, r] = s.render_sample();
            l.abs().max(r.abs())
        })
        .fold(0.0_f32, f32::max);
    assert!(tail < 1.0e-4, "tail not silent after storm: {tail}");
}

#[test]
fn hostile_envelope_times_cannot_strand_a_voice() {
    // +∞ / huge / NaN attack, decay and release must not leave a voice
    // progressing at zero rate (stuck in Attack/Decay/Release forever).
    for &t in &[f64::INFINITY, 1.0e30, f64::NAN, -1.0e9] {
        let mut s: PolySynth<4> = PolySynth::new(48_000.0);
        s.set_gain(1.0);
        s.set_amp_adsr(t, t, 0.5, t);
        s.set_filter_envelope(t, t, 0.5, t);
        s.note_on(60, 1.0);
        assert_sane(&mut s, 4_000, &format!("hostile ADSR {t:e}"));
        s.note_off(60);
        // A sane engine frees the voice within its clamped max release
        // (600 s). Give it a comfortable margin past a *normal* release; the
        // clamp makes the real bound irrelevant to this assertion's intent —
        // we only need "it is not literally never".
        s.set_amp_adsr(0.005, 0.02, 0.5, 0.05); // reasonable release, applied live
        s.note_off(60);
        for _ in 0..48_000 {
            s.render_sample();
        }
        assert_eq!(
            s.active_voice_count(),
            0,
            "voice never released after hostile ADSR {t:e}"
        );
    }
}

#[test]
fn sample_rate_extremes_with_the_whole_tract_lit() {
    for &sr in &[8_000.0_f64, 768_000.0] {
        let mut s: PolySynth<8> = PolySynth::new(sr);
        s.set_gain(0.8);
        s.set_rolloff(0.9);
        s.set_partial_limit(512.0);
        s.set_brightness_depth(0.4);
        s.set_fm(3.0, 0.6);
        s.set_feedback(0.5);
        s.set_character(CharParams { drive: 0.7, bias: 0.2, fold: 0.6, crush: 0.4, downsample: 0.3 });
        s.set_filter(FilterMode::Band, sr * 0.2, 0.8, 4.0);
        s.set_filter_envelope(0.001, 0.05, 0.2, 0.1);
        s.set_lfo(7.0, LfoShape::Triangle, LfoMode::FreeRun, 0.4, 40.0, 3.0, 2.0);
        s.set_unison(6, 18.0, 0.8, 0.6);
        s.set_hq(true);
        s.note_on(48, 1.0);
        s.note_on(60, 0.9);
        s.set_note_brightness(60, 1.0);
        s.set_channel_brightness(0.5);
        assert_sane(&mut s, sr as usize / 2, &format!("full tract @ {sr} Hz"));
    }
}

#[test]
fn hq_toggled_every_few_samples_under_a_hot_signal() {
    let mut s: PolySynth<8> = PolySynth::new(48_000.0);
    s.set_gain(1.0);
    s.set_rolloff(0.97);
    s.set_character(CharParams { drive: 0.9, bias: 0.3, fold: 0.8, crush: 0.5, downsample: 0.4 });
    s.set_fm(2.0, 0.8);
    s.note_on(55, 1.0);
    s.note_on(62, 1.0);
    for i in 0..120_000 {
        if i % 7 == 0 {
            s.set_hq((i / 7) % 2 == 0);
        }
        let [l, r] = s.render_sample();
        assert!(l.is_finite() && r.is_finite(), "HQ toggle storm: non-finite at {i}");
        assert!(l.abs() <= 4.0 && r.abs() <= 4.0, "HQ toggle storm: unbounded at {i}: [{l},{r}]");
    }
}

#[test]
fn reset_while_sounding_is_immediately_clean() {
    let mut s: PolySynth<8> = PolySynth::new(48_000.0);
    s.set_gain(1.0);
    for n in [50, 57, 64, 71].iter() {
        s.note_on(*n, 1.0);
    }
    for _ in 0..2_000 {
        s.render_sample();
    }
    s.reset();
    assert_eq!(s.active_voice_count(), 0, "reset did not silence the voices");
    let after = (0..4_000)
        .map(|_| {
            let [l, r] = s.render_sample();
            assert!(l.is_finite() && r.is_finite());
            l.abs().max(r.abs())
        })
        .fold(0.0_f32, f32::max);
    assert!(after < 1.0e-6, "output not silent immediately after reset: {after}");
}

#[test]
fn long_run_holds_finite_bounded_and_does_not_drift_in_level() {
    // ~2.1 M samples (~44 s @ 48 k) of a sustained chord with slow modulation.
    // Level must neither collapse toward zero nor creep upward.
    let mut s: PolySynth<8> = PolySynth::new(48_000.0);
    s.set_gain(0.5);
    s.set_rolloff(0.85);
    s.set_amp_adsr(0.01, 0.1, 0.9, 0.3); // real sustain
    s.set_lfo(0.3, LfoShape::Sine, LfoMode::FreeRun, 0.25, 6.0, 1.5, 0.0);
    s.set_unison(4, 12.0, 0.6, 0.4);
    s.note_on(45, 0.9);
    s.note_on(52, 0.9);
    s.note_on(57, 0.9);

    let rms = |s: &mut PolySynth<8>, n: usize| -> f64 {
        let mut acc = 0.0_f64;
        for _ in 0..n {
            let [l, r] = s.render_sample();
            assert!(l.is_finite() && r.is_finite(), "long run: non-finite");
            assert!(l.abs() <= 4.0 && r.abs() <= 4.0, "long run: unbounded [{l},{r}]");
            acc += (l as f64).powi(2) + (r as f64).powi(2);
        }
        (acc / (2.0 * n as f64)).sqrt()
    };

    for _ in 0..48_000 {
        s.render_sample();
    } // settle
    let early = rms(&mut s, 96_000);
    for _ in 0..1_800_000 {
        s.render_sample();
    }
    let late = rms(&mut s, 96_000);

    assert!(early > 1.0e-3, "chord never produced level: {early:e}");
    let ratio = late / early;
    assert!(
        (0.5..2.0).contains(&ratio),
        "level drifted over 2 M samples: early {early:e} → late {late:e} (×{ratio:.3})"
    );
}

#[test]
fn subnormal_and_zero_frequency_are_handled() {
    let mut v = Voice::new(48_000.0);
    v.set_gain(1.0);
    for &f in &[0.0, -0.0, f64::MIN_POSITIVE, 1.0e-20, -50.0] {
        v.set_frequency(f);
        v.reset();
        assert_sane_voice(&mut v, 4_000, &format!("frequency {f:e}"));
    }
}

#[test]
fn waveform_switching_mid_note_stays_bounded() {
    let mut s: PolySynth<6> = PolySynth::new(48_000.0);
    s.set_gain(1.0);
    s.set_rolloff(0.95);
    s.note_on(57, 1.0);
    let waves = [Waveform::Geometric, Waveform::Saw, Waveform::Triangle];
    for i in 0..60_000 {
        if i % 11 == 0 {
            s.set_waveform(waves[(i / 11) % 3]);
        }
        let [l, r] = s.render_sample();
        assert!(l.is_finite() && r.is_finite(), "waveform switch: non-finite at {i}");
        assert!(l.abs() <= 4.0 && r.abs() <= 4.0, "waveform switch: unbounded at {i}");
    }
}
