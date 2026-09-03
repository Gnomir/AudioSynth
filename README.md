# AudioSynth

Band-limited **additive synthesis from a closed-form oscillator** — no wavetable,
no per-partial loop, no oversampling filter. A `no_std` Rust DSP core and a
polyphonic VST3 / CLAP synth plugin built on it.

## What it is

The oscillator renders the first *n* harmonics of a fundamental as one truncated
complex geometric series that collapses smoothly to the Dirichlet kernel (a
band-limited impulse train) at the limit:

```
Σ_{k=1..n} rᵏ·cos(2πkp)  =  (r·c₁ − r² − r^{n+1}·c_{n+1} + r^{n+2}·c_n) / (1 − 2r·c₁ + r²)
```

`r` is a spectral tilt (dark → bright); `n` is clamped to Nyquist so the clean
oscillator cannot alias. On top of it: a character stage (drive / wavefolder /
bit-crush / downsampler), FM with operator feedback, a ZDF state-variable filter,
two ADSR envelopes, an LFO, unison, pitch bend and equal-power pan.

> Cost is **Θ(log n)** per sample (one `rⁿ` by exponentiation-by-squaring),
> **Θ(1)** at fixed partial count — measured ~20 M samples/s per voice, flat
> across a 400× range of harmonic counts.

This project is **not** "AQOE-AudioSynth" / `cos²(2εθ)` — that idea was analysed
and dropped (the formula is degenerate as a spectral envelope). Details:
[`docs/07_LIMITATIONS.md`](docs/07_LIMITATIONS.md).

## Layout

| Path | What |
|---|---|
| `harmonic_core/` | `no_std`, **zero-dependency** DSP crate — `src/{trig,kernel,character,filter,env,lfo,voice,poly,ffi}.rs` + C ABI |
| `harmonic_synth/` | 24-voice polyphonic VST3 + CLAP plugin (via `nih-plug`) |
| `docs/` | Full technical documentation — start at [`docs/README.md`](docs/README.md) |
| `AGENTS.md` | Contributor / AI-agent conventions (build, test, style, boundaries) |

## Build

```sh
# library + tests
cd harmonic_core
cargo test                                    # 45 unit + integration tests
cargo build --no-default-features --release    # the real no_std build

# plugin bundle (VST3 + CLAP)
cd ../harmonic_synth
cargo xtask bundle harmonic_synth --release    # → target/bundled/harmonic_synth.{vst3,clap}

# a demo render
cd ../harmonic_core
cargo run --example wide_demo --release        # → wide_demo.wav (7× unison stereo pad)
```

`harmonic_synth` fetches `nih-plug` from git at a pinned rev — the first build
needs network. Rust stable (1.97+ known-good); the `portable-simd` feature needs
nightly.

## Status

45 tests pass; `clippy` clean on `std` and `no_std`. The plugin builds and
bundles, but has **not** been validated in a live DAW or against `pluginval`
yet — see [`docs/06_VERIFICATION.md`](docs/06_VERIFICATION.md) for exactly what
is and isn't covered.

## License

Dual-licensed under **MIT** ([`LICENSE-MIT`](LICENSE-MIT)) OR **Apache-2.0**
([`LICENSE-APACHE`](LICENSE-APACHE)), at your option — the standard Rust
convention. Both `harmonic_core` and `harmonic_synth` declare
`license = "MIT OR Apache-2.0"`.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you shall be dual-licensed as above,
without any additional terms or conditions.
