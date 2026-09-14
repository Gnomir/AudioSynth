// Builds and returns the configured Express app, without binding to a port.
// Split out of server.js (AUDIT.md M-5) so tests can exercise real routes
// against an in-process app on an ephemeral port, instead of only ever
// running as a standalone process.
'use strict';

require('dotenv').config();

const path = require('node:path');
const crypto = require('node:crypto');
const express = require('express');
const session = require('express-session');
const helmet = require('helmet');

const { db } = require('./db');
const { SqliteSessionStore } = require('./lib/sessionStore');
const { loadUser } = require('./middleware/auth');
const { flash } = require('./middleware/flash');
const { issueCsrf } = require('./middleware/csrf');
const logger = require('./lib/logger');

const publicRoutes = require('./routes/public');
const adminRoutes = require('./routes/admin');
const webhookRoutes = require('./routes/webhooks');

const app = express();
const isProd = process.env.NODE_ENV === 'production';

// AUDIT.md C-1: the fallback below is intentionally a known, published
// string — it exists only so local dev works with zero setup. Deploying
// with it unset (or NODE_ENV not set to "production") signs every session
// cookie with a secret anyone who has read this file already knows, which
// means anyone can forge a session for any user, including an admin. Refuse
// to boot rather than fail open.
//
// A blocklist of the one literal fallback string is not enough on its own:
// the first version of this guard only checked SESSION_SECRET against that
// exact string and missed the far more likely real mistake — copying
// .env.example to .env and never editing the "change-me" placeholder
// it ships with, which *is* a set, non-default env var, just still a
// guessable one. Catch that class of mistake with a minimum-length check
// (any properly generated secret, hex or base64 of 32+ random bytes, is
// comfortably longer than any placeholder a human would type by hand)
// alongside a small blocklist of exact known placeholders for a clearer
// error message on the common cases.
const DEFAULT_SESSION_SECRET = 'dev-secret-change-me-in-.env';
const KNOWN_PLACEHOLDER_SECRETS = new Set([DEFAULT_SESSION_SECRET, 'change-me', 'changeme', 'secret']);
const MIN_SECRET_LENGTH = 32;
const SESSION_SECRET = process.env.SESSION_SECRET || DEFAULT_SESSION_SECRET;
const secretLooksWeak =
  KNOWN_PLACEHOLDER_SECRETS.has(SESSION_SECRET) || SESSION_SECRET.length < MIN_SECRET_LENGTH;

if (isProd && secretLooksWeak) {
  console.error(
    '\nFATAL: NODE_ENV=production but SESSION_SECRET is missing, a known placeholder, or too\n' +
    `short (< ${MIN_SECRET_LENGTH} chars) to be a real random secret. Anyone who can guess it can forge a\n` +
    'session for any account, including admin. Set a real one in .env — generate one with:\n' +
    '  node -e "console.log(require(\'crypto\').randomBytes(32).toString(\'hex\'))"\n'
  );
  process.exit(1);
}
if (!isProd && secretLooksWeak && process.env.NODE_ENV !== 'test') {
  console.warn('[warn] SESSION_SECRET is missing or a placeholder — fine for local dev only, never deploy like this.');
}

app.set('view engine', 'ejs');
app.set('views', path.join(__dirname, '..', 'views'));
app.set('trust proxy', 1);

// AUDIT.md I-3: a request id (for correlating this request's log lines,
// including any error it triggers) and a structured completion log. Placed
// first so every request gets an id, even one that 404s or throws before
// reaching a route. /healthz is excluded from the completion log — a
// liveness probe polling every few seconds would otherwise drown out
// everything else in the log stream.
app.use((req, res, next) => {
  req.id = crypto.randomUUID();
  res.setHeader('X-Request-Id', req.id);
  if (req.path === '/healthz') return next();
  const start = process.hrtime.bigint();
  res.on('finish', () => {
    const durationMs = Number(process.hrtime.bigint() - start) / 1e6;
    logger.info('request', {
      reqId: req.id,
      method: req.method,
      path: req.path,
      status: res.statusCode,
      durationMs: Math.round(durationMs * 100) / 100,
    });
  });
  next();
});

// A random nonce per request, so the two small inline <script> blocks
// index.ejs needs (the server-rendered window.__CMS_UK__ data, and nothing
// else) can be allow-listed individually instead of the CSP falling back to
// 'unsafe-inline' for script-src — which would allow *any* inline script,
// defeating most of the point of having a CSP at all.
app.use((req, res, next) => {
  res.locals.cspNonce = crypto.randomBytes(16).toString('base64');
  next();
});

// AUDIT.md L-2: baseline security headers (CSP, X-Content-Type-Options,
// frame-ancestors, etc). Google Fonts is the only third-party origin this
// app loads, so the default CSP is widened just enough for that and nothing
// else; tighten further if a stricter policy is ever needed.
//
// 'wasm-unsafe-eval' on script-src: the /demo page calls
// WebAssembly.instantiate() to run harmonic_core's wasm32 build. Chrome
// treats wasm compilation as covered by script-src's eval restriction and
// blocks it under a strict CSP unless this keyword (or the much broader
// 'unsafe-eval', which this app does NOT want — it would also re-allow
// plain JS eval()/Function()) is present. Firefox/Safari don't gate wasm
// this way, but the keyword is harmless there.
app.use((req, res, next) => helmet({
  contentSecurityPolicy: {
    directives: {
      ...helmet.contentSecurityPolicy.getDefaultDirectives(),
      'style-src': ["'self'", "'unsafe-inline'", 'https://fonts.googleapis.com'],
      'font-src': ["'self'", 'https://fonts.gstatic.com'],
      'script-src': ["'self'", `'nonce-${res.locals.cspNonce}'`, "'wasm-unsafe-eval'"],
    },
  },
})(req, res, next));

app.use(express.urlencoded({ extended: false }));
app.use(express.static(path.join(__dirname, '..', 'public')));
app.use('/uploads', express.static(path.join(__dirname, '..', 'storage', 'uploads')));

// AUDIT.md I-4: a cheap liveness endpoint for a process supervisor / load
// balancer, ahead of the session middleware so it never touches the DB.
app.get('/healthz', (req, res) => res.status(200).json({ ok: true }));

// Ahead of the session middleware, same reasoning as /healthz — a
// third-party webhook carries no session cookie and shouldn't get one.
app.use('/webhooks', webhookRoutes);

app.use(session({
  store: new SqliteSessionStore(db),
  secret: SESSION_SECRET,
  resave: false,
  saveUninitialized: false,
  cookie: {
    httpOnly: true,
    sameSite: 'lax',
    secure: isProd,
    maxAge: 30 * 24 * 60 * 60 * 1000, // 30 days
  },
}));

app.use(loadUser);
app.use(flash);
app.use(issueCsrf);
app.use((req, res, next) => { res.locals.path = req.path; next(); });

app.use('/', publicRoutes);
app.use('/admin', adminRoutes);

app.use((req, res) => {
  res.status(404).render('site/404', { title: '404' });
});

// eslint-disable-next-line no-unused-vars
app.use((err, req, res, next) => {
  // AUDIT.md I-3
  logger.error('unhandled error', {
    reqId: req.id,
    method: req.method,
    path: req.path,
    message: err.message,
    stack: err.stack,
  });
  res.status(500).send('Internal error. Check the server logs.');
});

module.exports = app;
