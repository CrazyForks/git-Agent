# Git Agent

Git Agent is a desktop Git helper built with Rust and egui.

## Build

```powershell
cargo build --release
```

Release binaries are produced in `target/release/`.

## Local Development

Start the local watcher and desktop app from the repository root:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\dev.ps1
```

## Theme Palette

`theme.json` is the default color pool for the desktop UI. Theme tokens use HSL templates; `${c}` is replaced with the selected accent hue. The app loads a sibling `theme.json` beside the executable first, then falls back to the embedded default.

Create `theme.local.json` beside the selected `theme.json` to customize the palette without editing the default file. Only keys declared in `theme.local.json` are replaced; all other hues and theme tokens remain inherited from `theme.json`. The local file is ignored by Git. Set `GIT_AGENT_THEME` to select another base palette file; its sibling `theme.local.json` remains the local override.

## GitHub Actions

Pushing a `v*` tag runs the `Build` workflow for Linux, macOS, and Windows. Each job runs tests, builds release binaries, and uploads a native desktop installer for that platform.

macOS releases are published as a universal Apple Silicon/Intel `.dmg`. Open it and drag `Git Agent.app` to Applications.

Linux releases are published as an amd64 `.deb` with an application-menu entry, icon, command-line launchers, and all three Git Agent executables. Open it with the distribution's software installer. Debian and Ubuntu are supported by this package format.

Windows releases are published as `GitAgentSetup-<version>.exe`. The setup wizard lets you choose the install path and installs both executables.

Installed applications keep user data in a writable per-user location:

```text
Windows: <install path>/data
macOS:   ~/Library/Application Support/Git Agent
Linux:   $XDG_DATA_HOME/git-agent or ~/.local/share/git-agent
```

Existing macOS/Linux data from the former `~/.local/bin/data` installer is copied automatically on first launch.

The workflow keeps only the latest 3 build runs and sets uploaded installer artifacts to expire after 3 days, which helps limit GitHub Actions storage usage.

## Release

Create and push a version tag to publish a GitHub Release with Linux, macOS, and Windows installer packages:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

The release assets are generated automatically from the workflow build outputs.
