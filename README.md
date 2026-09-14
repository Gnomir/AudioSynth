# Cosine

Band-limited **additive synthesis from a closed-form oscillator** — no wavetable,
no per-partial loop, no oversampling filter. A `no_std` Rust DSP core
(`harmonic_core`) and a polyphonic VST3 / CLAP synth plugin (**Cosine**) built on
it. The repository is `Gnomir/AudioSynth`; the crate / directory names still read
`harmonic_synth` pending a launch-time rename.

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
| `harmonic_core/` | `no_std`, **zero-dependency** DSP crate — `src/{trig,kernel,character,filter,env,lfo,voice,poly,tuning,ffi,verify}.rs` + C ABI; also builds to `wasm32` (`contrib/wasm-demo/`) |
| `harmonic_synth/` | 24-voice polyphonic VST3 + CLAP plugin (via `nih-plug`), 39 params, microtuning (+ Scala `.scl` / `.kbm` import), `nih_plug_vizia` editor: grouped sections + spectrum with the closed-form partial comb and the filter response drawn over it + honest aliasing meter + A/B morph + seed randomiser + 22 presets |
| `harmonic_synth/license/` | `harmonic_license` — watermarked Ed25519 key-file format, verified offline (no dongle, no activation server). Drives the enforced free-**Core** / paid-**Studio** split; `cargo xtask keygen` issues keys. |
| `docs/` | Full technical documentation — start at [`docs/README.md`](docs/README.md) |
| `product/` | [`product/QA_REPORT.md`](product/QA_REPORT.md) — the independent technical audit every measured claim on the site traces back to. (Internal business/legal material is kept out of this repo.) |
| `webapp/` | The web portal — the landing page + a public FAQ + customer accounts, content-managed through a browser admin panel. Node.js/Express + SQLite (`node:sqlite`, no separate DB server), server-rendered, bilingual EN/Українська ([`webapp/README.md`](webapp/README.md)) |
| `site/` | Superseded by `webapp/` — the earlier single static HTML file, kept as a dependency-free fallback ([`site/README.md`](site/README.md)) |
| `AGENTS.md` | Contributor / AI-agent conventions (build, test, style, boundaries) |

## Reading it

| For | Start with |
|---|---|
| **Musicians / integrators** — every capability, with examples, workflows, MIDI map, shortcuts | [**Capabilities & Usage Handbook**](https://claude.ai/code/artifact/a734ea1b-a6ff-4afa-91e4-9a984016ccd0) (published page) · [`harmonic_synth/MANUAL.md`](harmonic_synth/MANUAL.md) (ships with the download) |
| **Evaluating the maths** | [Scientific monograph](https://claude.ai/code/artifact/c4b2806f-90f3-4eb9-84c2-35b43461c30d) → [`docs/01_MATHEMATICS.md`](docs/01_MATHEMATICS.md) → [`docs/06_VERIFICATION.md`](docs/06_VERIFICATION.md) |
| **Integrating the engine** | [`docs/03_ARCHITECTURE.md`](docs/03_ARCHITECTURE.md) → [`docs/05_API_REFERENCE.md`](docs/05_API_REFERENCE.md) → [`docs/08_EMBEDDED_INTEGRATION.md`](docs/08_EMBEDDED_INTEGRATION.md) |
| **Try it now** | [Playable browser demo](https://claude.ai/code/artifact/ad41ef31-c87c-4d3c-b2c3-34d9cb85a5ed) (the same DSP, `wasm32`) |
| **The evidence behind the marketing** | [`product/QA_REPORT.md`](product/QA_REPORT.md) — every performance/accuracy number on the site, traced to a measurement |

## Build

```sh
# library + tests
cd harmonic_core
cargo test                                    # 120 (101 unit + 19 integration); + `-- --ignored` drift test
cargo build --no-default-features --release    # the real no_std build
bash scripts/cross-verify.sh                   # 120/120 bit-identical on ARM (QEMU) + wasm32 (Node) + bare-metal compile-check

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

120 `harmonic_core` tests pass (101 unit + 19 integration, 11 of them an
adversarial RT-safety suite) plus 34 plugin tests and 9 `harmonic_license`
tests, plus a `#[ignore]` long-run drift test; `clippy` clean on `std`,
`no_std` and nightly `portable-simd`. The
whole core suite — including a whole-signal-path FNV-1a hash compared against an
x86-64 reference — passes **bit-for-bit (delta 0.0)** on
`aarch64-unknown-linux-gnu` and `armv7-unknown-linux-gnueabihf` under QEMU, and
the same render hashes to the same value in the `wasm32` build under Node
(`harmonic_core/scripts/cross-verify.sh` + `verify-wasm.mjs`). `pluginval --strictness-level 8`
passes on the VST3 (editor tests included); `clap-validator` passes **35/35** on
the CLAP — the `nih-plug` `ext_state_load` bugs (one an OOM abort on a corrupt
preset) are fixed via a `[patch]` onto a vendored copy, see
[`docs/10_NIH_PLUG_CLAP_BUGS.md`](docs/10_NIH_PLUG_CLAP_BUGS.md).

**Free-Core / paid-Studio split:** enforced (`TIER_ENFORCED = true`). An
unlicensed build is the free **Core** tier — oscillator, Brightness, Partials,
amp envelope, the full filter, unison, pitch bend, 12-TET; a watermarked
Ed25519 key file (verified offline, no dongle, no server) unlocks **Studio**
(Formant, Character, FM, filter envelope, LFO matrix, MPE, microtuning, HQ).
`harmonic_synth/license/`. The DAW-visible product name is **Cosine**.

**Live-DAW status:** a first pass in REAPER 7.79 (CLAP, Windows 11) is done —
sound, editor, spectrum, presets and the randomiser all work; it also caught and
fixed an editor-layout bug the validators can't see. The rest of the manual
checklist (state recall across a restart, sample-rate sweep, voice stealing,
sustain pedal, MPE, other hosts) is still pending — see
[`docs/11_DAW_CHECKLIST.md`](docs/11_DAW_CHECKLIST.md) and
[`docs/06_VERIFICATION.md`](docs/06_VERIFICATION.md) for exactly what is and isn't
covered.

## License

The two crates are licensed separately:

- **`harmonic_core`** (the DSP engine) — **AGPL-3.0-only**
  ([`LICENSE-AGPL`](LICENSE-AGPL)), or a separate commercial license for
  closed-source use ([`LICENSE-COMMERCIAL.md`](LICENSE-COMMERCIAL.md)). This
  replaced an earlier permissive license before any public release.
- **`harmonic_synth`** (the Cosine plugin) and **`harmonic_synth/license`**
  (the `harmonic_license` key-file crate) — dual-licensed under **MIT**
  ([`harmonic_synth/LICENSE-MIT`](harmonic_synth/LICENSE-MIT)) OR
  **Apache-2.0** ([`harmonic_synth/LICENSE-APACHE`](harmonic_synth/LICENSE-APACHE)),
  at your option. The compiled plugin binary carries its own separate
  end-user terms (the Studio-tier key file, what you may and may not do with
  it) — contact **cityobukhov@gmail.com** for a copy.

> `harmonic_synth` staying permissively-licensed while its end-user terms
> restrict reverse-engineering and tier-gate circumvention is a real,
> currently unresolved tension: a permissive license already grants the
> freedoms those terms try to restrict. This needs an owner decision, not a
> silent default.

By contributing to `harmonic_core`, you agree to the terms in
[`CONTRIBUTING.md`](CONTRIBUTING.md) (AGPL-3.0-only plus a relicensing grant
to the maintainer). Unless you explicitly state otherwise, any contribution
to `harmonic_synth` or `harmonic_synth/license` is dual-licensed as above,
without any additional terms or conditions.
