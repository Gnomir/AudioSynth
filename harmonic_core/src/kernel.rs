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
    let nf = n as f64;
    let c1 = cos_turns(p);
    let cn = cos_turns(nf * p);
    let cn1 = cos_turns((nf + 1.0) * p);

    let rn1 = powi_pos(r, n + 1);
    let rn2 = rn1 * r;

    let num = r * c1 - r * r - rn1 * cn1 + rn2 * cn;
    let den = 1.0 - 2.0 * r * c1 + r * r;
    num / den
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
    let peak = r * (1.0 - powi_pos(r, n)) / (1.0 - r);
    if peak > 0.0 {
        peak
    } else {
        1.0
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
