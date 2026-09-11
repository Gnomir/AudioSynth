// Read/write helpers for `content_blocks` — the editable site copy.
'use strict';

const { db } = require('../db');

const selectAll = db.prepare('SELECT key, lang, value, is_html, section FROM content_blocks');
const selectOneBoth = db.prepare('SELECT key, lang, value, is_html, section FROM content_blocks WHERE key = ?');
const upsert = db.prepare(`
  INSERT INTO content_blocks (key, lang, value, is_html, section, updated_at)
  VALUES (?, ?, ?, ?, ?, datetime('now'))
  ON CONFLICT(key, lang) DO UPDATE SET
    value = excluded.value, is_html = excluded.is_html,
    section = COALESCE(excluded.section, content_blocks.section),
    updated_at = datetime('now')
`);

/** Load every content block into { en: {...}, uk: {...} }, keyed by `key`. */
function loadAll() {
  const rows = selectAll.all();
  const out = { en: {}, uk: {}, meta: {} };
  for (const row of rows) {
    out[row.lang][row.key] = row.value;
    out.meta[row.key] = { is_html: !!row.is_html, section: row.section };
  }
  return out;
}

/** Grouped by `section`, both languages side by side — for the admin editor. */
function loadForAdmin() {
  const rows = selectAll.all();
  const byKey = new Map();
  for (const row of rows) {
    if (!byKey.has(row.key)) {
      byKey.set(row.key, { key: row.key, section: row.section || 'other', is_html: !!row.is_html, en: '', uk: '' });
    }
    byKey.get(row.key)[row.lang] = row.value;
  }
  const groups = new Map();
  for (const item of byKey.values()) {
    if (!groups.has(item.section)) groups.set(item.section, []);
    groups.get(item.section).push(item);
  }
  for (const list of groups.values()) list.sort((a, b) => a.key.localeCompare(b.key));
  return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b));
}

function getBoth(key) {
  const rows = selectOneBoth.all(key);
  const out = { key, en: '', uk: '', is_html: false, section: null };
  for (const row of rows) {
    out[row.lang] = row.value;
    out.is_html = !!row.is_html;
    out.section = row.section;
  }
  return out;
}

function setValue(key, lang, value, isHtml, section) {
  upsert.run(key, lang, value, isHtml ? 1 : 0, section || null);
}

module.exports = { loadAll, loadForAdmin, getBoth, setValue };
