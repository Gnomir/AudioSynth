// AUDIT.md I-3: structured (JSON Lines) logging for anything that happens
// per-request or per-error, so an incident can be grepped/aggregated
// instead of read off raw stdout. No new dependency — a JSON console line
// is enough for what a small single-process portal needs, consistent with
// the project's existing minimal-dependency posture.
//
// Deliberately NOT included: an external error-tracking integration
// (Sentry or similar) — that needs an account/DSN the owner has to
// provision, which is an operational decision, not something to wire in
// silently on their behalf.
//
// Deliberately left alone: the human-facing console.log lines in
// src/server.js's startup banner and src/db/seed.js's interactive output.
// Those are read directly by whoever ran the command, not aggregated from a
// log stream — turning them into JSON would only make them harder to read.
'use strict';

function log(level, message, fields = {}) {
  // Quiet during the test suite — real assertions cover the behavior these
  // lines would report on; the JSON noise just buries `node --test` output.
  if (process.env.NODE_ENV === 'test') return;
  const line = {
    ts: new Date().toISOString(),
    level,
    message,
    ...fields,
  };
  const out = level === 'error' ? console.error : console.log;
  out(JSON.stringify(line));
}

module.exports = {
  info: (message, fields) => log('info', message, fields),
  warn: (message, fields) => log('warn', message, fields),
  error: (message, fields) => log('error', message, fields),
};
