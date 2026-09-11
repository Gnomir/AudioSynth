// CSP-safe replacement for inline onsubmit="return confirm(...)" handlers
// (a strict Content-Security-Policy script-src blocks inline event-handler
// attributes just like it blocks inline <script> blocks — see AUDIT.md L-2
// and server.js's CSP config). Any <form data-confirm="..."> gets a native
// confirm() before it's allowed to submit.
(function () {
  'use strict';
  document.addEventListener('submit', function (e) {
    var msg = e.target && e.target.getAttribute && e.target.getAttribute('data-confirm');
    if (msg && !window.confirm(msg)) e.preventDefault();
  });
})();
