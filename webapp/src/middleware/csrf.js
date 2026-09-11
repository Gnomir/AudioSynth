// Minimal same-session CSRF token (no external dependency). Every GET that
// renders a form calls `res.locals.csrfToken` (set here); every POST is
// checked against `req.session.csrfToken` by `verifyCsrf`.
'use strict';

const crypto = require('node:crypto');

function issueCsrf(req, res, next) {
  if (!req.session.csrfToken) {
    req.session.csrfToken = crypto.randomBytes(24).toString('hex');
  }
  res.locals.csrfToken = req.session.csrfToken;
  next();
}

function verifyCsrf(req, res, next) {
  const sent = req.body && req.body._csrf;
  if (!sent || sent !== req.session.csrfToken) {
    return res.status(403).send('Bad request — form token expired, go back and try again. (403, CSRF)');
  }
  next();
}

module.exports = { issueCsrf, verifyCsrf };
