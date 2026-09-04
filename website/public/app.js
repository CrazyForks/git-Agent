"use strict";

(() => {
  const REPOSITORY = "https://github.com/adoin/git-Agent";
  const RELEASES = `${REPOSITORY}/releases/latest`;
  const english = {
    "skip": "Skip to content",
    "nav.changelog": "Changelog",
    "nav.features": "Features", "nav.ai": "AI merge", "nav.download": "Download", "nav.docs": "Docs ↗",
    "hero.release": "Ready for your next great commit",
    "hero.title1": "Every change.", "hero.title2": "Clearly in control.",
    "hero.description": "Changes, history, branches, and conflicts. Finally, in one clear view. A native Git client that keeps your attention on the code, not the tooling.",
    "hero.download": "Download Git Agent", "hero.source": "Explore on GitHub",
    "capture.history": "Commit history · actual test-repo screenshot", "capture.workspace": "Workspace & diff · actual test-repo screenshot", "capture.merge": "AI-assisted merge · actual test-repo screenshot", "capture.zoom": "Click to enlarge ↗",
    "capture.themes": "One merge tool. Two appearances.", "capture.themeLabel": "Screenshot theme", "capture.dark": "Dark screenshot", "capture.light": "Light screenshot",
    "capture.note": "These are unaltered application screenshots. The AI output shows this test run only; other analyses may produce different suggestions.",
    "capture.dialogTitle": "Actual application screenshot", "capture.original": "Open original ↗", "capture.close": "Close ✕",
    "proof.intro": "Less switching. More focus.", "proof.native": "Native Rust app", "proof.git": "Your system Git", "proof.ai": "AI is optional",
    "features.title": "From the first change.<br>To the final merge.", "features.description": "Everyday Git, within easy reach.<br>Keep the flow. Keep the details.",
    "features.workspace.title": "Know exactly what changed.", "features.workspace.body": "See staged and unstaged changes separately. Select files together, review side-by-side diffs, and make every commit intentional.", "features.workspace.tag1": "Repository tabs", "features.workspace.tag2": "Batch staging", "features.workspace.tag3": "Syntax highlighting",
    "features.history.title": "See the story. Move it forward.", "features.history.body": "Follow the commit graph and search history by file. Find a clear path through branches, tags, stashes, and interactive rebases.", "features.history.tag1": "Commit graph", "features.history.tag2": "File blame", "features.history.tag3": "Interactive rebase",
    "features.merge.title": "Make sense of conflicts.", "features.merge.body": "Compare both sides and the final result in a dedicated three-way editor. Navigate conflicts, with undo and redo at hand.", "features.merge.tag1": "Three-way merge", "features.merge.tag2": "Conflict navigation", "features.merge.tag3": "Undo / Redo",
    "features.more": "And the tools you already rely on", "features.patches": "Patches", "features.ignore": ".gitignore explanations",
    "ai.title": "AI suggests.<br><span>You decide.</span>", "ai.description": "Get help understanding conflicts, with an explanation for each proposed resolution. Review suggestions individually or apply them together. The final save is always yours.",
    "ai.point1.title": "Bring the model you choose", "ai.point1.body": "Compatible with OpenAI Chat Completions and Claude Messages APIs.",
    "ai.point2.title": "Apply together. Undo together.", "ai.point2.body": "Apply actionable suggestions as a batch. Stale or conflicting edits reject the whole batch.",
    "ai.point3.title": "No silent saves", "ai.point3.body": "Review the result before saving and completing the merge.", "ai.docs": "Explore AI merging and privacy",
    "download.title": "Your platform. Your Git Agent.", "download.description": "Pick your platform. Make your next commit a smooth one.", "download.latest": "Latest stable GitHub release", "download.windows": "Download for Windows", "download.macos": "Download for macOS", "download.linux": "Download for Linux", "download.windows.note": "Run the installer to get started", "download.macos.note": "Universal build. Drag into Applications.", "download.linux.note": "Other distributions: build from source", "download.note": "Install Git first. Downloads come from GitHub Releases; if a download is blocked, try the release page.", "download.releases": "All versions & release notes ↗", "download.source": "Build from source ↗",
    "faq.title": "Before you<br>get started.", "faq.git.q": "Do I need to install Git first?", "faq.git.a": "Yes. Git Agent uses your installed Git. Check that git --version works in your terminal. Remote operations use your existing Git / SSH credentials.", "faq.ai.q": "Do I have to configure AI?", "faq.ai.a": "No. Commits, branches, history, diffs, and manual merging work independently. AI is an optional aid and requires a compatible model provider of your choice. Provider charges may apply.", "faq.privacy.q": "Will AI see my code?", "faq.privacy.a": "When you request AI analysis, file contents, conflict context, related source, and Git history may be sent to your configured model provider. Check its data policy and your repository rules before sending private code.", "faq.mac.q": "What if macOS cannot verify the developer?", "faq.mac.a": "The current package is ad-hoc signed, not Apple-notarized. Verify the download source and follow Apple's guidance. Do not disable system-wide security checks.", "faq.mac.link": "Read Apple's guide ↗",
    "support.title": "Enjoy the flow? Buy me a coffee.", "support.description": "A little support helps make the next version better.<br>Ideas, bug reports, and code contributions matter just as much.", "support.button": "Become a supporter", "support.note": "Voluntary support through Ko-fi",
    "footer.tagline": "Less tooling in the way. More room to create.", "footer.issues": "Report an issue ↗"
  };
  const translatedNodes = [...document.querySelectorAll("[data-i18n]")];
  const chinese = Object.fromEntries(translatedNodes.map(node => [node.dataset.i18n, node.innerHTML]));
  const languageToggle = document.getElementById("language-toggle");
  let language = "zh";
  let release = null;
  const captures = {
    history: { width: 1559, height: 802, zh: "Git Agent 实际截图：test-repo 的提交图、提交详情与代码差异", en: "Actual Git Agent screenshot: test-repo commit graph, commit details, and code diff" },
    workspace: { width: 1559, height: 802, zh: "Git Agent 实际截图：test-repo 工作区的暂存文件、未暂存文件与双栏差异", en: "Actual Git Agent screenshot: staged and unstaged files with a side-by-side workspace diff in test-repo" },
    "merge-dark": { width: 1180, height: 760, zh: "Git Agent 深色模式实际截图：三方合并编辑器、AI 合并建议与应用所有建议入口", en: "Actual Git Agent dark screenshot: three-way merge editor, AI suggestions, and Apply all suggestions" },
    "merge-light": { width: 1180, height: 760, zh: "Git Agent 浅色模式实际截图：三方合并编辑器、AI 合并建议与应用所有建议入口", en: "Actual Git Agent light screenshot: three-way merge editor, AI suggestions, and Apply all suggestions" }
  };
  const captureLinks = [...document.querySelectorAll("[data-capture]")];
  const captureDialog = document.getElementById("capture-dialog");
  const fullCapture = document.getElementById("capture-full");
  const originalCapture = document.getElementById("capture-original");
  const mergeCapture = document.getElementById("merge-capture-link");
  const themeButtons = [...document.querySelectorAll("[data-capture-theme]")];
  let activeCapture = "history";
  let captureTrigger = null;
  function renderCaptureLabels() {
    captureLinks.forEach(link => { link.querySelector("img").alt = captures[link.dataset.capture][language]; });
    fullCapture.alt = captures[activeCapture][language];
  }
  themeButtons.forEach(button => button.addEventListener("click", () => {
    const key = `merge-${button.dataset.captureTheme}`;
    if (!captures[key]) return;
    mergeCapture.dataset.capture = key;
    mergeCapture.href = `assets/${key}.png`;
    mergeCapture.querySelector("img").src = `assets/${key}.png`;
    themeButtons.forEach(item => item.setAttribute("aria-pressed", String(item === button)));
    renderCaptureLabels();
  }));
  captureLinks.forEach(link => link.addEventListener("click", event => {
    // Preserve the original-image fallback and the browser's modified-click behavior.
    if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey || event.button !== 0 || typeof captureDialog.showModal !== "function") return;
    event.preventDefault();
    activeCapture = link.dataset.capture;
    captureTrigger = link;
    fullCapture.src = link.href;
    fullCapture.width = captures[activeCapture].width;
    fullCapture.height = captures[activeCapture].height;
    originalCapture.href = link.href;
    renderCaptureLabels();
    captureDialog.showModal();
    document.body.classList.add("capture-open");
  }));
  document.getElementById("capture-close").addEventListener("click", () => captureDialog.close());
  captureDialog.addEventListener("click", event => {
    const bounds = captureDialog.getBoundingClientRect();
    if (event.target === captureDialog && (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom)) captureDialog.close();
  });
  captureDialog.addEventListener("close", () => {
    document.body.classList.remove("capture-open");
    captureTrigger?.focus({ preventScroll: true });
  });
  function renderRelease() {
    if (!release) return;
    const label = document.querySelector("#release-version [data-i18n]");
    label.textContent = language === "en" ? `${release.tag_name} · latest release` : `${release.tag_name} · 最新稳定版`;
  }

  function setLanguage(next) {
    language = next === "en" ? "en" : "zh";
    const strings = language === "en" ? english : chinese;
    translatedNodes.forEach(node => { node.innerHTML = strings[node.dataset.i18n] ?? chinese[node.dataset.i18n]; });
    document.documentElement.lang = language === "en" ? "en" : "zh-CN";
    document.body.classList.toggle("language-en", language === "en");
    document.querySelectorAll("[data-changelog-link]").forEach(link => {
      link.href = `changelog.html?lang=${language}`;
    });
    languageToggle.innerHTML = language === "en" ? "中文 <span aria-hidden=\"true\">↗</span>" : "EN <span aria-hidden=\"true\">↗</span>";
    languageToggle.setAttribute("aria-label", language === "en" ? "切换到简体中文" : "Switch to English");
    document.title = language === "en" ? "Git Agent — Every change. Clearly in control." : "Git Agent — 让每一次提交，都心中有数。";
    document.querySelector('meta[name="description"]').content = language === "en"
      ? "A native desktop Git client for Windows, macOS, and Linux. Clear history, side-by-side diffs, and AI-assisted merges that keep you in control."
      : "Git Agent：为 Windows、macOS 和 Linux 打造的原生桌面 Git 客户端。清晰的历史、直观的差异、由你掌控的 AI 辅助合并。";
    renderRelease();
    renderCaptureLabels();
  }

  const requestedLanguage = new URLSearchParams(location.search).get("lang");
  try { language = requestedLanguage || localStorage.getItem("git-agent-site-language") || "zh"; } catch { language = requestedLanguage || "zh"; }
  setLanguage(language);
  languageToggle.addEventListener("click", () => {
    setLanguage(language === "en" ? "zh" : "en");
    try { localStorage.setItem("git-agent-site-language", language); } catch { /* Storage may be disabled. */ }
    const url = new URL(location.href);
    url.searchParams.set("lang", language);
    history.replaceState(null, "", url);
  });

  const menuToggle = document.getElementById("menu-toggle");
  const menu = document.getElementById("main-nav");
  function closeMenu() { menu.classList.remove("open"); menuToggle.setAttribute("aria-expanded", "false"); }
  menuToggle.addEventListener("click", () => {
    const open = menu.classList.toggle("open");
    menuToggle.setAttribute("aria-expanded", String(open));
  });
  menu.querySelectorAll("a").forEach(link => link.addEventListener("click", closeMenu));
  document.addEventListener("click", event => { if (!menu.contains(event.target) && !menuToggle.contains(event.target)) closeMenu(); });
  document.addEventListener("keydown", event => { if (event.key === "Escape" && menu.classList.contains("open")) { closeMenu(); menuToggle.focus(); } });

  // Public, read-only metadata. No credentials, trackers, or server-side GitHub token.
  // Every link stays usable as a release-page link if GitHub is unavailable.
  async function loadLatestRelease() {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 6000);
    try {
      const response = await fetch("https://api.github.com/repos/adoin/git-Agent/releases/latest", {
        signal: controller.signal, headers: { Accept: "application/vnd.github+json" }, credentials: "omit", referrerPolicy: "no-referrer"
      });
      if (!response.ok) return;
      const candidate = await response.json();
      if (candidate.draft || candidate.prerelease || !/^v\d+\.\d+\.\d+$/.test(candidate.tag_name) || !Array.isArray(candidate.assets)) return;
      const patterns = { windows: /^GitAgentSetup-v[\d.]+\.exe$/, macos: /^GitAgent-[\d.]+-macOS\.dmg$/, linux: /^GitAgent_[\d.]+_amd64\.deb$/ };
      for (const link of document.querySelectorAll(".platform-download")) {
        const asset = candidate.assets.find(item => patterns[link.dataset.os].test(item.name));
        if (!asset) continue;
        try {
          const url = new URL(asset.browser_download_url);
          if (url.protocol !== "https:" || url.host !== "github.com" || url.username || url.password || !url.pathname.startsWith("/adoin/git-Agent/releases/download/")) continue;
          link.href = url.href;
        } catch { link.href = RELEASES; }
      }
      release = candidate;
      renderRelease();
    } catch { /* Keep the visible, usable latest-release links when offline or rate-limited. */ }
    finally { clearTimeout(timeout); }
  }
  loadLatestRelease();
})();
