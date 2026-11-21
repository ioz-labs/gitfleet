# GitFleet

[![Release](https://github.com/ioz-labs/gitfleet/actions/workflows/release.yml/badge.svg)](https://github.com/ioz-labs/gitfleet/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org/)
[![Safety](https://img.shields.io/badge/unsafe__code-forbid-brightgreen.svg)](#architecture)

**GitFleet** is an open-source, **100% pure-Rust** multi-repository Git desktop workspace manager. Built on native [`iced`](https://github.com/iced-rs/iced) with zero Electron or WebView overhead, GitFleet combines SmartGit-inspired commit graph & branch ergonomics with multi-repo fleet operations, an interactive conflict solver, encrypted secrets management, and an integrated PTY terminal.

---

## Features

- **Multi-Repository Fleet Workspace**
  - Organize dozens of repositories into nested groups with drag-and-drop, subfolder auto-discovery, dirty/ahead/behind filters, submodule navigation, and saved `.gitfleet` workspaces.
  - Run bulk operations (Fetch, Pull, Push, ordered dependency pushes) across selected repositories with live progress and cancellation.
- **Commit Graph & Branch Tree**
  - Multi-lane commit graph showing full upstream `origin` divergence (`origin = ▶ main`, `-10 ▶ develop`, `origin/develop`), Working Tree/Index node positioned above `HEAD`, reflog inspection, cherry-pick, revert, and soft/mixed/hard reset with per-repo undo history.
  - Nested branch tree with group checkboxes, tracking badges, worktree indicators, right-click `Add Branch… (F7)`, `Rename Branch… (F2)`, and `Merge… (Ctrl+M)` / `Rebase… (Ctrl+D)` / `Fast-Forward Merge`.
- **Smart Staging, Diff, Preview & Live Editor**
  - Unstaged & auto-showing Staged panels (Staged panel appears automatically once files are staged), color-coded file states, multi-file selection (`Shift+Click`, `Ctrl+Click`, `Ctrl+A`), and hunk-level staging/discard.
  - Selectable & copyable **Side-by-Side** (with synced scroll & connectors) and **Unified** diffs, rich **Preview** (Markdown, SVG, images, JSON, CSV, syntax-highlighted code), live **File Editor** (`Ctrl+S`), and a pop-out **Detached Viewer** window.
- **Live Merge Forecast & Interactive Conflict Solver**
  - Zero-touch in-memory conflict forecast (`git merge-tree --write-tree`) before merging, with **Create Merge-Commit** (`--no-ff`), **Merge to Working Tree** (`--no-commit --no-ff`), **Squash-Merge** (`--squash`), **Fast-Forward**, and **Rebase**.
  - Built-in hunk-by-hunk **Conflict Solver** supporting 2-way and `diff3`/`zdiff3` conflict markers: side-by-side Ours vs Theirs inspection, `Accept Ours` / `Accept Theirs` / `Accept Both` / `Theirs + Ours` / inline manual edit, Prev/Next navigation, EOL preservation, and one-click **Save & Mark Resolved** (`git add`).
- **Integrated Developer Tools**
  - **In-app PTY Terminal** with split panes, multiple shell profiles (PowerShell, CMD, Git Bash, WSL, zsh/bash), and copy/paste support.
  - **Fleet Search**: streaming cross-repository code search with instant cancellation.
  - **Secrets Vault & Pre-Commit Hygiene**: [`age`](https://age-encryption.org/)-encrypted `.gitfleet/secrets.age` vault for cross-machine `.env`/TOML/YAML sync, staged secret scanner with allowlists, oversized-file guard, and multi-layer `.gitignore` editor.

---

## Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+K` / `Ctrl+P` | Open Command Palette |
| `Ctrl+B` | Switch or Create Branch |
| `F7` | Add Branch |
| `F2` | Rename Local Branch |
| `Ctrl+M` | Merge Selected Branch (with Live Forecast) |
| `Ctrl+D` | Rebase onto Selected Branch |
| `Ctrl+T` | Stage Selected File(s) |
| `Ctrl+Shift+T` | Unstage Selected File(s) |
| `Ctrl+Z` | Discard Selected File(s) |
| `Ctrl+A` | Select All Files / Diff Text (context-aware) |
| `Ctrl+C` | Copy Selected Diff / Preview Text |
| `Ctrl+S` | Save Active Document in Editor |
| `Ctrl+Shift+F` | Toggle Fleet Search Drawer |
| `` Ctrl+` `` | Toggle Integrated Terminal Drawer |
| `Ctrl+Shift+~` / `Ctrl+Shift+5` | New Terminal Tab / Split Terminal Pane |
| `F5` / `Ctrl+Shift+R` | Refresh Selected Repo / Refresh All Repos |
| `Alt+↑` / `Alt+↓` | Previous / Next Repository |
| `Esc` | Close Active Modal or Overlay |

> On macOS, use `Cmd` in place of `Ctrl`.

---

## Installation

### Prebuilt Binaries (Recommended)

Download the latest release for your platform from the [GitHub Releases](https://github.com/ioz-labs/gitfleet/releases) page (verified via `SHA256SUMS.txt`):

| Platform | Architecture | Artifact |
| :--- | :--- | :--- |
| **Windows** | `x86_64` | `gitfleet-windows-x86_64.zip` |
| **Windows** | `ARM64` | `gitfleet-windows-arm64.zip` |
| **macOS** | Apple Silicon (`arm64`) | `gitfleet-macos-arm64.dmg` |
| **Linux** | `x86_64` | `gitfleet-linux-x86_64.tar.gz`, `gitfleet-linux-x86_64.deb` |
| **Linux** | `ARM64` | `gitfleet-linux-arm64.tar.gz`, `gitfleet-linux-arm64.deb` |

#### Platform Notes

- **Prerequisites**: `git` installed and available on `PATH`.
- **macOS**: Release `.app` bundles are ad-hoc signed. If macOS Gatekeeper warns on first launch, right-click `GitFleet.app` → **Open**, or run `xattr -dr com.apple.quarantine /Applications/GitFleet.app`.
- **Linux (Debian / Ubuntu)**:
  ```bash
  sudo apt install ./gitfleet-linux-x86_64.deb   # or gitfleet-linux-arm64.deb
  ```

---

## Building from Source

Requires [Rust](https://rustup.rs/) (edition 2024 / latest stable toolchain) and `git`.

### 1. Install System Dependencies (Linux only)

On Ubuntu / Debian:

```bash
sudo apt-get update && sudo apt-get install -y \
  pkg-config libxkbcommon-dev libwayland-dev libx11-dev \
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libfontconfig1-dev libssl-dev
```

On macOS, ensure Command Line Tools are installed (`xcode-select --install`). On Windows, install Visual Studio Build Tools with C++ support.

### 2. Build, Test & Run

```bash
git clone https://github.com/ioz-labs/gitfleet.git
cd gitfleet

# Strict warning-free check across all crates
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked

# Run unit tests
cargo test --workspace --lib --bins --locked

# Launch in development mode
cargo run -p gitfleet-ui

# Build optimized release binary (Windows release hides the console window)
cargo build --release --locked -p gitfleet-ui
```

### Automated Releases

GitHub Actions ([.github/workflows/release.yml](.github/workflows/release.yml)) runs strict checks (`-D warnings`), native unit tests, and packages all 5 platform/architecture targets automatically whenever a version tag matching `v*` is pushed (or when triggered manually via **Actions → Release → Run workflow**):

```bash
git tag v0.1.0
git push origin v0.1.0
```

---

## Architecture

GitFleet is organized as a modular 10-crate Cargo workspace enforcing `#![forbid(unsafe_code)]` at the workspace level and clean separation between UI-agnostic domain crates and the native GUI:

| Crate | Responsibility |
| :--- | :--- |
| `gitfleet-core` | Shared domain types, error model, broadcast `EventBus`, settings, command log, and per-repo undo stack |
| `gitfleet-workspace` | Multi-repo workspace persistence, lock/ownership management, folder discovery, nested groups, and prefix relocation |
| `gitfleet-git` | Core Git engine: status, staging/hunks, commits/amend/reset/revert, branches, tags, remotes, worktrees, Git Flow, submodules, merge/rebase forecast, 2-way/`diff3` conflict parser & solver, and bulk ops |
| `gitfleet-security` | `age`-encrypted secrets vault (`.gitfleet/secrets.age`), staged secret scanner + allowlists, oversized-file checks, and `.gitignore` layer manager |
| `gitfleet-search` | Streaming cross-repository Fleet Search with cancellation |
| `gitfleet-terminal` | In-app PTY session manager with shell profile detection and VT100 screen snapshots |
| `gitfleet-docs` | Optimistic-concurrency document editor with EOL preservation and repo doc scaffolding |
| `gitfleet-scheduler` | Background scheduled fetch, remote refresh orchestration, and filesystem watching |
| `gitfleet-app` | Unified `AppService` facade coordinating all subsystems |
| `gitfleet-ui` | Native multi-window desktop GUI (`iced::daemon`) for Host and Detached Viewer windows |

---

## Contributing

Contributions, bug reports, and feature ideas are welcome!

1. Fork the repository and create a branch from `main`.
2. Make your changes while keeping `#![forbid(unsafe_code)]` and zero compiler warnings intact.
3. Verify locally before opening a pull request:
   ```bash
   RUSTFLAGS="-D warnings" cargo check --workspace --all-targets --locked
   cargo test --workspace --lib --bins --locked
   ```
4. Open a pull request describing the change and why it helps.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in GitFleet shall be licensed under the **MIT License**, without any additional terms or conditions.

---

## License

GitFleet is licensed under the **MIT License** — see the [LICENSE](LICENSE) file for details.
