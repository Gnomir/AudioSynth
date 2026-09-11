'use strict';

const express = require('express');
const { db } = require('../db');
const { loadAll } = require('../lib/content');
const { hashPassword, verifyPassword } = require('../lib/password');
const { requireCustomer } = require('../middleware/auth');
const { verifyCsrf } = require('../middleware/csrf');

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

  res.render('site/index', { title: 'Cosine', cms, faqs, ukJson: JSON.stringify(ukMerged) });
});

router.get('/login', (req, res) => {
  res.render('site/login', { title: 'Log in — Cosine' });
});

router.post('/login', verifyCsrf, (req, res) => {
  const { email, password } = req.body;
  const user = findByEmail.get(String(email || '').trim().toLowerCase());
  if (!user || !user.is_active || !verifyPassword(password || '', user.password_hash)) {
    req.flash('error', 'Wrong email or password.');
    return res.redirect('/login');
  }
  req.session.userId = user.id;
  const dest = req.session.returnTo || (user.role === 'admin' ? '/admin' : '/account');
  delete req.session.returnTo;
  res.redirect(dest);
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
  req.session.userId = Number(info.lastInsertRowid);
  req.flash('success', 'Welcome — your account is ready.');
  res.redirect('/account');
});

router.post('/logout', (req, res) => {
  req.session.destroy(() => res.redirect('/'));
});

router.get('/account', requireCustomer, (req, res) => {
  res.render('site/account', { title: 'My account — Cosine' });
});

module.exports = router;
