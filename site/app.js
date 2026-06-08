// AI Translate landing page — language toggle + copy buttons. No dependencies.
(function () {
  "use strict";
  var root = document.documentElement;

  /* ---------------------------- language ---------------------------- */
  var STORE = "ai-translate-lang";
  function currentLang() {
    try {
      var saved = localStorage.getItem(STORE);
      if (saved === "en" || saved === "zh") return saved;
    } catch (e) {}
    return (navigator.language || "en").toLowerCase().indexOf("zh") === 0 ? "zh" : "en";
  }
  function applyLang(lang) {
    root.dataset.lang = lang;
    root.lang = lang === "zh" ? "zh-CN" : "en";
    try { localStorage.setItem(STORE, lang); } catch (e) {}
  }
  applyLang(currentLang());

  var toggle = document.getElementById("lang-toggle");
  if (toggle) {
    toggle.addEventListener("click", function () {
      applyLang(root.dataset.lang === "zh" ? "en" : "zh");
    });
  }

  /* ---------------------------- copy buttons ------------------------ */
  function flash(btn, ok) {
    var prev = btn.textContent;
    btn.textContent = ok ? "copied" : "press ⌘/Ctrl+C";
    btn.classList.add("done");
    setTimeout(function () { btn.textContent = prev; btn.classList.remove("done"); }, 1400);
  }
  function copyText(text, btn) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(
        function () { flash(btn, true); },
        function () { legacy(text, btn); }
      );
    } else {
      legacy(text, btn);
    }
  }
  function legacy(text, btn) {
    try {
      var ta = document.createElement("textarea");
      ta.value = text;
      ta.setAttribute("readonly", "");
      ta.style.position = "absolute";
      ta.style.left = "-9999px";
      document.body.appendChild(ta);
      ta.select();
      var ok = document.execCommand("copy");
      document.body.removeChild(ta);
      flash(btn, ok);
    } catch (e) { flash(btn, false); }
  }

  // Code-block copy buttons: copy the sibling <pre> text.
  document.querySelectorAll(".code .copy").forEach(function (btn) {
    btn.addEventListener("click", function () {
      var block = btn.closest(".code");
      var pre = block && block.querySelector("pre");
      if (pre) copyText(pre.innerText.replace(/ /g, " ").trim() + "\n", btn);
    });
  });

  // Checksum copy button: copy the full value from data-full.
  var shaBtn = document.getElementById("copy-sha");
  if (shaBtn) {
    shaBtn.addEventListener("click", function () {
      copyText(shaBtn.getAttribute("data-full") || "", shaBtn);
    });
  }
})();
