//! # harmonic_core
//!
//! An **honest** `no_std` DSP core for band-limited additive synthesis.
//!
//! ## What it actually does
//!
//! Synthesises a tone made of the first `n` harmonics of a fundamental `f0`,
//! all in one closed-form expression, using the **Dirichlet kernel**
//!
//! ```text
//!   Σ_{k=1}^{n} cos(2π k p)  =  sin(π(2n+1)p) / (2 sin(π p))  −  1/2
//! ```
//!
//! and its geometric-rolloff generalisation
//!
//! ```text
//!   Σ_{k=1}^{n} r^k cos(2π k p)
//!     = [ r·c₁ − r² − r^{n+1}·c_{n+1} + r^{n+2}·c_n ] / (1 − 2 r c₁ + r²)
//! ```
//!
//! where `c_k = cos(2π k p)`, `p` is the phase in turns, and `r ∈ (0,1)` is a
//! spectral-tilt ("brightness") control.
//!
//! ## Honest cost statement
//!
//! * Per output sample: a **fixed** number of trig evaluations and arithmetic
//!   ops, plus one `r^n` via exponentiation-by-squaring.
//! * That makes it **O(log n)** per sample, and **O(1)** whenever the partial
//!   count is held fixed. It is genuinely independent of `n` in the way that
//!   matters: no per-partial loop, no wavetable, no oversampling filter.
//! * It is **exactly band-limited** to `n` partials — the closed form *is* the
//!   finite sum, bit-for-bit (the `r^{n+1}` correction terms only underflow
//!   for `n` in the hundreds of thousands, far past any audio partial count).
//!   So it does not alias, provided the caller keeps `n ≤ ⌊fs / (2 f0)⌋`.
//!   [`Voice`] enforces that clamp.
//!
//! ## What it does NOT claim
//!
//! * No connection to quantum mechanics, Grover's algorithm, or `cos²(2εθ)`.
//!   The spectrum here is *designed* (flat, or `r^k` tilt), not borrowed from
//!   an unrelated identity.
//! * Not a free lunch on additive synthesis in general: an arbitrary
//!   per-partial amplitude/phase envelope still costs O(n). The O(1) trick
//!   works because flat and geometric spectra have closed forms (finite
//!   geometric series). Arbitrary spectra do not.
//! * An exact O(1) band-limited *sawtooth* (`Σ sin(kx)/k`) has no elementary
//!   closed form. The standard route is leaky-integrated BLIT
//!   (Stilson & Smith 1996) — noted here as future work, not implemented.
//!
//! ## no_std
//!
//! The DSP path uses only `core` float arithmetic — no `libm`, no `std` math,
//! no allocation, no locks, no panics on the audio path. Build the real
//! artifact with `--no-default-features --release`.

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(feature = "portable-simd", feature(portable_simd))]
#![deny(unsafe_op_in_unsafe_fn)]
// `f64::clamp` is not `core`-stable for our MSRV target, so the crate uses
// small hand-written clamp helpers on the no_std path. That is deliberate.
#![allow(clippy::manual_clamp)]

pub mod character;
pub mod trig;
pub mod kernel;
pub mod filter;
pub mod env;
pub mod lfo;
pub mod voice;
pub mod poly;
pub mod ffi;

pub use character::{CharParams, Character};
pub use env::Adsr;
pub use filter::{FilterMode, Svf};
pub use lfo::{Lfo, LfoShape};
pub use poly::{midi_to_hz, PolySynth};
pub use voice::Voice;

#[cfg(not(feature = "std"))]
#[panic_handler]
fn panic_handler(_: &core::panic::PanicInfo) -> ! {
    // The audio path is written to never panic. If a panic ever reaches here
    // in a real build, hanging is safer than unwinding through a C ABI.
    loop {}
}
