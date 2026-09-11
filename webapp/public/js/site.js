(function () {
  "use strict";

  /* ================= i18n ================= */
  // English is authored inline (readable with zero JS). We harvest it from the
  // DOM, then only *swap* when the viewer picks Українська.
  // The Ukrainian dictionary is data now, not code: the server renders it
  // into the page as `window.__CMS_UK__` (built from `content_blocks` where
  // lang = 'uk'), edited through /admin/content. This variable name is kept
  // so the rest of the i18n logic below is unchanged from the static prototype.
  var UK = window.__CMS_UK__ || {};

  var CANVAS_STR = {
    en: {
      axis: "harmonic weight  ·  rᵏ tilt + one movable hump",
      ceiling: "ceiling — exactly zero past here",
      hump: "hump ≈ partial ",
      cost: function (n, tilt) {
        return "Cost per sample: <b>3&nbsp;cos + 1&nbsp;pow</b> — identical whether this spectrum has <b>"
          + n + "</b> partials or <b>2000</b>. Tilt ≈ <b>" + tilt + "&nbsp;dB / harmonic</b>.";
      }
    },
    uk: {
      axis: "вага гармоніки  ·  нахил rᵏ + один рухомий горб",
      ceiling: "стеля — точний нуль далі",
      hump: "горб ≈ гармоніка ",
      cost: function (n, tilt) {
        return "Вартість на семпл: <b>3&nbsp;cos + 1&nbsp;pow</b> — однакова, чи має цей спектр <b>"
          + n + "</b> гармонік, чи <b>2000</b>. Нахил ≈ <b>" + tilt + "&nbsp;дБ / гармоніку</b>.";
      }
    }
  };

  var EN = { i18n: {}, html: {} };
  document.querySelectorAll("[data-i18n]").forEach(function (el) { EN.i18n[el.dataset.i18n] = el.textContent; });
  document.querySelectorAll("[data-i18n-html]").forEach(function (el) { EN.html[el.dataset.i18nHtml] = el.innerHTML; });

  var lang = "en";
  function stored(k) { try { return localStorage.getItem(k); } catch (e) { return null; } }
  function store(k, v) { try { localStorage.setItem(k, v); } catch (e) {} }

  function applyLang(next) {
    lang = next === "uk" ? "uk" : "en";
    document.documentElement.lang = lang;
    document.querySelectorAll("[data-i18n]").forEach(function (el) {
      var k = el.dataset.i18n;
      var v = lang === "uk" ? UK[k] : EN.i18n[k];
      if (v != null) el.textContent = v;
    });
    document.querySelectorAll("[data-i18n-html]").forEach(function (el) {
      var k = el.dataset.i18nHtml;
      var v = lang === "uk" ? UK[k] : EN.html[k];
      if (v != null) el.innerHTML = v;
    });
    document.querySelectorAll(".seg button").forEach(function (b) {
      b.setAttribute("aria-pressed", String(b.dataset.lang === lang));
    });
    store("cosine_lang", lang);
    if (typeof draw === "function") draw();
  }

  document.querySelectorAll(".seg button").forEach(function (b) {
    b.addEventListener("click", function () { applyLang(b.dataset.lang); });
  });

  /* ================= theme toggle ================= */
  var themeBtn = document.getElementById("theme");
  function currentTheme() {
    var t = document.documentElement.getAttribute("data-theme");
    if (t) return t;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  if (themeBtn) themeBtn.addEventListener("click", function () {
    var next = currentTheme() === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", next);
    store("cosine_theme", next);
    if (typeof draw === "function") draw();
  });
  (function initTheme() {
    var t = stored("cosine_theme");
    if (t === "dark" || t === "light") document.documentElement.setAttribute("data-theme", t);
  })();

  /* ================= live spectrum ================= */
  var cv = document.getElementById("cv"), ctx = cv && cv.getContext("2d");
  var sB = document.getElementById("s-b"), sP = document.getElementById("s-p"), sF = document.getElementById("s-f");
  var oB = document.getElementById("o-b"), oP = document.getElementById("o-p"), oF = document.getElementById("o-f");
  var costEl = document.getElementById("cost");
  var CW = 0, CH = 0, draw = null;

  function css(name) { return getComputedStyle(document.documentElement).getPropertyValue(name).trim(); }

  if (ctx) {
    var fit = function () {
      var w = cv.clientWidth || 440;
      var h = w * 520 / 900;
      var dpr = Math.min(window.devicePixelRatio || 1, 2);
      cv.width = Math.round(w * dpr);
      cv.height = Math.round(h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      CW = w; CH = h;
    };

    var params = function () {
      var b = +sB.value / 1000;
      var r = 0.02 + (0.9995 - 0.02) * b * b;
      var n = +sP.value;
      var f = +sF.value / 1000;
      var kc = 1.5 + 24 * f * f;
      var a = Math.pow(2, -1 / kc);
      var b2 = a * a;
      return { r: r, n: n, f: f, kc: kc, a: a, b2: b2 };
    };

    var weights = function (P) {
      var w = new Float64Array(P.n + 1), max = 0;
      for (var k = 1; k <= P.n; k++) {
        var wk = Math.pow(P.r, k) + P.f * (Math.pow(P.a, k) - Math.pow(P.b2, k));
        if (wk < 0) wk = 0;
        w[k] = wk; if (wk > max) max = wk;
      }
      return { w: w, max: max || 1 };
    };

    var FLOOR = -66;

    draw = function () {
      var P = params(), WT = weights(P);
      var trace = css("--trace"), line = css("--line"), muted = css("--muted"),
          surf2 = css("--surface-2"), hump = css("--hump"), gridc = css("--grid");
      var S = CANVAS_STR[lang] || CANVAS_STR.en;

      ctx.clearRect(0, 0, CW, CH);
      ctx.font = '11px "JetBrains Mono", ui-monospace, monospace';
      ctx.textBaseline = "alphabetic";
      ctx.textAlign = "left";

      var padX = 12, plotW = CW - 2 * padX;
      var top = 30, bot = CH - 36, spH = bot - top;
      var slots = Math.max(P.n + 4, 20);
      var bw = plotW / slots;

      // dB graticule
      ctx.strokeStyle = gridc; ctx.lineWidth = 1;
      for (var d = 0; d >= FLOOR; d -= 22) {
        var gy = top + (-d / -FLOOR) * spH;
        ctx.beginPath(); ctx.moveTo(padX, gy); ctx.lineTo(CW - padX, gy); ctx.stroke();
      }
      ctx.strokeStyle = line;
      ctx.beginPath(); ctx.moveTo(padX, bot); ctx.lineTo(CW - padX, bot); ctx.stroke();

      var humpLo = P.kc - 3, humpHi = P.kc + 4;
      for (var k = 1; k <= slots; k++) {
        var x = padX + (k - 1) * bw, w = Math.max(bw * 0.6, 1.6);
        if (k <= P.n) {
          var db = 20 * Math.log10((WT.w[k] / WT.max) + 1e-9);
          if (db < FLOOR) db = FLOOR;
          var hh = ((db - FLOOR) / -FLOOR) * spH;
          var inHump = P.f >= 0.06 && k >= humpLo && k <= humpHi;
          ctx.fillStyle = inHump ? hump : trace;
          ctx.fillRect(x + (bw - w) / 2, bot - hh, w, Math.max(hh, 1.2));
        } else {
          ctx.fillStyle = surf2;
          ctx.fillRect(x + (bw - w) / 2, bot - 2, w, 2);
        }
      }

      var cx = padX + P.n * bw;
      ctx.strokeStyle = muted; ctx.setLineDash([3, 3]); ctx.lineWidth = 1;
      ctx.beginPath(); ctx.moveTo(cx, top - 4); ctx.lineTo(cx, bot); ctx.stroke();
      ctx.setLineDash([]);

      ctx.fillStyle = muted;
      ctx.fillText(S.axis, padX, top - 11);
      ctx.textAlign = "right";
      ctx.fillText(S.ceiling, CW - padX, bot + 16);
      ctx.textAlign = "left";
      if (P.f >= 0.02) { ctx.fillStyle = hump; ctx.fillText(S.hump + P.kc.toFixed(0), padX, bot + 16); }

      oB.textContent = P.r.toFixed(3);
      oP.textContent = P.n;
      oF.textContent = P.f < 0.02 ? "off" : ("p≈" + P.kc.toFixed(0));

      var tilt = (20 * Math.log10(P.r)).toFixed(2);
      costEl.innerHTML = S.cost(P.n, tilt);
    };

    var rt;
    window.addEventListener("resize", function () {
      clearTimeout(rt); rt = setTimeout(function () { fit(); draw(); }, 120);
    });
    [sB, sP, sF].forEach(function (s) { s.addEventListener("input", draw); });

    var mq = window.matchMedia("(prefers-color-scheme: dark)");
    (mq.addEventListener ? mq.addEventListener.bind(mq, "change") : mq.addListener.bind(mq))(function () { draw(); });
    new MutationObserver(function () { draw(); })
      .observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

    fit();
  }

  /* ================= init language ================= */
  (function initLang() {
    var s = stored("cosine_lang");
    var nav = (navigator.language || navigator.userLanguage || "").toLowerCase();
    var initial = (s === "uk" || s === "en") ? s : (nav.indexOf("uk") === 0 ? "uk" : "en");
    if (initial === "uk") applyLang("uk");
    else if (draw) draw();
  })();

  /* ================= waitlist stub ================= */
  var nf = document.getElementById("notify"), nh = document.getElementById("notify-hint");
  if (nf) nf.addEventListener("submit", function (e) {
    e.preventDefault();
    var input = document.getElementById("notify-email");
    try { localStorage.setItem("cosine_waitlist_email", (input && input.value) || ""); } catch (_) {}
    var msg = lang === "uk"
      ? "Дякуємо — збережено в цьому браузері. Ми напишемо на релізі."
      : "Thanks — saved in this browser. We’ll be in touch when it ships.";
    nf.innerHTML = '<p style="font-family:\'JetBrains Mono\',monospace;font-size:.78rem;color:var(--teal-ink);margin:0">' + msg + '</p>';
    if (nh) nh.textContent = lang === "uk"
      ? "Власнику: адресу збережено лише локально. Підключіть справжній список перед запуском."
      : "Owner: stored locally only. Connect a real list before launch.";
  });
})();
