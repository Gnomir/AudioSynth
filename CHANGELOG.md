# Changelog

Every user-facing change to the Cosine plugin and Core/Studio builds,
newest first. Engine-internal changes with no effect on the shipped
plugin aren't listed here — see `git log` for those.

Format loosely follows [Keep a Changelog](https://keepachangelog.com/).

## [0.1.0-core] — 2026-09-14

First public build: the free **Core** tier, Windows only, unsigned.

### Added
- Full closed-form additive oscillator (Brightness, Partials, Formant),
  band-limited Saw/Triangle, the multimode filter, amp envelope, unison,
  A/B morph, seed randomiser, and the 22-preset bank — all free, no
  watermark, no time limit.
- VST3 and CLAP builds (Windows x86-64).

### Verified
- `pluginval --strictness 8`: SUCCESS.
- `clap-validator`: 35/35 passed.
- Both re-run directly against the shipped `Cosine.vst3` / `Cosine.clap`
  files, not a cached result from an earlier build.

### Known limitations
- Not code-signed — Windows SmartScreen will warn on first run. A paid
  certificate isn't in the budget yet for a pre-revenue solo project;
  will be fixed once Studio sales cover the cost.
- macOS and Linux builds aren't available yet.
- Studio tier (Formant/Character/FM extras, filter envelope, LFO matrix,
  MPE, HQ mode, microtuning beyond 12-TET) isn't purchasable yet — the
  license-key pipeline exists but isn't connected to a live storefront.

[0.1.0-core]: https://github.com/Gnomir/AudioSynth/releases/tag/v0.1.0-core
