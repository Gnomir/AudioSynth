# Technical Audit — Cosine Web Portal (`webapp/`)

*Independent security & architecture review, conducted as a senior technical
audit / chief-architect pass. Scope, methodology and every finding below are
based on reading the actual source in this repository at the commit noted
below — no finding is inferred from documentation or assumed from the
framework's reputation alone. Where a claim could not be independently
verified in this environment, that is stated explicitly rather than asserted.*

- **Scope**: `webapp/` — the Node.js/Express/SQLite portal (landing page,
  public FAQ, customer accounts, admin panel). Built in this session; never
  previously audited.
- **Explicitly out of scope**: `harmonic_core` / `harmonic_synth` (the DSP
  engine and plugin) received two dedicated, independent audit rounds earlier
  in this project's history (commits `8983808`, `9e70918`) — findings applied
  and verified (cross-platform bit-exact tests, RT-safety suite, `clippy`,
  `pluginval`, `clap-validator`; see `docs/06_VERIFICATION.md`). Re-auditing
  that surface here would be redundant; its three remaining accepted
  trade-offs (B‑3, C‑1, C‑4) are documented in project memory with reasons,
  not repeated here. Legal/licensing items (EULA review, `RELICENSING.md`
  execution, production signing key) are tracked in
  `../product/LAUNCH_CHECKLIST.md` and not duplicated in this document.
- **Audited at**: HEAD `4a22e88` (repo), Node `v24.18.0`, `npm audit`: 0 known
  vulnerabilities in the resolved dependency tree (verified below).
- **Method**: full manual read of every file under `src/`, every view under
  `views/`, the migration/schema, `package.json`; targeted `grep` sweeps for
  known anti-patterns (string-built SQL, missing CSRF, raw output of
  user-controlled data); `npm audit`; a live smoke test against a running
  instance for several findings. No fuzzing, no third-party scanner, no
  penetration test against a deployed instance — this is a source-level
  review.

---

## Executive summary

**23 findings**: 1 Critical, 1 High, 6 Medium, 8 Low, 7 informational/
architectural notes. **Zero SQL-injection surface found** (every query is
parameterized; verified by grep across all of `src/`) and **zero known
vulnerable dependencies** (`npm audit`, verified). The codebase is small,
readable, and its zero-native-dependency discipline held up under review —
the problems found are standard "first pass at auth" gaps, not structural
rot.

**Fix before this ever takes real traffic, in this order:**

1. **[C-1]** Refuse to boot in production with the default session secret — as written, this is a full authentication bypass (forge any session, including admin) the moment someone deploys without reading the README.
2. **[H-1]** Regenerate the session on login (session fixation) — one line, closes a real account-takeover path.
3. **[M-3]** Add CSRF checks to both logout routes — currently the only unprotected state-changing endpoints.
4. **[M-1]/[M-2]** Rate-limit `/login` + `/admin/login`, and stop leaking a timing signal on nonexistent emails — the two of these together turn "unlikely" brute force into "plausible."

**Status: all four items above are fixed and verified** (live HTTP tests during
remediation, plus regression coverage in `test/`) — see the roadmap
checklist at the bottom of this document for the full, itemized status.

Everything else is real but lower-severity or process/architecture work, detailed below.

---

## Findings

Severity: **C**ritical · **H**igh · **M**edium · **L**ow · **I**nfo

### C-1 — Hard-coded fallback session secret; no production safety check

**Where**: `src/server.js:32`
```js
secret: process.env.SESSION_SECRET || 'dev-secret-change-me-in-.env',
```
**Impact**: `express-session` signs the session-ID cookie with this secret. The
fallback string is committed, public, and identical in every checkout of this
repository. If the app is ever started with `NODE_ENV=production` (or simply
without a `.env`) and nobody has set `SESSION_SECRET`, **anyone who has read
this file** — which, once the repo is public per the launch plan, is
everyone — can forge a validly-signed session cookie for **any user ID,
including an admin's**, without ever knowing a password. This is a complete
authentication bypass, not a theoretical one: the exploit is "set a cookie."
`.env.example` and `webapp/README.md` both tell the operator to set a real
secret, but nothing in the code enforces it — a documentation-only mitigation
is not a mitigation against forgetting to read documentation.

**Recommendation**: at startup, if `NODE_ENV === 'production'` and
(`SESSION_SECRET` is unset **or** equals the literal fallback string), refuse
to start (`process.exit(1)`) with a clear error. Optionally also warn (not
block) in non-production modes so the gap is visible in local dev too. ~10
lines in `server.js`, no new dependency.

---

### H-1 — Session fixation: no session regeneration on login

**Where**: `src/routes/public.js:46`, `src/routes/admin.js:28`
```js
req.session.userId = user.id;
```
Both the customer and admin login handlers write the authenticated user's ID
directly into the **existing** session object. `express-session` does not
change the session ID on its own when session *data* changes — only
`req.session.regenerate()` (or the deprecated `req.session.destroy()` +
recreate) issues a new ID.

**Impact**: classic session fixation. An attacker who can get a victim to use
a session ID the attacker already knows (e.g. by setting the cookie
themselves before handing the victim a link, or via a subdomain that can set
cookies for this one) can visit `/login` first to obtain that ID's CSRF token
un-authenticated, wait for the victim to log in under that same ID, and then
use the still-valid, still-known session ID themselves — now authenticated
as the victim. This applies identically to the admin login path, so the
worst case is **full admin takeover**, not just one customer account.

**Recommendation**: call `req.session.regenerate((err) => { ... })` (or
express-session's newer `req.session.regenerate` promise-friendly wrapper)
immediately before setting `userId`, in both `public.js` and `admin.js`.
Re-derive the CSRF token after regeneration too (see M-6 note below) rather
than assuming the old one survives — verify empirically what express-session
does to `req.session` fields across `regenerate()` in the installed version,
since behaviour here has differed across major versions.

---

### M-1 — Timing side-channel on login enables account/email enumeration

**Where**: `src/routes/public.js:42`, `src/routes/admin.js:24`
```js
if (!user || !user.is_active || !verifyPassword(password || '', user.password_hash)) {
```
`verifyPassword` runs `crypto.scryptSync` — deliberately expensive (that's
the point of scrypt). Because `||` short-circuits, `scryptSync` is **never
called** when `!user` is true. A request for a non-existent email returns in
microseconds; a request for a real email with a wrong password takes however
long scrypt takes on this machine (measurable, typically single-digit to
tens of milliseconds). The error message is identical in both cases
("Wrong email or password"), so this doesn't leak via *content* — but the
*timing* difference is externally observable and lets an attacker enumerate
which emails have accounts.

**Recommendation**: always perform a scrypt comparison, even for a
non-existent user — e.g. keep a static dummy hash and verify against it when
`user` is null, so both branches pay the same cost:
```js
const user = findByEmail.get(email);
const ok = user
  ? user.is_active && verifyPassword(password || '', user.password_hash)
  : (verifyPassword(password || '', DUMMY_HASH), false);
```
Low effort, meaningfully closes the channel. Full timing-attack resistance
also depends on network jitter dominating in practice — this is a
best-practice fix, not a claim that it's trivially exploitable remotely.

---

### M-2 — No rate limiting or brute-force throttling on login

**Where**: `src/routes/public.js` (`POST /login`), `src/routes/admin.js`
(`POST /login`) — neither route, nor any middleware in `server.js`, limits
request rate.

**Impact**: both login endpoints accept unlimited attempts from a single
client. Combined with M-1, an attacker can enumerate valid emails quickly,
then brute-force passwords at whatever rate the server's CPU allows scrypt
computations (this actually self-limits somewhat, since each attempt costs
real CPU time — but there's no cap on concurrent connections either).

**Recommendation**: add a rate limiter. `express-rate-limit` is pure
JavaScript (no native build step, consistent with this project's
dependency policy — confirm before adding, per this project's own standard).
A simple per-IP + per-email counter is sufficient for the admin panel and
early customer volume; a SQLite-backed counter (matching the "no extra
infrastructure" philosophy already used for sessions) is a reasonable
from-scratch alternative if a new dependency is unwanted.

---

### M-3 — Logout routes are the only state-changing endpoints without CSRF protection

**Where**: `src/routes/public.js:75`, `src/routes/admin.js:32`
```js
router.post('/logout', (req, res) => { req.session.destroy(() => res.redirect('/')); });
```
Every other `POST`/state-changing route in the app (`/login`, `/register`,
`/admin/content/:key`, `/admin/faq/*`, `/admin/users/:id/toggle-active`)
passes through `verifyCsrf`. Both logout routes do not.

**Impact**: a third-party page can force a logged-in visitor's browser to
`POST /logout` (or `/admin/logout`) with a simple auto-submitting form and no
token — "logout CSRF." On its own this is a nuisance (forced session
termination), not data exposure or privilege escalation. It's included here
because it's an inconsistency in an otherwise-consistent CSRF policy, and
logout-CSRF has occasionally been chained with other bugs elsewhere (e.g. to
force a re-login through an attacker-controlled flow) — cheap to close, so
close it.

**Recommendation**: add `verifyCsrf` to both routes, same as every other
`POST`. Trivial, no design change.

---

### M-4 — `window.__CMS_UK__` JSON is embedded without `</script>`-breakout protection

**Where**: `views/site/index.ejs` (final lines):
```ejs
<script>window.__CMS_UK__ = <%- ukJson %>;</script>
```
`ukJson` is `JSON.stringify()` output inserted **raw** (`<%-`, unescaped).
`JSON.stringify` does not escape the substring `</script>` inside string
values. If any content-block or FAQ value ever contained that literal
substring, the browser's HTML parser would close the `<script>` tag early,
and everything after it in that value would be parsed as HTML/markup in the
page — a stored-injection vector.

**Impact today is bounded by trust**: only an authenticated admin can set
content-block or FAQ text, and an admin already has full control of the page
(they can set `is_html` and inject arbitrary markup deliberately anyway). So
this is not a *privilege-escalation* bug today. It becomes one the moment
any lower-trust actor's input reaches that JSON (a support-ticket field, a
customer-visible display name fed into a future feature, a second admin
account with less trust than the first) — and it costs nothing to fix now.

**Recommendation**: escape the JSON before embedding —
`ukJson.replace(/</g, '\\u003c')` (applied once, server-side, to the
stringified JSON, not to the object) is the standard fix and is exactly what
libraries like `serialize-javascript` do. One line in `src/routes/public.js`.

---

### M-5 — No automated test suite for `webapp/`

**Where**: no `test/` directory, no test script beyond the placeholder that
was never replaced; `package.json` has no `test` entry at all (it was
removed when `start`/`dev`/`seed` were added — there is currently *no*
`npm test` command in this package).

**Impact**: every one of the findings above, and any regression in future
changes to auth, CSRF, or content rendering, is currently only catchable by
manual testing (which is how several of them — and one real bug, the
`$`-in-`.replace()` corruption from the copy-sync script — were actually
found this session, by hand, via curl/`fetch`). `harmonic_core` has 120
automated tests and a hostile-input suite; `webapp/` has zero.

**Recommendation**: add a test suite using Node's built-in `node:test` +
`node:assert` (zero new dependency, consistent with the rest of this
project), covering at minimum: login success/failure/inactive-user, CSRF
rejection on a missing/wrong token, `requireAdmin`/`requireCustomer` gating,
registration validation (short password, duplicate email), and a
content-edit round-trip (POST a value, assert it comes back from `loadAll()`
and renders on `/`). This is the single highest-leverage item on this list
for long-term maintainability, even though no individual test is exciting.

---

### M-6 — `webapp/` is entirely absent from CI

**Where**: `.github/workflows/ci.yml` — four jobs (`core`, `core-simd`,
`plugin`, `cross-verify`), all Rust, none referencing `webapp/`. Verified: zero
occurrences of the string "webapp" in the file.

**Impact**: nothing in `webapp/` is checked on push or PR — not even
`node --check` on every file, let alone tests (which don't exist yet per
M-5). A syntax error or a broken route could be merged to `main` silently.

**Recommendation**: add a `webapp` job: `npm ci`, then (once M-5 lands)
`npm test`; in the meantime, at minimum `node --check` every `.js` file and
a smoke boot (`node src/server.js & sleep 1 && curl -f localhost:PORT/`)
catches crash-on-startup regressions for near-zero cost.

---

### L-1 — `node:sqlite` engine requirement is unverified at the stated floor

**Where**: `package.json`:
```json
"engines": { "node": ">=22.5.0" }
```
**What was verified**: `node:sqlite` loads cleanly with no warning and no
flag on the installed `v24.18.0` in this environment (checked directly:
`require('node:sqlite')` under a `process.on('warning', …)` listener emitted
nothing). **What was not verified**: whether `node:sqlite` requires the
`--experimental-sqlite` CLI flag anywhere within the stated `>=22.5.0` floor.
node:sqlite landed experimental (flagged) in the 22.x line before later
being usable without the flag in a subsequent release — the exact boundary
was not re-confirmed against Node's official changelog in this session, and
should not be taken on the audit's authority alone.

**Recommendation**: before relying on the current floor, either (a) test
against the oldest Node version the `engines` field claims to support, or
(b) consult Node's release notes for the exact version `--experimental-sqlite`
stopped being required, and raise `engines.node` to match, or (c)
conservatively add `"scripts": { "start": "node --experimental-sqlite src/server.js" }`
so the flag is harmless-if-unneeded and load-bearing-if-needed on any
supported version.

---

### L-2 — No security response headers

**Where**: `src/server.js` — no header-setting middleware of any kind (no
CSP, `X-Content-Type-Options`, `X-Frame-Options`/`frame-ancestors`,
`Referrer-Policy`, HSTS).

**Impact**: standard defense-in-depth gaps. Given the app already avoids
inline dynamic user-content injection in the ways that matter (see M-4's
fix), the practical exposure is modest today, but a plugin-selling site
inviting embeds, iframes, or third-party analytics later would benefit from
having this in place from the start rather than retrofitted under time
pressure.

**Recommendation**: `helmet` — verify it has no native dependency before
adding (expected to be pure JS, but confirm per this project's stated
policy rather than assuming) — applied with its defaults is a reasonable
starting point; tighten the CSP specifically once the Google Fonts and any
future third-party script needs are finalized.

---

### L-3 — Password hashing uses Node's default scrypt cost parameters

**Where**: `src/lib/password.js:12`
```js
const hash = crypto.scryptSync(plain, salt, KEY_LEN);
```
No `cost`/`blockSize`/`parallelization` options are passed, so Node's
built-in defaults apply (`N = 16384`). Current OWASP guidance for scrypt
trends toward a higher minimum cost factor where server resources allow.
This is not a broken implementation — the salt is random per password, the
comparison is constant-time (`crypto.timingSafeEqual`), the scheme string
is versioned (`scrypt:salt:hash`) so cost parameters could be embedded and
upgraded later without breaking old hashes.

**Recommendation**: consider explicit, higher cost parameters
(`{ N: 2**17, ... }`, benchmarked against acceptable login latency on the
target deployment hardware), and store the parameters used *in* the hash
string (not just the scheme name) so they can be raised again later without
a breaking migration. Low urgency; the current scheme is far from
indefensible, just not tuned.

---

### L-4 — `requireCustomer` does not actually check role

**Where**: `src/middleware/auth.js:19-25`
```js
function requireCustomer(req, res, next) {
  if (!req.user) { ...redirect... }
  next();
}
```
Despite the name, this only checks "is any authenticated user," not
`role === 'customer'`. Functionally harmless today — an admin visiting
`/account` simply sees their own admin user's row, no cross-account
exposure — but the name promises a guarantee the code doesn't provide,
which is exactly the kind of mismatch that causes a real bug once someone
adds a second gated customer-only route and trusts the name.

**Recommendation**: rename to `requireLoggedIn` (matching what it actually
does), and add a distinct, real `requireCustomer` if a customer-only
(role-checked) route is ever needed.

---

### L-5 — `POST /admin/content/:key` does not verify the key exists before writing

**Where**: `src/routes/admin.js:64-72`. The `GET` route to the edit form
correctly 404s for an unknown key (`content/:key` line 58-62), but the `POST`
handler that saves it does not repeat that check — it upserts unconditionally.

**Impact**: admin-only surface (already gated by `requireAdmin`), so this is
data hygiene, not a security hole: a typo'd URL or a stale bookmark can
silently create an orphan `content_blocks` row that no template ever reads.

**Recommendation**: reuse the same `getBoth(key).section` check on the
`POST` handler and 404/400 on an unknown key, for defense-in-depth and to
stop silent orphan rows.

---

### L-6 — No email verification on registration

**Where**: `src/routes/public.js:56-73` — `POST /register` creates the
account immediately on submission; nothing confirms the submitter controls
the email address entered.

**Impact**: anyone can register an account under any email address without
proving ownership of it. Low impact while the account only unlocks a
same-site "my details" page with no side effects — but if a "forgot
password" flow, order-confirmation email, or any notification is ever built
on top of this, an unverified email becomes a real problem (account
takeover via someone else's real inbox, or simply undeliverable
notifications).

**Recommendation**: not urgent for the current feature set; flag as a
prerequisite before adding password reset or any email-based notification.

---

### L-7 — `license_key` has no format validation or uniqueness constraint

**Where**: `src/db/migrations/001_init.sql:10` — `license_key TEXT` with no
`UNIQUE`, no `CHECK`, no relationship to the real Ed25519 key-file scheme in
`harmonic_synth/license/`.

**Impact**: currently a free-text field the customer fills in themselves
(per `webapp/README.md`, explicitly not checked against a real purchase
yet). Two customers could enter the same reference; a typo is
indistinguishable from a real one. Acceptable for the current placeholder
state, worth flagging as a prerequisite for the real payment-webhook
integration `LAUNCH_CHECKLIST.md` already names as a next step.

---

### L-8 — No admin hierarchy: any admin can deactivate any other admin

**Where**: `src/routes/admin.js:125-134` — the only guard is
`if (user.id === req.user.id)` (can't deactivate yourself); deactivating a
*different* admin is unrestricted.

**Impact**: none with a single admin account (today's reality). Worth a note
for whenever a second admin is created — there is currently no "owner" /
super-admin concept, so any two admins are fully equal and can lock each
other out.

---

### I-1 — `PRAGMA foreign_keys = ON` currently has no effect

**Where**: `src/db/index.js:18` sets the pragma, but the schema
(`001_init.sql`) declares zero `REFERENCES` clauses — there is nothing for
the pragma to enforce yet. Not a bug; a forward-looking note for whenever a
table (e.g. a future `pages` or `orders` table) references `users.id` — add
the `REFERENCES` clause and this pragma starts doing real work with no
further change needed.

---

### I-2 — `site/` and `webapp/`'s content can silently drift

**Where**: process-level, not code — `site/index.html`'s copy is only kept
in sync with `webapp/`'s `content_blocks` by manually re-running
`webapp/scripts/sync-musician-copy-to-static-site.js` after an admin edits
content through the live portal. Nothing enforces this happens.

**Impact**: this was hit for real in this session (two separate copy passes
each required a manual sync run). Low severity today because `site/` is
explicitly a rarely-touched fallback (per its own README), but it is a
standing maintenance trap: edit content in `/admin/content`, forget the sync
script exists, and the "fallback" quietly starts showing stale or
contradictory copy.

**Recommendation**: either (a) accept the drift risk and document the sync
step as a required part of any content-edit checklist, (b) automate the
sync (e.g. a `postinstall`/release script, or trigger it from the admin save
handler itself), or (c) retire `site/` once `webapp/` has a real production
deployment target, removing the second copy of the truth entirely. This is
a decision for the owner, not a code fix — see `AskUserQuestion` note in
the accompanying chat reply if this hasn't been decided yet.

---

### I-3 — No structured logging or error monitoring

**Where**: `src/server.js:56-58` — the only error handling is
`console.error(err)` in the global error middleware; no request logging at
all (no morgan-equivalent), no error-tracking integration.

**Impact**: fine for local development; a genuine gap before any real
deployment — a production incident would be debugged from raw stdout, if
the hosting platform even retains it.

---

### I-4 — No health-check endpoint

**Where**: no `/healthz` or equivalent route exists anywhere in
`src/routes/`.

**Impact**: most hosting platforms (and any future load balancer / process
supervisor) expect a cheap endpoint to poll for liveness. Trivial to add
(`router.get('/healthz', (req, res) => res.sendStatus(200))`), worth doing
before the first real deployment rather than after an outage.

---

### I-5 — No documented backup/restore runbook

**Where**: `webapp/README.md` states "back it up by copying the file" —
true, but not a runbook (no restore steps, no verification that a copy taken
mid-write under WAL mode is consistent, no retention policy, no automation).

**Recommendation**: at minimum, document the exact restore procedure
(stop the app, replace `database/app.sqlite` — and any `-wal`/`-shm`
sidecar files if present, restart) and consider a scheduled copy (cron /
Task Scheduler) once this is running somewhere persistent.

---

### I-6 — No deployment manifest (Dockerfile / Procfile)

**Where**: no `Dockerfile`, no `Procfile`, no platform-specific config
anywhere under `webapp/`.

**Impact**: `webapp/README.md` names Render/Fly.io/a VPS as deploy targets,
but none of them has a checked-in, reproducible build description — the
exact Node version (see L-1) and start command live only in prose. A
`Dockerfile` pinning an exact Node version would also directly resolve the
L-1 ambiguity.

---

### I-7 — No admin action audit trail

**Where**: `content_blocks` and `faqs` both have `updated_at` but no
`updated_by`; there is no log of which admin changed what, when, beyond
"the content is different now."

**Impact**: none with a single admin. Worth adding the column now, while it
costs one migration and one extra bound parameter per write, rather than
after a second admin is added and a dispute happens.

---

## Verified safe (reported for completeness, per the audit's own honesty standard)

- **No SQL injection surface found anywhere.** Every `db.prepare(...)` call
  in `src/` binds parameters positionally (`?` placeholders); grepped the
  entire `src/` tree for string-built SQL (template literals or
  concatenation feeding a query) — zero matches.
- **`npm audit`: 0 known vulnerabilities** across the resolved dependency
  tree (`express`, `express-session`, `ejs`, `dotenv`, `cheerio` and their
  transitive dependencies), checked at audit time.
- **Every user-controlled value that reaches a template is escaped**
  (`<%=`, not `<%-`) at every render site checked: `account.ejs`,
  `admin/users.ejs`, `admin/content-list.ejs`'s tag-stripped preview,
  `admin/faq-edit.ejs`'s pre-filled form fields. The only raw (`<%-`)
  outputs are (a) admin-authored `content_blocks`/FAQ values explicitly
  marked `is_html` — by design, same trust level as the admin who set them
  — and (b) the `ukJson` blob covered in M-4.
- **Passwords are salted per-user and compared in constant time**
  (`crypto.timingSafeEqual`), not compared with `===`.
- **`role` on registration is hard-coded server-side** (`'customer'` literal
  in the `INSERT`), not read from client input — no privilege-escalation
  path via the registration form.
- **IDOR checked and not found**: no route accepts a user-supplied ID to
  fetch *another* user's private data; `/account` always reads `req.user`
  from the session, never from a request parameter.

---

## Prioritized remediation roadmap

Status as of the remediation pass below (commit history has the detail):

**Do before any real (non-localhost) deployment:**
1. ✅ C-1 — production boot guard on `SESSION_SECRET` (`src/app.js`; refuses to
   boot on a placeholder/short secret in prod, warns in dev)
2. ✅ H-1 — session regeneration on login (`public.js`, `admin.js`, both login
   *and* registration — fixation closes the anonymous→authenticated
   transition wherever it happens, not just the named login route)
3. ✅ M-3 — CSRF on both logout routes
4. ✅ M-1, M-2 — timing-safe login comparison (`DUMMY_HASH`, always-run
   `verifyPassword`) + rate limiting (`middleware/rateLimit.js`, keyed by
   IP+email via `express-rate-limit`'s `ipKeyGenerator`)
5. ✅ M-4 — escape `</` in the injected JSON

**Do soon after, before treating this as a maintained product:**
6. ✅ M-5 — a real test suite (`node:test`): 28 tests across
   `test/auth.test.js`, `test/admin-content.test.js`,
   `test/registration.test.js` — login/logout/session-regen, CSRF
   enforcement, access control (customer vs. admin vs. anonymous),
   registration validation, content-edit round-trip, FAQ CRUD. `npm test`
   green.
7. ✅ M-6 — a `webapp` job in `.github/workflows/ci.yml`: `npm ci`, a
   syntax-check sweep, `npm test`, and a production-mode smoke boot against
   `/healthz`.
8. ✅ L-1 — mitigated defensively (`--experimental-sqlite` kept on all npm
   scripts even though unneeded on the Node version actually used here); the
   underlying version-boundary claim in L-1's own text below was never
   independently re-verified and stays flagged as such.
9. ✅ L-2 — `helmet`, with a per-request CSP nonce (`script-src-attr: 'none'`
   fallout — inline `onclick`/`onsubmit` — fixed by moving to `data-*`
   attributes + `public/js/admin.js`)
10. ✅ I-4 — health-check route (`GET /healthz`, ahead of session middleware).
11. ✅ I-3 — structured (JSON Lines) logging (`src/lib/logger.js`): a
    request id + completion log for every request (`/healthz` excluded to
    avoid drowning the stream in liveness-probe noise), and the global
    error handler plus the three session-regenerate failure paths now log
    structured fields (reqId, route, message/stack) instead of a bare
    `console.error(err)`. External error-tracking (Sentry or similar) is
    still not wired in — that needs an account/DSN the owner has to
    provision, an operational decision rather than an engineering default.

**Also fixed opportunistically while in the relevant files (not in the
original "before deployment" list, but cheap and in-scope):**
- L-4 — `requireCustomer` renamed `requireLoggedIn` to match what it actually
  checks (any logged-in user, not customer-vs-admin).
- L-5 — `POST /admin/content/:key` 404s on an unknown key instead of
  silently creating an orphan row (same check the GET route already had).

**Do when the relevant feature actually gets built, not before — unchanged:**
11. L-6 (email verification), L-7 (license-key validation) — once payment
    integration is real
12. L-8, I-7 (admin hierarchy, audit trail) — once a second admin exists
13. I-1 (FK constraints) — once a table actually needs one

**Owner decision, not an engineering task — unchanged:**
14. I-2 — whether to keep syncing `site/` by hand, automate it, or retire it

---

*This document reflects a source-level read at one point in time. It is not
a substitute for a penetration test against a deployed instance, and it does
not cover the Rust DSP engine or the legal/licensing track, both of which
have their own, separate review trails referenced above.*
