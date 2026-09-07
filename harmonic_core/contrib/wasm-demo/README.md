# `harmonic_core` in the browser

A minimal, dependency-free demo: the **byte-for-byte identical** DSP core that
ships in the VST3/CLAP plugin, compiled to `wasm32-unknown-unknown` and driven
from an `AudioWorklet`.

The point of this directory is to make the portability claim concrete — the same
`Voice` C ABI that a Cortex-M firmware calls, running in a web page. Its render
hashes to the [same 64-bit value](../../scripts/verify-wasm.mjs) as the x86-64
and ARM builds.

**Play it now (no build):** a single-file version with the `.wasm` inlined is
published at <https://claude.ai/code/artifact/ad41ef31-c87c-4d3c-b2c3-34d9cb85a5ed>.
This directory is the reference implementation — `AudioWorklet`, real files.

## Build

```sh
rustup target add wasm32-unknown-unknown          # once
cd harmonic_core
cargo build --no-default-features --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/harmonic_core.wasm contrib/wasm-demo/
```

The `.wasm` is ~45 KB. It exports the full C ABI (`harmonic_voice_*`) plus
`wasm32`-only helpers that hand back static storage (a `no_std` `cdylib` has no
allocator): `harmonic_wasm_voice_at(i)` / `harmonic_wasm_pool_size()` for the
8-voice pool, `harmonic_wasm_voice()` for slot 0, and `harmonic_wasm_scratch` /
`harmonic_wasm_scratch_frames` for the render buffer.

## Serve

Any static server; `AudioWorklet` and `fetch` need `http(s)`, not `file://`:

```sh
cd contrib/wasm-demo
python -m http.server 8080      # then open http://localhost:8080
```

## What it shows

- **8 voices**, geometric / saw / triangle oscillator, the closed-form
  Brightness tilt, the fractional Partials ceiling, the Formant hump, the
  Cytomic SVF, the per-voice LFO — all live, playable as chords.
- **Note allocation, voice stealing and the attack/release gate all live in
  `worklet.js`, in JavaScript.** The core `Voice` has no envelope and no notion
  of polyphony — that is deliberate: it is the host's job, identical here and in
  a Daisy Seed firmware (`docs/08_EMBEDDED_INTEGRATION.md`). ~40 lines of JS.
- No build step for the page, no framework, no bundler — `index.html` +
  `worklet.js` + the `.wasm`.

## Not here yet

A `harmonic_poly_*` C ABI (so the polyphony policy could live in Rust for
integrators who want it — `docs/09_ROADMAP.md`), and a nicer UI. This is a proof,
not a product.
