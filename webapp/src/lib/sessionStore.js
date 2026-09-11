// A minimal express-session Store backed by the same SQLite file as
// everything else — avoids pulling in connect-sqlite3 (wraps the native
// `sqlite3` package) just to persist sessions across restarts.
'use strict';

const session = require('express-session');

class SqliteSessionStore extends session.Store {
  constructor(db) {
    super();
    this.db = db;
    this._get = db.prepare('SELECT data, expires_at FROM sessions WHERE sid = ?');
    this._upsert = db.prepare(
      `INSERT INTO sessions (sid, data, expires_at) VALUES (?, ?, ?)
       ON CONFLICT(sid) DO UPDATE SET data = excluded.data, expires_at = excluded.expires_at`
    );
    this._del = db.prepare('DELETE FROM sessions WHERE sid = ?');
    this._clear = db.prepare('DELETE FROM sessions');
    this._count = db.prepare('SELECT COUNT(*) AS n FROM sessions');
    this._sweep = db.prepare('DELETE FROM sessions WHERE expires_at < ?');

    this._sweepTimer = setInterval(() => {
      try { this._sweep.run(Date.now()); } catch (_) { /* best effort */ }
    }, 15 * 60 * 1000);
    this._sweepTimer.unref();
  }

  get(sid, cb) {
    try {
      const row = this._get.get(sid);
      if (!row) return cb(null, null);
      if (row.expires_at < Date.now()) {
        this._del.run(sid);
        return cb(null, null);
      }
      cb(null, JSON.parse(row.data));
    } catch (err) {
      cb(err);
    }
  }

  set(sid, sessionData, cb) {
    try {
      const ttlMs = sessionData.cookie && sessionData.cookie.maxAge
        ? sessionData.cookie.maxAge
        : 24 * 60 * 60 * 1000;
      this._upsert.run(sid, JSON.stringify(sessionData), Date.now() + ttlMs);
      cb && cb(null);
    } catch (err) {
      cb && cb(err);
    }
  }

  destroy(sid, cb) {
    try {
      this._del.run(sid);
      cb && cb(null);
    } catch (err) {
      cb && cb(err);
    }
  }

  length(cb) {
    try { cb(null, this._count.get().n); } catch (err) { cb(err); }
  }

  clear(cb) {
    try { this._clear.run(); cb && cb(null); } catch (err) { cb && cb(err); }
  }
}

module.exports = { SqliteSessionStore };
