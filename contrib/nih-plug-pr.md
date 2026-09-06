# Upstream PR — ready to submit

Fork `robbert-vdh/nih-plug`, create a branch, apply
`nih-plug-clap-state-load-fix.patch`, push, open a PR against `master` with the
title and body below.

Verified 2026-09-06: the patch applies cleanly to `master`
(`de421011f41a6d10fc8c7a6084e4f4dee0143683` — unchanged since the pin) and
every symbol it touches (`Task::RescanParamValues`, `Wrapper::schedule_gui`,
`Vec::try_reserve_exact`) is present upstream. `Task::RescanParamValues`'s
handler already calls
`host_params.rescan(CLAP_PARAM_RESCAN_VALUES)` — exactly what a host needs
after a state load.

---

## Title

```
clap: notify the host after ext_state_load, and don't abort on a corrupt preset
```

## Body

```
The CLAP wrapper's `ext_state_load` has two issues that make a plugin fail
several `clap-validator` tests.

### 1. No host rescan after `clap_plugin_state::load()`

`ext_state_load` calls `set_state_inner()`, which only posts
`Task::ParameterValuesChanged` — that just notifies the plugin's *own* editor.
The host that called `load()` is never told the parameter values changed, so
it keeps showing the pre-load values until something else triggers a rescan.
`set_state_object_from_gui()` (a preset load from the plugin's GUI) already
does the right thing by scheduling `Task::RescanParamValues`; this makes the
host-driven path do the same.

Fixes `clap-validator`'s `state-reproducibility-{basic,binary,buffered}`:

    After reloading the state, these parameter values changed
    without a rescan request:
     - Fold (3148801) - 0 (0.0000) vs 6 (0.0556)
     - Gain (3165055) - -12.0 dB (0.7593) vs -58.5 dB (0.1821)
     ...

### 2. `Vec::with_capacity` on an untrusted length aborts the process

The state length is read straight from the stream and used as
`Vec::with_capacity(length as usize)`. `clap-validator`'s
`state-invalid-random` feeds pure random bytes, so `length` is garbage and the
capacity request aborts the whole process instead of failing the load.
`try_reserve_exact` turns it into a recoverable `return false`.

### Testing

`clap-validator` on a `nih_plug_vizia` synth: 31 passed / 4 failed before,
35 passed / 0 failed / 0 warnings after. `pluginval --strictness 8` on the
VST3 path is unaffected (SUCCESS before and after).
```

---

## After it merges

Bump the pinned `rev` in `harmonic_synth/Cargo.toml` + `xtask/Cargo.toml`,
delete `harmonic_synth/vendor/nih-plug/` and the `[patch]` section, drop the
`harmonic_synth/vendor/**` line from `.gitattributes`. `clap-validator` must
stay 35/35. Update `docs/10`, `docs/06 §6`, `docs/09` (item Б2).
