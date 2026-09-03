//! Throughput of one voice vs partial count — confirms the cost is flat in `n`.
//!
//!   cargo run --example bench_hc --release

use harmonic_core::Voice;
use std::time::Instant;

fn run(f0: f64) -> (f64, u32) {
    let fs = 48_000.0;
    let mut v = Voice::new(fs);
    v.set_frequency(f0);
    v.set_rolloff(0.99);
    v.reset();
    let n = v.max_partials();

    let mut l = vec![0.0f32; 500_000];
    let mut r = vec![0.0f32; 500_000];
    v.render_block(&mut l, &mut r); // warmup

    let iters = 20;
    let t = Instant::now();
    for _ in 0..iters {
        v.render_block(&mut l, &mut r);
    }
    let secs = t.elapsed().as_secs_f64();
    let sps = (l.len() as f64 * iters as f64) / secs;
    (sps, n)
}

fn main() {
    for f0 in [8000.0, 880.0, 110.0, 20.0] {
        let (sps, n) = run(f0);
        println!(
            "f0={:>6.0} Hz  partials={:>4}  {:>8.1} M samples/s  ({:.1}x realtime @48k)",
            f0,
            n,
            sps / 1e6,
            sps / 48_000.0
        );
    }
}
