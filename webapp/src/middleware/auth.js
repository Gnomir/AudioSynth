'use strict';

const { db } = require('../db');

const findUser = db.prepare('SELECT id, email, role, display_name, license_key, is_active FROM users WHERE id = ?');

/** Attaches req.user from the session, on every request. */
function loadUser(req, res, next) {
  req.user = null;
  const uid = req.session && req.session.userId;
  if (uid) {
    const user = findUser.get(uid);
    if (user && user.is_active) req.user = user;
  }
  res.locals.user = req.user;
  next();
}

function requireCustomer(req, res, next) {
  if (!req.user) {
    req.session.returnTo = req.originalUrl;
    return res.redirect('/login');
  }
  next();
}

function requireAdmin(req, res, next) {
  if (!req.user || req.user.role !== 'admin') {
    return res.redirect('/admin/login');
  }
  next();
}

module.exports = { loadUser, requireCustomer, requireAdmin };
