# GitFleet

Open-source, 100% pure-Rust multi-repository Git desktop workspace manager.

## Architecture

GitFleet is structured as a modular Cargo workspace with strict separation between the UI-agnostic domain/engine crates and the native Rust GUI (`iced`):

- `gitfleet-core` — Shared domain types, error types, broadcast `EventBus`, settings, command log, and per-repo undo stack.
- `gitfleet-workspace` — Multi-repo workspace persistence, lock/ownership management, folder discovery, groups, locate/relocate prefix.
- `gitfleet-git` — Core Git engine (status cache + priority queue, staging/hunks, commits/amend/reset/revert, branches, tags + release triggers, remotes, worktrees, Git Flow, submodules, merge/rebase forecast, conflicts, bulk ops & ordered push).
- `gitfleet-security` — Age-based encrypted secrets vault (`.gitfleet/secrets.age`), staged secret scanner + allowlists, staged/oversized file checks, and `.gitignore` layer management.
- `gitfleet-search` — Streaming cross-repository Fleet Search with cancellation.
- `gitfleet-terminal` — In-app PTY session manager with shell profiles and backpressure acknowledgement.
- `gitfleet-docs` — Capability-based document editor (`documentId` + optimistic concurrency + EOL preservation) and repo doc scaffolding.
- `gitfleet-scheduler` — Background scheduled fetch, remote refresh orchestration, and filesystem watching.
- `gitfleet-app` — Unified `AppService` facade wiring all subsystems together.
- `gitfleet-ui` — Native Rust multi-window desktop GUI (`iced::daemon`) supporting the main Host window and a Detached Viewer window.

## Quick Start

```bash
cargo check --workspace
cargo test --workspace
cargo run -p gitfleet-ui
```

## Cross-Platform Builds (Windows / macOS / Linux)

### Automated Builds via GitHub Actions (Recommended)

- **Tag-Triggered CI & Release** ([.github/workflows/release.yml](.github/workflows/release.yml)): runs `cargo check` (`-D warnings`), native unit tests, and packages release artifacts when pushing a `v*` tag (or triggered manually via `workflow_dispatch`):
  - Windows (`x86_64`): `gitfleet-windows-x86_64.zip`
  - Windows (`arm64`): `gitfleet-windows-arm64.zip`
  - macOS Apple Silicon (`arm64`): `gitfleet-macos-arm64.dmg`
  - macOS Intel (`x86_64`): `gitfleet-macos-x86_64.dmg`
  - Linux (`x86_64`): `gitfleet-linux-x86_64.tar.gz` and `gitfleet-linux-x86_64.deb`
  - Linux (`arm64`): `gitfleet-linux-arm64.tar.gz` and `gitfleet-linux-arm64.deb`

Trigger a release by pushing a version tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Or run manually in GitHub via **Actions → Release → Run workflow** to download build artifacts without publishing a tag.

### Local Release Builds

- **Windows**:
  ```powershell
  cargo build --release -p gitfleet-ui
  # Output: target\release\gitfleet.exe
  ```
- **macOS (Apple Silicon & Intel)**:
  ```bash
  xcode-select --install
  cargo build --release -p gitfleet-ui
  # Output: target/release/gitfleet
  ```
- **Linux (Ubuntu / Debian)**:
  ```bash
  sudo apt-get update && sudo apt-get install -y \
    pkg-config libxkbcommon-dev libwayland-dev libx11-dev \
    libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libfontconfig1-dev libssl-dev
  cargo build --release -p gitfleet-ui
  # Output: target/release/gitfleet
  ```
