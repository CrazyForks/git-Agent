"""Dependency-free structural checks for the static public website."""
from collections import Counter
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit
import hashlib
import re
import struct
from build_changelog import build

ROOT = Path(__file__).resolve().parent / "public"


class Page(HTMLParser):
    def __init__(self):
        super().__init__()
        self.ids = []
        self.urls = []
        self.translations = []
        self.references = []
        self.tags = Counter()

    def handle_starttag(self, tag, pairs):
        attrs = dict(pairs)
        self.tags[tag] += 1
        if "id" in attrs:
            self.ids.append(attrs["id"])
        if "data-i18n" in attrs:
            self.translations.append(attrs["data-i18n"])
        for key in ("href", "src"):
            if key in attrs:
                self.urls.append(attrs[key])
        for key in ("aria-controls", "aria-labelledby"):
            self.references.extend(attrs.get(key, "").split())
        if tag == "img":
            assert "alt" in attrs, "Images need alt text (empty for decorative images)."
        if tag == "a" and attrs.get("target") == "_blank":
            assert "noopener" in attrs.get("rel", ""), "External tabs need noopener."
        assert not any(key.startswith("on") for key in attrs), "No inline event handlers."


page = Page()
html = (ROOT / "index.html").read_text(encoding="utf-8")
page.feed(html)
for removed_mock in ("product-window", "ai-playground", "data-tab=", "apply-demo", "result-code", "c82a9f1"):
    assert removed_mock not in html, f"Simulated product UI must not return: {removed_mock}"
assert page.tags["h1"] == 1
assert len(page.ids) == len(set(page.ids)), "Duplicate IDs"
for reference in page.references:
    assert reference in page.ids, f"Missing accessible reference: {reference}"
for value in page.urls:
    url = urlsplit(value)
    if url.scheme or url.netloc:
        assert url.scheme == "https", f"Insecure external link: {value}"
        continue
    if url.path:
        path = ROOT / unquote(url.path.lstrip("/"))
        assert path.is_file(), f"Missing local asset: {value}"
    if url.fragment and not url.path:
        assert url.fragment in page.ids, f"Missing anchor: {value}"
script = (ROOT / "app.js").read_text(encoding="utf-8")
for key in page.translations:
    assert re.search(r'"' + re.escape(key) + r'"\s*:', script), f"Missing English copy: {key}"
assert "https://ko-fi.com/adoin" in page.urls
assert "https://github.com/adoin/git-Agent/releases/latest" in page.urls
assert "changelog.html?lang=zh" in page.urls, "Homepage needs a changelog entry."
changelog_html = (ROOT / "changelog.html").read_text(encoding="utf-8")
assert changelog_html == build(), "Rebuild the website changelog after editing Markdown."
changelog = Page()
changelog.feed(changelog_html)
assert len(changelog.ids) == len(set(changelog.ids)), "Duplicate changelog IDs"
assert 'data-language="zh"' in changelog_html and 'data-language="en"' in changelog_html
assert re.search(r"<h2>\d+\.\d+\.\d+ — .+</h2>", changelog_html), "Missing version entry."
for value in changelog.urls:
    url = urlsplit(value)
    if not url.scheme and url.path and url.path != "./":
        assert (ROOT / url.path).is_file(), f"Missing changelog asset: {value}"
assert "https://ko-fi.com/adoin" in changelog.urls
for document in (page, changelog):
    assert "assets/logo-ga.svg?v=connected-1" in document.urls
    assert "favicon.ico?v=connected-1" in document.urls
icons = ROOT.parent.parent / "assets" / "icons"
for name in ("logo-ga.svg", "logo-ga.png"):
    assert (ROOT / "assets" / name).read_bytes() == (icons / name).read_bytes(), f"Logo mismatch: {name}"
favicon = (ROOT / "favicon.ico").read_bytes()
assert favicon == (icons / "git-agent.ico").read_bytes(), "Favicon must match the app icon."
assert favicon[:6] == bytes([0, 0, 1, 0, 1, 0])
assert favicon[22:] == (ROOT / "assets" / "logo-ga.png").read_bytes()
for filename in ("index.html", "styles.css", "captures.css", "app.js"):
    content = (ROOT / filename).read_text(encoding="utf-8")
    assert "\ufffd" not in content, f"Encoding error in {filename}"
    assert not re.search(r"[ \t]+$", content, re.MULTILINE), f"Trailing whitespace in {filename}"
assert page.tags["dialog"] == 1, "Screenshot enlargement dialog is required."
assert html.count('class="capture-link"') == 3, "History, workspace and merge captures are required."
assert 'data-capture-theme="light"' in html and 'data-capture-theme="dark"' in html
for name, size in {"history": (1559, 802), "workspace": (1559, 802),
                   "merge-dark": (1180, 760), "merge-light": (1180, 760)}.items():
    data = (ROOT / "assets" / f"{name}.png").read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", f"Invalid PNG: {name}"
    assert struct.unpack(">II", data[16:24]) == size, f"Unexpected screenshot dimensions: {name}"
    print(f"Original capture: {name}.png {size[0]}x{size[1]} SHA256={hashlib.sha256(data).hexdigest()}")
print(f"PASS: {len(page.ids)} unique IDs, {len(page.urls)} links/assets, "
      f"{len(page.translations)} translated text nodes; accessibility references valid.")
