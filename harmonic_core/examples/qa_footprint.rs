//! Exact, analytical memory footprint of the engine's own state, plus its
//! construction time — neither was previously measured (QA_REPORT.md had
//! only whole-host RSS, 149-155MB, dominated by the DAW host process and
//! the plugin's GUI toolkit, not the engine). `size_of` on the actual,
//! fixed-size structs gives a number that's exactly right rather than
//! approximately subtracted; timing `PolySynth::new` (no I/O, no
//! allocation — no_std, no allocator) gives a real construction-time floor.
//!
//!   cargo run --release --example qa_footprint

use harmonic_core::{PolySynth, Voice};
use std::mem::size_of;
use std::time::Instant;

fn main() {
    // Construction time — not previously measured at all. `new` does no
    // I/O and no allocation (no_std, no allocator), so this is close to a
    // pure floor: whatever a real plugin adds on top (GUI init, license
    // check, host negotiation) is host/GUI-toolkit time, not engine time.
    const RUNS: usize = 10_000;
    let start = Instant::now();
    for _ in 0..RUNS {
        std::hint::black_box(PolySynth::<24>::new(48_000.0));
    }
    let elapsed = start.elapsed();
    println!(
        "PolySynth::<24>::new() construction: {:.1}ns average over {RUNS} runs ({:.3}ms total)",
        elapsed.as_nanos() as f64 / RUNS as f64,
        elapsed.as_secs_f64() * 1000.0
    );
    println!();

    let voice = size_of::<Voice>();
    // The real plugin's polyphony limit (harmonic_synth::MAX_VOICES) — kept
    // in sync by hand since harmonic_synth is a separate (private) repo.
    const MAX_VOICES: usize = 24;
    let poly = size_of::<PolySynth<MAX_VOICES>>();

    println!("size_of::<Voice>()                 = {voice} bytes");
    println!(
        "size_of::<PolySynth<{MAX_VOICES}>>()        = {poly} bytes ({:.2} KiB)",
        poly as f64 / 1024.0
    );
    println!(
        "  ({MAX_VOICES} voices x {voice} B = {} B accounted for by the Voice array alone; \
the remaining {} B is PolySynth's own state — the HQ bus decimator, unison/pan tables, etc.)",
        MAX_VOICES * voice,
        poly.saturating_sub(MAX_VOICES * voice)
    );
    println!(
        "\nThis is the entire audio-thread-visible engine state for one plugin instance — \
no heap allocation exists on this path (no_std, no allocator). The 149-155MB whole-host RSS \
figure elsewhere in the QA report is overwhelmingly the DAW host process and the plugin's \
GUI toolkit, not this."
    );
}
