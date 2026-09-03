# contrib/

Patches for upstream dependencies, kept here until they land upstream.

## `nih-plug-clap-state-load-fix.patch`

Fixes two bugs in the nih-plug CLAP wrapper's `ext_state_load`
(`src/wrapper/clap/wrapper.rs`) that make `harmonic_synth.clap` fail four
`clap-validator` tests. Full analysis, reproduction and verification:
[`../docs/10_NIH_PLUG_CLAP_BUGS.md`](../docs/10_NIH_PLUG_CLAP_BUGS.md).

Applies cleanly on `de421011` (our pinned rev) and on `master` (Sep 2026).

```sh
git clone https://github.com/robbert-vdh/nih-plug.git
cd nih-plug
git apply /path/to/contrib/nih-plug-clap-state-load-fix.patch
```

Not yet submitted upstream.
