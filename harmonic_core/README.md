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

**Resonant hump** — a *second* closed form, the difference of two geometric
sums, weighting harmonic *k* by `a^k − b^k` (single peak, then decays):

```
Σ_{k=1}^{n} (a^k − b^k) cos(2π k p)  =  S_n(a) − S_n(b)
```

One knob drives it (`b = a²`, `a = 2^(−1/kc)`) so the bump lands on partial
`kc ≈ 1.5 + 24·formant²` (≈ 2nd…26th). Mixed in at weight `h`; `h = 0` is
bit-identical to the tilt-only sum. Tilt and hump are orthogonal and together
cover most static spectral envelopes of musical interest.

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
| `src/poly.rs` | `PolySynth<VOICES>` — alloc/stealing, unison, pitch bend, shared LFO, both ADSRs, `set_tuning`. Stereo out. |
| `src/tuning.rs` | `Tuning` — note→frequency: 12-TET (default, byte-identical to `midi_to_hz`), n-EDO, arbitrary Scala scale, Scala `.kbm` keyboard map (dead keys). |
| `src/ffi.rs` | C ABI (interleaved-stereo `process`). Caller owns voice memory; crate never allocates. `wasm32`-only static-storage helpers (`harmonic_wasm_*`). |
| `src/verify.rs` | The shared scripted whole-tract render + FNV-1a hash + reference constants — called by the integration test, the ARM cross-check and the `wasm32` cross-check so all three render the same bytes. |
| `tests/spectrum.rs` | closed form == brute sum; rendered voice proven non-aliasing via single-bin DFT. |
| `tests/stress.rs` | adversarial RT-safety: `NaN`/`±∞`/`±1e300` into every public setter, parameter & note-event storms at sample rate, sample-rate extremes, HQ under load, 2 M-sample run — output stays finite, bounded, and the voice pool always recovers. |
| `tests/cross_platform_bit_exact.rs` | whole-tract render hash vs an x86-64 reference, at 48 **and** 96 kHz, and independent of render block size — bit-identical on `aarch64` / `armv7-hf` under QEMU and `wasm32` under Node. |
| `scripts/` | `cross-verify.sh` (QEMU ARM + wasm + bare-metal compile-check), `verify-wasm.mjs` (render hash in Node). |
| `examples/` | `render_wav`, `poly_demo`, `character_demo`, `filter_demo`, `wide_demo` (unison); benches `bench_hc`, `bench_poly`, `bench_hq_bus`. |

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

## Microtuning (`src/tuning.rs`)

The oscillator sums on `k·f0` of a **retuned** fundamental, so any regular scale
fits: `Tuning::equal(edo, …)`, `Tuning::from_cents(&cents, period, …)` (arbitrary
Scala `.scl`), or `Tuning::from_kbm(…)` (an explicit key→degree table, with dead
keys). The plugin adds `.scl` / `.kbm` file import. 12-TET / A = 440 is the
**byte-identical** default path (`note_hz` short-circuits to `midi_to_hz`;
`Tuning::from_kbm` on the identity map is bit-exact too — tested on all 128
notes). It retunes the *fundamental*, not individual partials — a chord in just
intonation stops beating, but bell inharmonicity needs a different kernel
(`docs/09`).

## Measured (release, x86-64; `examples/bench_*`, `docs/06 §3`)

| | number |
|---|---|
| One clean voice | **~26 M samples/s** (~550× realtime @ 48 kHz), flat within 1 % from 3 to 1200 harmonics |
| 64-voice chord (`PolySynth<64>`) | **~9.4× realtime @ 48 kHz** (~590 voice-realtime of margin) |
| PolyBLEP saw / triangle | ~90 M / ~77 M samples/s (cheaper than the geometric carrier) |
| Cross-architecture render hash | **delta 0.0** on `aarch64` + `armv7-hf` (QEMU) and `wasm32` (Node), 48 + 96 kHz, any block size |
| `no_std` cdylib | ~14 KB native, ~45 KB `wasm32` |
| Tests | **114** (96 unit + 18 integration, 11 = adversarial `stress.rs`) + 1 `#[ignore]` drift; run on every push by CI (`.github/workflows/ci.yml`) |

## Status & roadmap

The plugin (`../harmonic_synth`), Character stage, ZDF filter, dual ADSR,
stereo + unison + drift, pitch bend, per-voice LFO with a modulation matrix,
per-note brightness expression (MPE / aftertouch → per-voice rolloff), a
resonant "Formant" hump (a second closed-form term), microtuning with Scala
`.scl` / `.kbm` import, PolyBLEP saw/triangle, the fractional "Partials" knob,
sample-rate validation, the batched oscillator, the HQ oversampling bus, the
clean-voice fast path, the `wasm32` build (`contrib/wasm-demo/` — same DSP,
same render hash), the CI matrix, and the `nih_plug_vizia` GUI are all done.
`pluginval --strictness 8` and `clap-validator 35/35` pass; a first live-DAW
pass in REAPER is done (`../docs/11_DAW_CHECKLIST.md`). The plugin ships as
**Cosine** with an enforced free-Core / paid-Studio split (watermarked
Ed25519 key file, `../harmonic_synth/license/`); the engine itself is not
gated.

What's left, and the deliberately-deferred directions with their reasons, are in
**[`../docs/09_ROADMAP.md`](../docs/09_ROADMAP.md)** — the short version is: the
rest of the live-DAW checklist, an upstream PR for the nih-plug CLAP fix, real
hardware / Daisy firmware, an MTS-ESP client, and an inharmonic third kernel /
SoA-SIMD if a concrete need ever appears.

Commercial packaging of all of the above — capability spec sheet, positioning,
pricing models, first-customer channels — is in
**[`../product/`](../product/README.md)**.
