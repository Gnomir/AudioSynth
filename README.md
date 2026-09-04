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
oscillator cannot alias. Two extra waveforms — a band-limited **sawtooth** and
**triangle** via PolyBLEP / PolyBLAMP (stateless, flat to DC) — sit alongside it.
On top: a character stage (drive / wavefolder / bit-crush / downsampler), FM with
operator feedback, a ZDF state-variable filter, two ADSR envelopes, a per-voice
LFO (retrigger / free-run, routed to brightness / pitch / cutoff / FM index),
unison with a slow per-voice drift so the stack breathes, pitch bend and
equal-power pan.

> Cost is **Θ(log n)** per sample (one `rⁿ` by exponentiation-by-squaring),
> **Θ(1)** at fixed partial count — measured ~26 M samples/s per clean voice, flat
> across a 400× range of harmonic counts.

This project is **not** "AQOE-AudioSynth" / `cos²(2εθ)` — that idea was analysed
and dropped (the formula is degenerate as a spectral envelope). Details:
[`docs/07_LIMITATIONS.md`](docs/07_LIMITATIONS.md).

## Layout

| Path | What |
|---|---|
| `harmonic_core/` | `no_std`, **zero-dependency** DSP crate — `src/{trig,kernel,character,filter,env,lfo,voice,poly,ffi}.rs` + C ABI |
| `harmonic_synth/` | 24-voice polyphonic VST3 + CLAP plugin (via `nih-plug`), with a `nih_plug_vizia` editor + filter-bank spectrum display |
| `docs/` | Full technical documentation — start at [`docs/README.md`](docs/README.md) |
| `AGENTS.md` | Contributor / AI-agent conventions (build, test, style, boundaries) |

## Build

```sh
# library + tests
cd harmonic_core
cargo test                                    # 63 (58 unit + 5 integration); + `-- --ignored` drift test
cargo build --no-default-features --release    # the real no_std build
bash scripts/cross-verify.sh                   # 63/63 bit-identical on ARM (Docker + QEMU)

# plugin bundle (VST3 + CLAP)
cd ../harmonic_synth
cargo xtask bundle harmonic_synth --release    # → target/bundled/harmonic_synth.{vst3,clap}

# a demo render
cd ../harmonic_core
cargo run --example wide_demo --release        # → wide_demo.wav (7× unison stereo pad)
```

`harmonic_synth` fetches `nih-plug` + `nih_plug_vizia` from git at one pinned
rev — the first build needs network and takes a few minutes (vizia pulls a
large tree). Rust stable (1.97+ known-good); the `portable-simd` feature needs
nightly.

## Status

63 tests pass (58 unit + 5 integration), plus a `#[ignore]` long-run drift test; `clippy` clean on `std`, `no_std` and
nightly `portable-simd`. The whole suite — including a whole-signal-path hash
compared against an x86-64 reference — passes bit-for-bit on
`aarch64-unknown-linux-gnu` and `armv7-unknown-linux-gnueabihf` under QEMU
(`harmonic_core/scripts/cross-verify.sh`). `pluginval --strictness-level 8` passes on the VST3
(editor tests included); `clap-validator` passes **35/35** on the CLAP — the
`nih-plug` `ext_state_load` bugs (one an OOM abort on a corrupt preset) are
fixed via a `[patch]` onto a vendored copy, see
[`docs/10_NIH_PLUG_CLAP_BUGS.md`](docs/10_NIH_PLUG_CLAP_BUGS.md). The plugin has
a `nih_plug_vizia` editor (all params + a live spectrum). Not yet validated in a
live DAW — see [`docs/06_VERIFICATION.md`](docs/06_VERIFICATION.md) for exactly
what is and isn't covered.

## License

Dual-licensed under **MIT** ([`LICENSE-MIT`](LICENSE-MIT)) OR **Apache-2.0**
([`LICENSE-APACHE`](LICENSE-APACHE)), at your option — the standard Rust
convention. Both `harmonic_core` and `harmonic_synth` declare
`license = "MIT OR Apache-2.0"`.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you shall be dual-licensed as above,
without any additional terms or conditions.
