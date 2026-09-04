# Git Agent website

Static, bilingual product site for **https://git-agent.emssion.com/**.
No framework, build dependency, analytics, account, or backend is required.

## Local preview

From the repository root:

```sh
python -m http.server 4173 --bind 127.0.0.1 --directory website/public
```

Open `http://127.0.0.1:4173`. On systems where Python is named `python3`, use that instead.

The default language is Simplified Chinese. The language button switches to English;
`?lang=en` and `?lang=zh` are shareable overrides. Only the language preference is stored locally.

## Downloads

All download links work without JavaScript and lead to GitHub's latest release page.
On page load, the site makes one anonymous, read-only request to GitHub's public release API.
When successful, the platform links point to the latest matching release assets. Requests
time out after six seconds; offline, rate-limited, and invalid responses preserve the release-page fallback.
No GitHub token is stored or sent. There is no polling or automatic download.

GitHub receives the visitor's normal network request when release metadata is loaded.
If that request is blocked, the release-page links remain available.

Product visuals must be actual application screenshots supplied by the maintainer.
Do not reconstruct application windows, fabricate commit histories, or simulate AI results
as product demonstrations. The four PNGs are byte-for-byte copies of the maintainer's
captures; do not redraw, recolor, or crop their contents:

| Original | Public asset | Placement |
| --- | --- | --- |
| `2.png` | `assets/history.png` | Main history showcase |
| `1.png` | `assets/workspace.png` | Workspace and diff |
| `4.png` | `assets/merge-dark.png` | AI merge, default dark capture |
| `3.png` | `assets/merge-light.png` | Alternate light capture |

Images retain their original proportions. Clicking opens an accessible native dialog;
Escape, the close button, or the backdrop dismisses it and returns focus to the image link.
The original-image link supports full-resolution viewing. With JavaScript disabled,
the images link directly to their originals and a separate link exposes the light capture.
The theme buttons switch screenshots, not the captured application state. The AI results
belong to those actual test runs and do not promise identical future model output.

## Deployment

Before publishing, run `python website/build_changelog.py` from the repository root.
Edit `CHANGELOG.md` and `CHANGELOG.zh-CN.md` together; the generated
`public/changelog.html` is committed with the site so deployments need no Markdown
renderer. Mark planned releases as unreleased until they are actually published.
On release day, replace that status with the release date in both sources and rebuild.
The website navigation links to the matching language; without JavaScript the page
shows both languages in full.

The existing Caddy service hosts this domain separately from the server's other sites.
Public files live in versioned directories under `/srv/git-agent-site/releases/`, with
`/srv/git-agent-site/current` pointing to the active version. The Caddy configuration
imports `/etc/caddy/sites/git-agent.caddy` from the existing Caddyfile.

`deploy/git-agent.caddy` is the domain-specific configuration. Deploy only `public/`,
never the repository root or SSH credentials. Validate the combined Caddy configuration
before reloading the service. Keep the previous release and Caddyfile backup for rollback.

Use the existing authenticated SSH configuration on the maintainer's machine; no credentials
belong in this directory. HTTPS certificates are managed and renewed by Caddy.

On the initial deployment, the in-app browser successfully loaded HTTPS and the server
returned HTTP 200 with a valid certificate. A separate direct network test returned an
Alibaba Cloud `Non-compliance ICP Filing` page for HTTP and a reset for HTTPS. Check the
domain's filing/access status with the hosting provider; do not assume a successful test
from one network proves reachability for all visitors.

## Checks

```sh
python website/build_changelog.py --check
node --check website/public/app.js
node --check website/public/changelog.js
python website/check.py
```

Keep website sources, checks, deployment scripts, and the generated changelog in Git.
Only local caches, environment secrets, private keys, logs, and temporary deployment
archives are ignored. Ignore rules are not a security boundary: deploy only the
reviewed `public/` output and never place credentials inside it.

Also verify the live page in a browser: language switching, FAQ disclosure, narrow layout
and mobile navigation, download destinations, and Ko-fi links. Confirm there are no
reconstructed product interfaces or simulated AI interactions. Keep live model calls out
of these checks. Also verify all four images, theme switching, original-image destinations,
dialog opening and closing, Escape/focus restoration, translated captions, and mobile overflow.
