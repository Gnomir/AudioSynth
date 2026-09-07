//! Cross-platform bit-exactness.
//!
//! `harmonic_core` uses no `libm` and no FMA (`docs/08 §3`): every hot-path
//! operation is an IEEE-754 `+ − × ÷` or a bit reinterpret / saturating cast.
//! IEEE-754 pins those to a single correctly-rounded result on every target, so
//! the rendered signal is bit-identical on x86-64, AArch64, 32-bit ARM **and
//! `wasm32`** alike — at **any** sample rate (checked here at 48 kHz and 96 kHz).
//!
//! The scripted render and the reference hashes live in
//! [`harmonic_core::verify`] so this test, the ARM cross-check
//! (`scripts/cross-verify.sh`) and the wasm cross-check
//! (`scripts/verify-wasm.mjs`) all exercise byte-for-byte the same pass.
//!
//! Regenerate the constants with:  `RENDER_EMIT_HASH=1 cargo test --release
//! --test cross_platform_bit_exact -- --nocapture`

use harmonic_core::verify::{
    render_verification, render_verification_2, render_verification_2_at, render_verification_at,
    verify_hash, VERIFY_2_HASH, VERIFY_2_HASH_96K, VERIFY_FRAMES, VERIFY_HASH, VERIFY_HASH_96K,
};

enum Pass {
    One,
    Two,
}

fn check(pass: Pass, sr: f64, expected: u64, label: &str) {
    let mut sig = vec![0.0_f32; VERIFY_FRAMES * 2];
    match (pass, sr == 48_000.0) {
        (Pass::One, true) => render_verification(&mut sig),
        (Pass::One, false) => render_verification_at(sr, &mut sig),
        (Pass::Two, true) => render_verification_2(&mut sig),
        (Pass::Two, false) => render_verification_2_at(sr, &mut sig),
    }

    // sanity: the render actually produced sound and stayed finite / bounded
    let peak = sig.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    assert!(peak.is_finite() && peak > 0.05 && peak <= 1.5, "{label}: peak = {peak}");

    let h = verify_hash(&sig);
    if std::env::var_os("RENDER_EMIT_HASH").is_some() {
        eprintln!("{label} signal hash = {h:#018x}");
    }
    assert_eq!(
        h, expected,
        "{label}: rendered signal differs from the x86_64 reference — a platform \
         introduced a non-IEEE-754 rounding or an FMA contraction (hash {h:#018x})"
    );
}

#[test]
fn rendered_signal_is_bit_identical_across_architectures() {
    check(Pass::One, 48_000.0, VERIFY_HASH, "48 kHz");
    check(Pass::One, 96_000.0, VERIFY_HASH_96K, "96 kHz");
}

#[test]
fn second_pass_is_bit_identical_across_architectures() {
    // HQ bus + Saw/Triangle + fractional partials + Formant hump.
    check(Pass::Two, 48_000.0, VERIFY_2_HASH, "48 kHz pass 2");
    check(Pass::Two, 96_000.0, VERIFY_2_HASH_96K, "96 kHz pass 2");
}
