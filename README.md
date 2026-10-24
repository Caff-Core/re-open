# Re-Open

A local desktop utility that checkpoints open applications and browser tabs, then helps reopen a selected workspace after a crash, restart or forced shutdown.

**Status:** Phase 0 (repository and tooling). The repository currently contains documentation only, not a working application. Progress is tracked in [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md), mirrored to GitHub issues and the [Re-Open project board](https://github.com/users/Caff-Core/projects/1).

## Agreed stack

| Area                        | Choice                                                   |
| --------------------------- | -------------------------------------------------------- |
| Background engine           | Rust                                                     |
| Desktop shell               | Tauri 2                                                  |
| Interface                   | React, TypeScript, Vite                                  |
| Design system               | Tailwind CSS 4, shadcn/ui, ReUI                          |
| Persistence                 | SQLite through rusqlite                                  |
| Process and memory sampling | sysinfo                                                  |
| Browser extension           | TypeScript, Manifest V3 (Brave first, then Firefox)      |
| Browser transport           | Native Messaging to a Rust bridge                        |
| Local IPC (Linux)           | Per-user Unix domain sockets verified with `SO_PEERCRED` |
| Repository                  | pnpm workspace plus Cargo workspace                      |

**First target:** Fedora 44 x86_64, GNOME 50 on Wayland, Brave (native RPM), then Firefox. Editors VS Code and Zed; file manager Nautilus. See [ADR-009](docs/DECISIONS.md#adr-009-linuxgnome-wayland-and-brave-first) and the target matrix in the [implementation plan](docs/IMPLEMENTATION_PLAN.md#target-matrix). X11, KDE Plasma, Chrome/Chromium/Edge, Flatpak browsers, Windows 11 and macOS are later adapters (Phase 7), not first-release support.

## Start here

1. [AGENTS.md](AGENTS.md): coding-agent instructions and non-negotiable rules.
2. [Product requirements](docs/PRODUCT.md): scope, user flows and acceptance criteria.
3. [Architecture](docs/ARCHITECTURE.md): processes, boundaries and lifecycle.
4. [Monorepo](docs/MONOREPO.md): proposed layout and tooling.
5. [Implementation plan](docs/IMPLEMENTATION_PLAN.md): phased tasks and tracking.

## Reference documents

- [Data model and durability](docs/DATA_MODEL.md)
- [Protocols and contracts](docs/PROTOCOL.md)
- [Capture and restoration](docs/CAPTURE_RESTORE.md)
- [Browser extension](docs/BROWSER_EXTENSION.md)
- [Desktop interface](docs/UI.md)
- [Security and privacy](docs/SECURITY.md)
- [Testing and release](docs/TESTING_RELEASE.md)
- [Architecture decisions](docs/DECISIONS.md)
- [Official technical references](docs/REFERENCES.md)

## Important limits

This tool reopens resources; it does not checkpoint process memory, preserve unsaved edits, recover private browsing, guarantee authenticated browser sessions, or resume an interrupted compilation. No software running on the same frozen PC can guarantee a final capture. Recovery uses the last successfully committed checkpoint.

No cloud service, account, hosted backend or database server is required. SQLite is an intentional exception to a server-database stack: this is a single-user local application.

## Prerequisites

Exact versions are pinned in the repository and recorded in [ADR-014](docs/DECISIONS.md#adr-014-toolchain-and-dependency-versions).

| Tool    | Version                            | Pinned by                                                              |
| ------- | ---------------------------------- | ---------------------------------------------------------------------- |
| Node.js | 24.22.1 (LTS)                      | `.nvmrc` (`nvm install && nvm use`), `engines` in `package.json`       |
| pnpm    | 12.15.0                            | `packageManager` in `package.json` (pnpm switches to it automatically) |
| Rust    | 1.99.1 with `rustfmt` and `clippy` | `rust-toolchain.toml` (rustup installs it automatically)               |

Fedora 44 system packages for Tauri 2 (WebKitGTK 4.1):

```sh
sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel openssl-devel \
  librsvg2-devel libappindicator-gtk3-devel libxdo-devel file curl
sudo dnf group install c-development
```

`libappindicator-gtk3-devel` is only needed to build tray support; GNOME shows tray icons only with an AppIndicator extension installed (see [Desktop interface](docs/UI.md)).

## Running the agent (development)

`pnpm dev:agent` runs `reopen-agent --dev run` against the isolated `reopen-dev` directories with the fake desktop provider (real adapters arrive in Phase 2). In another terminal: `cargo run -q -p reopen-agent -- --dev status`, `capture`, `history`, or `checkpoint show <id>`. Exit codes: 3 another agent is running, 4 storage is damaged or newer (capture disabled), 5 no agent is running.

## Working on the project

Pick the next open task in [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md); each task has a GitHub issue and one branch/PR. Coding agents start from [AGENTS.md](AGENTS.md). No build commands are claimed to work until the corresponding manifests and scripts exist (tasks 0.4–0.10).

## Licence

[MIT](LICENSE), copyright Caff-Core.
