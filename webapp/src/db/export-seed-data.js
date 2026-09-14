// Snapshot the live database's content_blocks + published faqs into
// seed-data.json, so a fresh/wiped database (a new clone, or a host with an
// ephemeral filesystem restarting) can reseed itself with the real, current
// site copy instead of stale placeholder text.
//
// Run whenever the live content changes meaningfully (after an admin-panel
// edit session, or a content-focused commit): node src/db/export-seed-data.js
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { db } = require('./index');

const OUT = path.join(__dirname, 'seed-data.json');

const blocks = db.prepare(
  'SELECT key, lang, value, is_html, section FROM content_blocks ORDER BY key, lang'
).all();
const faqs = db.prepare(
  'SELECT sort_order, question_en, answer_en, question_uk, answer_uk FROM faqs WHERE published = 1 ORDER BY sort_order'
).all();

fs.writeFileSync(OUT, JSON.stringify({ blocks, faqs }, null, 2) + '\n');
console.log(`[export-seed-data] wrote ${blocks.length} content_blocks rows, ${faqs.length} faqs -> ${path.relative(process.cwd(), OUT)}`);
