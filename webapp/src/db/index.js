// SQLite connection + a tiny migration runner. Uses Node's built-in
// `node:sqlite` (stable enough as of Node 24, no native module to compile,
// no separate DB server — the whole database is one file, per the project's
// "no PostgreSQL, works on your PC" requirement).
'use strict';

const { DatabaseSync } = require('node:sqlite');
const fs = require('node:fs');
const path = require('node:path');

const DB_PATH = process.env.DB_PATH || path.join(__dirname, '..', '..', 'database', 'app.sqlite');
const MIGRATIONS_DIR = path.join(__dirname, 'migrations');

fs.mkdirSync(path.dirname(DB_PATH), { recursive: true });

const db = new DatabaseSync(DB_PATH);
db.exec('PRAGMA journal_mode = WAL;');
db.exec('PRAGMA foreign_keys = ON;');

function runMigrations() {
  db.exec(`CREATE TABLE IF NOT EXISTS migrations (
    name       TEXT PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
  )`);

  const applied = new Set(
    db.prepare('SELECT name FROM migrations').all().map((r) => r.name)
  );

  const files = fs.readdirSync(MIGRATIONS_DIR).filter((f) => f.endsWith('.sql')).sort();
  for (const file of files) {
    if (applied.has(file)) continue;
    const sql = fs.readFileSync(path.join(MIGRATIONS_DIR, file), 'utf8');
    db.exec('BEGIN');
    try {
      db.exec(sql);
      db.prepare('INSERT INTO migrations (name) VALUES (?)').run(file);
      db.exec('COMMIT');
      console.log(`[db] applied migration ${file}`);
    } catch (err) {
      db.exec('ROLLBACK');
      throw new Error(`Migration ${file} failed: ${err.message}`);
    }
  }
}

runMigrations();

module.exports = { db, DB_PATH };
