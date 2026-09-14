// Gumroad Ping receiver: turns a completed sale into a signed Ed25519
// license key file, using the existing offline key-file scheme
// (harmonic_synth/license/) instead of Gumroad's own built-in license-key
// feature — that one validates online against Gumroad's server on every
// check, which is incompatible with this product's "no dongle, no internet
// check, no activation server" design. Gumroad here is payment + webhook
// only; the actual license is still signed and verified entirely offline.
//
// Verified against Gumroad's own API docs (2026-09-14):
//   - Ping (Settings -> Advanced -> "Ping URL") POSTs
//     application/x-www-form-urlencoded on every sale, account-wide. Fields
//     used here: sale_id, product_id, email, full_name, test.
//   - Ping has NO signature, secret, or HMAC of any kind — anyone who
//     learns the URL could POST a fake sale to it. Gumroad's own docs say
//     the fix is to call back and confirm the sale, so that's what
//     verifySale() does: GET /v2/sales/:id with a Bearer access token
//     (requires the `view_sales` OAuth scope) returns the authoritative
//     email/product_id/refunded/chargedback for that sale_id — the Ping
//     body itself is only used to know which sale_id to look up.
//   - GET /v2/sales/:id response shape: { success, sale: { email,
//     product_id, refunded, chargedback, disputed, ... } }.
'use strict';

const path = require('path');
const fs = require('fs');
const { execFile } = require('child_process');
const express = require('express');
const logger = require('../lib/logger');

const router = express.Router();

const LICENSE_DIR = path.join(__dirname, '..', '..', 'storage', 'licenses');
fs.mkdirSync(LICENSE_DIR, { recursive: true });

async function verifySale(saleId, accessToken) {
  // Read at call time, not module load — overridable so tests can point
  // this at a local stub instead of the real Gumroad API.
  const apiBase = process.env.GUMROAD_API_BASE || 'https://api.gumroad.com';
  const url = `${apiBase}/v2/sales/${encodeURIComponent(saleId)}`;
  const res = await fetch(url, { headers: { Authorization: `Bearer ${accessToken}` } });
  if (!res.ok) return null;
  const body = await res.json();
  if (!body || body.success === false || !body.sale) return null;
  return body.sale;
}

function signLicense({ name, email, order }) {
  return new Promise((resolve, reject) => {
    const secret = process.env.LICENSE_SIGNING_SECRET;
    if (!secret) return reject(new Error('LICENSE_SIGNING_SECRET is not set'));

    const bin = process.env.XTASK_BIN; // path to a prebuilt `xtask` binary, if set
    const cwd = process.env.XTASK_CWD || path.join(__dirname, '..', '..', '..', 'harmonic_synth');
    const outFile = path.join(LICENSE_DIR, `${order || Date.now()}.key`);

    const signArgs = [
      'keygen', 'sign',
      '--secret', secret,
      '--name', name,
      '--email', email,
      '--tier', 'studio',
      '--out', outFile,
    ];
    if (order) signArgs.push('--order', order);

    const [cmd, args] = bin ? [bin, signArgs] : ['cargo', ['xtask', ...signArgs]];

    execFile(cmd, args, { cwd }, (err) => {
      if (err) return reject(err);
      resolve(outFile);
    });
  });
}

router.post('/gumroad', express.urlencoded({ extended: false }), async (req, res) => {
  const saleId = req.body && req.body.sale_id;
  const fullName = req.body && req.body.full_name;
  const test = req.body && req.body.test;

  if (!saleId) {
    logger.warn('gumroad webhook: no sale_id in payload');
    return res.status(400).json({ error: 'missing sale_id' });
  }

  const accessToken = process.env.GUMROAD_ACCESS_TOKEN;
  if (!accessToken) {
    logger.error('gumroad webhook: GUMROAD_ACCESS_TOKEN is not set');
    return res.status(500).json({ error: 'server not configured' });
  }

  logger.info('gumroad webhook received', { saleId, test });

  let sale;
  try {
    sale = await verifySale(saleId, accessToken);
  } catch (err) {
    logger.error('gumroad webhook: sale verification request failed', { message: err.message });
    return res.status(502).json({ error: 'could not verify sale' });
  }

  if (!sale) {
    logger.warn('gumroad webhook: sale_id did not verify against the Gumroad API', { saleId });
    return res.status(400).json({ error: 'sale did not verify' });
  }

  const expectedProduct = process.env.GUMROAD_PRODUCT_ID;
  if (expectedProduct && String(sale.product_id) !== String(expectedProduct)) {
    logger.info('gumroad webhook: sale is for a different product, ignoring', { saleId, productId: sale.product_id });
    return res.status(200).json({ ok: true, skipped: true });
  }

  if (sale.refunded || sale.chargedback || sale.disputed) {
    logger.warn('gumroad webhook: sale is refunded/disputed, not issuing a license', { saleId });
    return res.status(200).json({ ok: true, skipped: true });
  }

  if (!sale.email) {
    logger.error('gumroad webhook: verified sale has no email', { saleId });
    return res.status(500).json({ error: 'no email on verified sale' });
  }

  try {
    const keyPath = await signLicense({ name: fullName || sale.email, email: sale.email, order: String(saleId) });
    logger.info('license signed', { email: sale.email, order: saleId, keyPath });
    // TODO: email delivery isn't wired up — no mail provider is configured
    // in this project yet (see package.json). Until it is, the signed
    // .key file lands in storage/licenses/ for manual follow-up; wire a
    // provider (Postmark, Resend, SES, ...) here before relying on this
    // for real customers.
    return res.status(200).json({ ok: true });
  } catch (err) {
    logger.error('gumroad webhook: failed to sign license', { message: err.message });
    return res.status(500).json({ error: 'internal error' });
  }
});

module.exports = router;
