# harmonic_synth

Polyphonic MIDI instrument plugin (**VST3 + CLAP**) built on
[`harmonic_core`](../harmonic_core). Every note is a band-limited additive tone
from the Dirichlet-kernel closed form — no wavetable, no per-partial oscillator
loop, no oversampling.

## Signal path

```
MIDI ──▶ PolySynth<24>            (harmonic_core::poly)   — STEREO
          ├─ note-on stacks 1–8 unison voices (detune + spread + phase-decorrelate)
          ├─ voice alloc + stealing (idle → oldest-releasing → oldest)
          ├─ pitch bend → every sounding voice (smoothed)
          ├─ per-voice Voice
          │    ├─ LFO (sine/tri/saw) → brightness ± / vibrato cents
          │    ├─ FM: sine modulator + operator self-feedback
          │    ├─ oscillator  Σ r^k cos(2π k (p+pm)) @ freq·bend·2^vib, clamped n
          │    ├─ free-running phase OR reset + 16-sample de-click
          │    ├─ character   drive/bias → fold → crush → downsample
          │    ├─ SVF filter  LP/BP/HP/notch + resonance (cutoff/res smoothed per sample)
          │    └─ equal-power pan → [L, R]
          ├─ per-voice amp ADSR      → VCA + voice lifetime
          ├─ per-voice filter ADSR   → cutoff (± octaves), independent
          └─ Σ → master gain → soft clip (tanh Padé) → stereo out
```

The audio thread never allocates. `nih_plug`'s `assert_process_allocs`
feature is left on, so any allocation in `process()` panics in-host.

## Parameters

| Param | Range | Notes |
|---|---|---|
| **Brightness** | 0–100 % | geometric rolloff `r ∈ [0.02, 0.9995]` (quadratic). 0 % ≈ pure sine, 100 % ≈ bright pulse. |
| **Attack** | 0.5 ms – 2 s | linear |
| **Release** | 5 ms – 5 s | exponential |
| **Gain** | −60 – 0 dB | pre soft-clip |
| **Drive** | 0–100 % | asymmetric saturation (bias rides with it). The "fatten". |
| **Fold** | 0–100 % | reflective wavefolder. Dense, evolving upper spectrum. |
| **Grit** | 0–100 % | bit-crush + sample-rate reduction together. Early-digital dirt. |
| **FM Amount** | 0–4 | phase-modulation depth. 0 = off. |
| **FM Ratio** | 0.5–12 | modulator freq ÷ fundamental. Integers stay harmonic. |
| **Feedback** | 0–0.9 | operator self-feedback. Sine → saw → noise. |
| **Filter** | Off / LP / BP / HP / Notch | ZDF state-variable filter after the character stage. |
| **Cutoff** | 20 Hz – 20 kHz | block-rate automation. |
| **Resonance** | 0–100 % | Q 0.5 → 32, stable at the top (no self-oscillation). |
| **Filter Env** | −6 – +6 oct | dedicated filter envelope → cutoff, per-sample per-voice. Bipolar. |
| **F.Env Attack / Decay / Sustain / Release** | | the filter envelope's own ADSR, independent of the amp envelope. |
| **Free-Run Phase** | on / off | analog-style: oscillator phase is not reset on note-on. |
| **Unison** | 1–8 | detuned + stereo-spread voices stacked per note (1/√n make-up gain). |
| **Uni Detune** | 0–50 ct · **Uni Spread** 0–100 % | unison spread in pitch and stereo. |
| **Bend Range** | 1–24 st | pitch-bend wheel range. |
| **LFO Rate / Shape** | 0.02–30 Hz · sine/tri/saw | per-voice, key-retriggered. |
| **LFO → Bright** | −100…+100 % | LFO modulates spectral tilt. |
| **LFO Vibrato** | 0–100 ct | LFO modulates pitch. |

Drive / Fold / Grit / FM / Feedback push content past Nyquist and alias when
hard — intentional (see `harmonic_core::character`). Clean at zero.

MIDI: Note On/Off, Choke, **Pitch Bend**, CC#123 (all-notes-off). 24 voices
(unison stacks share the pool). No GUI (host-generic parameter view).

## Build

```
# dev build (fast, checks the plugin compiles against nih-plug)
cargo build --release

# proper VST3 + CLAP bundles
cargo xtask bundle harmonic_synth --release
```

Output:

```
target/bundled/harmonic_synth.vst3     (Contents/x86_64-win/harmonic_synth.vst3)
target/bundled/harmonic_synth.clap
```

Install: copy `harmonic_synth.vst3` to your VST3 folder
(`%COMMONPROGRAMFILES%\VST3` on Windows, `~/.vst3` on Linux,
`~/Library/Audio/Plug-Ins/VST3` on macOS) and `harmonic_synth.clap` to the
matching CLAP folder.

## Status

- Builds and bundles clean on `x86_64-pc-windows-msvc`, nih-plug pinned to
  `f36931f7`.
- Bundle structure verified (`clap_entry` export; VST3
  `Contents/x86_64-win/` + `GetPluginFactory`/`InitDll`/`ExitDll`).
- Engine (voice alloc, stealing, envelope, pitch, saturation) covered by 5
  unit tests in `harmonic_core::poly`; not yet validated in a live DAW or
  against `pluginval` from this environment.

## Next

- `pluginval` / DAW smoke test.
- Stereo detune (two geometric stacks per voice) for width.
- Optional simple GUI (`nih_plug_vizia`): brightness + envelope + a spectrum
  strip.
