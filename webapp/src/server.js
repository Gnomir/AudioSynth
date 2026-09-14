'use strict';

const app = require('./app');
const { seedContent, seedAdmin } = require('./db/seed');

// Self-heals a wiped database on boot — needed on a host with an ephemeral
// filesystem (e.g. Render's free web services, which drop local files on
// every restart/spin-down, not just a redeploy). Both functions already
// no-op if the data is already there, so this is a no-op on a normal
// persistent host too.
seedContent();
seedAdmin();

const PORT = process.env.PORT || 4000;

app.listen(PORT, () => {
  console.log(`Cosine portal listening on http://localhost:${PORT}`);
});
