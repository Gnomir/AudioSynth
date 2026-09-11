'use strict';

require('dotenv').config();

const path = require('node:path');
const express = require('express');
const session = require('express-session');

const { db } = require('./db');
const { SqliteSessionStore } = require('./lib/sessionStore');
const { loadUser } = require('./middleware/auth');
const { flash } = require('./middleware/flash');
const { issueCsrf } = require('./middleware/csrf');

const publicRoutes = require('./routes/public');
const adminRoutes = require('./routes/admin');

const app = express();
const PORT = process.env.PORT || 4000;
const isProd = process.env.NODE_ENV === 'production';

app.set('view engine', 'ejs');
app.set('views', path.join(__dirname, '..', 'views'));
app.set('trust proxy', 1);

app.use(express.urlencoded({ extended: false }));
app.use(express.static(path.join(__dirname, '..', 'public')));
app.use('/uploads', express.static(path.join(__dirname, '..', 'storage', 'uploads')));

app.use(session({
  store: new SqliteSessionStore(db),
  secret: process.env.SESSION_SECRET || 'dev-secret-change-me-in-.env',
  resave: false,
  saveUninitialized: false,
  cookie: {
    httpOnly: true,
    sameSite: 'lax',
    secure: isProd,
    maxAge: 30 * 24 * 60 * 60 * 1000, // 30 days
  },
}));

app.use(loadUser);
app.use(flash);
app.use(issueCsrf);
app.use((req, res, next) => { res.locals.path = req.path; next(); });

app.use('/', publicRoutes);
app.use('/admin', adminRoutes);

app.use((req, res) => {
  res.status(404).render('site/404', { title: '404' });
});

// eslint-disable-next-line no-unused-vars
app.use((err, req, res, next) => {
  console.error(err);
  res.status(500).send('Internal error. Check the server console.');
});

app.listen(PORT, () => {
  console.log(`Cosine portal listening on http://localhost:${PORT}`);
});
