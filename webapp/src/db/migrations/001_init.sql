-- Core schema. Runs once, tracked in the `migrations` table (see db/index.js).
-- SQLite, via Node's built-in `node:sqlite` — no native module, no server.

CREATE TABLE users (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  email         TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  role          TEXT NOT NULL DEFAULT 'customer' CHECK (role IN ('admin', 'customer')),
  display_name  TEXT,
  license_key   TEXT,                 -- free text for now; not cryptographically checked
  is_active     INTEGER NOT NULL DEFAULT 1,
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- express-session store. `sid` is the session id, `data` the serialized
-- session JSON, `expires_at` a unix-ms timestamp for sweeping.
CREATE TABLE sessions (
  sid        TEXT PRIMARY KEY,
  data       TEXT NOT NULL,
  expires_at INTEGER NOT NULL
);

-- Every piece of editable site copy. `key` mirrors the `data-i18n` /
-- `data-i18n-html` attribute it fills on the page (e.g. "hero.h1",
-- "price.studio.amt"), so the template and the CMS stay in lockstep.
-- `is_html` marks a value that may contain inline markup (<em>, <code>, …)
-- and must be inserted with the template's raw-output helper, not escaped.
CREATE TABLE content_blocks (
  key        TEXT NOT NULL,
  lang       TEXT NOT NULL CHECK (lang IN ('en', 'uk')),
  value      TEXT NOT NULL,
  is_html    INTEGER NOT NULL DEFAULT 0,
  section    TEXT,                    -- grouping label for the admin UI only
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (key, lang)
);

-- The public FAQ (the "question-answer tool"), bilingual per row so a whole
-- entry is one admin-editable unit instead of four loose content_blocks.
CREATE TABLE faqs (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  sort_order   INTEGER NOT NULL DEFAULT 0,
  published    INTEGER NOT NULL DEFAULT 1,
  question_en  TEXT NOT NULL,
  answer_en    TEXT NOT NULL,
  question_uk  TEXT NOT NULL,
  answer_uk    TEXT NOT NULL,
  updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
