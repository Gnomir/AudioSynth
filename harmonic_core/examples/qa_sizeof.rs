//! Trivial, exact fact-checks that don't need any signal analysis: struct
//! sizes and latency constants, read directly from the compiled type layout
//! rather than quoted from documentation.
use harmonic_core::Voice;

fn main() {
    println!("size_of::<Voice>() = {} bytes", std::mem::size_of::<Voice>());
    println!("align_of::<Voice>() = {} bytes", std::mem::align_of::<Voice>());
    println!("Voice::HQ_LATENCY = {} samples", Voice::HQ_LATENCY);
}
