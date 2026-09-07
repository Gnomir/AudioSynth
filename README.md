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
On top: a resonant "Formant" hump (a second closed-form term), a character stage (drive / wavefolder / bit-crush / downsampler), FM with
operator feedback, a ZDF state-variable filter, two ADSR envelopes, a per-voice
LFO (retrigger / free-run, routed to brightness / pitch / cutoff / FM index),
unison with a slow per-voice drift so the stack breathes, microtuning (just
intonation / historical / equal-division scales, Scala `.scl` + `.kbm` import),
pitch bend and equal-power pan.

> Cost is **Θ(log n)** per sample (one `rⁿ` by exponentiation-by-squaring),
> **Θ(1)** at fixed partial count — measured **~26 M samples/s per clean voice**
> (~550× realtime @ 48 kHz), flat within 1 % from 3 to 1200 harmonics; a full
> 64-voice chord renders at **~9.4× realtime** (~590 voice-realtime of margin).

This project is **not** "AQOE-AudioSynth" / `cos²(2εθ)` — that idea was analysed
and dropped (the formula is degenerate as a spectral envelope). Details:
[`docs/07_LIMITATIONS.md`](docs/07_LIMITATIONS.md).

## Layout

| Path | What |
|---|---|
| `harmonic_core/` | `no_std`, **zero-dependency** DSP crate — `src/{trig,kernel,character,filter,env,lfo,voice,poly,tuning,ffi}.rs` + C ABI |
| `harmonic_synth/` | 24-voice polyphonic VST3 + CLAP plugin (via `nih-plug`), 39 params, microtuning (+ Scala `.scl` / `.kbm` import), `nih_plug_vizia` editor: grouped sections + spectrum with the closed-form partial comb and the filter response drawn over it + honest aliasing meter + A/B morph + seed randomiser + 22 presets |
| `docs/` | Full technical documentation — start at [`docs/README.md`](docs/README.md) |
| `product/` | Commercial material — capability spec sheet and go-to-market brief ([`product/README.md`](product/README.md)) |
| `AGENTS.md` | Contributor / AI-agent conventions (build, test, style, boundaries) |

## Build

```sh
# library + tests
cd harmonic_core
cargo test                                    # 114 (96 unit + 18 integration); + `-- --ignored` drift test
cargo build --no-default-features --release    # the real no_std build
bash scripts/cross-verify.sh                   # 114/114 bit-identical on ARM (QEMU) + wasm32 (Node) + bare-metal compile-check

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

114 `harmonic_core` tests pass (96 unit + 18 integration, 11 of them an
adversarial RT-safety suite) plus 30 plugin tests, plus a `#[ignore]` long-run
drift test; `clippy` clean on `std`, `no_std` and nightly `portable-simd`. The
whole core suite — including a whole-signal-path FNV-1a hash compared against an
x86-64 reference — passes **bit-for-bit (delta 0.0)** on
`aarch64-unknown-linux-gnu` and `armv7-unknown-linux-gnueabihf` under QEMU, and
the same render hashes to the same value in the `wasm32` build under Node
(`harmonic_core/scripts/cross-verify.sh` + `verify-wasm.mjs`). `pluginval --strictness-level 8`
passes on the VST3 (editor tests included); `clap-validator` passes **35/35** on
the CLAP — the `nih-plug` `ext_state_load` bugs (one an OOM abort on a corrupt
preset) are fixed via a `[patch]` onto a vendored copy, see
[`docs/10_NIH_PLUG_CLAP_BUGS.md`](docs/10_NIH_PLUG_CLAP_BUGS.md).

**Live-DAW status:** a first pass in REAPER 7.79 (CLAP, Windows 11) is done —
sound, editor, spectrum, presets and the randomiser all work; it also caught and
fixed an editor-layout bug the validators can't see. The rest of the manual
checklist (state recall across a restart, sample-rate sweep, voice stealing,
sustain pedal, MPE, other hosts) is still pending — see
[`docs/11_DAW_CHECKLIST.md`](docs/11_DAW_CHECKLIST.md) and
[`docs/06_VERIFICATION.md`](docs/06_VERIFICATION.md) for exactly what is and isn't
covered.

## License

Dual-licensed under **MIT** ([`LICENSE-MIT`](LICENSE-MIT)) OR **Apache-2.0**
([`LICENSE-APACHE`](LICENSE-APACHE)), at your option — the standard Rust
convention. Both `harmonic_core` and `harmonic_synth` declare
`license = "MIT OR Apache-2.0"`.

> The permissive license is a deliberate decision point before any commercial
> release — it currently means the engine can be shipped in a closed product for
> free. Options (keep permissive / open-core dual-license / source-available):
> [`product/COMMERCIAL_BRIEF.md §5`](product/COMMERCIAL_BRIEF.md).

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you shall be dual-licensed as above,
without any additional terms or conditions.
