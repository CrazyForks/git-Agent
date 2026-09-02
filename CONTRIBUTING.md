# Contributing to Git Agent

Git Agent is a Rust/egui desktop application. The standard Cargo workflow works on Windows,
macOS, and Linux; PowerShell is not a requirement for macOS or Linux development.

[Back to the README](README.md)

## Development prerequisites

On every platform, install:

- Git, available on `PATH`.
- A current stable Rust toolchain, including Cargo, through the
  [official Rust installation guide](https://rust-lang.org/tools/install/).
- Your platform's native compiler/linker and the libraries below.

The release workflow uses stable Rust and the committed `Cargo.lock`. There is currently
no separately declared minimum supported Rust version. Do not update dependencies or the
lockfile incidentally when working on an unrelated change.

### macOS

Install Apple's Command Line Tools if they are not already present:

```sh
xcode-select --install
```

After installation, check `git --version`, `rustc --version`, and `cargo --version` in a new
terminal. See the [Rust installation chapter](https://doc.rust-lang.org/book/ch01-01-installation.html)
for compiler/linker setup details.

Local Cargo builds target the architecture of your toolchain (Apple Silicon or Intel).
You do not need to build a universal binary for everyday development. Release CI builds
both `aarch64-apple-darwin` and `x86_64-apple-darwin` and combines them with `lipo`.

### Linux

For Debian/Ubuntu, install a compiler and the native libraries used by the release workflow:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libgtk-3-dev libx11-dev libxcb1-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev
```

On other distributions, install the equivalent development packages. The package list above
is for Debian/Ubuntu, not a claim that every distribution has been tested.

Use a graphical desktop session to run the UI. AI credential storage additionally needs
an unlocked Secret Service-compatible keyring available in your desktop session.

### Windows

Install Git for Windows and the MSVC Rust toolchain. Install the Visual Studio C++ build
tools and Windows SDK when prompted by Rust's installer; follow the
[official Windows setup instructions](https://rust-lang.org/tools/install/).

PowerShell 7 is needed only for the optional development watcher below, not for Cargo itself.
Inno Setup is needed only when building a Windows installer.

## Build and run

From the repository root, on any supported platform:

```sh
cargo build --locked --bins
cargo run --locked --bin git-agent
```

The first command builds the main app and its sibling `git-agent-diff` and `git-agent-merge`
executables. Rebuild all binaries when you change the diff or merge tool, since the main
app launches these tools from its own executable directory.

Close and relaunch the app to load a rebuilt binary. On Windows, close all three applications
before building because a running `.exe` can be locked. On macOS and Linux, the Cargo
commands above are the supported manual edit/build/run loop; there is no bundled Unix watcher.

### Optional Windows watcher

From PowerShell 7:

```powershell
pwsh -NoLogo -NoProfile -File ./scripts/dev.ps1
```

The watcher rebuilds and restarts the application when `src/`, `assets/`, `Cargo.toml`, or
`Cargo.lock` changes. Logs are written under `target/dev-watch/`.

**Save your work before using it.** The script stops running `git-agent`, `git-agent-diff`,
and `git-agent-merge` processes by name, so it can also close other instances. It uses
Windows process-management APIs and `.exe` paths; installing PowerShell on a Mac does not
make this script a macOS development workflow.

## Tests and checks

Run the standard test suite from the repository root:

```sh
cargo test --locked
git diff --check
```

For focused work, select the appropriate integration test target:

```sh
cargo test --locked --test merge_tool_tests
cargo test --locked --test diff_tool_tests
cargo test --locked --test dev_script_tests
cargo test --locked --test windows_packaging_tests
```

Many unit tests live alongside their implementations in `src/`. Integration tests are in
`tests/` and use temporary repositories or inspect packaging/scripts. Run relevant tests
as well as the full suite before submitting a behavior change.

Ignored live-AI tests require an explicitly selected conflicted repository and a configured
model. They can send repository content to a provider and incur charges. Do not enable all
ignored tests as a routine test command or point them at a valuable working repository.

For manual Git-operation testing, use a dedicated disposable repository, not this source
checkout. Test both success and failure paths, including pending states and recovery.
State which operating systems you actually tested; compiling locally does not verify the
other platforms' UI or installer behavior.

## Standalone diff and merge tools

These examples use Cargo so the commands work without platform-specific executable paths.
Replace the example filenames with existing test files before running them.

### Diff viewer

Create a patch file from a test repository:

```sh
git diff --no-color --output=changes.patch
```

Then, from the Git Agent source root, pass the path to that patch:

```sh
cargo run --locked --bin git-agent-diff -- --title "Working changes" --left "Index" --right "Working tree" --diff /path/to/changes.patch
```

The `--left` and `--right` arguments are display labels, not input file paths. On Windows,
substitute a Windows path such as `"C:/work/test-repo/changes.patch"`.

### Merge editor

```sh
cargo run --locked --bin git-agent-merge -- --base base.txt --local local.txt --remote remote.txt --output merged.txt
```

The input files represent the base and two versions to merge. Saving writes to `--output`;
choose a disposable output file. Adding `--repo-root <repository> --stage` also stages the
output when saved, so use those options only when that is intended. Both standalone tools
accept `--theme dark|light` and `--language en|zh`.

## Code layout

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | Main executable and window setup |
| `src/app.rs` | Application UI, state, settings, and task orchestration |
| `src/git.rs` | Git command execution and repository operations |
| `src/graph.rs` | Commit graph layout |
| `src/diff_tool.rs` | Standalone diff viewer |
| `src/merge_tool.rs` | Three-way merge editor and AI recommendations |
| `src/bin/` | Diff/merge executable entry points |
| `src/gitignore.rs` | Ignore-rule parsing and explanations |
| `src/i18n.rs`, `src/theme.rs`, `src/syntax.rs` | Localization, appearance, syntax highlighting |
| `src/diagnostics.rs`, `src/updater.rs` | Diagnostics and updates |
| `tests/` | Integration and packaging regression tests |
| `installer/`, `.github/workflows/build.yml` | Native packages and release automation |

## Change guidelines

- Read [AGENTS.md](AGENTS.md) before changing repository behavior or UI transitions.
  Long-running actions must have immediate visible feedback, asynchronous ownership,
  shared busy gating, a fresh reload where required, and reliable cleanup on failure.
- Before editing title-bar or repository-tab interaction, read
  [the window-dragging guide](docs/top-bar-window-drag.md).
- Keep user-facing strings available in both English and Simplified Chinese.
- Add regression tests for the bug or behavior you change. Multi-selection actions should
  test the entire selection, not just the item used to open a context menu.
- Avoid unrelated formatting, generated files, local configuration, credentials, and logs
  in a pull request. Include reproduction steps and the exact checks you ran.
- Documentation should distinguish shipped behavior from unreleased work. Design documents
  in `docs/` are not by themselves evidence that a feature is implemented.

## Packaging and releases

Packaging is separate from normal development. To build optimized binaries locally:

```sh
cargo build --release --locked --bins
```

Platform-specific packaging entry points are:

- macOS: [installer/macos/package.sh](installer/macos/package.sh), using Apple's packaging
  tools. It packages the architectures present in the supplied binary directory; it does
  not create universal binaries itself.
- Linux: [installer/linux/package-deb.sh](installer/linux/package-deb.sh), using `dpkg-deb`.
- Windows: [installer/windows/git-agent.iss](installer/windows/git-agent.iss), using Inno Setup.

See [the build workflow](.github/workflows/build.yml) for exact packaging invocations and
the universal macOS build steps. Packaging scripts replace their own staging/output files;
use a dedicated output directory, not a directory containing personal files.

Publishing is a maintainer operation, not part of a contributor's setup:

1. Update the package version and lockfile together, and finish the release notes/docs.
2. Run the tests and review the changes that will ship.
3. Create and push a new `v<version>` tag matching `Cargo.toml` exactly.
4. Confirm that the Linux, macOS, and Windows jobs pass and that all three installers are
   attached to the GitHub release.

The `Build` workflow runs on `v*` tags, not on every branch push or pull request. It verifies
the tag/version match, tests and builds each platform, then publishes the release packages.
CI artifacts expire after three days; downloadable release assets are separate from those
temporary artifacts.
