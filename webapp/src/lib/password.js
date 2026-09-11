// Password hashing via Node's built-in `crypto.scrypt` — no bcrypt/argon2
// dependency (both need native compilation; scrypt is in core Node).
'use strict';

const crypto = require('node:crypto');

const KEY_LEN = 64;
const SALT_LEN = 16;

function hashPassword(plain) {
  const salt = crypto.randomBytes(SALT_LEN);
  const hash = crypto.scryptSync(plain, salt, KEY_LEN);
  return `scrypt:${salt.toString('hex')}:${hash.toString('hex')}`;
}

function verifyPassword(plain, stored) {
  const [scheme, saltHex, hashHex] = String(stored).split(':');
  if (scheme !== 'scrypt' || !saltHex || !hashHex) return false;
  const salt = Buffer.from(saltHex, 'hex');
  const expected = Buffer.from(hashHex, 'hex');
  const actual = crypto.scryptSync(plain, salt, expected.length);
  return actual.length === expected.length && crypto.timingSafeEqual(actual, expected);
}

// AUDIT.md M-1: a valid-format hash to verify against when no such user
// exists, so a login attempt for an unknown email pays the same scrypt cost
// as one for a real email with a wrong password — otherwise the two cases
// are distinguishable by response time (short-circuiting `!user || ...`
// skips scryptSync entirely for an unknown email), which lets an attacker
// enumerate registered addresses. Computed once at startup; the "password"
// and salt are arbitrary, never used for a real comparison.
const DUMMY_HASH = hashPassword('not-a-real-account-timing-placeholder');

module.exports = { hashPassword, verifyPassword, DUMMY_HASH };
