'use strict';

const express = require('express');
const { db } = require('../db');
const { loadForAdmin, getBoth, setValue } = require('../lib/content');
const { verifyPassword, DUMMY_HASH } = require('../lib/password');
const { requireAdmin } = require('../middleware/auth');
const { verifyCsrf } = require('../middleware/csrf');
const { loginLimiter } = require('../middleware/rateLimit');
const logger = require('../lib/logger');

const router = express.Router();

const findByEmail = db.prepare('SELECT * FROM users WHERE email = ?');

// ---------- admin auth (shares the users table; role = 'admin') ----------

router.get('/login', (req, res) => {
  if (req.user && req.user.role === 'admin') return res.redirect('/admin');
  res.render('admin/login', { title: 'Admin login' });
});

router.post('/login', loginLimiter, verifyCsrf, (req, res) => {
  const { email, password } = req.body;
  const user = findByEmail.get(String(email || '').trim().toLowerCase());
  // AUDIT.md M-1: same timing-safe treatment as the customer login — always
  // pay the scrypt cost so a nonexistent/non-admin email isn't distinguishable
  // by response time from a real admin email with a wrong password.
  const passwordOk = verifyPassword(password || '', user ? user.password_hash : DUMMY_HASH);
  if (!user || user.role !== 'admin' || !user.is_active || !passwordOk) {
    req.flash('error', 'Wrong email or password.');
    return res.redirect('/admin/login');
  }

  // AUDIT.md H-1: regenerate on login — an admin session is the highest-value
  // target for fixation in this app, so this matters most exactly here.
  req.session.regenerate((err) => {
    if (err) {
      logger.error('session regenerate failed', { reqId: req.id, route: 'admin-login', message: err.message });
      req.flash('error', 'Something went wrong logging you in — try again.');
      return res.redirect('/admin/login');
    }
    req.session.userId = user.id;
    res.redirect('/admin');
  });
});

router.post('/logout', verifyCsrf, (req, res) => {
  req.session.destroy(() => res.redirect('/admin/login'));
});

router.use(requireAdmin);

// ---------- dashboard ----------

router.get('/', (req, res) => {
  const stats = {
    customers: db.prepare("SELECT COUNT(*) AS n FROM users WHERE role = 'customer'").get().n,
    admins: db.prepare("SELECT COUNT(*) AS n FROM users WHERE role = 'admin'").get().n,
    faqs: db.prepare('SELECT COUNT(*) AS n FROM faqs').get().n,
    faqsPublished: db.prepare('SELECT COUNT(*) AS n FROM faqs WHERE published = 1').get().n,
    contentKeys: db.prepare('SELECT COUNT(DISTINCT key) AS n FROM content_blocks').get().n,
  };
  res.render('admin/dashboard', { title: 'Dashboard', stats });
});

// ---------- site content editor ----------

router.get('/content', (req, res) => {
  const groups = loadForAdmin();
  res.render('admin/content-list', { title: 'Site content', groups });
});

router.get('/content/:key', (req, res) => {
  const item = getBoth(req.params.key);
  if (!item.section) return res.status(404).send('Unknown content key');
  res.render('admin/content-edit', { title: `Edit — ${item.key}`, item });
});

router.post('/content/:key', verifyCsrf, (req, res) => {
  const key = req.params.key;
  const existing = getBoth(key);
  // AUDIT.md L-5: the GET edit form already 404s on an unknown key; repeat
  // the check here so a stale bookmark or a hand-typed URL can't silently
  // create an orphan content_blocks row nothing ever reads.
  if (!existing.section) return res.status(404).send('Unknown content key');
  const isHtml = req.body.is_html === 'on';
  setValue(key, 'en', req.body.en || '', isHtml, existing.section);
  setValue(key, 'uk', req.body.uk || '', isHtml, existing.section);
  req.flash('success', `Saved "${key}".`);
  res.redirect('/admin/content#' + encodeURIComponent(key));
});

// ---------- FAQ CRUD ----------

router.get('/faq', (req, res) => {
  const faqs = db.prepare('SELECT * FROM faqs ORDER BY sort_order').all();
  res.render('admin/faq-list', { title: 'FAQ', faqs });
});

router.get('/faq/new', (req, res) => {
  res.render('admin/faq-edit', { title: 'New FAQ entry', item: null });
});

router.post('/faq/new', verifyCsrf, (req, res) => {
  const { question_en: qEn, answer_en: aEn, question_uk: qUk, answer_uk: aUk, sort_order: sortOrder } = req.body;
  db.prepare(`
    INSERT INTO faqs (question_en, answer_en, question_uk, answer_uk, sort_order, published)
    VALUES (?, ?, ?, ?, ?, ?)
  `).run(qEn, aEn, qUk, aUk, Number(sortOrder) || 0, req.body.published === 'on' ? 1 : 0);
  req.flash('success', 'FAQ entry created.');
  res.redirect('/admin/faq');
});

router.get('/faq/:id/edit', (req, res) => {
  const item = db.prepare('SELECT * FROM faqs WHERE id = ?').get(req.params.id);
  if (!item) return res.status(404).send('Not found');
  res.render('admin/faq-edit', { title: 'Edit FAQ entry', item });
});

router.post('/faq/:id/edit', verifyCsrf, (req, res) => {
  const { question_en: qEn, answer_en: aEn, question_uk: qUk, answer_uk: aUk, sort_order: sortOrder } = req.body;
  db.prepare(`
    UPDATE faqs SET question_en = ?, answer_en = ?, question_uk = ?, answer_uk = ?,
      sort_order = ?, published = ?, updated_at = datetime('now')
    WHERE id = ?
  `).run(qEn, aEn, qUk, aUk, Number(sortOrder) || 0, req.body.published === 'on' ? 1 : 0, req.params.id);
  req.flash('success', 'FAQ entry updated.');
  res.redirect('/admin/faq');
});

router.post('/faq/:id/delete', verifyCsrf, (req, res) => {
  db.prepare('DELETE FROM faqs WHERE id = ?').run(req.params.id);
  req.flash('success', 'FAQ entry deleted.');
  res.redirect('/admin/faq');
});

// ---------- users (read-only list + activate/deactivate) ----------

router.get('/users', (req, res) => {
  const users = db.prepare('SELECT id, email, role, display_name, license_key, is_active, created_at FROM users ORDER BY created_at DESC').all();
  res.render('admin/users', { title: 'Users', users });
});

router.post('/users/:id/toggle-active', verifyCsrf, (req, res) => {
  const user = db.prepare('SELECT * FROM users WHERE id = ?').get(req.params.id);
  if (!user) return res.status(404).send('Not found');
  if (user.id === req.user.id) {
    req.flash('error', "You can't deactivate your own account.");
    return res.redirect('/admin/users');
  }
  db.prepare('UPDATE users SET is_active = ? WHERE id = ?').run(user.is_active ? 0 : 1, user.id);
  res.redirect('/admin/users');
});

module.exports = router;
