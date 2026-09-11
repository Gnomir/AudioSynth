// Applies the same musician-first copy rewrite (rewrite-musician-copy.js /
// its FAQ rows) to the static fallback at ../../site/index.html, so it
// doesn't drift out of sync with the live webapp.
//
// Pure string splicing on the raw file text — not cheerio's serializer.
// cheerio is a fine PARSER (used read-only elsewhere in this repo) but its
// re-serialization reformats the whole document (quote style, self-closing
// tags, line wrapping), which turns a ~50-string content edit into a
// thousand-line diff. Locate each element by its stable data-i18n[-html]
// attribute with a regex, then replace only the text between its open and
// close tag — every other byte in the file is untouched.
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { db } = require('../src/db');

const SITE = path.join(__dirname, '..', '..', 'site', 'index.html');
let html = fs.readFileSync(SITE, 'utf8');

const rows = db.prepare('SELECT key, lang, value FROM content_blocks').all();
const byKey = new Map();
for (const r of rows) {
  if (!byKey.has(r.key)) byKey.set(r.key, {});
  byKey.get(r.key)[r.lang] = r.value;
}

function escapeHtmlText(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function escapeJsString(s) {
  return s.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

// ---- content_blocks: replace each data-i18n[-html] element's inner content ----
let replaced = 0;
let missing = [];
const elementRe = /<([a-zA-Z][a-zA-Z0-9]*)\b[^>]*\bdata-i18n(-html)?="([^"]+)"[^>]*>/g;
{
  let out = '';
  let cursor = 0;
  let m;
  while ((m = elementRe.exec(html))) {
    const [full, tag, htmlFlag, key] = m;
    const openTagEnd = m.index + full.length;
    const closeTag = `</${tag}>`;
    const closeIdx = html.indexOf(closeTag, openTagEnd);
    if (closeIdx === -1) { missing.push(key); continue; }

    out += html.slice(cursor, openTagEnd);
    const pair = byKey.get(key);
    if (pair && pair.en != null) {
      out += htmlFlag ? pair.en : escapeHtmlText(pair.en);
      replaced++;
    } else {
      out += html.slice(openTagEnd, closeIdx); // unknown key (e.g. faq.q1..) — leave as-is
    }
    cursor = closeIdx;
    elementRe.lastIndex = closeIdx;
  }
  out += html.slice(cursor);
  html = out;
}

// ---- the UK dictionary object literal inside the inline <script> ----
// NOTE: .replace(re, someString) treats "$" in the REPLACEMENT string as a
// capture-group reference ($1, $2, ... — "$30" would be parsed as group 3
// followed by a literal "0"!). Several UK strings contain a literal "$30" /
// "$89" price. Always pass a replacer *function* here, never a template
// string — a function's return value is inserted verbatim, no $-parsing.
let ukReplaced = 0;
for (const [key, pair] of byKey) {
  if (pair.uk == null) continue;
  const re = new RegExp(`("${key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}":\\s*")((?:[^"\\\\]|\\\\.)*)(")`);
  if (!re.test(html)) continue;
  const escaped = escapeJsString(pair.uk);
  html = html.replace(re, (_m, p1, _p2, p3) => p1 + escaped + p3);
  ukReplaced++;
}

// ---- FAQ: 6 fixed <details> blocks in the static page, keyed faq.q1..q6 / faq.a1..a6 ----
const faqs = db.prepare('SELECT * FROM faqs ORDER BY sort_order').all();
let faqReplaced = 0;
faqs.forEach((f, i) => {
  const n = i + 1;
  for (const [attr, tag, text] of [['data-i18n="faq.q' + n + '"', 'summary', f.question_en], ['data-i18n-html="faq.a' + n + '"', 'p', f.answer_en]]) {
    const openRe = new RegExp(`(<${tag}\\b[^>]*${attr.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}[^>]*>)`);
    const om = html.match(openRe);
    if (!om) return;
    const openEnd = om.index + om[0].length;
    const closeIdx = html.indexOf(`</${tag}>`, openEnd);
    if (closeIdx === -1) return;
    const body = attr.includes('-html') ? text : escapeHtmlText(text);
    html = html.slice(0, openEnd) + body + html.slice(closeIdx);
    faqReplaced++;
  }
  for (const [key, val] of [[`faq.q${n}`, f.question_uk], [`faq.a${n}`, f.answer_uk]]) {
    const re = new RegExp(`("${key}":\\s*")((?:[^"\\\\]|\\\\.)*)(")`);
    if (!re.test(html)) continue;
    const escaped = escapeJsString(val);
    html = html.replace(re, (_m, p1, _p2, p3) => p1 + escaped + p3);
    ukReplaced++;
  }
});

fs.writeFileSync(SITE, html);
console.log(`site/index.html: ${replaced} content_blocks elements, ${faqReplaced} FAQ elements, ${ukReplaced} UK dict entries replaced.`);
if (missing.length) console.warn('No closing tag found for keys:', missing);
