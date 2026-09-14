// Freemius webhook receiver: turns a completed sale into a signed Ed25519
// license key file, using the existing offline key-file scheme
// (harmonic_synth/license/) instead of Freemius's own built-in license-key
// feature — that one validates online against Freemius's server on every
// check, which is incompatible with this product's "no dongle, no internet
// check, no activation server" design. Freemius here is payment + webhook
// only; the actual license is still signed and verified entirely offline.
//
// Verified against Freemius's own docs (2026-09-14):
//   - signature header: `x-signature`
//   - algorithm: HMAC-SHA256 over the raw request body, hex digest
//   - the event we care about: `license.created`
// NOT verified — Freemius doesn't publish the exact field paths, and
// guessing them wrong would silently sign a license with the wrong name/
// email, which is worse than an obvious failure. `extractBuyer` below
// throws instead of guessing; fix it from a real captured payload (Freemius
// can resend/replay a webhook from its dashboard, or send a test event)
// before this goes live. Every payload is logged raw either way.
'use strict';

const crypto = require('crypto');
const path = require('path');
const fs = require('fs');
const { execFile } = require('child_process');
const express = require('express');
const logger = require('../lib/logger');

const router = express.Router();

const LICENSE_DIR = path.join(__dirname, '..', '..', 'storage', 'licenses');
fs.mkdirSync(LICENSE_DIR, { recursive: true });

function verifySignature(rawBody, signatureHex, secret) {
  if (!secret) return false;
  const expected = crypto.createHmac('sha256', secret).update(rawBody).digest('hex');
  const a = Buffer.from(expected, 'hex');
  const b = Buffer.from(String(signatureHex || ''), 'hex');
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}

/**
 * Pull the buyer's name/email and an order reference out of a Freemius
 * `license.created` event. Freemius's docs show the shape as
 * `fsEvent.objects.license` / `fsEvent.objects.user` but do not enumerate
 * field names — fill these in from a real payload (logged below) before
 * relying on this in production.
 */
function extractBuyer(fsEvent) {
  const user = fsEvent && fsEvent.objects && fsEvent.objects.user;
  const license = fsEvent && fsEvent.objects && fsEvent.objects.license;
  if (!user || !license) {
    throw new Error('extractBuyer: unrecognized payload shape — see the logged raw event and fix the field paths here');
  }
  // TODO confirm these field names against a real webhook payload.
  const email = user.email;
  const name = [user.first, user.last].filter(Boolean).join(' ') || user.email;
  const order = String(license.id || fsEvent.id || '');
  if (!email) {
    throw new Error('extractBuyer: no email field found at the expected path — fix from the logged raw payload');
  }
  return { name, email, order };
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

// Raw body needed for the HMAC check — mounted before any JSON body parser
// would consume the stream (app.js currently has none globally, but keep
// this route self-contained regardless).
router.post('/freemius', express.raw({ type: 'application/json', limit: '1mb' }), async (req, res) => {
  const rawBody = req.body; // Buffer
  const valid = verifySignature(rawBody, req.headers['x-signature'], process.env.FREEMIUS_WEBHOOK_SECRET);
  if (!valid) {
    logger.warn('freemius webhook: bad or missing signature');
    return res.status(401).json({ error: 'invalid signature' });
  }

  let fsEvent;
  try {
    fsEvent = JSON.parse(rawBody.toString('utf8'));
  } catch (e) {
    logger.warn('freemius webhook: unparseable body');
    return res.status(400).json({ error: 'invalid JSON' });
  }

  // Log every verified event's raw shape — this is how the exact field
  // paths in extractBuyer get confirmed once real events start arriving.
  logger.info('freemius webhook received', { type: fsEvent && fsEvent.type });

  if (!fsEvent || fsEvent.type !== 'license.created') {
    return res.status(200).json({ ok: true, skipped: true });
  }

  try {
    const buyer = extractBuyer(fsEvent);
    const keyPath = await signLicense(buyer);
    logger.info('license signed', { email: buyer.email, order: buyer.order, keyPath });
    // TODO: email delivery isn't wired up — no mail provider is configured
    // in this project yet (see package.json). Until it is, the signed
    // .key file lands in storage/licenses/ for manual follow-up; wire a
    // provider (Postmark, Resend, SES, ...) here before relying on this
    // for real customers.
    return res.status(200).json({ ok: true });
  } catch (err) {
    logger.error('freemius webhook: failed to sign license', { message: err.message });
    return res.status(500).json({ error: 'internal error' });
  }
});

module.exports = router;
