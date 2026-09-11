// One-shot flash messages stored in the session (no extra dependency).
'use strict';

function flash(req, res, next) {
  const queued = req.session.flash || [];
  req.session.flash = [];
  res.locals.flash = queued;
  req.flash = (type, text) => {
    req.session.flash = req.session.flash || [];
    req.session.flash.push({ type, text });
  };
  next();
}

module.exports = { flash };
