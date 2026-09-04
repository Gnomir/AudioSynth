//! Polyphony headroom: how many simultaneous voices render in real time.
//!
//!   cargo run --example bench_poly --release
//!
//! Holds a big chord (all `VOICES` slots active), renders a few seconds of
//! stereo, reports throughput and the implied max polyphony at 48 kHz.

use harmonic_core::PolySynth;
use std::time::Instant;

const VOICES: usize = 64;

fn main() {
    let fs = 48_000.0;
    let mut synth: PolySynth<VOICES> = PolySynth::new(fs);

    // Fill every voice: a wide cluster so none are stolen.
    for i in 0..VOICES {
        let note = 24 + (i as u8 * 2) % 72;
        synth.note_on(note, 0.8);
    }
    let frames = 240_000; // 5 s
    // warmup
    for _ in 0..frames {
        synth.render_sample();
    }

    let iters = 4;
    let t = Instant::now();
    for _ in 0..iters {
        for _ in 0..frames {
            std::hint::black_box(synth.render_sample());
        }
    }
    let secs = t.elapsed().as_secs_f64();
    let sps = (frames as f64 * iters as f64) / secs;
    let active = synth.active_voice_count();

    println!(
        "{active} voices active  ·  {:.2} M stereo-frames/s  ·  {:.1}x realtime @48k  ·  ~{} voices @ realtime",
        sps / 1e6,
        sps / fs,
        (active as f64 * sps / fs) as u32
    );
}
