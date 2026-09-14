# Cosine web portal

The real web portal — content-managed landing page, public FAQ, and customer
accounts, administered through a browser, backed by one SQLite file. No
PostgreSQL, no separate database server, no PHP: it's a small Node.js/Express
app that runs on your own PC or any server that has Node.

This **replaces** the earlier static prototype at [`../site/`](../site/README.md)
as the live implementation — that file's design and copy is exactly what got
migrated in here (see "Where the content came from" below); it isn't rebuilt
from scratch.

> **Before deploying this anywhere real, read [`AUDIT.md`](AUDIT.md).** A
> source-level security/architecture review found one critical issue (a
> default session secret with no production guard — full auth bypass if
> deployed as-is) and several others worth fixing first. Nothing here has
> been penetration-tested; it's had one careful read.

## Quick start

```sh
cd webapp
npm install          # already run once here — re-run after pulling changes
cp .env.example .env # then edit SESSION_SECRET at least
npm run seed          # first time only — creates database/app.sqlite,
                       # imports the landing page + FAQ text, creates the
                       # first admin account (prints its password ONCE)
npm start              # → http://localhost:4000
```

Then:
- **Public site:** http://localhost:4000/
- **Admin panel:** http://localhost:4000/admin/login — log in with the
  email/password `npm run seed` printed (or `ADMIN_EMAIL` / `ADMIN_PASSWORD`
  from `.env` if you set them before seeding).
- **Customer accounts:** anyone can register at `/register` — there's no
  payment check yet (see "What's deliberately not built yet").

`npm run dev` restarts on file changes (`node --watch`); no build step, no
bundler, nothing to compile.

## Why this stack

Picked for **zero extra installs on a bare Windows/Mac/Linux machine with
Node** — no native modules to compile, no second language runtime, no
database server to run alongside it:

| Piece | Choice | Why |
|---|---|---|
| Runtime | Node.js (≥ 22.5, developed on 24) | Already the project's JS runtime (the landing page's own script); no PHP/Composer needed. |
| Database | **`node:sqlite`** (built into Node) | One file (`database/app.sqlite`), no server, no native compile (unlike `better-sqlite3`/`sqlite3`). |
| Web framework | Express 5 | Minimal, unopinionated, everyone knows it. |
| Views | EJS, server-rendered | No build step, no separate frontend deploy; the admin panel is exactly as complex as it needs to be (forms + tables). |
| Sessions | `express-session` + a ~60-line custom SQLite-backed store | Avoids `connect-sqlite3`, which wraps the native `sqlite3` package. |
| Passwords | Node's built-in `crypto.scrypt` | Avoids `bcrypt`/`argon2` native compilation. |
| CSRF | A ~20-line same-session token check | Avoids the (now unmaintained) `csurf` package for a two-function need. |

Net result: `npm install` pulls only pure-JS packages (`express`,
`express-session`, `ejs`, `dotenv`, and `cheerio` — used only by the one-time
seed script, not at runtime). Nothing here needs a C++ toolchain, Python, or
a second interpreter installed on the machine.

## Architecture

```
webapp/
  src/
    app.js              express app: sessions, security headers, routes, error handler
                          (exports the app, no .listen() — split from server.js for
                          in-process testing, see test/)
    server.js           entry point — requires app.js, calls app.listen()
    db/
      index.js            SQLite connection + migration runner
      migrations/*.sql     schema, applied once each, tracked in a `migrations` table
      seed.js              one-time import from site/index.html + first admin account
    lib/
      content.js           content_blocks read/write helpers
      password.js           scrypt hash/verify + a DUMMY_HASH for timing-safe login
      sessionStore.js        express-session Store backed by the same SQLite file
      logger.js              structured (JSON Lines) request/error logging
    middleware/
      auth.js               req.user, requireLoggedIn, requireAdmin
      flash.js               one-shot session flash messages
      csrf.js                 issue/verify a same-session form token
      rateLimit.js            login rate limiting (IP + email)
    routes/
      public.js              /, /demo, /handbook, /monograph, /login, /register,
                               /logout, /account
      admin.js                /admin/*  (content editor, FAQ CRUD, user list)
  views/
    partials/               <head> for site + admin, nav, flash
    site/                    index (the landing page), demo, handbook, monograph
                              (ported from Claude artifacts — see below), login,
                              register, account, 404
    admin/                   login, dashboard, content list/edit, FAQ list/edit, users
  public/
    css/site.css              the landing page's own styles (ported verbatim)
    js/admin.js                small delegated listener (CSP forbids inline onclick)
  test/                    node:test suite (M-5) — auth, admin-content, registration
  test-support/            shared test harness (ephemeral-port server, cookie client)
    css/admin.css              @imports site.css, adds admin-only components
    js/site.js                the landing page's i18n toggle + live spectrum canvas
  database/app.sqlite        the whole database — gitignored, back it up by copying it
  storage/uploads/            local file storage, for whenever a feature needs it
  scripts/
    generate-index-ejs.js     dev tool used once to build views/site/index.ejs — see below
    _body_source.html          its input: the original static page's <body>
```

### How the landing page became content-managed

The static prototype (`../site/index.html`) already tagged every editable
string with `data-i18n="key"` (plain text) or `data-i18n-html="key"` (text
that may contain `<em>`, `<code>`, links, …) — that was originally there to
drive its own client-side English/Ukrainian toggle. `src/db/seed.js` walks
that file with `cheerio` (a real HTML parser — not regex, so nested tags
extract correctly) and imports every one of those 131 keys into
`content_blocks`, both languages, plus the six FAQ entries into their own
`faqs` table.

`views/site/index.ejs` is the same markup with each of those elements'
literal text swapped for `<%= cms.en['key'] %>` / `<%- cms.en['key'] %>` (see
`scripts/generate-index-ejs.js` — a one-time code-generation pass, not a
runtime dependency; string-spliced rather than run through an HTML
serializer, because a DOM library would see `<%=` as malformed markup and
HTML-escape it). The **English** text renders straight from the database on
every request; the **Ukrainian** text is serialized once per request as
`window.__CMS_UK__` and the *exact same client-side toggle script* that the
static prototype used reads it — `public/js/site.js` is that script with
exactly one line changed (the hardcoded Ukrainian object literal became
`window.__CMS_UK__`). The FAQ list works the same way, but through a loop
over the `faqs` table instead of fixed keys, since its length changes.

### Database schema

Four tables (`src/db/migrations/001_init.sql`): `users` (admin + customer,
one table, a `role` column), `sessions` (express-session storage),
`content_blocks` (every editable string, keyed like the template's
`data-i18n` attributes, `(key, lang)` primary key), `faqs` (one row per
question, both languages, so add/remove/reorder is a normal CRUD form instead
of juggling a flat key list).

## What's deliberately not built yet

The brief for this portal named a bigger end state — documentation,
benchmarks, news, and a full customer self-service area — on top of what
shipped in this pass (landing page + FAQ content management + accounts). Built
this way on purpose, so each is a natural extension instead of a rewrite:

- **Docs / benchmarks as CMS pages.** `/demo`, `/handbook` and `/monograph`
  (ported from three Claude artifacts that used to be the site's only links
  for them — those only ever worked for whoever was signed into the account
  that owned the artifact) are real pages on this domain now, but they're
  each one static EJS view, not admin-editable content. `content_blocks` and
  the admin editor pattern generalise directly to a `pages` table (slug,
  title, body per language) rendered through one more EJS view + route — the
  same shape as what FAQ already is — the natural next step if these need
  in-admin editing rather than a code change. The repo's own `docs/*.md` is
  the obvious first import source (a `marked`-based Markdown-to-HTML step,
  similar in spirit to `db/seed.js`'s HTML import).
- **News / changelog.** Same shape as `faqs` (a table with a publish flag and
  sort/date order) with its own small admin CRUD screen.
- **A real support Q&A / ticket system**, if the public FAQ ever isn't
  enough. The FAQ tool this pass built is deliberately the simpler "public
  FAQ, admin-edited" reading of that requirement, not a per-customer ticket
  inbox.
- **Payment / license integration.** Registration is self-serve with a free-text
  "order reference" field today — no check against a merchant of record or the
  Ed25519 key file (`harmonic_synth/license/`). Wiring a purchase webhook to
  auto-create/upgrade an account is the natural next step once one is live.
- **File uploads.** `storage/uploads/` and static-serving for it exist; nothing
  writes there yet — wire it up when a feature needs it (a press-kit asset
  download, a preset-bank attachment, …).

## Security notes for whoever deploys this

- Set a real `SESSION_SECRET` in `.env` before this leaves your machine.
- `cookie.secure` turns on automatically when `NODE_ENV=production` — serve
  production over HTTPS or logins won't set a cookie at all.
- Back up `database/app.sqlite` — it is the entire database. Copying the file
  while the app isn't writing to it is a safe backup (SQLite + WAL mode).
- No rate limiting on `/login` or `/admin/login` yet — add one
  (`express-rate-limit`, still a pure-JS dependency) before a public launch.
