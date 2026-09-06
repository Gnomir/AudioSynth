//! Cross-platform bit-exactness.
//!
//! `harmonic_core` uses no `libm` and no FMA (`docs/08 §3`): every hot-path
//! operation is an IEEE-754 `+ − × ÷` or a bit reinterpret / saturating cast.
//! IEEE-754 pins those to a single correctly-rounded result on every target, so
//! the rendered signal is bit-identical on x86-64, AArch64, 32-bit ARM **and
//! `wasm32`** alike.
//!
//! The scripted render and the reference hash live in
//! [`harmonic_core::verify`] so this test, the ARM cross-check
//! (`scripts/cross-verify.sh`) and the wasm cross-check
//! (`scripts/verify-wasm.mjs`) all exercise byte-for-byte the same pass.
//!
//! The reference was produced on `x86_64-pc-windows-msvc`. The same value must
//! come out under QEMU on `aarch64-unknown-linux-gnu` and
//! `armv7-unknown-linux-gnueabihf` (hard-float, same VFP `f64` semantics as
//! `thumbv7em-none-eabihf` / Cortex-M4F), and in `wasm32` under Node. Any
//! single-ULP divergence anywhere in ~40 k samples changes the hash.
//!
//! Regenerate the constant with:  `RENDER_EMIT_HASH=1 cargo test --release
//! --test cross_platform_bit_exact -- --nocapture`

use harmonic_core::verify::{render_verification, verify_hash, VERIFY_FRAMES, VERIFY_HASH};

#[test]
fn rendered_signal_is_bit_identical_across_architectures() {
    let mut sig = vec![0.0_f32; VERIFY_FRAMES * 2];
    render_verification(&mut sig);

    // sanity: the render actually produced sound and stayed finite / bounded
    let peak = sig.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    assert!(peak.is_finite() && peak > 0.05 && peak <= 1.5, "peak = {peak}");

    let h = verify_hash(&sig);

    if std::env::var_os("RENDER_EMIT_HASH").is_some() {
        eprintln!("bit-exact signal hash = {h:#018x}");
    }

    assert_eq!(
        h, VERIFY_HASH,
        "rendered signal differs from the x86_64 reference — a platform \
         introduced a non-IEEE-754 rounding or an FMA contraction (hash {h:#018x})"
    );
}
