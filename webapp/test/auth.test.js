'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { startServer, stopServer, createUser, makeClient, loginAs } = require('../test-support/helpers');

test.before(async () => {
  await startServer();
  createUser({ email: 'admin@test.local', password: 'correct-horse-battery', role: 'admin' });
  createUser({ email: 'customer@test.local', password: 'correct-horse-battery', role: 'customer' });
  createUser({ email: 'disabled@test.local', password: 'correct-horse-battery', role: 'customer', isActive: 0 });
});
test.after(stopServer);

test('GET /login renders a form with a CSRF token', async () => {
  const client = makeClient();
  const csrf = await client.getCsrf('/login');
  assert.ok(csrf && csrf.length > 10, 'expected a non-trivial CSRF token');
});

test('POST /login with the wrong password is rejected and redirects back to /login', async () => {
  const client = makeClient();
  const csrf = await client.getCsrf('/login');
  const res = await client.post('/login', { email: 'customer@test.local', password: 'nope', _csrf: csrf });
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/login');
});

test('POST /login for a deactivated account is rejected even with the right password', async () => {
  const client = makeClient();
  const csrf = await client.getCsrf('/login');
  const res = await client.post('/login', { email: 'disabled@test.local', password: 'correct-horse-battery', _csrf: csrf });
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/login');
});

test('POST /login without a CSRF token is rejected (403), not silently accepted', async () => {
  const client = makeClient();
  await client.getCsrf('/login'); // establish a session, but don't send its token
  const res = await client.post('/login', { email: 'customer@test.local', password: 'correct-horse-battery' });
  assert.equal(res.status, 403);
});

test('POST /login with correct credentials succeeds and redirects to /account', async () => {
  const { res } = await loginAs('', 'customer@test.local', 'correct-horse-battery');
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/account');
});

test('AUDIT.md H-1: login issues a new session id (regenerate), not a mutated old one', async () => {
  const client = makeClient();
  await client.getCsrf('/login');
  const preLoginCookie = client.currentCookie();
  const csrf = await client.getCsrf('/login'); // same session, fresh token isn't needed twice but harmless
  const res = await client.post('/login', { email: 'customer@test.local', password: 'correct-horse-battery', _csrf: csrf });
  assert.equal(res.status, 302);
  const postLoginCookie = client.currentCookie();
  assert.notEqual(postLoginCookie, preLoginCookie, 'session id must change across the anonymous -> authenticated transition');
});

test('GET /account without a session redirects to /login', async () => {
  const client = makeClient();
  const res = await client.fetch('/account');
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/login');
});

test('GET /account with a customer session succeeds and shows that account', async () => {
  const { client } = await loginAs('', 'customer@test.local', 'correct-horse-battery');
  const res = await client.fetch('/account');
  assert.equal(res.status, 200);
  const html = await res.text();
  assert.ok(html.includes('customer@test.local'));
});

test('POST /logout without a CSRF token is rejected (AUDIT.md M-3)', async () => {
  const { client } = await loginAs('', 'customer@test.local', 'correct-horse-battery');
  const res = await client.post('/logout', {});
  assert.equal(res.status, 403);
});

test('POST /logout with a CSRF token succeeds and the session no longer reaches /account', async () => {
  const { client } = await loginAs('', 'customer@test.local', 'correct-horse-battery');
  const csrf = await client.getCsrf('/account'); // same session already has a token; harmless to re-read
  const logoutRes = await client.post('/logout', { _csrf: csrf });
  assert.equal(logoutRes.status, 302);
  const after = await client.fetch('/account');
  assert.equal(after.status, 302);
  assert.equal(after.headers.get('location'), '/login');
});

test('GET /admin without any session redirects to /admin/login', async () => {
  const client = makeClient();
  const res = await client.fetch('/admin');
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/admin/login');
});

test('a logged-in customer cannot reach /admin (requireAdmin actually checks role)', async () => {
  const { client } = await loginAs('', 'customer@test.local', 'correct-horse-battery');
  const res = await client.fetch('/admin');
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/admin/login');
});

test('an admin can log in via /admin/login and reach /admin', async () => {
  const { client, res: loginRes } = await loginAs('/admin', 'admin@test.local', 'correct-horse-battery');
  assert.equal(loginRes.status, 302);
  assert.equal(loginRes.headers.get('location'), '/admin');
  const res = await client.fetch('/admin');
  assert.equal(res.status, 200);
});

test('a non-admin account is rejected by /admin/login even with the right password', async () => {
  const { res } = await loginAs('/admin', 'customer@test.local', 'correct-horse-battery');
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('location'), '/admin/login');
});

test('AUDIT.md M-1: login timing for a nonexistent email is not wildly faster than a real one', async () => {
  // A loose bound, not a precise timing-attack proof — the fix (always run
  // scrypt) is verified structurally in src/lib/password.js and src/routes/*;
  // this just checks the two paths are in the same ballpark, catching a
  // regression where the fast-path short-circuit comes back.
  async function timeAttempt(email) {
    const client = makeClient();
    const csrf = await client.getCsrf('/login');
    const t0 = process.hrtime.bigint();
    await client.post('/login', { email, password: 'whatever-wrong', _csrf: csrf });
    return Number(process.hrtime.bigint() - t0) / 1e6;
  }
  const real = await timeAttempt('customer@test.local');
  const fake = await timeAttempt('no-such-user-at-all@test.local');
  // Skipping verifyPassword entirely (the old bug) is or­ders of magnitude
  // faster than actually running scrypt — a >3x gap would indicate that.
  const ratio = Math.max(real, fake) / Math.max(Math.min(real, fake), 0.001);
  assert.ok(ratio < 3, `timing ratio ${ratio.toFixed(2)}x looks like scrypt is being skipped for one case (real=${real}ms fake=${fake}ms)`);
});
