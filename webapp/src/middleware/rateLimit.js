// AUDIT.md M-2: throttle the two login endpoints. Keyed by IP + the
// attempted email (not IP alone) so one slow, patient attacker trying many
// emails from one address doesn't get lumped into a single counter with
// every legitimate user behind the same NAT/office IP.
'use strict';

const { rateLimit, ipKeyGenerator } = require('express-rate-limit');

function keyByIpAndEmail(req) {
  const email = String((req.body && req.body.email) || '').trim().toLowerCase();
  // ipKeyGenerator (not req.ip directly) normalises IPv6 addresses to a
  // /56 subnet before use as a rate-limit key — a bare req.ip would let an
  // IPv6 client cycle through addresses in its own subnet to dodge the
  // limit, which is exactly what express-rate-limit's own startup
  // validation flags this as unsafe without.
  return `${ipKeyGenerator(req.ip)}:${email}`;
}

const loginLimiter = rateLimit({
  windowMs: 15 * 60 * 1000, // 15 minutes
  limit: 10,                // 10 attempts per IP+email per window
  standardHeaders: 'draft-7',
  legacyHeaders: false,
  keyGenerator: keyByIpAndEmail,
  message: 'Too many login attempts. Wait a few minutes and try again.',
});

module.exports = { loginLimiter };
