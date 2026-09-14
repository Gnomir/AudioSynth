# AGENTS.md

## Project overview

`harmonic_core` — a `no_std`, **zero-dependency** Rust DSP crate: band-limited
additive synthesis from a closed-form Dirichlet-kernel sum
(`Σ rᵏ·cos(2πkp)`), plus PolyBLEP saw/triangle (`Waveform` enum),
character (drive/fold/grit), a ZDF state-variable filter, two ADSRs, a per-voice
LFO (retrigger/free-run + mod matrix), unison (with a breathing drift), pitch
bend, equal-power pan, and note→frequency microtuning (`Tuning`).
`harmonic_synth` — a 24-voice polyphonic VST3 + CLAP plugin over it, via
`nih-plug`. The plugin ships as **Cosine** (`NAME = "Cosine"`; the crate keeps
its name) with an enforced free-**Core** / paid-**Studio** split (`CORE_LOCKS`,
`TIER_ENFORCED = true`, watermarked Ed25519 key file). Full design docs: `docs/`
(start at `docs/README.md`); the independent audit: `product/QA_REPORT.md`.
Internal business/legal material lives outside this repo, not under `product/`.

Layout: `harmonic_core/src/{trig,kernel,character,filter,env,lfo,voice,poly,tuning,ffi,verify}.rs`
· `harmonic_core/tests/{spectrum,stress,cross_platform_bit_exact}.rs` (integration) ·
`harmonic_synth/src/{lib,editor,analyzer,presets,rando,tuning}.rs` (host glue +
`nih_plug_vizia` GUI + a cheap filter-bank spectrum display + a 22-preset bank +
a seed randomiser + built-in microtuning scales) · `harmonic_synth/xtask/`
(bundler + `keygen`) · `harmonic_synth/license/` (`harmonic_license` — the
watermarked Ed25519 key-file format, one dep: `ed25519-compact`) ·
`harmonic_synth/vendor/nih-plug/` (patched framework copy, see below).

## Setup

Rust stable (1.97 known-good). `harmonic_core` has **no dependencies** — keep it
that way. `harmonic_synth` pulls `nih-plug` + `nih_plug_vizia` (heavy tree:
baseview, vizia/femtovg, fonts) at **one** pinned rev, plus `atomic_float` and
`harmonic_license` (→ `ed25519-compact`, zero transitive deps in the plugin's
verify-only mode; the `sign` feature used by `cargo xtask keygen` adds
`getrandom`) — **first build needs network** and takes a few minutes. The
`portable-simd` feature (core) needs nightly.

`harmonic_synth/vendor/nih-plug/` is a **trimmed, patched copy** of that pinned
tree, wired in via `[patch]` in `harmonic_synth/Cargo.toml` — it carries the
CLAP `ext_state_load` fix until it lands upstream (`docs/10_NIH_PLUG_CLAP_BUGS.md`).
Don't edit it beyond that fix; it's `-text` in `.gitattributes` to stay
diff-able against the real repo.

## Build & test

```
cd harmonic_core
cargo test                                     # 120 tests (101 unit + 19 integration; 11 of them tests/stress.rs) + 1 #[ignore] drift
bash scripts/cross-verify.sh                    # 120/120 bit-identical on aarch64 + armv7-hf (QEMU) + wasm32 (Node); bare-metal compile-check
node scripts/verify-wasm.mjs                    # wasm32 render == x86-64/ARM reference hash (needs the wasm32 build)
cargo test --lib <name-substr>                 # one test, e.g. cargo test --lib per_sample_smoothing
cargo clippy --all-targets                     # must be 0 warnings
cargo clippy --no-default-features --release   # no_std lint — must also be 0
cargo build --no-default-features --release    # the real no_std build (release only)
cargo check                                     # fast typecheck
# NB: do NOT run `cargo fmt` — rustfmt 1.9 reformats every checked-in file
#     (the style predates it). Match the surrounding style by hand. See *Code style*.

cd harmonic_synth
cargo build --release
cargo test --workspace                           # 34 plugin + 9 harmonic_license tests
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask bundle harmonic_synth --release    # → target/bundled/harmonic_synth.{vst3,clap}
cargo xtask keygen new                          # license signing keypair (see license/README.md)
cargo xtask validate                            # build + pluginval (VST3) + clap-validator (CLAP)
```

CI (`.github/workflows/ci.yml`) runs on push / PR to `main`: `harmonic_core`
test + both clippy configs + `no_std` build + wasm32 build & `verify-wasm.mjs` +
bare-metal compile-check (thumbv7em / thumbv6m / riscv32imac / aarch64-none),
nightly `portable-simd` clippy+build, `harmonic_synth` test + clippy + bundle
(uploads the Linux VST3/CLAP), and `cross-verify.sh` (ARM QEMU bit-exactness) as
its own job. **No `cargo fmt --check`** — see *Code style*. `cargo xtask
validate` (pluginval + clap-validator) is **not** in CI — run it locally; it
covers state recall, block-size / sample-rate changes and allocation checks.
Still run the lints and tests yourself before pushing.

## Code style

- Style approximates default `rustfmt` (4-space, no project config) but the repo
  is **not** rustfmt-1.9-clean — that release's stricter defaults would rewrite
  every file, so `cargo fmt` is not run and CI does not check it. Match the
  checked-in style by hand. Imports: grouped `use` lines, `core`/`std` then `crate::`.
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
- **Exception, and it is deliberate:** `harmonic_synth/license/DEV_SECRET_KEY.txt`
  is a *development* Ed25519 signing secret. It is **not confidential** — it
  signs only `SAMPLE_LICENSE.key`, which verifies against the equally-throwaway
  `LICENSE_PUBKEY` in `license/src/lib.rs`. The real production secret is
  generated with `cargo xtask keygen new` at launch and never committed
  (`.gitignore` blocks `*SECRET*` / `license.key`). Do not treat the dev file as
  a leak.
- The product is named **Cosine** (`NAME = "Cosine"`, `VENDOR = "Cosine Audio"`).
  `CLAP_ID` (`io.github.gnomir.cosine`, reverse-DNS on the GitHub repo, not a
  purchased domain) and `VST3_CLASS_ID` (`CosineSynth\0\0\0\0\0`) are
  **frozen as of the first public release (2026-09-14)** — changing either
  afterwards breaks every saved project that references the plugin, even
  once a real domain exists. `URL` / `EMAIL` are informational only (points
  at the GitHub repo and a real contact address) and can move to a real
  domain anytime without that concern. The crate / directory /
  bundle filename are still `harmonic_synth` — renaming them is a separate
  launch-prep step.
- The **Core/Studio split is enforced** (`TIER_ENFORCED = true` in
  `harmonic_synth/src/lib.rs`): an unlicensed build is the free `Core` tier
  (`CORE_LOCKS` held at neutral, microtuning forced to 12-TET); a valid key file
  unlocks `Studio`. **To run a Studio build in dev / a DAW:** set
  `COSINE_LICENSE=<repo>/harmonic_synth/license/SAMPLE_LICENSE.key`, or
  drop that file at the `load_license` config path. The gated audio path and the
  greyed editor aren't unit-testable (nih-plug walls off the host param
  setters) — a real change to the gate needs a pluginval-s8 pass plus eyes in a
  DAW.

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
- changing the pinned `nih_plug` rev, or the `[patch]` / `vendor/nih-plug`
  contents in `harmonic_synth/`;
- deleting files, `git init`, force-push, or setting up CI;
- restructuring `docs/` (a numbered, cross-referenced set).
