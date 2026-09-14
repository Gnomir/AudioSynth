'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const { startServer, stopServer } = require('../test-support/helpers');

const SECRET = 'test-freemius-secret';
let baseUrl;

test.before(async () => {
  process.env.FREEMIUS_WEBHOOK_SECRET = SECRET;
  baseUrl = await startServer();
});
test.after(stopServer);

function sign(body, secret = SECRET) {
  return crypto.createHmac('sha256', secret).update(body).digest('hex');
}

async function post(body, signatureHex) {
  const headers = { 'Content-Type': 'application/json' };
  if (signatureHex !== undefined) headers['x-signature'] = signatureHex;
  return fetch(baseUrl + '/webhooks/freemius', { method: 'POST', headers, body });
}

test('a webhook with no x-signature header is rejected', async () => {
  const res = await post(JSON.stringify({ type: 'license.created' }));
  assert.equal(res.status, 401);
});

test('a webhook with a wrong signature is rejected, not processed', async () => {
  const body = JSON.stringify({ type: 'license.created' });
  const res = await post(body, sign(body, 'not-the-real-secret'));
  assert.equal(res.status, 401);
});

test('a correctly-signed event of a type we do not act on is acknowledged and skipped', async () => {
  const body = JSON.stringify({ type: 'subscription.canceled' });
  const res = await post(body, sign(body));
  assert.equal(res.status, 200);
  const json = await res.json();
  assert.equal(json.skipped, true);
});

test('a correctly-signed license.created event with an unrecognized payload shape fails loudly, not silently', async () => {
  // No `objects.user` / `objects.license` — extractBuyer must refuse to
  // guess at field names rather than sign a license for nobody.
  const body = JSON.stringify({ type: 'license.created', objects: {} });
  const res = await post(body, sign(body));
  assert.equal(res.status, 500);
});
