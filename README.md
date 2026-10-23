# Workspace Recovery

A local desktop utility that checkpoints open applications and browser tabs, then helps reopen a selected workspace after a crash, restart or forced shutdown.

**Status:** implementation specification only. This pack contains Markdown documentation, not a working application. Workspace Recovery is a working name.

## Agreed stack

| Area | Choice |
| --- | --- |
| Background engine | Rust |
| Desktop shell | Tauri 2 |
| Interface | React, TypeScript, Vite |
| Design system | Tailwind CSS 4, shadcn/ui, ReUI |
| Persistence | SQLite through rusqlite |
| Process and memory sampling | sysinfo |
| Browser extension | TypeScript, Chromium Manifest V3 |
| Browser transport | Native Messaging to a Rust bridge |
| Repository | pnpm workspace plus Cargo workspace |

**Provisional first target:** Windows 11 x64 and Chrome/Edge. The user has not confirmed their operating system, browser, editor or hardware. Keep these assumptions explicit; confirm before platform-specific implementation. macOS, Linux and Firefox are future adapters, not promised first-release support.

## Start here

1. [AGENTS.md](AGENTS.md): coding-agent instructions and non-negotiable rules.
2. [Product requirements](docs/PRODUCT.md): scope, user flows and acceptance criteria.
3. [Architecture](docs/ARCHITECTURE.md): processes, boundaries and lifecycle.
4. [Monorepo](docs/MONOREPO.md): proposed layout and tooling.
5. [Implementation plan](docs/IMPLEMENTATION_PLAN.md): ordered delivery milestones.

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

## Using this pack

Copy the contents of this directory into the repository root. Begin a coding-agent session with: “Read AGENTS.md and implement milestone 0 in docs/IMPLEMENTATION_PLAN.md. Report assumptions and validation.” Use one milestone at a time. No build commands in this pack are claimed to work until the corresponding manifests and scripts exist.
