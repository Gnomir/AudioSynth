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

The `.wasm` is ~20 KB. It exports the full single-voice C ABI (`harmonic_voice_*`)
plus three `wasm32`-only helpers (`harmonic_wasm_voice` / `harmonic_wasm_scratch`
/ `harmonic_wasm_scratch_frames`) that hand back static storage — a `no_std`
`cdylib` has no allocator, so the page can't `malloc` a voice.

## Serve

Any static server; `AudioWorklet` and `fetch` need `http(s)`, not `file://`:

```sh
cd contrib/wasm-demo
python -m http.server 8080      # then open http://localhost:8080
```

## What it shows

- **One `Voice`**, geometric / saw / triangle oscillator, the closed-form
  Brightness tilt, the fractional Partials ceiling, the Formant hump, the
  Cytomic SVF, the per-voice LFO — all live.
- The amplitude **attack/release gate lives in `worklet.js`, in JavaScript**,
  because the core `Voice` has no envelope. That is deliberate: envelope and
  polyphony are the host's job, identical here and on a Daisy Seed
  (`docs/08_EMBEDDED_INTEGRATION.md`).
- No build step for the page, no framework, no bundler — `index.html` +
  `worklet.js` + the `.wasm`.

## Not here yet

Polyphony (the page would manage N voices + stealing in JS, or the crate would
grow a `harmonic_poly_*` C ABI — `docs/09_ROADMAP.md`), and a nicer UI. This is a
proof, not a product.
