'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const http = require('node:http');
const { startServer, stopServer } = require('../test-support/helpers');

const PRODUCT_ID = 'prod_cosine_studio';
const ACCESS_TOKEN = 'test-access-token';

// A tiny stand-in for api.gumroad.com's GET /v2/sales/:id. Node's test
// runner runs this file's top-level tests concurrently, so the response
// is keyed off the sale_id in the request path (SALES below) rather than
// a shared mutable variable, which would race. Gumroad's Ping webhook has
// no signature, so the real code always calls this back before trusting
// anything; these tests exercise that gating logic without ever reaching
// signLicense() (which shells out to `cargo xtask` — deliberately not
// exercised here, same as the previous Freemius tests never invoked it).
function saleFixture(overrides = {}) {
  return {
    email: 'buyer@example.com',
    product_id: PRODUCT_ID,
    refunded: false,
    chargedback: false,
    disputed: false,
    ...overrides,
  };
}

const SALES = {
  sale_forged: { status: 200, body: { success: false } },
  sale_ok: { status: 200, body: { success: true, sale: saleFixture() } },
  sale_diffprod: { status: 200, body: { success: true, sale: saleFixture({ product_id: 'some_other_product' }) } },
  sale_refunded: { status: 200, body: { success: true, sale: saleFixture({ refunded: true }) } },
  sale_disputed: { status: 200, body: { success: true, sale: saleFixture({ disputed: true }) } },
  sale_noemail: { status: 200, body: { success: true, sale: saleFixture({ email: undefined }) } },
};

let gumroadApi;
let baseUrl;

test.before(async () => {
  gumroadApi = http.createServer((req, res) => {
    const saleId = decodeURIComponent(req.url.split('/').pop());
    const resp = SALES[saleId] || { status: 404, body: { success: false } };
    res.writeHead(resp.status, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(resp.body));
  });
  await new Promise((resolve) => gumroadApi.listen(0, '127.0.0.1', resolve));
  process.env.GUMROAD_API_BASE = `http://127.0.0.1:${gumroadApi.address().port}`;
  process.env.GUMROAD_ACCESS_TOKEN = ACCESS_TOKEN;
  process.env.GUMROAD_PRODUCT_ID = PRODUCT_ID;
  baseUrl = await startServer();
});
test.after(async () => {
  await stopServer();
  await new Promise((resolve) => gumroadApi.close(resolve));
});

async function post(fields) {
  return fetch(baseUrl + '/webhooks/gumroad', {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams(fields).toString(),
  });
}

test('a ping with no sale_id is rejected', async () => {
  const res = await post({ email: 'buyer@example.com' });
  assert.equal(res.status, 400);
});

test('a ping is rejected if the server has no access token configured', async () => {
  delete process.env.GUMROAD_ACCESS_TOKEN;
  try {
    const res = await post({ sale_id: 'sale_1' });
    assert.equal(res.status, 500);
  } finally {
    process.env.GUMROAD_ACCESS_TOKEN = ACCESS_TOKEN;
  }
});

test('a sale_id that does not verify against the Gumroad API is rejected, not trusted', async () => {
  const res = await post({ sale_id: 'sale_forged' });
  assert.equal(res.status, 400);
});

test('a verified sale for a different product is acknowledged and skipped', async () => {
  const res = await post({ sale_id: 'sale_diffprod' });
  assert.equal(res.status, 200);
  const json = await res.json();
  assert.equal(json.skipped, true);
});

test('a refunded sale is acknowledged and skipped, not issued a license', async () => {
  const res = await post({ sale_id: 'sale_refunded' });
  assert.equal(res.status, 200);
  const json = await res.json();
  assert.equal(json.skipped, true);
});

test('a disputed sale is acknowledged and skipped, not issued a license', async () => {
  const res = await post({ sale_id: 'sale_disputed' });
  assert.equal(res.status, 200);
  const json = await res.json();
  assert.equal(json.skipped, true);
});

test('a verified sale with no email fails loudly, not silently', async () => {
  const res = await post({ sale_id: 'sale_noemail' });
  assert.equal(res.status, 500);
});
