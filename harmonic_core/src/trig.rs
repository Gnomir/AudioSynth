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

#[allow(dead_code)]
const _MAX_KERNEL_ARG: f64 = FRAC_PI_4; // documents the kernel domain bound

// ============================================================================
// Batched, branchless cos — SIMD-friendly (§ SIMD task)
// ============================================================================

/// Round-to-nearest without a branch (`|x| < 2^51`). Round-half-to-even; the
/// tiny difference from [`round_int`] at exact halves does not matter for phase.
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
/// the fractional part goes through an 8-term (Taylor-through-`f⁷`) Horner
/// polynomial. Relative error `< 1.5e-6` on `f ∈ [0,1]` — about `2e-3` cents as
/// a pitch ratio, inaudible.
// The polynomial coefficients are the Taylor series of 2^f: ln2, (ln2)²/2, …
// clippy flags the first as "approximately LN_2" — it is exactly that, on purpose.
#[allow(clippy::approx_constant)]
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

    // 2^f, f ∈ [0,1]
    let p = 0.000_015_252_733_804_f64;
    let p = p * f + 0.000_154_035_303_934;
    let p = p * f + 0.001_333_355_814_643;
    let p = p * f + 0.009_618_129_107_628;
    let p = p * f + 0.055_504_108_664_822;
    let p = p * f + 0.240_226_506_959_101;
    let p = p * f + 0.693_147_180_559_945;
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
