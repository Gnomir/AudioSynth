/* harmonic_core.h — C ABI for the harmonic_core DSP crate.
 *
 * Honest O(log n) band-limited additive synthesis via the Dirichlet kernel,
 * plus FM, character, an SVF, an LFO, pitch bend and equal-power pan.
 * The caller owns the voice memory; the library never allocates.
 *
 * Hand-written to match src/ffi.rs. Regenerate with cbindgen if the ABI moves.
 */
#ifndef HARMONIC_CORE_H
#define HARMONIC_CORE_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque voice state. Treat as bytes; never dereference fields. */
typedef struct HarmonicVoice HarmonicVoice;

/* Storage requirements for one voice. */
size_t harmonic_voice_size(void);
size_t harmonic_voice_align(void);

/* Initialise a voice into caller-owned memory (>= harmonic_voice_size() bytes,
 * aligned to harmonic_voice_align()). sample_rate in Hz. */
void harmonic_voice_init(HarmonicVoice *voice, double sample_rate);

/* Parameter setters. Values are clamped internally.
 *   frequency  -> [1, fs/2)
 *   rolloff    -> [1e-3, 0.9995]     (0 = pure fundamental, ->1 = bright pulse)
 *   gain       -> [0, 8]
 *   pan        -> [-1, 1]            (equal-power, smoothed ~10 ms)
 *   pitch_bend -> semitones          (smoothed ~5 ms)
 *   free_running != 0 -> phase is NOT reset on harmonic_voice_reset()
 */
void harmonic_voice_set_frequency(HarmonicVoice *voice, double hz);
void harmonic_voice_set_rolloff(HarmonicVoice *voice, double r);
void harmonic_voice_set_gain(HarmonicVoice *voice, double g);
void harmonic_voice_set_pan(HarmonicVoice *voice, double pan);
void harmonic_voice_set_pitch_bend(HarmonicVoice *voice, double semitones);
void harmonic_voice_set_free_running(HarmonicVoice *voice, unsigned int free_running);

/* Subtractive filter. mode: 0 bypass / 1 LP / 2 BP / 3 HP / 4 notch.
 * cutoff_hz -> [20, 0.45*fs]; resonance -> [0, 1]. Both smoothed per sample. */
void harmonic_voice_set_filter(HarmonicVoice *voice, unsigned int mode,
                               double cutoff_hz, double resonance);

/* Per-voice LFO. shape: 0 sine / 1 triangle / 2 saw.
 * to_rolloff adds to brightness (+/-); to_pitch_cents is vibrato depth. */
void harmonic_voice_set_lfo(HarmonicVoice *voice, double rate_hz, unsigned int shape,
                            double to_rolloff, double to_pitch_cents);

/* Reset phase + smoothers + filter state (call on note-on). Honors
 * free_running: in that mode the oscillator phase keeps running. */
void harmonic_voice_reset(HarmonicVoice *voice);

/* Render num_frames INTERLEAVED STEREO samples into out (out must hold
 * 2 * num_frames floats: L R L R ...). RT-safe: no allocation, no locks,
 * no I/O, no panic path. */
void harmonic_voice_process(HarmonicVoice *voice, float *out, size_t num_frames);

/* Smoothed fundamental * pitch-bend, in Hz. Returns 0 for NULL. */
double harmonic_voice_current_frequency(const HarmonicVoice *voice);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* HARMONIC_CORE_H */
