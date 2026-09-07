//! The canonical cross-platform verification render.
//!
//! One fixed, deterministic pass through the whole engine — unison, drift, FM,
//! feedback, all four LFO routings, a resonant SVF, every [`Character`] stage —
//! written frame-interleaved into a caller-owned buffer, plus an FNV-1a hash of
//! the result.
//!
//! `harmonic_core` uses no `libm` and no FMA (`docs/08 §3`): every hot-path op
//! is an IEEE-754 `+ − × ÷` or a bit reinterpret / saturating cast, which
//! IEEE-754 pins to one correctly-rounded result on every target. The
//! `cross_platform_bit_exact` integration test and the `wasm32` export below
//! both call [`render_verification`], so **x86-64, AArch64, ARMv7-hf and
//! `wasm32` all hash to the same [`VERIFY_HASH`]** — one number across the
//! plugin, the firmware target and the browser build.
//!
//! [`Character`]: crate::Character

use crate::{CharParams, FilterMode, LfoMode, LfoShape, PolySynth, Waveform};

/// Frames the verification render produces (stereo). A [`render_verification`]
/// buffer must hold `2 ×` this many `f32`s.
pub const VERIFY_FRAMES: usize = 4_800;

/// FNV-1a hash of a correct [`render_verification`] output (48 kHz). Produced on
/// `x86_64-pc-windows-msvc` (rustc 1.97.1, release) and confirmed identical
/// under QEMU on `aarch64-unknown-linux-gnu` / `armv7-unknown-linux-gnueabihf`
/// and in `wasm32` under Node (`scripts/verify-wasm.mjs`).
///
/// Regenerate with `RENDER_EMIT_HASH=1 cargo test --release --test
/// cross_platform_bit_exact -- --nocapture`.
pub const VERIFY_HASH: u64 = 0x272c_f9c7_ecba_f653;

/// Same as [`VERIFY_HASH`] for the pass rendered at **96 kHz** — proof that the
/// "identical on any machine at any sample rate" claim holds at more than one
/// rate. Regenerated the same way.
pub const VERIFY_HASH_96K: u64 = 0x2bd0_d67f_29d1_6c1f;

/// FNV-1a of [`render_verification_2`] (48 kHz) — a second scripted pass that
/// covers what the first one does not touch: the **HQ bus** (whole-voice `2×`
/// path + master half-band decimator + the `2×`-rate master DC blocker), the
/// `Saw` and `Triangle` PolyBLEP/PolyBLAMP waveforms, a **fractional**
/// `partial_limit` (the ⌊n⌋↔⌈n⌉ crossfade), and the closed-form **Formant**
/// hump term. Same regeneration command.
pub const VERIFY_2_HASH: u64 = 0x2566_0025_905b_edc4;

/// [`VERIFY_2_HASH`] rendered at **96 kHz**.
pub const VERIFY_2_HASH_96K: u64 = 0x8adc_3df7_df5e_7bb6;

/// Render the fixed verification pass into `out` — frame-interleaved `L, R`;
/// `out.len()` must be `>= VERIFY_FRAMES * 2`. Deterministic, no allocation,
/// no `std`. Any single-ULP divergence anywhere changes [`verify_hash`] of the
/// result. [`render_verification`] fixes the rate at 48 kHz.
pub fn render_verification_at(sample_rate: f64, out: &mut [f32]) {
    let mut synth: PolySynth<8> = PolySynth::new(sample_rate);

    synth.set_rolloff(0.93);
    synth.set_gain(0.8);
    synth.set_character(CharParams {
        drive: 0.55,
        bias: -0.2,
        fold: 0.35,
        crush: 0.4,
        downsample: 0.3,
    });
    synth.set_fm(2.0, 0.6);
    synth.set_feedback(0.25);
    synth.set_free_running(false);
    synth.set_unison(4, 12.0, 0.7, 0.8);
    synth.set_amp_adsr(0.005, 0.08, 0.6, 0.15);
    synth.set_filter(FilterMode::Low, 1_400.0, 0.8, 2.5);
    synth.set_filter_envelope(0.002, 0.05, 0.3, 0.12);
    synth.set_lfo(
        5.5,
        LfoShape::Triangle,
        LfoMode::FreeRun,
        0.3,  // → rolloff
        18.0, // → pitch (cents)
        1.5,  // → cutoff (octaves)
        0.4,  // → FM index
    );

    for i in 0..VERIFY_FRAMES {
        match i {
            0 => synth.note_on(45, 0.9),
            600 => synth.note_on(52, 0.7),
            1_200 => synth.set_pitch_bend(1.5),
            1_800 => synth.set_pitch_bend(-0.75),
            2_400 => synth.note_off(45),
            3_000 => synth.note_on(59, 1.0),
            3_600 => synth.note_off(52),
            4_200 => synth.note_off(59),
            _ => {}
        }
        let [l, r] = synth.render_sample();
        out[i * 2] = l;
        out[i * 2 + 1] = r;
    }
}

/// [`render_verification_at`] at 48 kHz — the canonical pass behind
/// [`VERIFY_HASH`].
pub fn render_verification(out: &mut [f32]) {
    render_verification_at(48_000.0, out);
}

/// The second scripted pass — see [`VERIFY_2_HASH`] for what it covers that
/// [`render_verification_at`] does not. Same contract: frame-interleaved
/// `L, R`, `out.len() >= VERIFY_FRAMES * 2`, deterministic, no `std`.
pub fn render_verification_2_at(sample_rate: f64, out: &mut [f32]) {
    let mut synth: PolySynth<8> = PolySynth::new(sample_rate);

    synth.set_hq(true); // whole-voice 2× bus + master decimator + 2× DC blocker
    synth.set_rolloff(0.8);
    synth.set_gain(0.75);
    synth.set_waveform(Waveform::Saw);
    synth.set_partial_limit(37.5); // fractional — ⌊n⌋↔⌈n⌉ crossfade
    synth.set_formant(0.55); // the second closed-form (hump) term
    synth.set_character(CharParams {
        drive: 0.4,
        bias: 0.15, // drive + bias together ⇒ the fold generates real DC
        fold: 0.5,
        crush: 0.0,
        downsample: 0.0,
    });
    synth.set_fm(0.5, 0.3);
    synth.set_unison(3, 9.0, 0.5, 0.4);
    synth.set_amp_adsr(0.004, 0.10, 0.7, 0.20);
    synth.set_filter(FilterMode::Band, 900.0, 0.6, 1.5);
    synth.set_filter_envelope(0.003, 0.06, 0.25, 0.15);
    synth.set_lfo(
        0.9,
        LfoShape::Sine,
        LfoMode::Retrigger,
        0.2, // → rolloff
        6.0, // → pitch (cents)
        0.8, // → cutoff (octaves)
        0.0, // → FM index
    );

    for i in 0..VERIFY_FRAMES {
        match i {
            0 => synth.note_on(40, 0.8),
            700 => synth.note_on(47, 1.0),
            1_500 => synth.set_pitch_bend(-1.0),
            2_400 => {
                // waveform switched *while* two notes are held — also exercises
                // the `is_active()` fan-out guard in `set_waveform`.
                synth.set_waveform(Waveform::Triangle);
                synth.note_on(55, 0.6);
            }
            3_000 => synth.note_off(40),
            3_600 => synth.note_off(47),
            4_200 => synth.note_off(55),
            _ => {}
        }
        let [l, r] = synth.render_sample();
        out[i * 2] = l;
        out[i * 2 + 1] = r;
    }
}

/// [`render_verification_2_at`] at 48 kHz — the pass behind [`VERIFY_2_HASH`].
pub fn render_verification_2(out: &mut [f32]) {
    render_verification_2_at(48_000.0, out);
}

/// FNV-1a over the raw little-endian bits of every `f32` in `samples`, in order.
pub fn verify_hash(samples: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in samples {
        for b in s.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// `wasm32` exports so a browser / Node build can prove its render matches the
/// reference. Absent on every other target.
#[cfg(target_arch = "wasm32")]
mod wasm {
    use core::cell::UnsafeCell;

    const N: usize = super::VERIFY_FRAMES * 2;

    struct Buf(UnsafeCell<[f32; N]>);
    // `wasm32` is single-threaded and the buffer is only ever touched by the
    // `hc_verify_*` calls below, in sequence.
    unsafe impl Sync for Buf {}
    static BUF: Buf = Buf(UnsafeCell::new([0.0; N]));

    /// Run [`super::render_verification`]; return a pointer to `VERIFY_FRAMES*2`
    /// frame-interleaved `f32` samples in linear memory.
    #[no_mangle]
    pub extern "C" fn hc_verify_render() -> *const f32 {
        // SAFETY: single-threaded, no other live reference to BUF.
        let out = unsafe { &mut *BUF.0.get() };
        super::render_verification(out);
        out.as_ptr()
    }

    /// Run [`super::render_verification_2`]; same buffer, same layout — the
    /// `hc_verify_*` calls are always made in sequence.
    #[no_mangle]
    pub extern "C" fn hc_verify_render_2() -> *const f32 {
        // SAFETY: single-threaded, no other live reference to BUF.
        let out = unsafe { &mut *BUF.0.get() };
        super::render_verification_2(out);
        out.as_ptr()
    }

    /// Number of `f32` samples the last `hc_verify_render*` wrote.
    #[no_mangle]
    pub extern "C" fn hc_verify_len() -> usize {
        N
    }

    /// Low 32 bits of [`super::verify_hash`] over the last render (`u64` is not
    /// a portable wasm return everywhere).
    #[no_mangle]
    pub extern "C" fn hc_verify_hash_lo() -> u32 {
        // SAFETY: single-threaded, no concurrent mutation.
        super::verify_hash(unsafe { &*BUF.0.get() }) as u32
    }

    /// High 32 bits of [`super::verify_hash`] over the last render.
    #[no_mangle]
    pub extern "C" fn hc_verify_hash_hi() -> u32 {
        // SAFETY: single-threaded, no concurrent mutation.
        (super::verify_hash(unsafe { &*BUF.0.get() }) >> 32) as u32
    }
}
