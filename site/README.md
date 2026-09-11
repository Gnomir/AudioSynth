# site/

> **Superseded by [`../webapp/`](../webapp/README.md).** This static file was
> step one (out of Claude's artifact hosting into the repo); step two turned
> it into a real, content-managed portal — same design and copy, now editable
> through a web admin panel instead of a text editor, plus a public FAQ and
> customer accounts. Kept here as a dependency-free fallback (e.g. a plain
> static mirror, or if you want the page without running a Node server) — but
> **edit content in the admin panel, not this file**, or the two will drift.

The marketing landing page — moved out of Claude's artifact hosting into the
repo, so it's a real file you own and can deploy anywhere. Bilingual
(English / Українська, auto-detected + a toggle), light/dark theme, no build
step, no framework, no dependency beyond two Google Fonts stylesheets.

## Preview locally

Just open it — no server needed:

```sh
# Windows
start site/index.html
# macOS
open site/index.html
# Linux
xdg-open site/index.html
```

Or serve it (closer to production, and required if you add `fetch`-based
features later):

```sh
cd site && python3 -m http.server 8000   # → http://localhost:8000
```

## Deploy

It's one self-contained HTML file — any static host works. Two free options
that fit the budget in `product/GO_TO_MARKET_RESEARCH.md §6`:

- **Cloudflare Pages** — connect the repo, set the build output directory to
  `site/`, no build command.
- **Netlify** — drag-and-drop `site/index.html` in the dashboard, or connect
  the repo with publish directory `site`.

Point the registered domain's DNS at whichever host you pick, then update
`URL` / `EMAIL` in `harmonic_synth/src/lib.rs` to match
(`product/LAUNCH_CHECKLIST.md` §1).

## Editing

Everything is in `index.html` — plain CSS custom properties for the two
themes (`:root` = light, `[data-theme="dark"]` + `@media (prefers-color-scheme:
dark)` = dark) and a small vanilla-JS i18n layer: English is authored inline
in the HTML (so it's readable and correct with JavaScript disabled or in a
search-engine crawl); the Ukrainian strings live in the `UK` object near the
top of the `<script>` block, keyed the same as each element's `data-i18n` /
`data-i18n-html` attribute. Add a string in both places when you add one to
the page — the harvest step at the top of the script only reads the *existing*
English text out of the DOM, it doesn't invent a translation.

The live interactive spectrum (the hero panel) is a from-scratch Canvas
illustration of the closed-form oscillator's spectral tilt + Formant hump —
it approximates the real DSP for teaching purposes, it isn't `harmonic_core`
compiled to `wasm32`. For the real engine running in a browser, see the
separate demo: `harmonic_core/contrib/wasm-demo/`.

## Where the content comes from

Copy traces to [`../product/CAPABILITIES.md`](../product/CAPABILITIES.md) —
update that first when a number or capability changes, then bring the same
change here (and to the Ukrainian string, and to the archived copy that may
still exist as a Claude artifact if one hasn't been retired).

## History

Designed and iterated as a Claude artifact first (published preview, fast
redeploys, live audit via browser automation); moved into the repo on request
once the design and both languages were settled, so the real implementation
lives in version control rather than only on Claude's hosting. See
`product/LAUNCH_CHECKLIST.md` and the `harmonic-core-project` /
`audiosynth-artifacts` project memory for the fuller history.
