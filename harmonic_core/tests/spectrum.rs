//! End-to-end checks on the rendered signal:
//!   1. the closed form equals the brute-force partial sum, and
//!   2. the rendered voice has no measurable energy above its Nyquist clamp
//!      (i.e. it does not alias).
//!
//! No FFT dependency — a single-bin Goertzel-style DFT is enough.

use harmonic_core::kernel::{dirichlet_blit, geometric_partials};
use harmonic_core::Voice;

const TAU: f64 = std::f64::consts::TAU;

/// |X(f)| of `x` at absolute frequency `f` Hz, normalised by N.
fn dft_bin_mag(x: &[f32], fs: f64, f: f64) -> f64 {
    let w = TAU * f / fs;
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
fn closed_form_equals_bruteforce() {
    let naive = |p: f64, n: u32| {
        (1..=n).map(|k| (k as f64 * p * TAU).cos()).sum::<f64>()
    };
    for &n in &[1u32, 3, 16, 100, 512, 2048] {
        let mut p = 0.0;
        let mut worst = 0.0_f64;
        while p < 1.0 {
            worst = worst.max((dirichlet_blit(p, n) - naive(p, n)).abs());
            p += 0.00037;
        }
        // ~1e-11 trig error, amplified by the (2n+1)·p argument
        assert!(worst < 5e-8 * n as f64 + 1e-6, "n={n} worst={worst:e}");
    }
}

#[test]
fn geometric_is_a_true_finite_sum() {
    // If the closed form secretly summed to infinity, truncating at n and at
    // 4n would differ. They must not (below trig noise).
    let n = 200;
    let mut p = 0.017;
    while p < 1.0 {
        let a = geometric_partials(p, 0.98, n);
        let b: f64 = (1..=n).map(|k| 0.98_f64.powi(k as i32) * (k as f64 * p * TAU).cos()).sum();
        assert!((a - b).abs() < 1e-6, "p={p} closed={a} sum={b}");
        p += 0.011;
    }
}

#[test]
fn rendered_voice_does_not_alias() {
    let fs = 48_000.0;
    let f0 = 440.0;
    let mut v = Voice::new(fs);
    v.set_frequency(f0);
    v.set_rolloff(0.995); // bright — worst case for aliasing
    v.set_gain(1.0);
    v.reset();

    let mut buf = vec![0.0_f32; 1 << 16];
    let mut scratch = vec![0.0_f32; 1 << 16];
    v.render_block(&mut buf, &mut scratch); // pan defaults to centre → L == R

    // Nyquist clamp: 48000 / (2*440) = 54 partials → highest partial ≈ 23.76 kHz
    let n_partials = (fs / (2.0 * f0)) as u32; // 54
    let highest = f0 * n_partials as f64;

    // Energy must be present at the fundamental...
    let e_f0 = dft_bin_mag(&buf, fs, f0);
    assert!(e_f0 > 1e-3, "fundamental missing: {e_f0:e}");

    // ...and at a mid partial...
    let e_mid = dft_bin_mag(&buf, fs, f0 * 10.0);
    assert!(e_mid > 1e-4, "10th partial missing: {e_mid:e}");

    // ...but effectively nothing above the clamp (would-be aliases).
    for m in 1..8 {
        let f_above = highest + f0 * m as f64;
        if f_above >= fs / 2.0 {
            break;
        }
        let leak = dft_bin_mag(&buf, fs, f_above);
        assert!(
            leak < 1e-4,
            "aliasing: {leak:e} at {f_above:.0} Hz (partial {})",
            n_partials as usize + m
        );
    }

    // And a classic alias target: fs - f0*(n_partials+3) folded back.
    let alias_src = f0 * (n_partials as f64 + 3.0);
    let folded = (fs - alias_src).abs();
    if folded < fs / 2.0 && folded > 20.0 {
        let leak = dft_bin_mag(&buf, fs, folded);
        assert!(leak < 1e-4, "folded alias at {folded:.0} Hz: {leak:e}");
    }
}

#[test]
fn cost_is_flat_in_partial_count() {
    // Not a hard perf assertion (CI machines vary) — just confirms the call
    // does not scale with n the way an O(n) oscillator bank would.
    use std::time::Instant;

    let fs = 48_000.0;
    let render = |f0: f64| {
        let mut v = Voice::new(fs);
        v.set_frequency(f0);
        v.set_rolloff(0.99);
        v.reset();
        let mut l = vec![0.0_f32; 200_000];
        let mut r = vec![0.0_f32; 200_000];
        let t = Instant::now();
        v.render_block(&mut l, &mut r);
        t.elapsed().as_secs_f64()
    };

    let t_few = render(8_000.0); // 3 partials
    let t_many = render(40.0); // clamped to MAX_PARTIALS = 2048

    // O(n) would be ~600×. Allow a wide band for noisy machines.
    let ratio = t_many / t_few.max(1e-9);
    assert!(ratio < 25.0, "cost ratio {ratio:.1} looks O(n), not O(log n)");
}
