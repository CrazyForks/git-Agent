# Changelog

[English](CHANGELOG.md) · [简体中文](CHANGELOG.zh-CN.md)

User-facing changes are recorded here starting with version 1.4.1.
Earlier versions are documented in [GitHub Releases](https://github.com/adoin/git-Agent/releases).
Unreleased entries describe development changes, not features in the latest downloadable package.

## Unreleased

### Changed

- Enhanced commit search: file searches show filename matches before content matches, support cancellation with Git process cleanup, and use localized progress/error messages. Slow searches allow up to 120 seconds and clearly mark incomplete results.
- Refined commit-detail cards: titles and branch lists are limited to two lines, with full content available in a single scrollable hover tooltip in both history and search.
- Improved layout: increased the commit-search input height and aligned the commit panel's AI generation, history, and options controls on one centerline, with a more compact AI button.

### Fixed

- Corrected the logo's branch-to-node connections and restored fully hollow circular nodes. Window, installer, and website icon resources now share the same SVG artwork.

## 1.4.1 — 2026-09-04

### Added

- AI commit-message generation from staged changes, with a conventional commit subject and numbered, business-oriented change details.
- On-demand AI context lookup in related tracked files and symbol references, including files outside the staged diff. Analysis uses a fixed index-tree snapshot, excluding unstaged edits and untracked files.
- An English / Chinese commit-message language setting, independent of the application interface language.
- A Simplified Chinese README with language-switching links, and bilingual changelogs accessible from the README and project website.

### Changed

- AI generation starts with one click; hovering displays the current model and data-sharing notice. Successful output goes straight into the commit editor, without a separate confirmation or suggestion-acceptance step.
- Draft edits made during generation are preserved. Generation can be cancelled, and changed index or HEAD state invalidates the result. Nothing is committed or pushed automatically.
- The AI generation entry uses a prominent icon button. Disabled primary buttons share the commit button's appearance, including the icon and label colors.

### Licensing

- Added Apache License 2.0 with Commons Clause License Condition v1.0, attribution notices, and license files in release packages. These terms apply together; the project is source-available, not licensed under unmodified Apache 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
