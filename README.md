# Git Agent

A native desktop Git client for **Windows, macOS, and Linux**, built with Rust and egui.
Manage repositories, review changes, explore history, and resolve conflicts in one place,
with optional AI assistance for three-way merges.

[Download](https://github.com/adoin/git-Agent/releases/latest) ·
[Getting started](#getting-started) ·
[Build from source](#build-from-source) ·
[Contribute](CONTRIBUTING.md) ·
[Support](#support) ·
[Report an issue](https://github.com/adoin/git-Agent/issues)

Git Agent is under active development. Packaged releases and development checkouts may
differ; check the release you are using before relying on a newly added feature.

## Features

### Everyday Git workflows

- Open, clone, and initialize repositories; organize them into workspaces and repository tabs.
- Inspect staged and unstaged changes, stage or unstage multiple files, and create commits.
- Manage branches, tags, and stashes; configure remotes and branch upstreams.
- Fetch, pull, and push with configurable options, and set repository-specific commit identities.
- Edit `.gitignore` rules with line-by-line explanations based on Git syntax, without an AI call.

### History and change review

- Browse the commit graph, search commits, and find commits by changed file.
- Inspect commit details, file changes, and blame information.
- Compare the working tree with a selected commit.
- Open a dedicated side-by-side diff window with syntax highlighting.
- Create and apply patches, including patches for selected worktree files or commits.

### Branching and conflict resolution

- Merge, cherry-pick, revert, and reset commits.
- Plan interactive rebases with pick, reword, edit, squash, fixup, and drop actions.
- Continue, skip, or abort an in-progress rebase.
- Resolve conflicts in a dedicated three-way merge editor: compare both sides, edit the
  result, navigate conflicts, and undo or redo edits before saving.
- Request AI merge recommendations with explanations and proposed resolutions; keep
  control over which changes are applied. See [AI-assisted merging](#ai-assisted-merging).

### Repository tools and customization

- Configure Git Flow feature, release, and hotfix workflows.
- Add submodules and subtrees; configure Git LFS tracking and run LFS operations.
- Use custom Git actions and repository performance diagnostics.
- Choose English or Simplified Chinese, light or dark mode, accent colors, and UI/code fonts.
- Extend syntax highlighting with [data-only language plugins](docs/syntax-plugins.md).

## Download and install

Choose the installer for your system from the
[latest GitHub release](https://github.com/adoin/git-Agent/releases/latest).
You do **not** need Rust or PowerShell to use the packaged application.

| Platform | Release package | Installation |
| --- | --- | --- |
| Windows x64 | `GitAgentSetup-v<version>.exe` | Run the installer and choose an installation folder. |
| macOS, Apple Silicon and Intel | `GitAgent-<version>-macOS.dmg` | Open the disk image and drag **Git Agent.app** to **Applications**. |
| Linux, Debian/Ubuntu amd64 | `GitAgent_<version>_amd64.deb` | Open the package with your software installer, or install it with `apt`. |

The macOS release is a universal build. Linux releases currently provide a Debian package;
other distributions can [build from source](#build-from-source), with the corresponding
native development libraries installed.

For a downloaded Debian package, substitute its actual filename:

```sh
sudo apt install "./GitAgent_<version>_amd64.deb"
```

### Runtime requirements

- **Git must be installed and available on `PATH`.** Git Agent invokes your system Git;
  the installers do not bundle it. Check with `git --version`.
- Configure SSH or an HTTPS credential helper for authenticated remote operations.
  Being signed in to GitHub in a browser does not configure Git authentication.
- Install Git LFS separately if you use LFS features. Subtree operations require
  `git subtree` to be available in your Git installation.
- On Linux, run the app in a graphical desktop session. Saving AI credentials also
  requires an available, unlocked Secret Service-compatible keyring.

The current macOS packaging script uses ad-hoc signing, not Developer ID notarization.
If macOS blocks a downloaded build, verify its source and consult
[Apple's guidance on opening downloaded apps](https://support.apple.com/en-us/102445).
Do not disable system-wide security checks to launch it.

## Getting started

1. Launch Git Agent and open an existing repository, clone a remote, or initialize a new one.
2. Review the repository's commit identity and remote configuration before your first commit
   or push. Remote credentials use your Git/SSH setup.
3. In **Workspace**, select a changed file to inspect its diff, stage the changes you want,
   enter a commit message, and commit.
4. Use **History** to explore the graph and inspect past changes, or **Search** to find commits.
5. If a merge or rebase stops on conflicts, open the merge editor for a conflicted file,
   review the result, and save it. Resolve the remaining files before completing the operation.

Normal Git workflows do not require an AI model. Try unfamiliar or destructive operations
in a disposable test repository before using them on important work.

## AI-assisted merging

AI is an optional aid to reviewing conflicts, not an unattended merge service.

1. Open **Settings → AI** in the main application and add a model configuration.
2. Select the API format, enter the provider's base URL, API key, and model ID, and test
   the connection. OpenAI-compatible Chat Completions and Claude-compatible Messages
   formats are supported; the provider/model must support the structured tool calls
   used by the merge assistant.
3. Open a conflicted file in the merge editor, choose a configured model, and request analysis.
4. Review the explanations and proposed changes, apply the ones you accept, and inspect
   the resulting code before saving. Run your project's tests before completing the merge.

**New in v1.4.0:** use **Apply all suggestions** next to the AI analysis button to apply all
actionable suggestions in one undoable step. Manual-only advice remains for review, and
stale or conflicting edits reject the entire batch. Applying suggestions does not save the
file automatically; inspect the result before saving.

### Privacy and credentials

Analysis can send file contents, conflict context, related source files, and Git history to
the configured model provider. Check the provider's data policy and your repository's rules
before analyzing private code. Provider usage charges may apply.

API keys are encrypted in the local configuration using a key stored in the operating
system's credential store. That does not anonymize the source code sent for analysis.
Do not share configuration files or diagnostic logs without reviewing and redacting them.

## Build from source

Install Git, a current stable Rust toolchain, and the native prerequisites for your platform.
See the [contributor setup guide](CONTRIBUTING.md#development-prerequisites) for macOS,
Linux, and Windows instructions.

The basic development workflow is the same in macOS/Linux terminals and Windows PowerShell:

```sh
git clone https://github.com/adoin/git-Agent.git
cd git-Agent
cargo build --locked --bins
cargo run --locked --bin git-agent
```

Build all binaries first: the main application launches the diff and merge tools as sibling
executables. Running only `cargo run --bin git-agent` on a fresh checkout does not build them.

For an optimized build:

```sh
cargo build --release --locked --bins
```

The output directory is `target/release/` (`target/debug/` for development builds):

| Binary | Purpose |
| --- | --- |
| `git-agent` | Main desktop application |
| `git-agent-diff` | Standalone side-by-side diff viewer |
| `git-agent-merge` | Standalone three-way merge editor |

Windows binaries have the `.exe` suffix. Keep the three binaries together when running
outside Cargo. For tests, the optional Windows watcher, standalone tool examples, and
packaging, see [CONTRIBUTING.md](CONTRIBUTING.md).

## Configuration and themes

Application settings and repository-tab state are stored locally:

| Platform | Settings directory |
| --- | --- |
| Windows | `data/` beside the executable; installed builds use `<install folder>/data/` |
| macOS | `~/Library/Application Support/Git Agent/` |
| Linux | `$XDG_DATA_HOME/git-agent/`, or `~/.local/share/git-agent/` when unset |

These paths describe application settings. Diagnostic logs and syntax plugins currently
use a separate `data/` directory beside the executable, including on macOS and Linux.
See [Troubleshooting](#troubleshooting) for the implications on installed builds.

Appearance can be changed in Settings. For custom palettes, the application starts with the
embedded [theme.json](theme.json) and overlays the first valid external theme found through:

1. The `GIT_AGENT_THEME` environment variable.
2. A file beside the executable.
3. A file in the current working directory.

Place a `theme.local.json` beside that external theme file to override only selected tokens.
The repository-root `theme.local.json` is ignored by Git. Restart the app after editing theme
files. On macOS, prefer an external theme file over modifying the installed app bundle.

## Troubleshooting

- **Git or authentication fails:** check `git --version`, then try the failing Git operation
  in a terminal in the same repository. Review your Git credential helper or SSH configuration.
- **The diff or merge window does not open:** make sure all three binaries are present in
  the same directory. For a source checkout, rerun `cargo build --locked --bins`.
- **AI settings cannot be saved or decrypted:** check that your OS credential store is
  available and unlocked. Copying `config.json` to another machine does not copy its encryption key.
- **macOS/Linux logs are missing:** diagnostics currently attempt to write to `data/` beside
  the executable, which may not be writable in an installed application. For a reproducible
  report, use a source build in a writable checkout; its logs are under `target/debug/data/`.
- **Windows rebuild fails because an executable is in use:** close the app, diff viewer,
  and merge editor before building. The optional Windows development watcher handles restart.

When [reporting an issue](https://github.com/adoin/git-Agent/issues), include your OS and
architecture, app version, Git version, reproduction steps, and expected/actual behavior.
Use a small test repository when possible. Remove credentials, private URLs, and proprietary
code from screenshots and logs before posting them.

## Contributing

Bug reports, documentation improvements, platform testing, and focused pull requests are
welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) for setup, tests, the code layout,
and project conventions. Please discuss large changes in an issue first.

## Support

If Git Agent helps you in your daily work, you can support its continued development
with a voluntary tip on Ko-fi.

[Support Git Agent on Ko-fi](https://ko-fi.com/adoin)

Sponsorship is optional and does not unlock paid features or guarantee feature requests
or priority support. Bug reports, documentation improvements, and contributions are also
welcome.

## License

This repository does not currently include a `LICENSE` file. No open-source license is
declared here; contact the maintainer to clarify licensing before reusing or redistributing
the code.
