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
pub use voice::{Voice, Waveform};

/// Supported sample-rate range, in Hz. Outside this the prewarp `tan(π fc/fs)`
/// and the smoother time constants lose meaning.
pub const SAMPLE_RATE_MIN: f64 = 8_000.0;
pub const SAMPLE_RATE_MAX: f64 = 768_000.0;

/// Outcome of validating a host-supplied sample rate. `#[repr(i32)]` so it can
/// double as the C-ABI return code of [`ffi::harmonic_voice_init`].
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleRateStatus {
    /// Inside `[SAMPLE_RATE_MIN, SAMPLE_RATE_MAX]` — used exactly as requested.
    Ok = 0,
    /// Below the minimum — raised to `SAMPLE_RATE_MIN`. Pitch/time will be off;
    /// the host should pick a supported rate.
    ClampedLow = 1,
    /// Above the maximum — lowered to `SAMPLE_RATE_MAX`. Same caveat.
    ClampedHigh = 2,
    /// Not finite (NaN / ∞) — defaulted to 48 kHz.
    Defaulted = 3,
}

/// Clamp a requested sample rate into the supported range and report what
/// happened, instead of silently substituting a default.
#[inline]
pub fn validate_sample_rate(hz: f64) -> (f64, SampleRateStatus) {
    if !hz.is_finite() {
        (48_000.0, SampleRateStatus::Defaulted)
    } else if hz < SAMPLE_RATE_MIN {
        (SAMPLE_RATE_MIN, SampleRateStatus::ClampedLow)
    } else if hz > SAMPLE_RATE_MAX {
        (SAMPLE_RATE_MAX, SampleRateStatus::ClampedHigh)
    } else {
        (hz, SampleRateStatus::Ok)
    }
}

#[cfg(not(feature = "std"))]
#[panic_handler]
fn panic_handler(_: &core::panic::PanicInfo) -> ! {
    // The audio path is written to never panic. If a panic ever reaches here
    // in a real build, hanging is safer than unwinding through a C ABI.
    loop {}
}

#[cfg(test)]
mod sr_tests {
    use super::*;

    #[test]
    fn sample_rate_validation_reports_instead_of_substituting() {
        assert_eq!(validate_sample_rate(48_000.0), (48_000.0, SampleRateStatus::Ok));
        assert_eq!(validate_sample_rate(4_000.0), (8_000.0, SampleRateStatus::ClampedLow));
        assert_eq!(validate_sample_rate(2_000_000.0), (768_000.0, SampleRateStatus::ClampedHigh));
        assert_eq!(validate_sample_rate(f64::NAN).1, SampleRateStatus::Defaulted);
        assert_eq!(validate_sample_rate(f64::INFINITY).1, SampleRateStatus::Defaulted);

        let (v, s) = Voice::new_checked(1_000.0);
        assert_eq!(s, SampleRateStatus::ClampedLow);
        assert_eq!(v.sample_rate(), 8_000.0);

        let mut poly: PolySynth<4> = PolySynth::new(48_000.0);
        assert_eq!(poly.set_sample_rate(999_999.0), SampleRateStatus::ClampedHigh);
        assert_eq!(poly.sample_rate(), 768_000.0);
        assert_eq!(poly.set_sample_rate(96_000.0), SampleRateStatus::Ok);
    }
}
