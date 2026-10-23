# Architecture decisions

## ADR-001: Rust engine and Tauri desktop

**Status:** accepted. Rust owns capture, persistence and restoration; Tauri hosts a React interface. Resource control and native integration matter because the triggering workload already stresses memory. Keep engine independent of webview lifetime.

## ADR-002: React, shadcn/ui and ReUI

**Status:** accepted by the user. React + TypeScript + Vite replaces the earlier Svelte suggestion. Tailwind CSS 4 and a consistent shadcn/ReUI primitive family provide one design system. Verify exact package compatibility during bootstrap.

## ADR-003: App and extension monorepo

**Status:** accepted by the user. pnpm manages frontend packages; Cargo manages Rust crates. Shared versioned schemas prevent the extension/agent contract drifting. A native host is infrastructure inside the same repository, not a third independent product.

## ADR-004: Local SQLite with one owner

**Status:** proposed implementation choice. No database server is needed. The background agent owns SQLite and exposes typed queries. WAL with FULL synchronization prioritizes checkpoint durability over maximum write throughput. Keep data small through deduplication and retention.

## ADR-005: Separate per-user agent

**Status:** proposed implementation choice. Capture survives UI/browser exits. A tray shell provides login startup and opens a webview on demand. No elevated or system-wide service; desktop-session discovery and user-specific resource access belong to the logged-in user context. On Linux, login startup uses an XDG autostart entry inside the graphical session.

## ADR-006: Native Messaging plus local sockets

**Status:** proposed implementation choice. Extension talks to a restricted native bridge, then the agent over a per-user local socket (Unix domain socket on Linux; named pipe in the later Windows adapter). Avoid public localhost endpoints, browser debugging ports and permission-heavy page inspection. Additional binaries require installer/registration tests.

## ADR-007: Explicit restore and durable action journal

**Status:** proposed implementation choice. The first release restores only after user selection. Capture retries are idempotent; external launch effects cannot be guaranteed exactly once. Interrupted actions are reconciled or labelled uncertain, never blindly retried.

## ADR-008: Windows and Chromium first

**Status:** superseded by ADR-009. Targeting one OS/browser family makes a useful first release achievable. Platform interfaces preserve extension points without claiming cross-platform support before implementation. The Windows design (named pipes, registry host registration, per-user installer) is retained as the Phase 7 Windows adapter.

## ADR-009: Linux/GNOME Wayland and Brave first

**Status:** accepted by the user (2026-10-23). The first target is the development machine: Fedora 44 x86_64, GNOME 50 on Wayland, 15 GiB RAM, Brave (native RPM) as the daily browser. Firefox follows on the same extension codebase. Editors: VS Code, then Zed; Android Studio only after a capability spike. File manager: Nautilus. Other Linux desktops, X11, Chrome/Chromium/Edge, Flatpak browsers, Windows and macOS are later adapters (Phase 7), not first-release claims.

Consequences: local IPC uses per-user Unix domain sockets verified with `SO_PEERCRED`; storage follows the XDG Base Directory specification; pairing credentials use the Secret Service API; window discovery on Wayland is an open spike (task 2.1) because Wayland does not expose a global window list to ordinary clients; stock GNOME has no tray (task 4.1). Native validation runs on the reference machine; CI uses Linux runners.

## ADR-010: Product name and identifiers

**Status:** accepted by the user (2026-10-23). Product name **Re-Open**, replacing the working name "Workspace Recovery". Tauri identifier `com.caffcore.reopen`; Native Messaging host name `com.caffcore.reopen.host`; binaries `reopen-agent` (background agent) and `reopen-host` (native bridge); XDG directory name `reopen`. Development builds use the directory name `reopen-dev`, host name `com.caffcore.reopen.dev.host` and separate extension IDs so they never share data, sockets or host registrations with production.

## ADR-011: Schema generator

**Status:** accepted (2026-10-23, task 0.9). Canonical JSON Schemas live in `packages/contracts/schema/*.schema.json`, written in **draft-07** (`definitions`, not `$defs`): typify 0.8 reads schemas through schemars 0.8, which models draft-07, and ajv and the `jsonschema` crate both support it.

- **Rust:** `crates/protocol-codegen` (a workspace binary, so no global install) runs typify 0.8 and formats with prettyplease into `crates/protocol/src/generated/mod.rs`. `additionalProperties: false` becomes `#[serde(deny_unknown_fields)]`. The module is `#[rustfmt::skip]` so `cargo fmt` cannot cause drift.
- **TypeScript:** `packages/contracts/scripts/generate.mjs` runs json-schema-to-typescript 16 for types and ajv 8 **standalone** code generation for validators into `packages/contracts/src/generated/`. Standalone validators are needed because the extension's Manifest V3 CSP forbids the runtime code generation (`new Function`) that ajv otherwise uses. The generator inlines ajv's one CommonJS helper so the output is pure ESM with no runtime dependency, and fails if any `require(` remains.
- **Validation** is separate from generated types. `reopen-protocol` (`decode_envelope`, `decode_error_response`) and `@workspace/contracts` (`decodeEnvelope`, `decodeErrorResponse`) both: reject messages over 256 KiB before parsing (`LIMIT_EXCEEDED`), parse JSON, reject other protocol versions before schema validation (`VERSION_UNSUPPORTED`), then validate against the schema (`INVALID_PAYLOAD`, unknown fields included). Error details name only the schema path, never input values.
- **Shared fixtures** in `fixtures/protocol/<group>/cases.json` are run by both test suites; large inputs are generated in memory with identical rules.
- `pnpm contracts:generate` regenerates both sides; `pnpm contracts:check` fails when committed output differs. If a generator cannot represent a required construct, simplify the schema rather than hand-editing generated code, and record the change here.

## ADR-012: shadcn primitive family and ReUI style

**Status:** accepted (2026-10-23, task 0.8). Style `base-nova`: the nova style on Base UI (`@base-ui/react`), neutral base colour, CSS variables, Lucide icons, Geist variable font bundled locally through Fontsource (no remote fonts, so the CSP stays `'self'`).

- Shared primitives and theme live in `packages/ui` (`@workspace/ui`): `src/styles/globals.css` (Tailwind CSS 4 theme tokens for light and dark), `src/lib/utils.ts`, shadcn components in `src/components/`, ReUI components in `src/components/reui/`. Apps import them as `@workspace/ui/components/<name>` and `@workspace/ui/components/reui/<name>`.
- Both `packages/ui/components.json` and `apps/desktop/components.json` declare the free `@reui` registry. Run installs from `apps/desktop` with the pinned CLI (`pnpm dlx shadcn@4.22.1 add <name> --yes`, or `@reui/<name>`); the CLI writes shared components into `packages/ui`.
- Installed component files are vendored: never edit them to restyle (change theme tokens instead), and they are excluded from Prettier and import ordering so a reinstall stays diff-free.
- The ReUI MCP server URL carries `?style=base-nova` in every agent client config so its docs and previews match Base UI.
- Premium ReUI items are not used without an explicit licence decision.

## ADR-013: MIT licence

**Status:** accepted by the user (2026-10-23). The repository is released under the MIT licence, copyright holder Caff-Core. Dependencies must have licences compatible with MIT distribution; record any exception here before adding it.

## ADR-014: Toolchain and dependency versions

**Status:** accepted (2026-10-23). Versions were checked against the npm registry, crates.io, the Node.js release index and the Rust stable channel manifest on 2026-10-23. Committed manifests use exact versions; lockfiles are committed when packages are added (tasks 0.5–0.8).

| Component                             | Pinned version                | Notes                                                                                                                             |
| ------------------------------------- | ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Node.js                               | 24.22.1                       | Active LTS ("Krypton"). Node 26 is Current, not yet LTS; revisit when it enters LTS.                                              |
| pnpm                                  | 12.15.0                       | `packageManager` field.                                                                                                           |
| Rust                                  | 1.99.1                        | Latest stable. Tauri 2.12 needs 1.90 or newer.                                                                                    |
| `tauri` / `tauri-build` crates        | 2.12.5 / 2.7.4                | Latest 2.x. crates.io `latest` is 3.0.0-alpha; do not use it.                                                                     |
| `@tauri-apps/cli` / `@tauri-apps/api` | 2.12.4                        | npm and crate must share the same major.minor (2.12).                                                                             |
| React / React DOM                     | 19.3.1                        | Required by ReUI (React + Tailwind CSS v4).                                                                                       |
| TypeScript                            | 6.0.3                         | TypeScript 7.0 is `latest`, but typescript-eslint 8.72 supports `>=4.8.4 <6.1.0`. Move to 7.x when typescript-eslint supports it. |
| Vite / `@vitejs/plugin-react`         | 8.3.5 / 6.1.2                 | plugin-react 6 requires Vite 8.                                                                                                   |
| Tailwind CSS / `@tailwindcss/vite`    | 4.3.3                         | Vite plugin supports Vite 5–8.                                                                                                    |
| shadcn CLI                            | 4.22.1                        | Run as `pnpm dlx shadcn@4.22.1`, replacing `shadcn@latest` in commands returned by the ReUI MCP.                                  |
| ReUI                                  | registry (unversioned source) | Free `@reui` registry items are copied as source; Base UI and Radix builds exist (style choice in ADR-012).                       |
| Vitest                                | 5.0.5                         | Supports Vite 6–8 and `@types/node` 24.                                                                                           |
| ESLint / typescript-eslint / Prettier | 10.13.0 / 8.72.0 / 3.9.10     | Confirm plugin compatibility in task 0.5.                                                                                         |

Upgrades happen in dedicated `chore` PRs that re-check these constraints and update this table.

## ADR-015: Application versioning

**Status:** accepted (2026-10-23). Semantic Versioning, one application version for everything that ships together.

- `[workspace.package].version` in the root `Cargo.toml` is the source of truth for every Rust binary (desktop shell, agent, native host). `tauri.conf.json` has no `version` key, so Tauri inherits it.
- Shipped JavaScript manifests carry the same version: `apps/desktop/package.json` now, the browser extension `package.json` and generated `manifest.json` from task 3.4. `scripts/versions.test.mjs` (run by `pnpm test`) fails on any mismatch.
- Bump in the PR that changes shipped behaviour, not in documentation, CI or tooling-only PRs. Before 1.0: minor for new user-visible features or breaking changes to stored data or protocol; patch for fixes. The first tagged release (task 6.5) sets the starting public version.
- Internal crates and packages (`publish = false`, `private`) do not version independently.
- `protocol_version`, the database schema version and the extension compatibility matrix are separate counters (`docs/PROTOCOL.md`, `docs/DATA_MODEL.md`, task 6.4); bumping the app version does not change them, and changing them requires an app version bump.
- Release tags are `vX.Y.Z`.

## Open decisions

- Window enumeration approach on GNOME Wayland (task 2.1).
- Tray fallback on stock GNOME (task 4.1).
- Signing and distribution approach for RPM, AppImage and extension packages.
- Verify editor/project discovery for VS Code, Zed and Android Studio (tasks 5.1–5.3).
- Confirm schema generator (task 0.9) and shadcn/ReUI primitive family (task 0.8).
- Calibrate memory thresholds and resource budgets on the 15 GiB reference machine.
- Decide later whether encrypted export or additional platforms justify their complexity.
