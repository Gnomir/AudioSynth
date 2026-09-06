# contrib/

Patches for upstream dependencies.

## `nih-plug-clap-state-load-fix.patch`

Fixes two bugs in the nih-plug CLAP wrapper's `ext_state_load`
(`src/wrapper/clap/wrapper.rs`) that made `harmonic_synth.clap` fail four
`clap-validator` tests (one an OOM abort on a corrupt preset). Full analysis,
reproduction and verification:
[`../docs/10_NIH_PLUG_CLAP_BUGS.md`](../docs/10_NIH_PLUG_CLAP_BUGS.md).

**This patch is already applied** to the vendored copy at
`../harmonic_synth/vendor/nih-plug/`, which the plugin uses via `[patch]` — so
`cargo xtask validate` passes `clap-validator` 35/35 with no `--exclude`.

The `.patch` file is kept here for the upstream PR. Re-verified 2026-09-06: it
applies cleanly to `master` (`de421011…`, unchanged since the pin) and every
symbol it touches exists upstream.

```sh
git clone https://github.com/robbert-vdh/nih-plug.git
cd nih-plug && git apply /path/to/contrib/nih-plug-clap-state-load-fix.patch
```

**Ready-to-submit PR title + body: [`nih-plug-pr.md`](nih-plug-pr.md).**
Not yet submitted upstream — that step is the repo owner's to take.
