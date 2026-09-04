"use strict";

(() => {
  let stored;
  try { stored = localStorage.getItem("git-agent-site-language"); } catch { /* Optional preference. */ }
  const requested = new URLSearchParams(location.search).get("lang");
  const language = (requested || stored) === "en" ? "en" : "zh";
  document.documentElement.lang = language === "en" ? "en" : "zh-CN";
  document.title = `${language === "en" ? "Changelog" : "更新日志"} — Git Agent`;
  document.querySelectorAll("[data-language]").forEach(section => {
    section.hidden = section.dataset.language !== language;
  });
  const toggle = document.querySelector("[data-language-toggle]");
  toggle.href = `?lang=${language === "en" ? "zh" : "en"}`;
  toggle.textContent = language === "en" ? "简体中文 ↗" : "English ↗";
  document.querySelectorAll("[data-home]").forEach(link => { link.href = `./?lang=${language}`; });
  try { localStorage.setItem("git-agent-site-language", language); } catch { /* Optional preference. */ }
})();
