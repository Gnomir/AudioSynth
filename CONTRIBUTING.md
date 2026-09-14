# Contributing

Thanks for looking. This project is a `no_std` closed-form additive DSP engine
(`harmonic_core`) and a VST3/CLAP synth built on it (`harmonic_synth`, product
name **Cosine**). Conventions — build, test, style, boundaries — are in
[`AGENTS.md`](AGENTS.md); read that first.

## Before you open a PR

- **Discuss non-trivial changes in an issue first.** The engine has a hard
  determinism contract (`docs/06_VERIFICATION.md`) — a change that alters
  `VERIFY_HASH` needs a reason and a regenerated hash, not a surprise.
- Run the full check locally: `cargo test` in `harmonic_core`, both `clippy`
  configs, `cargo test -p harmonic_synth`. See `AGENTS.md`.
- Do **not** run `cargo fmt` — the checked-in style predates rustfmt 1.9; match
  the surrounding code by hand.
- Keep `harmonic_core` zero-dependency and `no_std`. Adding a dependency,
  dropping `panic = "abort"`, or changing the pinned `nih-plug` rev needs
  discussion first.

## Licensing of contributions (Developer Certificate of Origin + dual-licence grant)

`harmonic_core` is dual-licensed: **AGPL-3.0-only** for the community, and a
**commercial licence** for closed products (see [`LICENSE-AGPL`](LICENSE-AGPL)
and [`LICENSE-COMMERCIAL.md`](LICENSE-COMMERCIAL.md)). For the project to keep
offering the commercial licence, every contribution must come in under terms
that allow it.

By adding a `Signed-off-by:` line to your commits (`git commit -s`), you certify
the [Developer Certificate of Origin 1.1](https://developercertificate.org/)
**and** you agree that:

> Your contribution is licensed to the project under **AGPL-3.0-only**, and you
> additionally grant the project maintainer a perpetual, worldwide,
> non-exclusive, royalty-free, irrevocable licence to relicense your
> contribution — on its own or as part of the project — under other terms,
> including a commercial licence. You retain copyright in your contribution.

If your employer owns your work, make sure you have their sign-off before
contributing.

Every commit on a PR must carry `Signed-off-by: Real Name <email>` matching the
commit author. PRs without it can't be merged.

## What's most useful right now

- **Portability fixes** — a target where the bit-exact render diverges, a
  `no_std` build break, a Cortex-M / RISC-V issue.
- **DAW reports** — `docs/11_DAW_CHECKLIST.md` items in hosts other than REAPER.
- **Presets tuned by ear** — the shipped bank is verified bounded, not
  auditioned.

Not looking for: new oscillator models or effects beyond the roadmap
(`docs/09_ROADMAP.md`) without a discussion first — scope is deliberate.
