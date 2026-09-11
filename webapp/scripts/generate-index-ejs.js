// One-time build helper: turns the static body fragment (scripts/_body_source.html,
// extracted once from the original ../../site/index.html prototype) into an
// EJS template where every data-i18n / data-i18n-html element's content is
// replaced by a reference to the DB-backed `cms` object, instead of literal
// English text. Output: views/site/_body_generated.html — paste the relevant
// part into views/site/index.ejs by hand (the FAQ block there is a
// hand-written loop, not generated, so this never fully overwrites it).
//
// Pure string splicing, not a DOM library — an HTML serializer (cheerio,
// jsdom, ...) would see `<%= %>` as malformed markup and HTML-escape the
// angle brackets, corrupting the EJS tag. We already know the exact,
// well-formed structure of this file (every data-i18n[-html] element is a
// leaf — none nest inside another one), so a single non-greedy "next closing
// tag of the same name" scan is safe and exact.
//
// Not needed at runtime — re-run only if the static prototype changes
// upstream and needs re-diffing into the CMS template.
'use strict';

const fs = require('node:fs');
const path = require('node:path');

const SRC = path.join(__dirname, '_body_source.html');
const OUT = path.join(__dirname, '..', 'views', 'site', '_body_generated.html');

const html = fs.readFileSync(SRC, 'utf8');
const FAQ_KEY_RE = /^faq\.(q|a)(\d+)$/;

const attrRe = /<([a-zA-Z][a-zA-Z0-9]*)\b[^>]*\bdata-i18n(-html)?="([^"]+)"[^>]*>/g;

let out = '';
let cursor = 0;
let m;
let count = 0;

while ((m = attrRe.exec(html))) {
  const [full, tag, htmlFlag, key] = m;
  const openTagEnd = m.index + full.length;
  const closeTag = `</${tag}>`;
  const closeIdx = html.indexOf(closeTag, openTagEnd);
  if (closeIdx === -1) {
    throw new Error(`No closing </${tag}> found for data-i18n${htmlFlag || ''}="${key}"`);
  }

  out += html.slice(cursor, openTagEnd);
  if (FAQ_KEY_RE.test(key)) {
    out += html.slice(openTagEnd, closeIdx); // FAQ text is wired by hand (dynamic list)
  } else {
    out += htmlFlag ? `<%- cms.en['${key}'] %>` : `<%= cms.en['${key}'] %>`;
    count++;
  }
  cursor = closeIdx;
  attrRe.lastIndex = closeIdx;
}
out += html.slice(cursor);

fs.writeFileSync(OUT, out);
console.log(`Wrote ${OUT} — replaced ${count} content blocks (FAQ q/a left for the hand-written loop).`);
