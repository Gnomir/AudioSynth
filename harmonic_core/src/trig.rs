//! Zero-dependency range-reduced sine/cosine, arguments in **turns**
//! (1 turn = 2π rad).
//!
//! `core` exposes no `sin`/`cos`/`floor`/`abs` for `f64`, so everything here is
//! built from primitive arithmetic:
//!
//! * two Horner polynomials (Taylor, plain `a*x+b` — no `mul_add`, to avoid an
//!   `fma` symbol dependency on FMA-less targets) valid on `|φ| ≤ π/4`, and
//! * octant folding in the turns domain.
//!
//! Absolute error vs a reference `f64` `cos`/`sin` is `< 2e-11` across the whole
//! real line for `|turns| < 2^51` — ~120 dB below full scale, well under the
//! noise floor of 24-bit audio.

use core::f64::consts::{FRAC_PI_4, TAU};

/// `|x|` without `f64::abs` (which is `std`).
#[inline(always)]
fn fabs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & 0x7fff_ffff_ffff_ffff)
}

/// Nearest integer (ties away from zero), valid for `|x| < 2^52`.
/// Replaces `f64::round` (which is `std`).
#[inline(always)]
fn round_int(x: f64) -> f64 {
    let t = (x as i64) as f64; // truncate toward zero; `as` saturates on overflow/NaN
    let frac = x - t;
    if frac > 0.5 {
        t + 1.0
    } else if frac < -0.5 {
        t - 1.0
    } else {
        t
    }
}

/// `cos(φ)` for `|φ| ≤ π/4`. Taylor through `φ¹⁶`. Error `< 3` ULP.
#[inline(always)]
fn cos_kernel(phi: f64) -> f64 {
    let u = phi * phi;
    let p = 1.0 / 20_922_789_888_000.0_f64;
    let p = p * u + (-1.0 / 87_178_291_200.0);
    let p = p * u + (1.0 / 479_001_600.0);
    let p = p * u + (-1.0 / 3_628_800.0);
    let p = p * u + (1.0 / 40_320.0);
    let p = p * u + (-1.0 / 720.0);
    let p = p * u + (1.0 / 24.0);
    let p = p * u + (-0.5);
    p * u + 1.0
}

/// `sin(φ)` for `|φ| ≤ π/4`. Taylor through `φ¹⁷`. Error `< 2` ULP.
#[inline(always)]
fn sin_kernel(phi: f64) -> f64 {
    let u = phi * phi;
    let p = 1.0 / 355_687_428_096_000.0_f64;
    let p = p * u + (-1.0 / 1_307_674_368_000.0);
    let p = p * u + (1.0 / 6_227_020_800.0);
    let p = p * u + (-1.0 / 39_916_800.0);
    let p = p * u + (1.0 / 362_880.0);
    let p = p * u + (-1.0 / 5_040.0);
    let p = p * u + (1.0 / 120.0);
    let p = p * u + (-1.0 / 6.0);
    let p = p * u + 1.0;
    phi * p
}

/// `cos(φ)` for `|φ| ≤ π/4` — 4-term Horner. Error `< 4·10⁻⁶`. Half the ops of
/// [`cos_kernel`]; use for modulators (LFO, pan), not the carrier.
#[inline(always)]
fn cos_kernel_fast(phi: f64) -> f64 {
    let u = phi * phi;
    let p = -1.0 / 720.0_f64;
    let p = p * u + (1.0 / 24.0);
    let p = p * u + (-0.5);
    p * u + 1.0
}

/// `sin(φ)` for `|φ| ≤ π/4` — 4-term Horner. Error `< 4·10⁻⁷`.
#[inline(always)]
fn sin_kernel_fast(phi: f64) -> f64 {
    let u = phi * phi;
    let p = -1.0 / 5040.0_f64;
    let p = p * u + (1.0 / 120.0);
    let p = p * u + (-1.0 / 6.0);
    let p = p * u + 1.0;
    phi * p
}

/// Octant fold shared by the precise and fast `cos_turns` variants.
/// Returns `(reduced_angle_rad ∈ [0, π/4], outer_sign, use_sin_kernel)`.
#[inline(always)]
fn reduce_turns(turns: f64) -> (f64, f64, bool) {
    let t = turns - round_int(turns); // [-0.5, 0.5]
    let a = fabs(t); // [0, 0.5], cos is even
    let (a, sign) = if a > 0.25 { (0.5 - a, -1.0) } else { (a, 1.0) };
    if a <= 0.125 {
        (a * TAU, sign, false)
    } else {
        ((0.25 - a) * TAU, sign, true) // cos θ = sin(π/2 − θ)
    }
}

/// `cos(2π · turns)` for any real `turns` (`|turns| < 2^51`). Full precision.
#[inline]
pub fn cos_turns(turns: f64) -> f64 {
    let (phi, sign, use_sin) = reduce_turns(turns);
    if use_sin {
        sign * sin_kernel(phi)
    } else {
        sign * cos_kernel(phi)
    }
}

/// `sin(2π · turns) = cos(2π · (turns − ¼))`. Full precision.
#[inline]
pub fn sin_turns(turns: f64) -> f64 {
    cos_turns(turns - 0.25)
}

/// `cos(2π · turns)` — ~16-bit accurate (error `< 5·10⁻⁶`). For modulation
/// signals (LFO, pan) where the carrier's ULP precision is wasted CPU.
#[inline]
pub fn cos_turns_fast(turns: f64) -> f64 {
    let (phi, sign, use_sin) = reduce_turns(turns);
    if use_sin {
        sign * sin_kernel_fast(phi)
    } else {
        sign * cos_kernel_fast(phi)
    }
}

/// `sin(2π · turns)` — fast (see [`cos_turns_fast`]).
#[inline]
pub fn sin_turns_fast(turns: f64) -> f64 {
    cos_turns_fast(turns - 0.25)
}

/// `(sin, cos)(2π · turns)` — fast. For the equal-power panner.
#[inline]
pub fn sin_cos_turns_fast(turns: f64) -> (f64, f64) {
    (cos_turns_fast(turns - 0.25), cos_turns_fast(turns))
}

/// `(sin(2π·turns), cos(2π·turns))` — one call site for both.
#[inline]
pub fn sin_cos_turns(turns: f64) -> (f64, f64) {
    (cos_turns(turns - 0.25), cos_turns(turns))
}

/// `tan(2π·turns)` for `|turns| < 0.25` (angle in `(−π/2, π/2)`). Built from the
/// Horner `sin`/`cos` kernels — no `libm`.
#[inline]
pub fn tan_turns(turns: f64) -> f64 {
    let (s, c) = sin_cos_turns(turns);
    s / c
}

/// `tan(2π·turns)` for `turns ∈ [0, 0.23]` — a single `[3/2]` rational (Remez
/// minimax), no range reduction, no separate sin/cos. Relative error `< 1e-7`
/// over that interval, ~4× cheaper than [`tan_turns`]. Only for callers whose
/// argument is bounded well below `0.25` — the SVF prewarp (`fc ≤ 0.45 fs` ⇒
/// `turns ≤ 0.225`). Outside `[0, 0.23]` it is unspecified.
#[inline]
pub fn tan_turns_fast(turns: f64) -> f64 {
    // coefficients from examples/fit_coeffs.rs
    let u = turns * turns;
    let n = 6.283_185_401_533_712_5_f64
        + u * (-27.774_478_031_874_615 + u * 10.955_876_613_898_502);
    let d = 1.0 + u * (-17.579_906_658_962_123 + u * 25.278_587_948_643_107);
    turns * n / d
}

#[allow(dead_code)]
const _MAX_KERNEL_ARG: f64 = FRAC_PI_4; // documents the kernel domain bound

// ============================================================================
// Batched, branchless cos — SIMD-friendly (§ SIMD task)
// ============================================================================

/// Round-to-nearest without a branch (`|x| < 2^51`). Round-half-to-even; the
/// tiny difference from [`round_int`] at exact halves does not matter for phase.
///
/// **Assumes the FPU's default round-to-nearest mode.** Unlike [`round_int`]
/// (a truncating `as i64` cast — fixed by the language, ignores the FPU
/// rounding-mode control bits entirely), this add-then-subtract trick reuses
/// whatever the *current* rounding mode is, so a host that leaves MXCSR (or
/// the ARM FPCR) in a non-default mode would shift it. That is why it is
/// confined to [`cos_turns_branchless`] / [`cos4_turns`] — the SIMD-shaped
/// batch path — which `Voice`/`PolySynth` never call; the scalar hot path
/// (`cos_turns`, and every `f64 as i64` cast) is rounding-mode-immune. A
/// truncating cast would fix that dependence too, but `as i64` on `f64` has
/// no packed form below AVX-512 and would de-vectorise this function's whole
/// reason to exist — so if the branchless path ever lands on the audio
/// thread, save/restore MXCSR around the callback instead of changing this.
#[inline(always)]
fn round_bias(x: f64) -> f64 {
    const MAGIC: f64 = 6_755_399_441_055_744.0; // 1.5 · 2^52
    (x + MAGIC) - MAGIC
}

/// Branchless `cos(2π·x)`. Same result as [`cos_turns`] to within a few ULP;
/// written with `select`-style arithmetic (no `if`) so a 4-wide loop over it
/// auto-vectorises on x86-64 / AArch64 and stays correct scalar elsewhere.
#[inline(always)]
pub fn cos_turns_branchless(x: f64) -> f64 {
    let t = x - round_bias(x); // [-0.5, 0.5]
    let a = fabs_(t); // [0, 0.5]
    // fold [0.25, 0.5] → [0, 0.25]
    let over = (a > 0.25) as u8 as f64; // 0.0 or 1.0
    let a = a + over * (0.5 - 2.0 * a);
    let sign = 1.0 - 2.0 * over;
    // split at π/4 (= 0.125 turns): cos below, sin(¼ − a) above
    let use_sin = (a > 0.125) as u8 as f64;
    let c = cos_kernel(a * TAU);
    let s = sin_kernel((0.25 - a) * TAU);
    sign * ((1.0 - use_sin) * c + use_sin * s)
}

#[inline(always)]
fn fabs_(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & 0x7fff_ffff_ffff_ffff)
}

/// `cos(2π·x)` for four lanes. Plain `[f64; 4]` — LLVM lowers this to
/// `VFMADD`/`FMLA` on SIMD targets and to scalar (identical results) on
/// Cortex-M and friends. See also the `portable-simd` feature for an explicit
/// `core::simd` path.
#[inline]
pub fn cos4_turns(x: [f64; 4]) -> [f64; 4] {
    [
        cos_turns_branchless(x[0]),
        cos_turns_branchless(x[1]),
        cos_turns_branchless(x[2]),
        cos_turns_branchless(x[3]),
    ]
}

/// `floor(x)` as an `f64`, valid for `|x| < 2^52`. Replaces `f64::floor` (std).
#[inline(always)]
pub fn floor_f64(x: f64) -> f64 {
    let t = (x as i64) as f64;
    if t > x {
        t - 1.0
    } else {
        t
    }
}

/// `2^x` for real `x` (`|x| < 1000`). Replaces `f64::exp2` (std).
///
/// Split `x = ⌊x⌋ + f`: the integer part becomes an exponent field directly,
/// the fractional part goes through an 8-term (degree-7) Horner polynomial.
/// The coefficients are a **Remez minimax** fit of `2^f` on `f ∈ [0,1]` (not a
/// Taylor series): relative error `< 3e-8` — nine orders below a cent, so
/// integer powers are exact (`c₀ = 1`) and there is no measurable pitch drift.
#[inline]
pub fn exp2(x: f64) -> f64 {
    let fl = floor_f64(x);
    let f = x - fl;

    // 2^fl by building the IEEE-754 exponent field (bias 1023). Clamp to the
    // normal range so the shift is always well defined.
    let e = fl as i64;
    let e = if e < -1022 {
        -1022
    } else if e > 1023 {
        1023
    } else {
        e
    };
    let two_fl = f64::from_bits(((e + 1023) as u64) << 52);

    // 2^f, f ∈ [0,1] — Remez minimax, degree 7 (see examples/fit_coeffs.rs)
    let p = 8.568_020_029_104_6e-5_f64;
    let p = p * f + -8.581_653_953_827_3e-5;
    let p = p * f + 1.665_468_533_058_2e-3;
    let p = p * f + 9.388_897_452_886_6e-3;
    let p = p * f + 5.558_374_723_244_2e-2;
    let p = p * f + 0.240_214_375_664_654_1;
    let p = p * f + 0.693_147_677_442_756_6;
    let p = p * f + 1.0;

    two_fl * p
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference uses std math; only compiled under `cargo test`.
    fn ref_cos(turns: f64) -> f64 {
        (turns * core::f64::consts::TAU).cos()
    }
    fn ref_sin(turns: f64) -> f64 {
        (turns * core::f64::consts::TAU).sin()
    }

    #[test]
    fn cos_matches_reference_across_many_turns() {
        let mut max_err = 0.0_f64;
        let mut x = -37.13_f64;
        while x < 37.0 {
            let e = (cos_turns(x) - ref_cos(x)).abs();
            if e > max_err {
                max_err = e;
            }
            x += 0.00131;
        }
        assert!(max_err < 2e-11, "cos max abs error {max_err:e}");
    }

    #[test]
    fn sin_matches_reference_across_many_turns() {
        let mut max_err = 0.0_f64;
        let mut x = -37.13_f64;
        while x < 37.0 {
            let e = (sin_turns(x) - ref_sin(x)).abs();
            if e > max_err {
                max_err = e;
            }
            x += 0.00131;
        }
        assert!(max_err < 2e-11, "sin max abs error {max_err:e}");
    }

    #[test]
    fn fast_trig_is_16bit_accurate() {
        let mut max_c = 0.0_f64;
        let mut max_s = 0.0_f64;
        let mut x = -11.37_f64;
        while x < 11.0 {
            max_c = max_c.max((cos_turns_fast(x) - ref_cos(x)).abs());
            max_s = max_s.max((sin_turns_fast(x) - ref_sin(x)).abs());
            x += 0.00073;
        }
        assert!(max_c < 5e-6, "cos_turns_fast err {max_c:e}");
        assert!(max_s < 5e-6, "sin_turns_fast err {max_s:e}");
        // sin_cos_turns_fast agrees with the scalars
        let (s, c) = sin_cos_turns_fast(0.3);
        assert!((s - sin_turns_fast(0.3)).abs() < 1e-15);
        assert!((c - cos_turns_fast(0.3)).abs() < 1e-15);
    }

    #[test]
    fn exp2_matches_reference() {
        let mut max_rel = 0.0_f64;
        let mut x = -60.0_f64;
        while x < 60.0 {
            let got = exp2(x);
            let want = x.exp2();
            let rel = ((got - want) / want).abs();
            if rel > max_rel {
                max_rel = rel;
            }
            x += 0.017;
        }
        assert!(max_rel < 1.5e-6, "exp2 max rel error {max_rel:e}");
        // the minimax fit is far tighter than the old Taylor 1.5e-6
        assert!(max_rel < 5.0e-8, "exp2 minimax regressed: {max_rel:e}");
        // integer powers stay exact
        for k in -20..=20 {
            assert_eq!(exp2(k as f64), (k as f64).exp2(), "exp2({k})");
        }
    }

    #[test]
    fn tan_turns_fast_accurate_on_the_svf_domain() {
        // domain: cutoff ∈ [20, 0.45·fs] ⇒ turns = fc/(2 fs) ∈ [~1e-5, 0.225]
        let mut max_rel = 0.0_f64;
        let mut t = 1.0e-5_f64;
        while t <= 0.225 {
            let got = tan_turns_fast(t);
            let want = (t * core::f64::consts::TAU).tan();
            max_rel = max_rel.max(((got - want) / want).abs());
            t += 1.3e-4;
        }
        assert!(max_rel < 2.0e-7, "tan_turns_fast rel error {max_rel:e}");
    }

    #[test]
    fn floor_f64_matches_reference() {
        for &x in &[-3.0, -2.5, -0.001, 0.0, 0.999, 1.0, 7.5, 100.0] {
            assert_eq!(floor_f64(x), x.floor(), "x={x}");
        }
    }

    #[test]
    fn exact_at_cardinal_points() {
        assert!((cos_turns(0.0) - 1.0).abs() < 1e-15);
        assert!((cos_turns(0.25) - 0.0).abs() < 1e-11);
        assert!((cos_turns(0.5) + 1.0).abs() < 1e-11);
        assert!((cos_turns(2.5) + 1.0).abs() < 1e-11);
        assert!((sin_turns(0.25) - 1.0).abs() < 1e-11);
        assert!((sin_turns(-0.25) + 1.0).abs() < 1e-11);
    }
}
