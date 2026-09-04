# AGENTS.md

## Project overview

`harmonic_core` — a `no_std`, **zero-dependency** Rust DSP crate: band-limited
additive synthesis from a closed-form Dirichlet-kernel sum
(`Σ rᵏ·cos(2πkp)`), plus band-limited BLIT saw/triangle (`Waveform` enum),
character (drive/fold/grit), a ZDF state-variable filter, two ADSRs, an LFO,
unison, pitch bend, equal-power pan.
`harmonic_synth` — a 24-voice polyphonic VST3 + CLAP plugin over it, via
`nih-plug`. Full design docs: `docs/` (start at `docs/README.md`).

Layout: `harmonic_core/src/{trig,kernel,character,filter,env,lfo,voice,poly,ffi}.rs`
· `harmonic_core/tests/spectrum.rs` (integration) ·
`harmonic_synth/src/lib.rs` (host glue) · `harmonic_synth/xtask/` (bundler).

## Setup

Rust stable (1.97 known-good). `harmonic_core` has no dependencies.
`harmonic_synth` pulls `nih-plug` from git at a pinned rev in `Cargo.toml` —
**first build needs network**. The `portable-simd` feature needs nightly.

## Build & test

```
cd harmonic_core
cargo test                                     # all 53 tests (49 unit + tests/spectrum.rs)
cargo test --lib <name-substr>                 # one test, e.g. cargo test --lib per_sample_smoothing
cargo clippy --all-targets                     # must be 0 warnings
cargo clippy --no-default-features --release   # no_std lint — must also be 0
cargo build --no-default-features --release    # the real no_std build (release only)
cargo fmt                                       # default rustfmt, no config
cargo check                                     # fast typecheck

cd harmonic_synth
cargo build --release
cargo xtask bundle harmonic_synth --release    # → target/bundled/harmonic_synth.{vst3,clap}
cargo xtask validate                            # build + run Tracktion pluginval (needs pluginval installed)
```

There is **no CI** — run the lints and tests yourself. `cargo xtask validate`
(or `scripts/validate.{ps1,sh}`) covers state recall, block-size / sample-rate
changes, and allocation checks via `pluginval` once it's on PATH.

## Code style

- Default `rustfmt` (4-space, no project config). Imports: grouped `use` lines,
  `core`/`std` then `crate::`.
- `snake_case` fns, `CamelCase` types, `SCREAMING_SNAKE` consts; one concept per
  module. Setters are `set_*`; smoothed param fields carry a `_z` suffix
  (`freq_z`, `pan_z`); per-sample effective values are `_eff`.
- **Phase is in _turns_, not radians** (1 turn = 2π). All trig takes turns.
- `no_std` discipline in `harmonic_core`: never call `f64::{sin,cos,exp2,floor,
  abs,sqrt,mul_add,clamp,powi,…}` in non-test code — use `crate::trig::*` and
  the local `clamp` helpers. No `libm`, no new dependencies.
- Audio path: no allocation, no `unwrap`/`expect`/`panic!`, guard every
  division by construction. `[profile.release] panic = "abort"` stays.

## Testing conventions

- Unit tests: a `#[cfg(test)] mod tests` at the bottom of each `src/*.rs`.
  Cross-cutting spectral checks: `harmonic_core/tests/spectrum.rs`.
- Assert **properties** (bounded / finite / monotone / non-aliasing via
  single-bin DFT), not magic output values; check the closed form against a
  brute-force `Θ(n)` sum. Tolerances scale with `n`.
- New or changed DSP code must add a test and keep `cargo test` + **both**
  `clippy` invocations green. `docs/06_VERIFICATION.md` catalogues every test —
  update it in the same change.

## Security

- Pure DSP math — no secrets, keys, tokens, or `.env` anywhere. Do not add any.
- `VST3_CLASS_ID`, vendor URL, email in `harmonic_synth/src/lib.rs` are
  `example.invalid` placeholders — leave them until the user gives real values.

## Git / commit & PR rules

- Repo: `github.com/Gnomir/AudioSynth`, branch `main`. `.gitignore` excludes
  every `target/`, `*.wav` demo renders and `harmonic_synth/tools/`.
- Commit style: Conventional Commits — `feat:`, `fix:`, `chore:`, `test:`,
  `docs:`. End commit messages with the trailer in `CLAUDE.md`.
- Before a PR / hand-off: `cargo test`, `cargo clippy --all-targets`, and
  `cargo clippy --no-default-features --release` must pass; for `harmonic_synth`,
  `cargo xtask bundle` must succeed.
- `harmonic_*/Cargo.lock` **is** committed; no `target/` ever is.

## Boundaries

Ask first before:

- adding a dependency to `harmonic_core`, or dropping `panic = "abort"` / the
  `no_std` build;
- changing the pinned `nih_plug` rev in `harmonic_synth/Cargo.toml`;
- deleting files, `git init`, force-push, or setting up CI;
- restructuring `docs/` (a numbered, cross-referenced set).
