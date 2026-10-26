/* Client script of the launch hub page. Inlined by launch-hub.mjs. The page is complete without it:
   it adds copy buttons, live counts, the destination and theme pickers, and the checklist's saved ticks. */
(function () {
  "use strict";
  var doc = document;
  doc.documentElement.classList.add("js");
  var $ = function (s, r) { return (r || doc).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || doc).querySelectorAll(s)); };
  var seg = null;
  try { seg = typeof Intl !== "undefined" && Intl.Segmenter ? new Intl.Segmenter() : null; } catch (e) { seg = null; }

  /* ---------- text, UTM, counts ---------- */
  var UTM_KEY = "edytlab-launch-utm";
  function utmOn() { var c = $("#utm-toggle"); return !!(c && c.checked); }
  function addUtm(text, spec) {
    var p = spec.split("|");
    return text.replace(/https:\/\/edytlab\.com(\/[A-Za-z0-9\/_-]*)?(?![A-Za-z0-9\/_?=&-])/g, function (m) {
      return m + "?utm_source=" + p[0] + "&utm_medium=social&utm_campaign=launch&utm_content=" + p[1];
    });
  }
  function textOf(el) {
    var t = el.textContent;
    var spec = el.getAttribute("data-utm");
    if (spec && utmOn()) t = addUtm(t, spec);
    return t;
  }
  function countOf(el) {
    var t = textOf(el);
    if (el.hasAttribute("data-desc")) { var i = t.indexOf("Description: "); if (i >= 0) t = t.slice(i + 13); }
    if (el.getAttribute("data-mode") === "x") t = t.replace(/https?:\/\/\S+/g, "xxxxxxxxxxxxxxxxxxxxxxx");
    return seg ? Array.from(seg.segment(t)).length : Array.from(t).length;
  }
  function paintMeter(el) {
    var post = el.closest(".post"); if (!post) return;
    var m = $(".meter", post); if (!m) return;
    var lim = el.getAttribute("data-limit");
    var n = countOf(el);
    if (lim) { var L = +lim; var over = n > L; m.textContent = n + " / " + L + (over ? " (over)" : ""); m.classList.toggle("over", over); }
    else { m.textContent = n + " characters"; }
  }
  function paintAll() { $$(".txt[data-limit],.txt[data-nolimit]").forEach(paintMeter); }

  /* ---------- copy ---------- */
  function selectText(el) {
    try { var r = doc.createRange(); r.selectNodeContents(el); var s = window.getSelection(); s.removeAllRanges(); s.addRange(r); } catch (e) { /* ignore */ }
  }
  function flash(btn, label, ok) {
    var old = btn.getAttribute("data-label") || btn.textContent;
    btn.setAttribute("data-label", old);
    btn.textContent = label; btn.classList.toggle("done", !!ok);
    clearTimeout(btn._t);
    btn._t = setTimeout(function () { btn.textContent = old; btn.classList.remove("done"); }, 1600);
  }
  doc.addEventListener("click", function (e) {
    var btn = e.target.closest && e.target.closest(".copy"); if (!btn) return;
    var el = doc.getElementById(btn.getAttribute("data-for")); if (!el) return;
    var text = textOf(el);
    var fail = function () { selectText(el); flash(btn, "Press Ctrl/Cmd+C", false); };
    try {
      if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(function () { flash(btn, "Copied", true); }, fail);
      else fail();
    } catch (err) { fail(); }
  });

  var ut = $("#utm-toggle");
  if (ut) {
    try { ut.checked = localStorage.getItem(UTM_KEY) === "1"; } catch (e) { /* ignore */ }
    ut.addEventListener("change", function () { try { localStorage.setItem(UTM_KEY, ut.checked ? "1" : "0"); } catch (e) { /* ignore */ } paintAll(); });
  }
  paintAll();

  /* ---------- destination picker, theme picker ---------- */
  function showDest(id) {
    $$(".dest").forEach(function (d) { d.hidden = d.id !== id; });
    $$(".destnav button").forEach(function (b) { b.setAttribute("aria-pressed", b.getAttribute("data-dest") === id ? "true" : "false"); });
  }
  $$(".destnav button").forEach(function (b) { b.addEventListener("click", function () { showDest(b.getAttribute("data-dest")); }); });
  function showTheme(n) {
    $$(".theme").forEach(function (t) { t.hidden = t.getAttribute("data-theme-n") !== String(n); });
    $$(".themenav button").forEach(function (b) { b.setAttribute("aria-pressed", b.getAttribute("data-n") === String(n) ? "true" : "false"); });
  }
  $$(".themenav button").forEach(function (b) { b.addEventListener("click", function () { showTheme(b.getAttribute("data-n")); }); });
  if ($(".dest")) showDest($(".dest").id);
  if ($(".theme")) showTheme(1);

  $$(".theme").forEach(function (t) {
    var img = $(".prev img", t), sel = $("select", t), open = $(".prev .openlink", t);
    function setSize(v) {
      var src = "img/" + t.getAttribute("data-slug") + "__" + v + ".png";
      img.src = src; if (open) open.href = src;
      var wh = v.split("-")[1].split("x"); img.width = +wh[0]; img.height = +wh[1];
      if (sel) sel.value = v;
    }
    if (sel) sel.addEventListener("change", function () { setSize(sel.value); });
    $$("a.pv", t).forEach(function (a) { a.addEventListener("click", function (e) { e.preventDefault(); setSize(a.getAttribute("data-size")); img.scrollIntoView({ block: "nearest" }); }); });
  });

  function reveal(hash) {
    if (!hash || hash.length < 2) return;
    var el = null; try { el = doc.getElementById(decodeURIComponent(hash.slice(1))); } catch (e) { return; }
    if (!el) return;
    var d = el.closest(".dest"); if (d) showDest(d.id);
    var th = el.closest(".theme"); if (th) showTheme(th.getAttribute("data-theme-n"));
    el.scrollIntoView({ block: "start" });
  }
  doc.addEventListener("click", function (e) {
    var a = e.target.closest && e.target.closest('a[href^="#"]'); if (!a) return;
    var id = a.getAttribute("href").slice(1); if (!id || !doc.getElementById(id)) return;
    e.preventDefault(); history.replaceState(null, "", "#" + id); reveal("#" + id);
  });
  if (location.hash) reveal(location.hash);

  /* ---------- active nav link ---------- */
  var links = $$(".navrow a"), secs = links.map(function (a) { return doc.getElementById(a.getAttribute("href").slice(1)); });
  if ("IntersectionObserver" in window) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) {
        if (en.isIntersecting) links.forEach(function (a, i) { a.setAttribute("aria-current", secs[i] === en.target ? "true" : "false"); });
      });
    }, { rootMargin: "-20% 0px -70% 0px" });
    secs.forEach(function (s) { if (s) io.observe(s); });
  }

  /* ---------- checklist ---------- */
  var LS_KEY = "edytlab-launch-checklist";
  var boxes = $$("input[data-ck]");
  var ids = boxes.map(function (b) { return b.getAttribute("data-ck"); });
  var done = {};
  var db = null, ref = null, chain = Promise.resolve(), remote = false;
  var note = $("#ck-note");

  function loadLocal() { try { var v = JSON.parse(localStorage.getItem(LS_KEY) || "{}"); return v && typeof v === "object" ? v : {}; } catch (e) { return {}; } }
  function saveLocal() { try { localStorage.setItem(LS_KEY, JSON.stringify(done)); } catch (e) { /* ignore */ } }
  function paint() {
    var n = 0;
    boxes.forEach(function (b) {
      var on = !!done[b.getAttribute("data-ck")];
      b.checked = on; if (on) n++;
      b.closest("li").classList.toggle("is-done", on);
    });
    var c = $("#ck-count"); if (c) c.textContent = n + " of " + ids.length + " done";
    var bar = $("#ck-bar"); if (bar) bar.style.width = (ids.length ? Math.round(100 * n / ids.length) : 0) + "%";
    $$(".day").forEach(function (d) {
      var bs = $$("input[data-ck]", d), k = bs.filter(function (b) { return b.checked; }).length;
      var s = $(".day-count", d); if (s) s.textContent = k + "/" + bs.length;
    });
  }
  function say(t) { if (note) note.textContent = t; }

  function write(id, val) {
    chain = chain.then(function () {
      if (!ref) return null;
      return ref.get().then(function (snap) {
        var body = { done: {} }; body.done[id] = val;
        return snap.exists ? ref.update(body) : ref.set(body);
      });
    }).catch(function () {
      remote = false;
      say("Could not save to claude.ai for this viewer. Ticks are kept in this browser.");
    });
    return chain;
  }
  boxes.forEach(function (b) {
    b.addEventListener("change", function () {
      var id = b.getAttribute("data-ck");
      done[id] = b.checked; paint(); saveLocal();
      if (remote) write(id, b.checked);
    });
  });

  done = loadLocal(); paint();
  say("Checking whether ticks can be saved to claude.ai...");
  var viaUse = null;
  try { viaUse = window.claude && typeof window.claude.use === "function" ? window.claude.use("db") : null; } catch (e) { viaUse = null; }
  Promise.resolve(viaUse).then(function (d) {
    db = d || null;
    if (!db) { say("Sign in to claude.ai to save ticks. Until then they are kept in this browser."); return; }
    try {
      ref = db.doc("launch/checklist"); remote = true;
      say("Ticks are saved to claude.ai and shared with everyone who can edit this page.");
      ref.onSnapshot(function (snap) {
        var data = snap.exists ? snap.data() : null;
        var d2 = data && data.done && typeof data.done === "object" ? data.done : {};
        done = {}; Object.keys(d2).forEach(function (k) { if (d2[k]) done[k] = true; });
        paint(); saveLocal();
      }, function () { remote = false; say("Sign in to claude.ai to save ticks. Until then they are kept in this browser."); });
    } catch (e) { remote = false; say("Sign in to claude.ai to save ticks. Until then they are kept in this browser."); }
  }, function () { say("Sign in to claude.ai to save ticks. Until then they are kept in this browser."); });
})();
