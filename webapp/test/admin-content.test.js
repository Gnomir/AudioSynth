'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { startServer, stopServer, createUser, loginAs } = require('../test-support/helpers');
const { setValue, getBoth } = require('../src/lib/content');

let admin;
let customer;

test.before(async () => {
  await startServer();
  createUser({ email: 'admin@test.local', password: 'correct-horse-battery', role: 'admin' });
  createUser({ email: 'customer@test.local', password: 'correct-horse-battery', role: 'customer' });
  // Seed one real content key, the way the site's own content actually
  // arrives (through lib/content, not a raw insert), so the admin editor has
  // something pre-existing to round-trip.
  setValue('hero.title', 'en', 'Original English title', 0, 'hero');
  setValue('hero.title', 'uk', 'Оригінальний заголовок', 0, 'hero');
  admin = await loginAs('/admin', 'admin@test.local', 'correct-horse-battery');
  customer = await loginAs('', 'customer@test.local', 'correct-horse-battery');
});
test.after(stopServer);

test('AUDIT.md L-5: editing an unknown content key 404s instead of creating an orphan row', async () => {
  const csrf = await admin.client.getCsrf('/admin/content/hero.title');
  const res = await admin.client.post('/admin/content/this-key-does-not-exist', {
    en: 'x', uk: 'y', _csrf: csrf,
  });
  assert.equal(res.status, 404);
});

test('GET /admin/content/:key for an unknown key also 404s', async () => {
  const res = await admin.client.fetch('/admin/content/this-key-does-not-exist');
  assert.equal(res.status, 404);
});

test('content edit round-trip: admin can update an existing key and the new value is persisted', async () => {
  const csrf = await admin.client.getCsrf('/admin/content/hero.title');
  const res = await admin.client.post('/admin/content/hero.title', {
    en: 'Updated English title', uk: 'Оновлений заголовок', _csrf: csrf,
  });
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/admin/content#hero.title');

  const stored = getBoth('hero.title');
  assert.equal(stored.en, 'Updated English title');
  assert.equal(stored.uk, 'Оновлений заголовок');
  assert.equal(stored.section, 'hero');
});

test('content edit is rejected without a CSRF token', async () => {
  await admin.client.getCsrf('/admin/content/hero.title');
  const res = await admin.client.post('/admin/content/hero.title', { en: 'no csrf', uk: 'без токена' });
  assert.equal(res.status, 403);
  assert.equal(getBoth('hero.title').en, 'Updated English title', 'value must be unchanged');
});

test('a logged-in customer cannot reach the admin content editor', async () => {
  const res = await customer.client.fetch('/admin/content');
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/admin/login');
});

test('an anonymous visitor cannot reach the admin content editor', async () => {
  const { makeClient } = require('../test-support/helpers');
  const anon = makeClient();
  const res = await anon.fetch('/admin/content');
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/admin/login');
});

test('FAQ CRUD: create, edit, and delete round-trip', async () => {
  let csrf = await admin.client.getCsrf('/admin/faq/new');
  let res = await admin.client.post('/admin/faq/new', {
    question_en: 'Does it work offline?',
    answer_en: 'Yes, fully.',
    question_uk: 'Чи працює офлайн?',
    answer_uk: 'Так, повністю.',
    sort_order: '1',
    published: 'on',
    _csrf: csrf,
  });
  assert.equal(res.status, 302);

  const { db } = require('../src/db');
  const created = db.prepare('SELECT * FROM faqs WHERE question_en = ?').get('Does it work offline?');
  assert.ok(created, 'FAQ row should exist after creation');
  assert.equal(created.published, 1);

  csrf = await admin.client.getCsrf(`/admin/faq/${created.id}/edit`);
  res = await admin.client.post(`/admin/faq/${created.id}/edit`, {
    question_en: 'Does it run offline?',
    answer_en: 'Yes, fully.',
    question_uk: 'Чи працює офлайн?',
    answer_uk: 'Так, повністю.',
    sort_order: '1',
    _csrf: csrf, // published omitted => unpublish
  });
  assert.equal(res.status, 302);
  const updated = db.prepare('SELECT * FROM faqs WHERE id = ?').get(created.id);
  assert.equal(updated.question_en, 'Does it run offline?');
  assert.equal(updated.published, 0);

  csrf = await admin.client.getCsrf('/admin/faq');
  res = await admin.client.post(`/admin/faq/${created.id}/delete`, { _csrf: csrf });
  assert.equal(res.status, 302);
  const gone = db.prepare('SELECT * FROM faqs WHERE id = ?').get(created.id);
  assert.equal(gone, undefined);
});

test('an admin cannot deactivate their own account', async () => {
  const adminUser = require('../src/db').db.prepare('SELECT * FROM users WHERE email = ?').get('admin@test.local');
  const csrf = await admin.client.getCsrf('/admin/users');
  const res = await admin.client.post(`/admin/users/${adminUser.id}/toggle-active`, { _csrf: csrf });
  assert.equal(res.status, 302);
  const stillActive = require('../src/db').db.prepare('SELECT is_active FROM users WHERE id = ?').get(adminUser.id);
  assert.equal(stillActive.is_active, 1);
});
