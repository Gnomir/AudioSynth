//! Cross-platform bit-exactness (RFC-15).
//!
//! `harmonic_core` uses no `libm` and no FMA (`docs/08 §3`): every hot-path
//! operation is an IEEE-754 `+ − × ÷` or a bit reinterpret / saturating cast.
//! IEEE-754 pins those to a single correctly-rounded result on every target, so
//! the rendered signal *should* be bit-identical on x86-64, AArch64 and 32-bit
//! ARM alike. This test turns "should" into "measured": it renders a fixed,
//! deterministic pass through the whole engine (unison, drift, FM, feedback,
//! all four LFO routings, a resonant SVF, every `Character` stage) and folds
//! every output sample's bits into one hash.
//!
//! The reference hash below was produced on `x86_64-pc-windows-msvc`. The same
//! value must come out under QEMU on `aarch64-unknown-linux-gnu` and
//! `armv7-unknown-linux-gnueabihf` (hard-float, same VFP `f64` semantics as
//! `thumbv7em-none-eabihf` / Cortex-M4F). Any single-ULP divergence anywhere in
//! ~40 k samples changes the hash.
//!
//! Regenerate the constant with:  `RENDER_EMIT_HASH=1 cargo test --release
//! --test cross_platform_bit_exact -- --nocapture`

use harmonic_core::{CharParams, FilterMode, LfoMode, LfoShape, PolySynth};

const SAMPLE_RATE: f64 = 48_000.0;
const FRAMES: usize = 4_800; // 100 ms of stereo audio → 9 600 f32 samples

/// FNV-1a over the raw bits of every sample, in render order.
fn hash_signal(frames: &[[f32; 2]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let fold = |bits: u32, h: &mut u64| {
        for b in bits.to_le_bytes() {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for f in frames {
        fold(f[0].to_bits(), &mut h);
        fold(f[1].to_bits(), &mut h);
    }
    h
}

/// Deterministic, control-scripted render through the full voice chain.
fn render_reference() -> Vec<[f32; 2]> {
    let mut synth: PolySynth<8> = PolySynth::new(SAMPLE_RATE);

    synth.set_rolloff(0.93);
    synth.set_gain(0.8);
    synth.set_character(CharParams {
        drive: 0.55,
        bias: -0.2,
        fold: 0.35,
        crush: 0.4,
        downsample: 0.3,
    });
    synth.set_fm(2.0, 0.6);
    synth.set_feedback(0.25);
    synth.set_free_running(false);
    synth.set_unison(4, 12.0, 0.7, 0.8);
    synth.set_amp_adsr(0.005, 0.08, 0.6, 0.15);
    synth.set_filter(FilterMode::Low, 1_400.0, 0.8, 2.5);
    synth.set_filter_envelope(0.002, 0.05, 0.3, 0.12);
    synth.set_lfo(
        5.5,
        LfoShape::Triangle,
        LfoMode::FreeRun,
        0.3,   // → rolloff
        18.0,  // → pitch (cents)
        1.5,   // → cutoff (octaves)
        0.4,   // → FM index
    );

    let mut out = Vec::with_capacity(FRAMES);
    for i in 0..FRAMES {
        match i {
            0 => synth.note_on(45, 0.9),
            600 => synth.note_on(52, 0.7),
            1_200 => synth.set_pitch_bend(1.5),
            1_800 => synth.set_pitch_bend(-0.75),
            2_400 => synth.note_off(45),
            3_000 => synth.note_on(59, 1.0),
            3_600 => synth.note_off(52),
            4_200 => synth.note_off(59),
            _ => {}
        }
        out.push(synth.render_sample());
    }
    out
}

#[test]
fn rendered_signal_is_bit_identical_across_architectures() {
    let sig = render_reference();

    // sanity: the render actually produced sound and stayed finite / bounded
    let peak = sig
        .iter()
        .flat_map(|f| [f[0].abs(), f[1].abs()])
        .fold(0.0_f32, f32::max);
    assert!(peak.is_finite() && peak > 0.05 && peak <= 1.5, "peak = {peak}");

    let h = hash_signal(&sig);

    if std::env::var_os("RENDER_EMIT_HASH").is_some() {
        eprintln!("bit-exact signal hash = {h:#018x}");
    }

    // Reference produced on x86_64-pc-windows-msvc, rustc 1.97.1, release.
    const EXPECTED: u64 = 0xc7f7_86d4_0586_da75;
    assert_eq!(
        h, EXPECTED,
        "rendered signal differs from the x86_64 reference — a platform \
         introduced a non-IEEE-754 rounding or an FMA contraction (hash {h:#018x})"
    );
}
