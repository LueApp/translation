// AI Translate × web2local — install & run from the page, no terminal.
// Depends on window.Web2Local (web2local-client.js). All execution goes through
// the local daemon, which graylists this origin and shows a native approval
// dialog for every command. Degrades silently when web2local isn't installed.
(function () {
  "use strict";
  if (!window.Web2Local) return;

  var $ = function (id) { return document.getElementById(id); };
  var lang = function () { return document.documentElement.dataset.lang === "zh" ? "zh" : "en"; };
  var T = {
    checking:    { en: "checking…",                 zh: "检测中…" },
    detected:    { en: "web2local detected",            zh: "已检测到 web2local" },
    offline:     { en: "web2local not running",         zh: "web2local 未运行" },
    needHttps:   { en: "Installing via web2local needs https (or localhost).", zh: "通过 web2local 安装需要 https（或 localhost）。" },
    deploying:   { en: "Approve the install in the web2local dialog…", zh: "请在 web2local 弹窗中确认安装…" },
    installing:  { en: "Installing… (streaming log below)",  zh: "安装中…（下方实时日志）" },
    installDone: { en: "Done — AI Translate is installed.",  zh: "完成 — AI Translate 已安装。" },
    running:     { en: "Running…",                   zh: "运行中…" },
    started:     { en: "Started (PID ",                  zh: "已启动（PID " },
    needRun:     { en: "Start web2local first (see status above).", zh: "请先启动 web2local（见上方状态）。" },
    enterText:   { en: "Type something to translate.",   zh: "请输入要翻译的文字。" },
  };
  function t(k) { var e = T[k] || {}; return e[lang()] || e.en || ""; }

  var port = 7878;
  var w2l = new Web2Local(port);
  var detected = false;

  var badge = $("w2l-badge"), badgeText = $("w2l-badge-text");
  var portInput = $("w2l-port"), recheck = $("w2l-recheck");
  var installBtn = $("w2l-install"), installStop = $("w2l-install-stop");
  var installMsg = $("w2l-install-msg"), installLog = $("w2l-install-log");
  var out = $("w2l-out"), textInput = $("w2l-text"), textGo = $("w2l-text-go");
  var spawnBtns = Array.prototype.slice.call(document.querySelectorAll("[data-w2l-spawn]"));

  function setBadge(state) {
    if (!badge) return;
    badge.dataset.state = state;
    badgeText.textContent = t(state);
  }
  function enableActions(on) {
    detected = on;
    spawnBtns.forEach(function (b) { b.disabled = !on; });
    if (textGo) textGo.disabled = !on;
    if (installBtn) installBtn.disabled = !on;
  }
  function showOut(el, msg, cls) {
    if (!el) return;
    el.hidden = false;
    el.textContent = msg;
    el.className = "w2l-out" + (cls ? " " + cls : "");
    el.scrollTop = el.scrollHeight;
  }

  async function check() {
    port = parseInt(portInput && portInput.value, 10) || 7878;
    w2l = new Web2Local(port);
    setBadge("checking");
    var up = false;
    try { up = await w2l.isRunning(); } catch (e) { up = false; }
    if (up) {
      setBadge("detected");
      try { await w2l.addToGraylist(location.origin); } catch (e) {}
      enableActions(true);
    } else {
      setBadge("offline");
      enableActions(false);
    }
  }

  async function sha256hex(str) {
    var buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(str));
    return Array.prototype.map.call(new Uint8Array(buf), function (b) {
      return b.toString(16).padStart(2, "0");
    }).join("");
  }

  function pollLog(pid) {
    showOut(installLog, "", "");
    if (installStop) { installStop.hidden = false; installStop.onclick = async function () {
      try { await w2l.stop(pid); } catch (e) {}
      clearInterval(iv); installStop.hidden = true;
    }; }
    var iv = setInterval(async function () {
      try { var r = await w2l.tailLog(pid); if (r && r.tail) showOut(installLog, r.tail, ""); } catch (e) {}
      try {
        var ps = await w2l.ps();
        var alive = ps && ps.processes && ps.processes.some(function (p) { return p.pid === pid; });
        if (!alive) {
          clearInterval(iv);
          if (installStop) installStop.hidden = true;
          if (installMsg) installMsg.textContent = t("installDone");
        }
      } catch (e) {}
    }, 1500);
  }

  async function install() {
    if (!detected) { if (installMsg) installMsg.textContent = t("needRun"); return; }
    if (!(window.crypto && crypto.subtle)) { if (installMsg) installMsg.textContent = t("needHttps"); return; }
    if (installMsg) installMsg.textContent = t("deploying");
    try {
      var res = await fetch("/downloads/install-remote.sh", { cache: "no-store" });
      var src = await res.text();
      var sha = await sha256hex(src);
      var r = await w2l.deploy({ source: src, sha256: sha, filename: "ai-translate-install.sh", command: "bash", args: [] });
      if (installMsg) installMsg.textContent = t("installing");
      pollLog(r.pid);
    } catch (e) {
      if (installMsg) installMsg.textContent = "Error: " + e.message;
    }
  }

  async function spawnAction(action) {
    if (!detected) { showOut(out, t("needRun"), "out-err"); return; }
    showOut(out, t("running"), "out-dim");
    try {
      var r = await w2l.spawn("ai-translate", [action]);
      showOut(out, t("started") + r.pid + ")", "out-ok");
    } catch (e) {
      showOut(out, "Error: " + e.message, "out-err");
    }
  }

  async function translateText() {
    if (!detected) { showOut(out, t("needRun"), "out-err"); return; }
    var phrase = (textInput && textInput.value || "").trim();
    if (!phrase) { showOut(out, t("enterText"), "out-dim"); return; }
    showOut(out, t("running"), "out-dim");
    try {
      var r = await w2l.run("ai-translate", ["text", phrase]);
      var txt = (r.stdout || "").trim() || (r.stderr || "").trim() || ("exit code: " + r.exit_code);
      showOut(out, txt, r.exit_code === 0 ? "out-ok" : "out-err");
    } catch (e) {
      showOut(out, "Error: " + e.message, "out-err");
    }
  }

  function setPlaceholder() {
    if (textInput) textInput.placeholder = lang() === "zh" ? "输入要翻译的文字…" : "Type text to translate…";
  }

  // wire up
  setPlaceholder();
  var langToggle = $("lang-toggle");
  if (langToggle) langToggle.addEventListener("click", function () { setTimeout(setPlaceholder, 0); });
  if (recheck) recheck.addEventListener("click", check);
  if (portInput) portInput.addEventListener("keydown", function (e) { if (e.key === "Enter") check(); });
  if (installBtn) installBtn.addEventListener("click", install);
  spawnBtns.forEach(function (b) { b.addEventListener("click", function () { spawnAction(b.getAttribute("data-w2l-spawn")); }); });
  if (textGo) textGo.addEventListener("click", translateText);
  if (textInput) textInput.addEventListener("keydown", function (e) { if (e.key === "Enter") translateText(); });

  enableActions(false);
  check();
})();
