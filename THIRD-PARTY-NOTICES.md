# Third-Party Notices

The `harmonic_core` engine has **no dependencies** — nothing to list.

The **Cosine plugin** (`harmonic_synth`) links the components below. This file is
a starting point; regenerate the authoritative, version-pinned list before each
release with:

```sh
cargo install cargo-about
cd harmonic_synth && cargo about generate about.hbs > ../THIRD-PARTY-NOTICES.md
```

and include the result in the release bundle (`target/bundled/`).

## Direct dependencies

| Component | Used by | Licence | Source |
|---|---|---|---|
| `nih-plug` (`nih_plug`, `nih_plug_vizia`) | plugin framework, editor | ISC | github.com/robbert-vdh/nih-plug @ `de421011` (vendored copy with a local CLAP `ext_state_load` fix — `docs/10_NIH_PLUG_CLAP_BUGS.md`) |
| `atomic_float` | editor ↔ audio-thread meters | MIT OR Apache-2.0 | crates.io |
| `ed25519-compact` | `harmonic_license` — key-file signature verification | MIT | crates.io |

## Transitive tree (via `nih_plug_vizia`)

`vizia` and its rendering/text stack (`femtovg` or `skia-safe`, `cosmic-text` /
`swash`, `fontdb`, `unicode-*`, `winit`, `raw-window-handle`, …). These are
predominantly **MIT OR Apache-2.0** / **Zlib**; a small number are **MPL-2.0**
(file-level copyleft, satisfied by linking). `cargo about` enumerates them with
exact versions and licence texts — run it before release and paste the output
here.

## Fonts

The editor's typeface is provided by the `nih_plug_vizia` tree (a bundled
open-licence font — confirm which and its licence in the `cargo about` output).
No fonts are vendored in this repository.

## Note on the engine licence

`harmonic_core` itself is licensed **AGPL-3.0-only OR commercial** — see
`LICENSE-AGPL` and `LICENSE-COMMERCIAL.md`. The components above are the
plugin's, not the engine's — an embedded integrator who licenses
`harmonic_core` commercially links **none** of them.
