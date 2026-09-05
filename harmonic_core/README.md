# harmonic_core

An **honest** `no_std`, zero-dependency DSP core for band-limited additive
synthesis. It renders a tone made of the first *n* harmonics of a fundamental
in one closed-form expression — no per-partial oscillator loop, no wavetable,
no oversampling filter.

## The math (real, standard, ~1830s)

**Dirichlet kernel** — flat spectrum of *n* partials:

```
Σ_{k=1}^{n} cos(2π k p)  =  sin(π(2n+1)p) / (2 sin(π p))  −  1/2
```

**Geometric rolloff** — harmonic *k* weighted by `r^k`, a spectral tilt:

```
Σ_{k=1}^{n} r^k cos(2π k p)
  = [ r·c₁ − r² − r^{n+1}·c_{n+1} + r^{n+2}·c_n ] / (1 − 2 r c₁ + r²)
```

with `c_k = cos(2π k p)`. Denominator `≥ (1−r)² > 0` for `r < 1` — well
conditioned at every phase.

## Honest cost & aliasing statement

| Claim | Reality |
|---|---|
| "O(1) per sample" | **O(log n)** exactly (one `r^n` by squaring); **O(1)** at fixed partial count. No per-partial loop either way. |
| "no aliasing" | True **only** while `n ≤ ⌊fs / (2·f0)⌋`. [`Voice`] clamps to that. The closed form is the exact finite sum, so nothing lives above partial *n*. |
| "unlimited harmonics" | Capped at `MAX_PARTIALS = 2048` (error budget + `r^n` loop). Nyquist caps it lower in practice. |

## What this is NOT

- **Not** connected to quantum mechanics, Grover's algorithm, or `cos²(2εθ)`.
  The spectrum here is *designed*, not borrowed from an unrelated identity.
- **Not** a general O(1) additive synth. Arbitrary per-partial envelopes cost
  O(n). The trick only works for spectra with a closed-form sum (flat,
  geometric). That is the real, narrow, defensible claim.
- The geometric core is **not** a sawtooth — `Σ sin(kx)/k` has no elementary
  closed form. Saw and triangle are provided separately as stateless
  PolyBLEP / PolyBLAMP (`Waveform::Saw` / `Triangle`), not from the closed form.

## Build

```
cargo test                                   # std, runs all checks
cargo run --example render_wav --release      # writes harmonic_demo.wav
cargo build --no-default-features --release    # true #![no_std], cdylib+staticlib
```

Artifacts land in `target/release/`:
`harmonic_core.dll` / `.lib` (Windows), `libharmonic_core.so` / `.a` (Linux),
`libharmonic_core.dylib` / `.a` (macOS). C header in `include/harmonic_core.h`.

## Files

| File | Role |
|---|---|
| `src/trig.rs` | Range-reduced `sin`/`cos`/`exp2`/`floor`/`tan` in turns + a branchless batched `cos4_turns` — no `libm`, no `std`. |
| `src/kernel.rs` | The two closed-form sums + peak normalisation + `geometric_partials_x4` (batched, auto-vectorising). |
| `src/voice.rs` | Osc + FM + feedback + LFO + character + SVF + pitch bend + equal-power pan + de-click. **Stereo out** `[f32;2]`. |
| `src/character.rs` | Drive / bias / reflective fold / bit-crush / downsample. Identity when clean. |
| `src/filter.rs` | ZDF SVF (LP/BP/HP/notch), resonance, **per-sample internal cutoff/res smoothing**. |
| `src/env.rs` | `Adsr` — one impl for the amp envelope and the dedicated filter envelope. |
| `src/lfo.rs` | `Lfo` — sine / triangle / saw, phase-aligned, `no_std`. |
| `src/poly.rs` | `PolySynth<VOICES>` — alloc/stealing, unison, pitch bend, shared LFO, both ADSRs. Stereo out. |
| `src/ffi.rs` | C ABI (interleaved-stereo `process`). Caller owns voice memory; crate never allocates. |
| `tests/spectrum.rs` | closed form == brute sum; rendered voice proven non-aliasing via single-bin DFT. |
| `examples/` | `render_wav`, `poly_demo`, `character_demo`, `filter_demo`, `wide_demo` (unison), `bench_hc`. |

## `character` module — the dirt, on purpose

The clean Dirichlet core is the right *foundation* because you can always dirty
a clean signal but never clean a dirty one. `character` is everything the
perfect oscillator is not, all under a knob, none of it from CPU error:

| Stage | Effect |
|---|---|
| `drive` + `bias` | asymmetric saturation → harmonic fattening, even harmonics |
| `fold` | reflective (triangle) wavefolder → dense evolving upper spectrum |
| `crush` + `downsample` | deliberate quantisation + sample-rate reduction → PPG/DX7 grit |

Plus, in `Voice`: **phase modulation** (`set_fm(ratio, index)`) with a sine
modulator, and **operator self-feedback** (`set_feedback`) — sine → saw →
noise. These generate content above Nyquist and *will* alias when pushed;
that is intentional and matches the machines this is chasing. A surgically
clean drive is an oversampled v2.

Measured effect (see `examples/character_demo.rs`, high/low-mid energy ratio):
clean 0.01 → fold 0.10 → feedback 0.31 → FM 0.78.

## `filter` module — subtractive shaping

`Svf` — a zero-delay-feedback state-variable filter (trapezoidal integration,
after Cytomic). Unconditionally stable for every cutoff up to Nyquist and every
resonance; low / band / high / notch from one pair of integrators. `no_std`, no
`libm` — the `tan(π fc/fs)` prewarp is `sin_turns / cos_turns`. Resonance
`0..1` → Q `0.5..32` (capped, does not self-oscillate). In `Voice` the filter
sits after the character stage.
Verified: LP/HP/BP/notch response shape, resonance lift, stability under a
full-range cutoff sweep at max resonance.

## Voice architecture (stereo)

```
LFO ─┬─▶ vibrato (±cents)          pitch bend ─┐
     └─▶ brightness (±r)                        ▼
  osc  Σ r^k cos(2π k (p + FM + feedback))  @  freq_z·bend_z·2^(vib)
   │   Nyquist-clamped n ; free-running OR reset + 16-sample de-click
   ▼
 character   drive/bias → fold → crush → downsample
   ▼
 SVF   LP/BP/HP/notch, resonance, cutoff & res smoothed per sample inside
   ▼
 × gain × de-click ─▶ equal-power pan (sin/cos, ~10 ms smoothed) ─▶ [L, R]
```

`PolySynth` adds: **unison** (1–8 detuned, stereo-spread, phase-decorrelated
voices per note, 1/√n make-up), amp ADSR + independent filter ADSR, a shared
LFO, and pitch bend fanned to every sounding voice.

## `env` module — ADSR

`Adsr` — linear attack, one-pole decay/release, no `exp` (coeffs from `exp2`
so a `t`-second stage actually finishes in ≈ `t`, not `5t`). `sustain == 0` is
percussive (the voice frees even while held). One implementation drives both:

* the **amplitude** envelope (`set_amp_adsr` / `set_envelope`), and
* a **dedicated filter** envelope (`set_filter_envelope`), routed to cutoff by
  `PolySynth::set_filter`'s bipolar `env_octaves` (± octaves at the peak).

The two are fully independent — verified: a percussive filter sweep
(`sustain 0`) closes ~11× while the amp envelope sustains the note underneath.

## SIMD (batched oscillator)

`kernel::geometric_partials_x4(p0, dp, r, n)` renders four consecutive samples
through `trig::cos4_turns` (branchless). Plain `[f64; 4]` math — LLVM lowers it
to `VFMADD` / `FMLA` on x86-64 / AArch64 and to **correct scalar** on targets
without SIMD (Cortex-M). `--features portable-simd` (nightly) adds an explicit
`core::simd` `f64x4` path. The per-voice chain stays scalar because the SVF and
character stages are serial recursive filters; the batch API is for the bare
oscillator / offline rendering.

## Roadmap (honest)

1. ~~`nih-plug` VST3/CLAP wrapper, polyphony, amp envelope.~~ ✔ `../harmonic_synth`
2. ~~Character stage: drive / fold / grit / FM / feedback.~~ ✔
3. ~~ZDF state-variable filter, resonance, per-sample smoothing.~~ ✔
4. ~~Dedicated filter ADSR.~~ ✔ · ~~Stereo + equal-power pan + unison.~~ ✔
5. ~~Pitch bend + LFO (sine/tri/saw → brightness, vibrato).~~ ✔ · ~~De-click + free-running phase.~~ ✔
6. ~~Batched / SIMD oscillator.~~ ✔ `geometric_partials_x4`
7. ~~2× oversampled oscillator + Character ("HQ Mode") — clean nonlinear stages.~~ ✔
8. ~~Sample-rate validation with a status code (was a silent 48 k fallback).~~ ✔
9. ~~Fast 4-term trig for LFO / pan (16-bit is plenty for modulators).~~ ✔
10. ~~`pluginval` script (`cargo xtask validate`).~~ ✔ — run pending an install.
11. 4× HQ + oversampled filter; oversample the master `soft_clip`.
12. Leaky-integrated BLIT → band-limited saw/triangle (still O(1)).
13. Filter-free "clean voice" fast path over the batched oscillator.
14. Spatial SIMD: 4–8 voices in parallel (SoA `PolySynth`).
15. Minimal GUI (`nih_plug_vizia`).
