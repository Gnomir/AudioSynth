// One-time (idempotent) seed: loads content_blocks + faqs from seed-data.json
// (a snapshot of the real, current site copy — export it fresh with
// `node src/db/export-seed-data.js` whenever the live content changes
// meaningfully) and creates the first admin account.
//
// This also runs automatically at server startup if content_blocks is empty
// (see app.js) — a free-tier host with an ephemeral filesystem (e.g. Render's
// free web services) wipes the SQLite file on every restart/spin-down, so
// the site needs to be able to reseed itself without a manual step.
//
// Run standalone: node src/db/seed.js
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { db } = require('./index');
const { setValue } = require('../lib/content');
const { hashPassword } = require('../lib/password');

const SEED_DATA = path.join(__dirname, 'seed-data.json');

function seedContent() {
  const already = db.prepare('SELECT COUNT(*) AS n FROM content_blocks').get().n;
  if (already > 0) {
    console.log('[seed] content_blocks already populated — skipped');
    return;
  }

  const { blocks, faqs } = JSON.parse(fs.readFileSync(SEED_DATA, 'utf8'));
  for (const row of blocks) {
    setValue(row.key, row.lang, row.value, row.is_html, row.section);
  }

  const insertFaq = db.prepare(`
    INSERT INTO faqs (sort_order, published, question_en, answer_en, question_uk, answer_uk)
    VALUES (?, 1, ?, ?, ?, ?)
  `);
  for (const f of faqs) {
    insertFaq.run(f.sort_order, f.question_en, f.answer_en, f.question_uk, f.answer_uk);
  }

  console.log(`[seed] content_blocks: ${blocks.length} rows`);
  console.log(`[seed] faqs: ${faqs.length} inserted`);
}

function seedAdmin() {
  const existing = db.prepare("SELECT COUNT(*) AS n FROM users WHERE role = 'admin'").get().n;
  if (existing > 0) {
    console.log('[seed] admin account already exists — skipped');
    return;
  }
  const email = process.env.ADMIN_EMAIL || 'admin@localhost';
  const password = process.env.ADMIN_PASSWORD || require('node:crypto').randomBytes(9).toString('base64url');
  db.prepare(`
    INSERT INTO users (email, password_hash, role, display_name, is_active)
    VALUES (?, ?, 'admin', 'Admin', 1)
  `).run(email, hashPassword(password));

  console.log('[seed] admin account created:');
  console.log(`         email:    ${email}`);
  if (!process.env.ADMIN_PASSWORD) {
    console.log(`         password: ${password}   (generated — save it, shown once)`);
  } else {
    console.log('         password: (from ADMIN_PASSWORD env)');
  }
}

if (require.main === module) {
  seedContent();
  seedAdmin();
  console.log('[seed] done.');
}

module.exports = { seedContent, seedAdmin };
