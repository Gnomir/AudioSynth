'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { startServer, stopServer, createUser, makeClient } = require('../test-support/helpers');

test.before(async () => {
  await startServer();
  createUser({ email: 'existing@test.local', password: 'correct-horse-battery' });
});
test.after(stopServer);

test('registration rejects a password shorter than 8 characters', async () => {
  const client = makeClient();
  const csrf = await client.getCsrf('/register');
  const res = await client.post('/register', {
    email: 'shortpw@test.local', password: 'short', _csrf: csrf,
  });
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/register');

  const { db } = require('../src/db');
  const created = db.prepare('SELECT * FROM users WHERE email = ?').get('shortpw@test.local');
  assert.equal(created, undefined, 'no account should have been created');
});

test('registration rejects a duplicate email', async () => {
  const client = makeClient();
  const csrf = await client.getCsrf('/register');
  const res = await client.post('/register', {
    email: 'existing@test.local', password: 'another-long-password', _csrf: csrf,
  });
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/register');
});

test('registration is case-insensitive on email uniqueness', async () => {
  const client = makeClient();
  const csrf = await client.getCsrf('/register');
  const res = await client.post('/register', {
    email: 'EXISTING@test.local', password: 'another-long-password', _csrf: csrf,
  });
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/register');
});

test('registration without a CSRF token is rejected', async () => {
  const client = makeClient();
  await client.getCsrf('/register');
  const res = await client.post('/register', { email: 'nocsrf@test.local', password: 'a-fine-password' });
  assert.equal(res.status, 403);
});

test('a valid registration creates the account, logs the user in, and redirects to /account', async () => {
  const client = makeClient();
  const csrf = await client.getCsrf('/register');
  const res = await client.post('/register', {
    email: 'newcustomer@test.local',
    password: 'a-fine-password',
    display_name: 'New Customer',
    _csrf: csrf,
  });
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/account');

  const { db } = require('../src/db');
  const created = db.prepare('SELECT * FROM users WHERE email = ?').get('newcustomer@test.local');
  assert.ok(created);
  assert.equal(created.role, 'customer');
  assert.equal(created.display_name, 'New Customer');

  const account = await client.fetch('/account');
  assert.equal(account.status, 200, 'the freshly-registered session should already be logged in');
});
