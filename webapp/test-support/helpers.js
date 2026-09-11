// Shared test setup (AUDIT.md M-5). Node's built-in test runner + assert —
// no new dependency, consistent with the rest of this project.
//
// Each test *file* gets its own fresh in-memory database (`node --test`
// runs every file in its own process, and DB_PATH is read once when
// `../src/db` first loads) and its own ephemeral-port server, so tests
// don't share state across files and don't touch the real
// database/app.sqlite.
'use strict';

process.env.NODE_ENV = 'test';
process.env.DB_PATH = ':memory:';
process.env.SESSION_SECRET = 'test-only-secret-do-not-use-elsewhere-0123456789';

const http = require('node:http');
const app = require('../src/app');
const { hashPassword } = require('../src/lib/password');
const { db } = require('../src/db');

let server;
let baseUrl;

async function startServer() {
  server = http.createServer(app);
  await new Promise((resolve, reject) => {
    server.listen(0, '127.0.0.1', (err) => (err ? reject(err) : resolve()));
  });
  baseUrl = `http://127.0.0.1:${server.address().port}`;
  return baseUrl;
}

async function stopServer() {
  if (server) await new Promise((resolve) => server.close(() => resolve()));
}

function createUser({ email, password, role = 'customer', isActive = 1, displayName = null, licenseKey = null }) {
  db.prepare(
    `INSERT INTO users (email, password_hash, role, is_active, display_name, license_key)
     VALUES (?, ?, ?, ?, ?, ?)`
  ).run(email, hashPassword(password), role, isActive, displayName, licenseKey);
}

/** A tiny same-cookie client, the way a browser keeps one session across requests. */
function makeClient() {
  let cookie = '';
  const client = {
    async fetch(pathname, opts = {}) {
      const res = await fetch(baseUrl + pathname, {
        ...opts,
        redirect: 'manual',
        headers: { ...(opts.headers || {}), ...(cookie ? { Cookie: cookie } : {}) },
      });
      const setCookie = res.headers.get('set-cookie');
      if (setCookie) cookie = setCookie.split(';')[0];
      return res;
    },
    async getCsrf(pathname) {
      const res = await client.fetch(pathname);
      const html = await res.text();
      const m = html.match(/name="_csrf" value="([^"]+)"/);
      return m ? m[1] : null;
    },
    /** POST url-encoded fields; pass csrf explicitly (or omit the field entirely to test its absence). */
    async post(pathname, fields) {
      return client.fetch(pathname, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams(fields).toString(),
      });
    },
    currentCookie: () => cookie,
  };
  return client;
}

/** Log in via the real HTTP flow (not a DB shortcut) and return an authenticated client. */
async function loginAs(pathPrefix, email, password) {
  const client = makeClient();
  const csrf = await client.getCsrf(`${pathPrefix}/login`);
  const res = await client.post(`${pathPrefix}/login`, { email, password, _csrf: csrf });
  return { client, res };
}

module.exports = { startServer, stopServer, createUser, makeClient, loginAs, db };
