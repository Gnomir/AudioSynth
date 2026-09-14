// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 Mironov Mykhailo Viktorovych.
// A commercial licence without the AGPL obligations is available —
// see product/COMMERCIAL_LICENSE.md.

//! The closed-form spectral sums. Pure functions, no state.
//!
//! Phase `p` is in **turns**: harmonic `k` is `cos(2π k p)`.

use crate::trig::cos_turns;

/// `r` raised to a non-negative integer power, by squaring. `O(log exp)`.
/// Replaces `f64::powi` (which is `std`).
#[inline]
pub fn powi_pos(mut base: f64, mut exp: u32) -> f64 {
    let mut acc = 1.0_f64;
    while exp > 0 {
        if exp & 1 == 1 {
            acc *= base;
        }
        base *= base;
        exp >>= 1;
    }
    acc
}

/// Band-limited impulse train (BLIT): sum of the first `n` unit-amplitude
/// cosine harmonics at phase `p`.
///
/// ```text
///   Σ_{k=1}^{n} cos(2π k p)  =  sin(π(2n+1)p) / (2 sin(π p))  −  1/2
/// ```
///
/// Exact finite sum → spectrum is flat over exactly `n` partials and zero
/// above, so it cannot alias while `n ≤ ⌊fs / (2 f0)⌋`.
///
/// Peak value is `n` (at `p → 0`); mean over a period is `0`.
#[inline]
pub fn dirichlet_blit(p: f64, n: u32) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let nf = n as f64;
    let half = 0.5 * p;
    // sin(π p) = cos_turns(half − 0.25) ; guard the removable singularity at
    // p ≡ 0 (mod 1), where the true value is the peak, n.
    let denom = cos_turns(half - 0.25);
    if fabs(denom) < 1.0e-9 {
        return nf;
    }
    // sin(π(2n+1)p) = cos_turns((2n+1)·half − 0.25)
    let num = cos_turns((2.0 * nf + 1.0) * half - 0.25);
    num / (2.0 * denom) - 0.5
}

/// Geometrically weighted partial sum: `Σ_{k=1}^{n} r^k cos(2π k p)`.
///
/// Closed form (real part of a truncated complex geometric series):
///
/// ```text
///   [ r·c₁ − r² − r^{n+1}·c_{n+1} + r^{n+2}·c_n ] / (1 − 2 r c₁ + r²)
/// ```
///
/// with `c_k = cos(2π k p)`.
///
/// * `r ∈ (0, 1)` is a spectral tilt: harmonic `k` has weight `r^k`, i.e. a
///   `20·log10(r)` dB step per harmonic — a "darker" tone as `r` falls.
/// * The denominator is `≥ (1 − r)² > 0` for `r < 1`: **no singular phase**,
///   unlike the raw Dirichlet form.
/// * `r ≥ 1` falls back to [`dirichlet_blit`] (the `r → 1⁻` limit).
/// * Still the exact finite sum: band-limited to `n` partials.
///
/// Peak value (at `p → 0`) is `Σ_{k=1}^{n} r^k = r(1 − r^n)/(1 − r)`.
#[inline]
pub fn geometric_partials(p: f64, r: f64, n: u32) -> f64 {
    if n == 0 {
        return 0.0;
    }
    if r >= 1.0 {
        return dirichlet_blit(p, n);
    }
    if r <= 0.0 {
        return 0.0; // Σ 0^k cos(..) = 0
    }
    geometric_partials_pre(p, r, n, powi_pos(r, n + 1))
}

/// [`geometric_partials`] with `r^{n+1}` supplied by the caller. Compute it once
/// with `powi_pos(r, n + 1)` and reuse it across every sample at a fixed `r`,
/// `n` — the per-sample cost then drops to the three cosines. Bit-identical to
/// [`geometric_partials`] for the same `rn1`.
#[inline]
pub fn geometric_partials_pre(p: f64, r: f64, n: u32, rn1: f64) -> f64 {
    if n == 0 {
        return 0.0;
    }
    if r >= 1.0 {
        return dirichlet_blit(p, n);
    }
    if r <= 0.0 {
        return 0.0;
    }
    let nf = n as f64;
    let c1 = cos_turns(p);
    let cn = cos_turns(nf * p);
    let cn1 = cos_turns((nf + 1.0) * p);

    let rn2 = rn1 * r;

    let num = r * c1 - r * r - rn1 * cn1 + rn2 * cn;
    let den = 1.0 - 2.0 * r * c1 + r * r;
    num / den
}

/// [`geometric_partials_pre`] with the `(n+1)`-th partial faded in at weight
/// `frac ∈ [0, 1)` — a continuous partial count. `S_{n+frac}(p) = S_n(p) +
/// frac · r^{n+1} · cos(2π(n+1)p)`, the exact next term of the finite sum. Costs
/// one extra `cos` over the integer form. `frac = 0` is **not** special-cased
/// here — callers gate on it so `frac = 0` takes [`geometric_partials_pre`]
/// verbatim (bit-exactness).
#[inline]
pub fn geometric_partials_pre_frac(p: f64, r: f64, n: u32, rn1: f64, frac: f64) -> f64 {
    let base = geometric_partials_pre(p, r, n, rn1);
    if n == 0 || r >= 1.0 || r <= 0.0 {
        // degenerate branches of `geometric_partials_pre` — no well-defined
        // `r^{n+1}` partial to add (or `rn1` is not `r^{n+1}`).
        return base;
    }
    base + frac * rn1 * cos_turns((n as f64 + 1.0) * p)
}

/// Peak amplitude of [`geometric_partials`] for the given `r`, `n` — used for
/// normalisation so the rendered signal sits in `[-1, 1]`-ish.
#[inline]
pub fn geometric_peak(r: f64, n: u32) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if r >= 1.0 {
        return n as f64;
    }
    if r <= 0.0 {
        return 1.0;
    }
    geometric_peak_pre(r, n, powi_pos(r, n))
}

/// [`geometric_peak`] with `r^n` supplied by the caller (see
/// [`geometric_partials_pre`]). Bit-identical to [`geometric_peak`] for the
/// same `rn`.
#[inline]
pub fn geometric_peak_pre(r: f64, n: u32, rn: f64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if r >= 1.0 {
        return n as f64;
    }
    if r <= 0.0 {
        return 1.0;
    }
    let peak = r * (1.0 - rn) / (1.0 - r);
    if peak > 0.0 {
        peak
    } else {
        1.0
    }
}

/// Resonant-hump spectral term: the difference of two geometric rolloffs,
///
/// ```text
///   Σ_{k=1}^{n} (aᵏ − bᵏ) cos(2π k p)   =   S_n(p, a) − S_n(p, b)
/// ```
///
/// With `1 > a > b > 0` the weight `aᵏ − bᵏ` is `0`-ish at `k = 1`, rises to a
/// peak near `k* = ln(ln b / ln a) / ln(a/b)`, and decays after — a
/// controllable formant-like bump to add on top of the main `Σ rᵏ` spectrum.
/// Still a closed form, still `Θ(log n)`: two [`geometric_partials_pre`] calls,
/// so the caller supplies `an1 = a^{n+1}` and `bn1 = b^{n+1}` the same way.
/// Degenerate `a`/`b` (outside `(0,1)`, or `a ≤ b`) just yield a small or zero
/// term — never a NaN.
#[inline]
pub fn geometric_hump_pre(p: f64, a: f64, b: f64, n: u32, an1: f64, bn1: f64) -> f64 {
    geometric_partials_pre(p, a, n, an1) - geometric_partials_pre(p, b, n, bn1)
}

/// Peak (at `p → 0`) of [`geometric_hump_pre`]: `Σ aᵏ − Σ bᵏ`, `> 0` for
/// `a > b`. `an = a^n`, `bn = b^n` supplied by the caller (as for
/// [`geometric_peak_pre`]). Used to normalise the combined spectrum.
#[inline]
pub fn geometric_hump_peak(a: f64, b: f64, n: u32, an: f64, bn: f64) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let sum = |r: f64, rn: f64| {
        if r > 0.0 && r < 1.0 {
            r * (1.0 - rn) / (1.0 - r)
        } else {
            0.0
        }
    };
    let d = sum(a, an) - sum(b, bn);
    if d > 0.0 {
        d
    } else {
        0.0
    }
}

#[inline(always)]
fn fabs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & 0x7fff_ffff_ffff_ffff)
}

// ============================================================================
// Batched oscillator — 4 consecutive samples at once (SIMD task)
// ============================================================================

/// [`geometric_partials`] for four consecutive samples: phases
/// `p0, p0+dp, p0+2dp, p0+3dp`.
///
/// The three cosine evaluations are batched through [`crate::trig::cos4_turns`]
/// (branchless), which LLVM auto-vectorises to `VFMADD` / `FMLA` on x86-64 and
/// AArch64, and lowers to correct scalar code on targets without SIMD
/// (Cortex-M). Results are bit-identical to calling [`geometric_partials`] four
/// times, to within the branchless `cos`'s few-ULP tolerance.
///
/// For an explicit `core::simd` implementation (nightly), build with
/// `--features portable-simd` and use [`geometric_partials_x4_simd`].
#[inline]
pub fn geometric_partials_x4(p0: f64, dp: f64, r: f64, n: u32) -> [f64; 4] {
    if n == 0 {
        return [0.0; 4];
    }
    let p = [p0, p0 + dp, p0 + 2.0 * dp, p0 + 3.0 * dp];

    if r >= 1.0 {
        return [
            dirichlet_blit(p[0], n),
            dirichlet_blit(p[1], n),
            dirichlet_blit(p[2], n),
            dirichlet_blit(p[3], n),
        ];
    }
    if r <= 0.0 {
        return [0.0; 4];
    }

    let nf = n as f64;
    let c1 = crate::trig::cos4_turns(p);
    let cn = crate::trig::cos4_turns([p[0] * nf, p[1] * nf, p[2] * nf, p[3] * nf]);
    let cn1 = crate::trig::cos4_turns([
        p[0] * (nf + 1.0),
        p[1] * (nf + 1.0),
        p[2] * (nf + 1.0),
        p[3] * (nf + 1.0),
    ]);

    let rn1 = powi_pos(r, n + 1);
    let rn2 = rn1 * r;
    let rr = r * r;

    let mut out = [0.0_f64; 4];
    let mut i = 0;
    while i < 4 {
        let num = r * c1[i] - rr - rn1 * cn1[i] + rn2 * cn[i];
        let den = 1.0 - 2.0 * r * c1[i] + rr;
        out[i] = num / den;
        i += 1;
    }
    out
}

/// Explicit `core::simd` version of [`geometric_partials_x4`]. Nightly only —
/// gated behind the `portable-simd` feature (which turns on `#![feature(
/// portable_simd)]`). The geometric formula runs on `f64x4`; the cosine still
/// goes through the branchless scalar kernel per lane, which the compiler then
/// packs — the `core::simd` mask/select surface changes too often to pin here.
#[cfg(feature = "portable-simd")]
#[inline]
pub fn geometric_partials_x4_simd(p0: f64, dp: f64, r: f64, n: u32) -> [f64; 4] {
    use core::simd::f64x4;

    if n == 0 || r <= 0.0 {
        return [0.0; 4];
    }
    if r >= 1.0 {
        return geometric_partials_x4(p0, dp, r, n); // singularity-guarded path
    }

    let p = [p0, p0 + dp, p0 + 2.0 * dp, p0 + 3.0 * dp];
    let nf = n as f64;
    let c1 = f64x4::from_array(crate::trig::cos4_turns(p));
    let cn = f64x4::from_array(crate::trig::cos4_turns([
        p[0] * nf,
        p[1] * nf,
        p[2] * nf,
        p[3] * nf,
    ]));
    let cn1 = f64x4::from_array(crate::trig::cos4_turns([
        p[0] * (nf + 1.0),
        p[1] * (nf + 1.0),
        p[2] * (nf + 1.0),
        p[3] * (nf + 1.0),
    ]));

    let rv = f64x4::splat(r);
    let rr = f64x4::splat(r * r);
    let rn1 = f64x4::splat(powi_pos(r, n + 1));

    let num = rv * c1 - rr - rn1 * cn1 + rn1 * rv * cn;
    let den = f64x4::splat(1.0) - f64x4::splat(2.0) * rv * c1 + rr;
    (num / den).to_array()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive_blit(p: f64, n: u32) -> f64 {
        let mut s = 0.0;
        for k in 1..=n {
            s += (k as f64 * p * core::f64::consts::TAU).cos();
        }
        s
    }

    fn naive_geometric(p: f64, r: f64, n: u32) -> f64 {
        let mut s = 0.0;
        for k in 1..=n {
            s += r.powi(k as i32) * (k as f64 * p * core::f64::consts::TAU).cos();
        }
        s
    }

    #[test]
    fn dirichlet_matches_naive_sum() {
        for &n in &[1u32, 2, 4, 15, 64, 255, 1024] {
            let mut max_err = 0.0_f64;
            let mut p = 0.0_f64;
            while p < 1.0 {
                let e = (dirichlet_blit(p, n) - naive_blit(p, n)).abs();
                // tolerance scales with n: phase*k amplifies the ~1e-11 trig error
                let tol = 1e-9 * (n as f64);
                assert!(e < tol.max(1e-7), "n={n} p={p} err={e:e}");
                if e > max_err {
                    max_err = e;
                }
                p += 0.0007;
            }
        }
    }

    #[test]
    fn geometric_matches_naive_sum() {
        for &n in &[1u32, 4, 32, 200, 1024] {
            for &r in &[0.3_f64, 0.7, 0.9, 0.99, 0.999] {
                let mut p = 0.0_f64;
                while p < 1.0 {
                    let e = (geometric_partials(p, r, n) - naive_geometric(p, r, n)).abs();
                    let tol = (1e-9 * (n as f64)).max(1e-6);
                    assert!(e < tol, "n={n} r={r} p={p} err={e:e}");
                    p += 0.0013;
                }
            }
        }
    }

    #[test]
    fn dirichlet_peak_and_dc() {
        let n = 32;
        assert!((dirichlet_blit(0.0, n) - n as f64).abs() < 1e-9);
        // mean over a period ≈ 0
        let mut acc = 0.0;
        let steps = 20_000;
        for i in 0..steps {
            acc += dirichlet_blit(i as f64 / steps as f64, n);
        }
        assert!((acc / steps as f64).abs() < 1e-2, "DC = {}", acc / steps as f64);
    }

    #[test]
    fn batched_x4_matches_scalar() {
        for &n in &[1u32, 4, 37, 300, 1500] {
            for &r in &[0.2_f64, 0.6, 0.9, 0.995, 1.0] {
                let dp = 0.013;
                let mut p0 = 0.0;
                while p0 < 1.0 {
                    let b = geometric_partials_x4(p0, dp, r, n);
                    for (i, &got) in b.iter().enumerate() {
                        let want = geometric_partials(p0 + dp * i as f64, r, n);
                        assert!(
                            (got - want).abs() < 5e-6 * (n as f64) + 1e-6,
                            "n={n} r={r} p0={p0} lane={i}: {got} vs {want}"
                        );
                    }
                    p0 += 0.041;
                }
            }
        }
    }

    #[test]
    fn pre_variants_are_bit_identical() {
        // The `_pre` forms (used by Voice's per-(r,n) cache) must match the
        // originals bit-for-bit, not just approximately.
        for &n in &[1u32, 3, 27, 218, 1200, 2048] {
            for &r in &[1.0e-3_f64, 0.3, 0.7, 0.9, 0.99, 0.9995] {
                assert_eq!(geometric_peak(r, n), geometric_peak_pre(r, n, powi_pos(r, n)));
                let rn1 = powi_pos(r, n + 1);
                let mut p = 0.0_f64;
                while p < 1.0 {
                    assert_eq!(
                        geometric_partials(p, r, n),
                        geometric_partials_pre(p, r, n, rn1),
                        "n={n} r={r} p={p}"
                    );
                    p += 0.017;
                }
            }
        }
    }

    #[test]
    fn frac_partial_at_one_equals_the_next_integer_partial() {
        // `geometric_partials_pre_frac(.., frac=1.0)` adds a full r^{n+1}
        // partial, so it must equal S_{n+1} — proving the fractional term is
        // *exactly* the next member of the finite sum, not an approximation.
        for &n in &[1u32, 3, 27, 200, 1000] {
            for &r in &[0.3_f64, 0.7, 0.9, 0.99] {
                let rn1 = powi_pos(r, n + 1);
                let rn2 = powi_pos(r, n + 2);
                let mut p = 0.003_f64;
                while p < 1.0 {
                    let faded = geometric_partials_pre_frac(p, r, n, rn1, 1.0);
                    let next = geometric_partials_pre(p, r, n + 1, rn2);
                    // the two paths reach cos(2π(n+1)p) by different routes, so
                    // they differ by a few ULP of trig noise at large n·p, not
                    // by a whole partial.
                    assert!((faded - next).abs() < 1e-9, "n={n} r={r} p={p}: {faded} vs {next}");
                    p += 0.019;
                }
            }
        }
    }

    #[test]
    fn hump_matches_the_naive_weighted_sum_and_bumps_the_mids() {
        // 1. Closed form == Σ (aᵏ − bᵏ) cos(2πkp), the difference of two rolloffs.
        let naive = |p: f64, a: f64, b: f64, n: u32| {
            let mut s = 0.0;
            for k in 1..=n {
                let w = powi_pos(a, k) - powi_pos(b, k);
                s += w * (k as f64 * p * core::f64::consts::TAU).cos();
            }
            s
        };
        for &(a, b) in &[(0.92_f64, 0.79_f64), (0.977, 0.932), (0.8, 0.5)] {
            for &n in &[8u32, 40, 300] {
                let (an1, bn1) = (powi_pos(a, n + 1), powi_pos(b, n + 1));
                let mut p = 0.002;
                while p < 1.0 {
                    let got = geometric_hump_pre(p, a, b, n, an1, bn1);
                    let want = naive(p, a, b, n);
                    assert!((got - want).abs() < 1e-6 * n as f64 + 1e-6, "a={a} b={b} n={n} p={p}: {got} vs {want}");
                    p += 0.017;
                }
            }
        }

        // 2. The weight aᵏ − bᵏ genuinely peaks in the mid-partials, not at k=1.
        for &(a, b, want_k) in &[(0.924_f64, 0.79_f64, 7u32), (0.977, 0.932, 23)] {
            let w = |k: u32| powi_pos(a, k) - powi_pos(b, k);
            let (mut kmax, mut vmax) = (1u32, w(1));
            for k in 2..=48 {
                if w(k) > vmax {
                    vmax = w(k);
                    kmax = k;
                }
            }
            assert!(kmax.abs_diff(want_k) <= 2, "hump for a={a} b={b} peaked at k={kmax}, wanted ≈{want_k}");
            assert!(w(1) < 0.6 * vmax, "hump not humped: w(1)={} peak={}", w(1), vmax);
        }

        // 3. Peak helper is positive and matches Σ of the weights.
        let (a, b, n) = (0.95_f64, 0.8_f64, 60u32);
        let direct: f64 = (1..=n).map(|k| powi_pos(a, k) - powi_pos(b, k)).sum();
        let helper = geometric_hump_peak(a, b, n, powi_pos(a, n), powi_pos(b, n));
        assert!((direct - helper).abs() < 1e-9 && helper > 0.0, "{direct} vs {helper}");
    }

    #[test]
    fn geometric_reduces_to_fundamental_for_small_r() {
        // r very small: r^2 term negligible → nearly a pure cosine at f0
        let r = 1e-3;
        let n = 64;
        let mut p = 0.0;
        while p < 1.0 {
            let got = geometric_partials(p, r, n) / geometric_peak(r, n);
            let want = (p * core::f64::consts::TAU).cos();
            assert!((got - want).abs() < 5e-3, "p={p} got={got} want={want}");
            p += 0.01;
        }
    }
}
