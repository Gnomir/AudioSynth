// One-time (idempotent) seed: pulls every editable string out of the
// existing static `site/index.html` (built earlier as the first prototype)
// into `content_blocks` + `faqs`, and creates the first admin account.
//
// Run: node src/db/seed.js
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const cheerio = require('cheerio');
const { db } = require('./index');
const { setValue } = require('../lib/content');
const { hashPassword } = require('../lib/password');

const STATIC_SITE = path.join(__dirname, '..', '..', '..', 'site', 'index.html');

// FAQ question/answer keys live in content_blocks in the static page but
// become full rows in `faqs` here (they need add/remove/reorder, which a
// flat key→value block can't do). Everything else stays a content_block.
const FAQ_KEY_RE = /^faq\.(q|a)(\d+)$/;

function section(key) {
  return key.split('.')[0];
}

// The static source wraps long strings across multiple indented lines for
// readability; `.textContent`/`.html()` return that literal whitespace.
// Harmless in a browser (CSS collapses it on render) but ugly in a <textarea>
// admin editor — collapse it once, here, so the DB holds clean single-line
// values. Safe: nothing in this page relies on preserved whitespace (no
// <pre>/multi-line <code> blocks).
function normalize(str) {
  return str.replace(/\s+/g, ' ').trim();
}

function extractStatic() {
  const html = fs.readFileSync(STATIC_SITE, 'utf8');
  const $ = cheerio.load(html);

  const en = { text: {}, html: {} };
  $('[data-i18n]').each((_, el) => {
    const key = $(el).attr('data-i18n');
    if (!(key in en.text)) en.text[key] = normalize($(el).text());
  });
  $('[data-i18n-html]').each((_, el) => {
    const key = $(el).attr('data-i18n-html');
    if (!(key in en.html)) en.html[key] = normalize($(el).html());
  });

  // The Ukrainian dictionary lives in the page's own <script> as `var UK = {...}`.
  // Trusted, self-authored source (not user input) — safe to evaluate directly.
  const scriptBody = $('script').last().html() || '';
  const m = scriptBody.match(/var UK = (\{[\s\S]*?\n {2}\};)/);
  if (!m) throw new Error('Could not find the UK dictionary in site/index.html');
  // eslint-disable-next-line no-new-func
  const uk = new Function(`"use strict"; return ${m[1].slice(0, -1)};`)();

  return { en, uk };
}

function seedContent() {
  const { en, uk } = extractStatic();
  const allKeys = new Set([...Object.keys(en.text), ...Object.keys(en.html)]);

  let blocks = 0;
  let faqRows = new Map(); // n -> { question_en, answer_en, question_uk, answer_uk }

  for (const key of allKeys) {
    const isHtml = key in en.html;
    const enValue = isHtml ? en.html[key] : en.text[key];
    const ukValue = normalize(uk[key] != null ? uk[key] : enValue);

    const faqMatch = key.match(FAQ_KEY_RE);
    if (faqMatch) {
      const [, part, n] = faqMatch;
      if (!faqRows.has(n)) faqRows.set(n, {});
      const row = faqRows.get(n);
      if (part === 'q') { row.question_en = enValue; row.question_uk = ukValue; }
      else { row.answer_en = enValue; row.answer_uk = ukValue; }
      continue;
    }

    setValue(key, 'en', enValue, isHtml, section(key));
    setValue(key, 'uk', ukValue, isHtml, section(key));
    blocks++;
  }

  const insertFaq = db.prepare(`
    INSERT INTO faqs (sort_order, published, question_en, answer_en, question_uk, answer_uk)
    VALUES (?, 1, ?, ?, ?, ?)
  `);
  const faqCount = db.prepare('SELECT COUNT(*) AS n FROM faqs').get().n;
  let faqsInserted = 0;
  if (faqCount === 0) {
    for (const [n, row] of [...faqRows.entries()].sort((a, b) => +a[0] - +b[0])) {
      insertFaq.run(+n, row.question_en, row.answer_en, row.question_uk, row.answer_uk);
      faqsInserted++;
    }
  }

  console.log(`[seed] content_blocks: ${blocks * 2} rows (${blocks} keys × 2 languages)`);
  console.log(`[seed] faqs: ${faqsInserted} inserted${faqCount > 0 ? ' (skipped — already present)' : ''}`);
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
