//! RFC-16 DoD #4: measures the CPU cost of HQ mode, old architecture (each
//! voice decimates independently, via `Voice::render_sample`'s own still-
//! intact per-voice HQ path — the standalone/C-ABI path, and exactly what
//! `PolySynth` used to do internally) vs new (`PolySynth`'s unified HQ bus:
//! every voice emits an un-decimated `2×` subsample pair, one master
//! decimator for the whole stereo mix).
//!
//!   cargo run --example bench_hq_bus --release
//!
//! 24 voices (the plugin's voice count), all held, with `drive`+`fold` active
//! so the nonlinear `Character` stage actually engages (the scenario HQ mode
//! exists for).

use harmonic_core::filter::FilterMode;
use harmonic_core::{CharParams, PolySynth, Voice};
use std::time::Instant;

const VOICES: usize = 24;
const FRAMES: usize = 240_000; // 5 s @ 48k
const ITERS: usize = 4;

fn hq_params() -> CharParams {
    CharParams { drive: 0.6, fold: 0.4, ..CharParams::CLEAN }
}

/// Old architecture, reconstructed faithfully from primitives that are still
/// live in the crate: `VOICES` independent `Voice`s, each with its own HQ
/// path (own 2× oversample + own decimate, exactly `Voice::render_sample`'s
/// intact standalone behaviour), manually summed and clipped once — this is
/// exactly what `PolySynth::render_sample` did before RFC-16.
fn bench_old_per_voice_decimation() -> f64 {
    let fs = 48_000.0;
    let mut voices: Vec<Voice> = (0..VOICES)
        .map(|i| {
            let mut v = Voice::new(fs);
            v.set_frequency(55.0 * (1.0 + i as f64 * 0.37));
            v.set_gain(0.5);
            v.set_rolloff(0.9);
            v.set_character(hq_params());
            v.set_filter_mode(FilterMode::Low);
            v.set_filter_cutoff(6_000.0);
            v.set_filter_resonance(0.5);
            v.set_hq(true);
            v.reset();
            v
        })
        .collect();

    let render_one = |voices: &mut [Voice]| -> [f32; 2] {
        let mut l = 0.0f32;
        let mut r = 0.0f32;
        for v in voices.iter_mut() {
            let [vl, vr] = v.render_sample();
            l += vl;
            r += vr;
        }
        [harmonic_core::poly::soft_clip(l), harmonic_core::poly::soft_clip(r)]
    };

    for _ in 0..FRAMES {
        std::hint::black_box(render_one(&mut voices));
    }
    let t = Instant::now();
    for _ in 0..ITERS {
        for _ in 0..FRAMES {
            std::hint::black_box(render_one(&mut voices));
        }
    }
    (FRAMES * ITERS) as f64 / t.elapsed().as_secs_f64()
}

/// New architecture: `PolySynth`'s unified HQ bus.
fn bench_new_unified_bus() -> f64 {
    let fs = 48_000.0;
    let mut synth: PolySynth<VOICES> = PolySynth::new(fs);
    synth.set_rolloff(0.9);
    synth.set_gain(0.5);
    synth.set_character(hq_params());
    synth.set_filter(FilterMode::Low, 6_000.0, 0.5, 0.0);
    synth.set_hq(true);
    for i in 0..VOICES {
        let note = 24 + (i as u8 * 2) % 48;
        synth.note_on(note, 0.8);
    }

    for _ in 0..FRAMES {
        std::hint::black_box(synth.render_sample());
    }
    let t = Instant::now();
    for _ in 0..ITERS {
        for _ in 0..FRAMES {
            std::hint::black_box(synth.render_sample());
        }
    }
    (FRAMES * ITERS) as f64 / t.elapsed().as_secs_f64()
}

fn main() {
    let old_sps = bench_old_per_voice_decimation();
    let new_sps = bench_new_unified_bus();
    let fs = 48_000.0;
    println!(
        "old (per-voice decimate): {:.2} M samp/s  ({:.1}x realtime)",
        old_sps / 1e6,
        old_sps / fs
    );
    println!(
        "new (unified HQ bus):     {:.2} M samp/s  ({:.1}x realtime)",
        new_sps / 1e6,
        new_sps / fs
    );
    println!("change: {:+.1}%", (new_sps / old_sps - 1.0) * 100.0);
}
