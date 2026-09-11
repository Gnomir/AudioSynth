'use strict';

const express = require('express');
const { db } = require('../db');
const { loadAll } = require('../lib/content');
const { hashPassword, verifyPassword, DUMMY_HASH } = require('../lib/password');
const { requireLoggedIn } = require('../middleware/auth');
const { verifyCsrf } = require('../middleware/csrf');
const { loginLimiter } = require('../middleware/rateLimit');
const logger = require('../lib/logger');

const router = express.Router();

const findByEmail = db.prepare('SELECT * FROM users WHERE email = ?');
const insertUser = db.prepare(`
  INSERT INTO users (email, password_hash, display_name, license_key, role)
  VALUES (?, ?, ?, ?, 'customer')
`);

router.get('/', (req, res) => {
  const cms = loadAll();
  const faqs = db.prepare('SELECT * FROM faqs WHERE published = 1 ORDER BY sort_order').all();

  // Fold each FAQ's Ukrainian text into the same dictionary the client's
  // i18n toggle already reads (window.__CMS_UK__), under synthetic keys the
  // template gives each row (faq.q<n> / faq.a<n>) — no client-side change
  // needed for a dynamic-length FAQ list.
  const ukMerged = { ...cms.uk };
  faqs.forEach((f, i) => {
    ukMerged[`faq.q${i + 1}`] = f.question_uk;
    ukMerged[`faq.a${i + 1}`] = f.answer_uk;
  });

  // AUDIT.md M-4: JSON.stringify never escapes a literal "</script>" inside a
  // string value — left raw, that would close the <script> tag early and let
  // whatever follows in the value be parsed as markup. Escaping "<" to its
  // unicode escape (valid inside a JS string, inert to the HTML parser) is
  // the standard fix; harmless for every other character.
  const ukJson = JSON.stringify(ukMerged).replace(/</g, '\\u003c');
  res.render('site/index', { title: 'Cosine', cms, faqs, ukJson });
});

// Ported from the "harmonic_core · live" Claude artifact (was
// https://claude.ai/code/artifact/ad41ef31-...) so it's a real page on this
// site instead of a privately-owned artifact link the footer/hero CTAs
// pointed at — those links only ever worked for whoever was signed into the
// account that owned the artifact.
router.get('/demo', (req, res) => {
  res.render('site/demo');
});

// Ported from the "Cosine Handbook" artifact (was
// https://claude.ai/code/artifact/a734ea1b-...) for the same reason as /demo.
router.get('/handbook', (req, res) => {
  res.render('site/handbook');
});

// Ported from the "Замкнена форма адитивного синтезу" artifact (was
// https://claude.ai/code/artifact/c4b2806f-...) for the same reason as /demo.
router.get('/monograph', (req, res) => {
  res.render('site/monograph');
});

router.get('/login', (req, res) => {
  res.render('site/login', { title: 'Log in — Cosine' });
});

router.post('/login', loginLimiter, verifyCsrf, (req, res) => {
  const { email, password } = req.body;
  const user = findByEmail.get(String(email || '').trim().toLowerCase());
  // AUDIT.md M-1: always run verifyPassword, even for an email that doesn't
  // exist — against DUMMY_HASH when it doesn't — so both cases cost the same
  // scrypt computation and aren't distinguishable by response time.
  const passwordOk = verifyPassword(password || '', user ? user.password_hash : DUMMY_HASH);
  if (!user || !user.is_active || !passwordOk) {
    req.flash('error', 'Wrong email or password.');
    return res.redirect('/login');
  }

  // AUDIT.md H-1: regenerate the session on privilege change (anonymous ->
  // authenticated) so a session ID an attacker fixated before login is
  // worthless afterwards — express-session does not rotate the ID on its
  // own just because session *data* changed.
  const returnTo = req.session.returnTo;
  req.session.regenerate((err) => {
    if (err) {
      logger.error('session regenerate failed', { reqId: req.id, route: 'login', message: err.message });
      req.flash('error', 'Something went wrong logging you in — try again.');
      return res.redirect('/login');
    }
    req.session.userId = user.id;
    res.redirect(returnTo || (user.role === 'admin' ? '/admin' : '/account'));
  });
});

router.get('/register', (req, res) => {
  res.render('site/register', { title: 'Create account — Cosine' });
});

router.post('/register', verifyCsrf, (req, res) => {
  const email = String(req.body.email || '').trim().toLowerCase();
  const { password, display_name: displayName, license_key: licenseKey } = req.body;

  if (!email || !password || password.length < 8) {
    req.flash('error', 'Enter an email and a password of at least 8 characters.');
    return res.redirect('/register');
  }
  if (findByEmail.get(email)) {
    req.flash('error', 'An account with that email already exists.');
    return res.redirect('/register');
  }

  const info = insertUser.run(email, hashPassword(password), displayName || null, licenseKey || null);
  const newUserId = Number(info.lastInsertRowid);

  // Same reasoning as the /login fix (AUDIT.md H-1): registration is also an
  // anonymous -> authenticated transition and deserves a fresh session ID.
  req.session.regenerate((err) => {
    if (err) {
      logger.error('session regenerate failed', { reqId: req.id, route: 'register', message: err.message });
      req.flash('error', 'Account created — please log in.');
      return res.redirect('/login');
    }
    req.session.userId = newUserId;
    req.flash('success', 'Welcome — your account is ready.');
    res.redirect('/account');
  });
});

router.post('/logout', verifyCsrf, (req, res) => {
  req.session.destroy(() => res.redirect('/'));
});

router.get('/account', requireLoggedIn, (req, res) => {
  res.render('site/account', { title: 'My account — Cosine' });
});

module.exports = router;
