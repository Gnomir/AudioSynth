//! C ABI. The **caller owns the memory** for a [`Voice`]: this crate performs
//! no heap allocation anywhere, which is the simplest way to be genuinely
//! RT-safe and `no_std` (no global allocator, no `Box`).
//!
//! Typical use from C:
//!
//! ```c
//! #include "harmonic_core.h"
//!
//! void *mem = aligned_alloc(harmonic_voice_align(), harmonic_voice_size());
//! harmonic_voice_init(mem, 48000.0);
//! harmonic_voice_set_frequency(mem, 220.0);
//! harmonic_voice_set_rolloff(mem, 0.92);
//!
//! float buf[256];                          // 128 frames, INTERLEAVED stereo
//! harmonic_voice_process(mem, buf, 128);   // in the audio callback
//!
//! free(mem);
//! ```

use crate::filter::FilterMode;
use crate::lfo::{LfoMode, LfoShape};
use crate::voice::{Voice, Waveform};

/// Size in bytes of the opaque voice state. Allocate at least this much.
#[no_mangle]
pub extern "C" fn harmonic_voice_size() -> usize {
    core::mem::size_of::<Voice>()
}

/// Required alignment of the voice state.
#[no_mangle]
pub extern "C" fn harmonic_voice_align() -> usize {
    core::mem::align_of::<Voice>()
}

/// Initialise a voice into caller-owned memory.
///
/// Returns a status code:
/// * `0` — `sample_rate` accepted as given;
/// * `1` — was below 8000 Hz, clamped up (pitch/time will be wrong; pick a supported rate);
/// * `2` — was above 768000 Hz, clamped down (same caveat);
/// * `3` — was not finite, defaulted to 48000 Hz;
/// * `-1` — `ptr` was null, nothing written.
///
/// Call [`harmonic_voice_sample_rate`] to read the rate actually in use.
///
/// # Safety
/// `ptr` must be non-null, writable, and point to at least
/// [`harmonic_voice_size`] bytes aligned to [`harmonic_voice_align`].
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_init(ptr: *mut Voice, sample_rate: f64) -> i32 {
    if ptr.is_null() {
        return -1;
    }
    let (v, status) = Voice::new_checked(sample_rate);
    unsafe { ptr.write(v) }
    status as i32
}

/// The sample rate the voice actually runs at (after clamping). 0 on null.
///
/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`] or be null.
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_sample_rate(ptr: *const Voice) -> f64 {
    match unsafe { ptr.as_ref() } {
        Some(v) => v.sample_rate(),
        None => 0.0,
    }
}

/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`].
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_set_frequency(ptr: *mut Voice, hz: f64) {
    if let Some(v) = unsafe { ptr.as_mut() } {
        v.set_frequency(hz);
    }
}

/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`].
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_set_rolloff(ptr: *mut Voice, r: f64) {
    if let Some(v) = unsafe { ptr.as_mut() } {
        v.set_rolloff(r);
    }
}

/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`].
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_set_gain(ptr: *mut Voice, g: f64) {
    if let Some(v) = unsafe { ptr.as_mut() } {
        v.set_gain(g);
    }
}

/// Stereo pan, `−1.0` (hard left) … `+1.0` (hard right). Equal-power, smoothed.
///
/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`].
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_set_pan(ptr: *mut Voice, pan: f64) {
    if let Some(v) = unsafe { ptr.as_mut() } {
        v.set_pan(pan);
    }
}

/// Pitch bend in semitones (0 = none). Smoothed.
///
/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`].
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_set_pitch_bend(ptr: *mut Voice, semitones: f64) {
    if let Some(v) = unsafe { ptr.as_mut() } {
        v.set_pitch_bend(crate::trig::exp2(semitones / 12.0));
    }
}

/// `free_running != 0` → carrier/FM phase are not reset on
/// [`harmonic_voice_reset`] (analog-style); `0` → reset + a short de-click fade.
///
/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`].
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_set_free_running(ptr: *mut Voice, free_running: u32) {
    if let Some(v) = unsafe { ptr.as_mut() } {
        v.set_free_running(free_running != 0);
    }
}

/// Per-voice LFO. `shape`: 0 sine · 1 triangle · 2 saw. `mode`: 0 retrigger
/// (phase resets on note-on) · 1 free-run. Routing depths (`0` = not applied):
/// `to_rolloff` adds to brightness `r` (±), `to_pitch_cents` is vibrato,
/// `to_cutoff_oct` shifts the filter cutoff (± octaves), `to_fm` adds to the
/// FM index (±).
///
/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`].
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn harmonic_voice_set_lfo(
    ptr: *mut Voice,
    rate_hz: f64,
    shape: u32,
    mode: u32,
    to_rolloff: f64,
    to_pitch_cents: f64,
    to_cutoff_oct: f64,
    to_fm: f64,
) {
    if let Some(v) = unsafe { ptr.as_mut() } {
        v.set_lfo(rate_hz, LfoShape::from_u32(shape));
        v.set_lfo_mode(LfoMode::from_u32(mode));
        v.set_lfo_targets(to_rolloff, to_pitch_cents, to_cutoff_oct, to_fm);
    }
}

/// Configure the subtractive filter.
///
/// `mode`: 0 bypass · 1 low-pass · 2 band-pass · 3 high-pass · 4 notch.
/// `cutoff_hz` clamped to `[20, 0.45·fs]`; `resonance` clamped to `[0, 1]`.
///
/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`].
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_set_filter(
    ptr: *mut Voice,
    mode: u32,
    cutoff_hz: f64,
    resonance: f64,
) {
    if let Some(v) = unsafe { ptr.as_mut() } {
        v.set_filter_mode(FilterMode::from_u32(mode));
        v.set_filter_cutoff(cutoff_hz);
        v.set_filter_resonance(resonance);
    }
}

/// HQ mode: `!= 0` → 2×-oversample the oscillator + character stage so the
/// nonlinear stages do not alias. Adds a few samples of latency (see the
/// crate's `Voice::HQ_LATENCY`); `0` is bit-identical to leaving it off.
///
/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`].
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_set_hq(ptr: *mut Voice, hq: u32) {
    if let Some(v) = unsafe { ptr.as_mut() } {
        v.set_hq(hq != 0);
    }
}

/// Oscillator waveform: `0` geometric (additive core) · `1` sawtooth ·
/// `2` triangle. Saw/triangle are PolyBLEP / PolyBLAMP (stateless); they
/// ignore `rolloff` and HQ mode.
///
/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`].
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_set_waveform(ptr: *mut Voice, waveform: u32) {
    if let Some(v) = unsafe { ptr.as_mut() } {
        v.set_waveform(Waveform::from_u32(waveform));
    }
}

/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`].
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_reset(ptr: *mut Voice) {
    if let Some(v) = unsafe { ptr.as_mut() } {
        v.reset();
    }
}

/// Render `num_frames` **interleaved stereo** samples into `out` (so `out` must
/// hold `2 · num_frames` floats, laid out `L R L R …`). RT-safe: no allocation,
/// no locks, no I/O, no panic path.
///
/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`]; `out` must be
/// non-null and point to at least `2 · num_frames` writable `f32`s.
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_process(
    ptr: *mut Voice,
    out: *mut f32,
    num_frames: usize,
) {
    // `num_frames * 2` wraps silently in a release build if a hostile or
    // buggy caller passes something near `usize::MAX / 2` — the resulting
    // slice would claim a far shorter length than the loop below (which
    // uses the original, un-wrapped `num_frames`) then indexes past, so the
    // failure mode is an unbounded-index panic deep in the audio callback
    // rather than a clean rejection. No real host ever calls with anything
    // but a small frame count, but this is the caller-hostile-input C ABI —
    // reject overflow the same way the null/zero checks already do.
    let Some(len) = num_frames.checked_mul(2) else {
        return;
    };
    if ptr.is_null() || out.is_null() || num_frames == 0 {
        return;
    }
    let v = unsafe { &mut *ptr };
    let buf = unsafe { core::slice::from_raw_parts_mut(out, len) };
    for f in 0..num_frames {
        let [l, r] = v.render_sample();
        buf[2 * f] = l;
        buf[2 * f + 1] = r;
    }
}

/// Current smoothed fundamental (Hz), for host-side metering. Returns 0 on null.
///
/// # Safety
/// `ptr` must come from a successful [`harmonic_voice_init`] or be null.
#[no_mangle]
pub unsafe extern "C" fn harmonic_voice_current_frequency(ptr: *const Voice) -> f64 {
    match unsafe { ptr.as_ref() } {
        Some(v) => v.current_frequency(),
        None => 0.0,
    }
}
