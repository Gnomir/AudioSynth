# harmonic_synth

Polyphonic MIDI instrument plugin (**VST3 + CLAP**) built on
[`harmonic_core`](../harmonic_core). Every note is a band-limited additive tone
from the Dirichlet-kernel closed form — no wavetable, no per-partial oscillator
loop, no oversampling (except the optional HQ mode).

24 voices (unison stacks share the pool), stereo out, GUI on `nih_plug_vizia`
(all parameters + a live non-FFT spectrum display). The audio thread never
allocates; `nih_plug`'s `assert_process_allocs` is left on.

## Signal path

```
MIDI ─▶ PolySynth<24>
         ├─ note-on stacks 1–8 unison voices (detune + spread + phase-decorrelate + drift)
         ├─ voice alloc + stealing (idle → oldest-releasing → oldest)
         ├─ pitch bend + CC#64 sustain → every sounding voice
         └─ per-voice Voice:
              LFO (sine/tri/saw) → brightness / vibrato / cutoff / FM
              FM sine modulator + operator self-feedback
              oscillator  Σ r^k cos(2π k (p+pm))  (or PolyBLEP saw / PolyBLAMP triangle)
              character   drive/bias → fold → crush → downsample
              SVF filter  LP/BP/HP/notch + resonance (smoothed per sample)
              equal-power pan → [L, R]
         + per-voice amp ADSR + independent filter ADSR
         Σ → master gain → soft clip → (HQ: 2× bus + 65-tap decimate) → stereo out
```

## Build

```
cargo build --release                        # check it compiles
cargo xtask bundle harmonic_synth --release  # VST3 + CLAP bundles → target/bundled/
cargo xtask validate                         # pluginval (VST3) + clap-validator (CLAP)
```

## Everything else

Building, installing into a DAW, the parameter reference, and troubleshooting
(including "no sound → arm the track") are in
**[`../docs/12_USER_GUIDE.md`](../docs/12_USER_GUIDE.md)**. Full technical docs
start at [`../docs/README.md`](../docs/README.md).

nih-plug is vendored under `vendor/nih-plug/` with a local CLAP `ext_state_load`
fix — see [`../docs/10_NIH_PLUG_CLAP_BUGS.md`](../docs/10_NIH_PLUG_CLAP_BUGS.md).
