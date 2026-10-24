# Implementation plan

Phased delivery plan for Re-Open. Every phase starts **not implemented**. Tick a box only with actual evidence (merged PR, executed check, recorded measurement). Deliver useful vertical slices in order; do not start a phase before the previous phase's exit criteria pass unless this file says otherwise.

## How tracking works

| Plan item | GitHub object                                                                                                    | Notes                                               |
| --------- | ---------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| Phase N   | Milestone `Phase N: <name>`                                                                                      | Closed when every exit criterion is ticked          |
| Task N.M  | Issue, labelled `phase:N`, `type:*`, `area:*`                                                                    | One issue = one branch = one PR                     |
| Sub-task  | Checkbox in the issue body and in this file                                                                      | Tick both when done                                 |
| Status    | [Project board `Re-Open`](https://github.com/users/Caff-Core/projects/1) (Todo / In Progress / In Review / Done) | PRs use `Closes #<issue>` so merging moves the card |

**Branches:** `<type>/<issue#>-<slug>`, for example `feat/14-sqlite-migrations`, `fix/31-stale-epoch`, `docs/3-linux-retarget`. Types: `feat`, `fix`, `chore`, `docs`, `ci`, `refactor`, `test`; `spike` tasks use `docs/` because their output is a recorded decision.

**Commits and PRs:** Conventional Commits (`feat(storage): add migration runner`). No co-author trailers. Squash merge; the PR title is the squashed commit message. `main` is protected: PR required, CI must pass, no required approvals (solo project).

**Updating this file:** the PR that completes a sub-task ticks it here in the same change. Each task heading links its issue.

Status legend: `[ ]` not started, `[x]` done with evidence. Use the issue/PR for in-progress notes rather than this file.

## Target matrix

| Area            | First target (tested on the development machine) | Second                               | Later (Phase 7 / backlog)                       |
| --------------- | ------------------------------------------------ | ------------------------------------ | ----------------------------------------------- |
| OS              | Fedora 44, x86_64                                | Other Linux distributions with GNOME | X11, KDE Plasma, Windows 11, macOS              |
| Desktop session | GNOME 50 on Wayland                              | GNOME on Xorg/XWayland               | KWin, other compositors                         |
| Browser         | Brave (native RPM)                               | Firefox (native RPM)                 | Chrome, Chromium, Edge native; Flatpak browsers |
| Editors         | VS Code                                          | Zed                                  | Android Studio (after capability spike)         |
| File manager    | Nautilus                                         |                                      | Dolphin, Explorer, Finder                       |
| Hardware        | 15 GiB RAM reference machine                     |                                      |                                                 |

Identifiers: product **Re-Open**, Tauri identifier `com.caffcore.reopen`, native-messaging host `com.caffcore.reopen.host`, binaries `reopen-agent` and `reopen-host`.

Linux transport: per-user Unix domain sockets in `$XDG_RUNTIME_DIR/reopen/` (directory `0700`), caller verified with `SO_PEERCRED` against the current UID. This replaces the Windows named-pipe design for the first target; the protocol and framing are unchanged.

---

## Phase 0 — Repository, tooling and bootstrap

Goal: a working monorepo with real command contracts, CI, a Tauri + React desktop shell rendering shadcn/ui + ReUI components, a schema-generation pipeline, and GitHub tracking. No product logic.

### 0.1 Repository baseline — `chore` · [#1](https://github.com/Caff-Core/re-open/issues/1)

- [x] Commit the existing documentation pack and agent configuration to `main` (initial commit pushed directly; the repository is empty so no PR is possible)
- [x] Add `LICENSE` (MIT, copyright holder Caff-Core)
- [x] Add `.gitignore` (Node, pnpm, Vite, Rust `target/`, Tauri bundles, `.env*.local`, editor files, local dev data directories)
- [x] Add `.editorconfig` and `.gitattributes` (LF line endings, generated files marked `linguist-generated`)
- [x] Set remote `git@github.com:Caff-Core/re-open.git` and push
- [x] Enable branch protection on `main`: PR required, required status checks (added in 0.10), no force-push, no deletion, 0 required approvals
- [x] Repository settings: squash merge only, PR title as default commit message, auto-delete merged branches

### 0.2 GitHub tracking setup — `chore` · [#2](https://github.com/Caff-Core/re-open/issues/2)

- [x] Create labels: `phase:0`…`phase:7`; `type:feat|fix|chore|docs|ci|refactor|test|spike`; `area:engine|storage|protocol|browser|extension|desktop|ui|platform-linux|platform-windows|platform-macos|ci|docs`
- [x] Create milestones for Phases 0–7
- [x] Create Project (v2) board `Re-Open` with Status (Todo / In Progress / In Review / Done) and Phase fields; link it to the repository
- [x] Create one issue per task in this file with its sub-task checklist; add every issue to the board
- [x] Add `.github/ISSUE_TEMPLATE/task.yml` (phase, area, sub-tasks, exit evidence) and `bug.yml`
- [x] Add `.github/pull_request_template.md` (Conventional Commit title reminder, `Closes #`, checks run, invariants affected, docs updated)
- [x] Replace every `#TBD` in this file with the created issue numbers

### 0.3 Retarget documentation to Linux/GNOME first — `docs` · [#3](https://github.com/Caff-Core/re-open/issues/3)

- [x] `README.md` and `AGENTS.md`: product name Re-Open, Linux/GNOME + Brave first, Windows as a later adapter
- [x] `docs/DECISIONS.md`: ADR-008 marked superseded; add ADR-009 Linux/GNOME/Brave first, ADR-010 name and identifiers, ADR-011 schema generator, ADR-012 shadcn primitive family and ReUI style, ADR-013 MIT licence
- [x] `docs/ARCHITECTURE.md` and `docs/PROTOCOL.md`: Unix sockets, `SO_PEERCRED`, socket paths; named pipes moved to the Windows adapter section
- [x] `docs/DATA_MODEL.md`: XDG data/state/runtime directories; isolated development directory
- [x] `docs/BROWSER_EXTENSION.md`: Brave and Firefox host-manifest locations and manifest differences; Flatpak browsers out of scope
- [x] `docs/SECURITY.md`: Secret Service (libsecret) for pairing credentials on Linux
- [x] `docs/MONOREPO.md`: `platform-linux` crate; `package:linux` script; `package:windows` deferred
- [x] `docs/TESTING_RELEASE.md`: Linux CI runners, native validation on the Fedora reference machine
- [x] `docs/UI.md`: GNOME tray limitation noted (see 4.1)

### 0.4 Toolchain pinning — `chore` · [#4](https://github.com/Caff-Core/re-open/issues/4)

- [x] Verify current stable versions and compatibility of Node, pnpm, Rust, Tauri 2, React, Vite, Tailwind CSS 4, shadcn CLI and ReUI against official documentation; record the result in `docs/DECISIONS.md`
- [x] `rust-toolchain.toml` (exact stable version, `rustfmt` and `clippy` components)
- [x] `.nvmrc` (read by nvm and `actions/setup-node`), root `package.json` `packageManager` and `engines` fields
- [x] Record exact versions in `README.md` prerequisites, including Fedora system packages (`webkit2gtk4.1-devel`, `gtk3-devel`, `libsoup3-devel`, `openssl-devel`, `librsvg2-devel`, `libappindicator-gtk3-devel`, `libxdo-devel`)

### 0.5 pnpm workspace and TypeScript tooling — `chore` · [#5](https://github.com/Caff-Core/re-open/issues/5)

- [x] `pnpm-workspace.yaml` covering `apps/*` and `packages/*`; commit `pnpm-lock.yaml`
- [x] Shared strict `tsconfig.base.json` (`strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`)
- [x] ESLint flat config (TypeScript, React hooks, import rules) and Prettier
- [x] Vitest configured at the root
- [x] Root scripts implemented: `lint`, `typecheck`, `test`, `build`; `contracts:generate`, `contracts:check` and `dev:desktop` exist as failing placeholders until tasks 0.9 and 0.7 implement them
- [x] Scripts for later phases (`dev:agent`, `dev:extension`, `package:linux`) exist and exit non-zero with a message naming the phase that implements them
- [x] No unbounded `latest` versions in committed manifests

### 0.6 Cargo workspace — `chore` · [#6](https://github.com/Caff-Core/re-open/issues/6)

- [x] Root `Cargo.toml` workspace with `crates/*` (`apps/desktop/src-tauri` joins in 0.7); shared `[workspace.package]`, `[workspace.dependencies]` and `[workspace.lints]` (`unsafe_code = "forbid"` by default, overridden only in platform crates)
- [x] `crates/core` (package `reopen-core`, since `core` shadows the standard library crate) with a minimal domain type and one unit test (smoke only; real types in Phase 1)
- [x] `rustfmt.toml`, clippy configuration; commit `Cargo.lock`
- [x] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass locally

### 0.7 Tauri 2 + React desktop scaffold — `feat` · [#7](https://github.com/Caff-Core/re-open/issues/7)

- [x] Scaffold `apps/desktop` (React + TypeScript + Vite) and `apps/desktop/src-tauri` following current Tauri 2 documentation
- [x] Add `apps/desktop/src-tauri` to the Cargo workspace members
- [x] Set identifier `com.caffcore.reopen`, product name, window defaults
- [x] Lock down capabilities: no shell, fs, http or opener plugins; strict CSP; no remote content or arbitrary navigation
- [x] One typed example command (`app_version`) to prove the command path; no business logic in the shell
- [x] `pnpm dev:desktop` opens a window on GNOME Wayland; document any WebKitGTK environment workaround required on this machine (none needed on the Intel GPU reference machine)
- [x] `pnpm build` builds the frontend; `cargo build -p` for the shell succeeds

### 0.8 Tailwind CSS 4, shadcn/ui and ReUI via MCP — `feat` · [#8](https://github.com/Caff-Core/re-open/issues/8)

- [x] Choose the primitive family and style (default proposal `base-nova`; confirm via ReUI MCP `get_agent_skill` and shadcn docs); record in ADR-012
- [x] Tailwind CSS 4 with the Vite plugin; theme tokens and light/dark mode
- [x] `packages/ui` as `@workspace/ui` holding shared primitives and tokens; `components.json` configured for the monorepo layout
- [x] Add the `@reui` registry to `components.json` using the config returned by ReUI MCP `get_project_context` (free registry, no licence key)
- [x] Set the ReUI MCP style (`?style=base-nova` URL parameter, equivalent to the `X-Reui-Style` header) in `.mcp.json`, `.cursor/mcp.json`, `opencode.json` and `.gemini/settings.json`
- [x] Install components only through commands returned by ReUI MCP `get_install_command` with a pinned shadcn CLI version: shadcn `button` and ReUI `badge` as smoke components
- [x] Smoke page renders both components using semantic tokens in light and dark themes
- [x] Run ReUI MCP `validate_usage` and `get_audit_checklist` on the smoke page; fix every finding

### 0.9 Contracts and schema generation pipeline — `feat` · [#9](https://github.com/Caff-Core/re-open/issues/9)

- [x] `packages/contracts/schema/` with the canonical protocol envelope schema (version 1) and error schema
- [x] Generate Rust types with `typify` into `crates/protocol` and TypeScript types with `json-schema-to-typescript` into `packages/contracts`; commit generated output
- [x] Runtime validation wired: `ajv` standalone validators in TypeScript (no runtime code generation, MV3-safe), `jsonschema` plus explicit size and unknown-field checks in Rust
- [x] `fixtures/protocol/` with valid, malformed, missing-field, unknown-version and oversized envelope fixtures; tests on both sides consume the same fixtures
- [x] `pnpm contracts:generate` regenerates; `pnpm contracts:check` fails on drift

### 0.10 Continuous integration — `ci` · [#10](https://github.com/Caff-Core/re-open/issues/10)

- [x] `.github/workflows/ci.yml` on `ubuntu-latest` for PRs and pushes to `main`
- [x] Job `node`: `pnpm install --frozen-lockfile`, `lint`, `typecheck`, `test`, `build`, TypeScript half of `contracts:check`
- [x] Job `rust`: install Tauri system dependencies, `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`, Rust half of `contracts:check`
- [x] Caching for pnpm store and Cargo; actions pinned to commit SHAs
- [x] Mark both jobs as required status checks on `main`

### Phase 0 exit criteria

- [x] Target matrix documented and ADRs 009–013 recorded
- [x] Lockfiles committed; no unbounded dependency versions
- [x] Every root command in `docs/MONOREPO.md` exists and either works or fails with a useful phase message
- [x] Core crate, contracts fixtures and frontend smoke checks pass locally and in CI
- [x] Desktop window renders shadcn + ReUI smoke components on GNOME Wayland
- [x] Every Phase 0 issue closed; board reflects reality

---

## Phase 1 — Durable engine with fake adapters

Goal: platform-independent agent that captures from fake providers, persists checkpoints atomically, protects the previous session and exposes a CLI.

### 1.1 Core domain types — `feat` · [#11](https://github.com/Caff-Core/re-open/issues/11)

- [x] IDs, timestamps (UTC) and monotonic deadlines
- [x] Checkpoint, resource kinds, completeness, source states (`complete`, `partial`, `unavailable`, `stale`, `excluded`, `unsupported`)
- [x] Adapter capability model and versioned resource payloads
- [x] Unit tests for state and completeness rules

### 1.2 Storage and migrations — `feat` · [#12](https://github.com/Caff-Core/re-open/issues/12)

- [x] `crates/storage` with rusqlite; foreign keys, WAL, `synchronous=FULL`, bounded busy timeout
- [x] Numbered migration runner with `schema_migrations`; schema v1 tables from `docs/DATA_MODEL.md`; default workspace
- [x] Refuse writes on a newer schema; never create an empty replacement on corruption
- [x] Verified backup via the SQLite backup API before destructive migrations
- [x] Migration tests against prior versions

### 1.3 Single bounded writer — `feat` · [#13](https://github.com/Caff-Core/re-open/issues/13)

- [x] Dedicated writer thread with a bounded request channel; queries through the storage module
- [x] Atomic checkpoint transaction (checkpoint, sources, resources); success published only after commit
- [x] Content hash from meaningful state; unchanged state updates observation freshness only

### 1.4 Sessions and recovery protection — `feat` · [#14](https://github.com/Caff-Core/re-open/issues/14)

- [x] Session records with clean-exit marker
- [x] Startup transaction creates the pending recovery candidate and pin before capture starts
- [x] Multiple unresolved candidates; dismiss and resolve release only their own pin

### 1.5 Capture scheduler and fake provider — `feat` · [#15](https://github.com/Caff-Core/re-open/issues/15)

- [x] Tokio scheduler, 60-second default, serialised cycles with coalesced ticks
- [x] Provider deadlines (5 s each, 10 s total) producing partial status with stable error codes
- [x] `platform` crate traits plus a configurable fake provider (complete, partial, timeout, empty)

### 1.6 Manual checkpoints, pins and retention — `feat` · [#16](https://github.com/Caff-Core/re-open/issues/16)

- [x] "Save now" explicit checkpoints; pin/unpin with multiple reasons
- [x] Retention by age and count on eligible unpinned automatic checkpoints only, as a separate bounded transaction
- [x] Storage budget warning when pins prevent compliance

### 1.7 Memory sampling interface — `feat` · [#17](https://github.com/Caff-Core/re-open/issues/17)

- [x] Fakeable memory sampler trait; `sysinfo` implementation; separate 5-second schedule
- [x] Linux pressure stall information (`/proc/pressure/memory`) recorded as an additional signal where available

### 1.8 Agent binary, local IPC and CLI — `feat` · [#18](https://github.com/Caff-Core/re-open/issues/18)

- [x] `crates/agent` binary `reopen-agent` with per-user singleton lock and readiness handshake
- [x] XDG data directory; isolated development directory used by `pnpm dev:agent`
- [x] Desktop-role Unix socket endpoint (`0600`, `SO_PEERCRED` UID check, 4-byte little-endian framing, 16 outstanding requests)
- [x] CLI subcommands: `status`, `capture`, `history`, `checkpoint show`
- [x] Redacted `tracing` logging with rotation (3 × 1 MiB); no addresses, paths or titles

### 1.9 Durability test suite — `test` · [#19](https://github.com/Caff-Core/re-open/issues/19)

- [x] Kill the agent process at transaction boundaries; prior or new checkpoint survives, never half-written
- [x] Repeated restarts keep every pending candidate protected
- [x] Full-disk, read-only and corrupt database simulations
- [x] Retention and migration tests

### Phase 1 exit criteria

- [x] Kill-during-write, previous-session preservation, full-disk/error simulation, retention and migration tests pass
- [x] CLI shows complete, partial and stale fixtures honestly
- [ ] Phase 1 issues closed

---

## Phase 2 — Linux/GNOME app capture and safe app-only restore

Goal: discover visible GNOME applications on Wayland and reopen user-selected apps safely.

### 2.1 Spike: window enumeration on GNOME Wayland — `spike` · [#20](https://github.com/Caff-Core/re-open/issues/20)

- [x] Evaluate `org.gnome.Shell.Introspect` availability and access rules on GNOME 50
- [x] Evaluate a minimal companion GNOME Shell extension exposing a read-only D-Bus window list
- [x] Evaluate XWayland/X11 `_NET_CLIENT_LIST` coverage and a process + `.desktop` matching fallback
- [x] Record capability matrix and chosen approach as an ADR

### 2.2 `platform-linux` application identity — `feat` · [#21](https://github.com/Caff-Core/re-open/issues/21)

- [ ] Resolve app identity from freedesktop `.desktop` entries (system, user, Flatpak exports)
- [ ] Validate executable references; label Flatpak and Snap apps by app ID
- [ ] Exclude system/background processes and the configured exclusion list

### 2.3 Window discovery adapter — `feat` · [#22](https://github.com/Caff-Core/re-open/issues/22)

- [ ] Primary provider `linux.gnome-shell`: D-Bus client for `com.caffcore.ReOpen.Windows1.GetWindows` (ADR-016), checking the `Version` property; extension prototype in `apps/gnome-shell-extension`
- [ ] Fallback provider `linux.systemd-units`: app IDs from `app-gnome-*`, `app-flatpak-*` and D-Bus-activated user units, reported as `partial` with a limited-visibility error
- [ ] Development install script for the extension (copy to the user extensions directory; the user enables it and logs in again)
- [ ] Headless extension check (`scripts/test-gnome-extension.sh`) documented in TESTING_RELEASE
- [ ] Bounded probe workers and timeouts; unsupported capability reported, never guessed

### 2.4 Conservative launch adapter — `feat` · [#23](https://github.com/Caff-Core/re-open/issues/23)

- [ ] Launch from validated desktop entries with argument arrays (field codes stripped; never a shell string); `flatpak run <app-id>` for Flatpak apps
- [ ] Reject changed or missing executables instead of searching for same-named binaries

### 2.5 Restore plans and action journal — `feat` · [#24](https://github.com/Caff-Core/re-open/issues/24)

- [ ] `restore.preview` with plan ID, selections, warnings and expiry; invalidation on exclusion/resource changes
- [ ] Persisted attempt marker before each side effect; result afterwards
- [ ] Serial launches with a 2-second minimum gap, memory check before each action, cancellation
- [ ] Reconcile `launching` actions on restart as `uncertain`; never replay
- [ ] App-level duplicate detection (`already_open`)

### 2.6 Native validation on the reference machine — `test` · [#25](https://github.com/Caff-Core/re-open/issues/25)

- [ ] Explicitly selected apps reopen; unselected apps do not
- [ ] Changed or missing executables fail safely
- [ ] Restart during launch does not duplicate actions
- [ ] Unsupported apps are labelled

### Phase 2 exit criteria

- [ ] Native tests above pass on Fedora 44 / GNOME Wayland
- [ ] Phase 2 issues closed

---

## Phase 3 — Browser vertical slice (Brave, then Firefox)

Goal: Brave profiles captured end to end through the extension and native host, with user-approved browser restore; Firefox follows on the same codebase.

### 3.1 Browser protocol schemas — `feat` · [#26](https://github.com/Caff-Core/re-open/issues/26)

- [ ] Schemas for every browser operation in `docs/PROTOCOL.md`, limits encoded
- [ ] Shared fixtures: valid, malformed, duplicate, oversized, out-of-order, stale epoch

### 3.2 Native host bridge — `feat` · [#27](https://github.com/Caff-Core/re-open/issues/27)

- [ ] `crates/native-host` binary `reopen-host`: native-endian framing on stdio, oversize rejection before allocation, redacted stderr
- [ ] Forward to the agent's browser-only socket; report unavailable when the agent is absent; never launch anything

### 3.3 Agent browser endpoint and ingestion — `feat` · [#28](https://github.com/Caff-Core/re-open/issues/28)

- [ ] Separate browser-role socket with operation allowlist
- [ ] Leases and epochs; staging with 30-second expiry and one staged capture per source
- [ ] Chunk commit validation and idempotent ingestion IDs; acknowledge only after commit
- [ ] Private records rejected

### 3.4 Extension scaffold (Brave) — `feat` · [#29](https://github.com/Caff-Core/re-open/issues/29)

- [ ] `apps/extension` with its own Vite config, MV3 manifest, service worker and minimal popup
- [ ] Permissions limited to `tabs`, `storage`, `nativeMessaging`, `alarms`
- [ ] `pnpm dev:extension` watch-builds an unpacked extension
- [ ] Extension `package.json` and generated `manifest.json` version derived from the app version; add them to `scripts/versions.mjs` (ADR-015)

### 3.5 Extension capture — `feat` · [#30](https://github.com/Caff-Core/re-open/issues/30)

- [ ] Event listeners mark dirty; debounced full enumeration; chunked submission
- [ ] Private/incognito excluded at every stage
- [ ] Persisted source identity, pause state and latest pending capture only; alarm-based reconciliation after worker restart
- [ ] Reconnect with capped exponential backoff and jitter

### 3.6 Enrollment and pairing — `feat` · [#31](https://github.com/Caff-Core/re-open/issues/31)

- [ ] Source UUID per profile; enrollment request and approval (CLI until Phase 4 UI)
- [ ] Credentials stored via Secret Service (libsecret); revoke and re-enroll
- [ ] Duplicate source UUID detection requiring re-enrollment

### 3.7 Native host registration for Brave — `feat` · [#32](https://github.com/Caff-Core/re-open/issues/32)

- [ ] Per-user host manifest in `~/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts/` with explicit `allowed_origins`
- [ ] Separate development and production extension IDs; development registration never replaces production
- [ ] Registration and unregistration script that touches only entries owned by this app

### 3.8 Browser restore dispatch — `feat` · [#33](https://github.com/Caff-Core/re-open/issues/33)

- [ ] Jobs only from approved persisted plans, addressed to an enrolled connected source
- [ ] Poll/ack over the native port; batches of five with pacing; per-tab outcomes
- [ ] Job deduplication across worker restarts; uncertain outcomes reconciled
- [ ] Only `http:`, `https:` and controlled blank tabs restored

### 3.9 Firefox support — `feat` · [#34](https://github.com/Caff-Core/re-open/issues/34)

- [ ] Firefox manifest variant with `browser_specific_settings.gecko.id`
- [ ] API differences isolated behind a small browser-API module
- [ ] Host manifest in `~/.mozilla/native-messaging-hosts/` with `allowed_extensions`

### 3.10 Browser failure test suite — `test` · [#35](https://github.com/Caff-Core/re-open/issues/35)

- [ ] Worker restart, duplicate commit, missing chunk, private tabs, stale epochs, source disconnect, host absence
- [ ] Forged launch commands, source IDs and unknown kinds rejected

### Phase 3 exit criteria

- [ ] Brave development profiles work end to end; Firefox development profile works end to end
- [ ] Failure suite passes; extension has no desktop launch authority
- [ ] Phase 3 issues closed

---

## Phase 4 — Tauri interface and background lifecycle

Goal: tray-first desktop shell with on-demand UI wired to the existing agent.

### 4.1 Spike: tray on GNOME — `spike` · [#36](https://github.com/Caff-Core/re-open/issues/36)

- [ ] Confirm behaviour without the AppIndicator/KStatusNotifierItem GNOME extension (stock GNOME has no tray)
- [ ] Decide fallback: background-app portal, persistent notification or window-only; record ADR

### 4.2 Shell lifecycle — `feat` · [#37](https://github.com/Caff-Core/re-open/issues/37)

- [ ] Tray-only mode; webview created on demand and destroyed on close
- [ ] Per-user shell singleton; ensure one agent with readiness handshake
- [ ] Explicit "Quit background agent" with stop/suppression state

### 4.3 Login startup — `feat` · [#38](https://github.com/Caff-Core/re-open/issues/38)

- [ ] XDG autostart entry (via Tauri autostart plugin if it fits) launching tray mode; opt-in during setup
- [ ] No automatic restore on login

### 4.4 Narrow Tauri commands — `feat` · [#39](https://github.com/Caff-Core/re-open/issues/39)

- [ ] Typed commands forwarding to the desktop socket; no second storage path
- [ ] Capabilities limited to these commands

### 4.5 App shell and theme — `feat` · [#40](https://github.com/Caff-Core/re-open/issues/40)

- [ ] Layout and navigation using ReUI/shadcn components installed via MCP commands
- [ ] Empty, loading (Skeleton) and inline error with Retry for every data region

### 4.6 Overview screen — `feat` · [#41](https://github.com/Caff-Core/re-open/issues/41)

- [ ] Capture status, last saved time, recovery candidate, memory status, Save now

### 4.7 Checkpoints screen — `feat` · [#42](https://github.com/Caff-Core/re-open/issues/42)

- [ ] ReUI `data-grid` with timestamp, session, completeness, source freshness, pins; protected-delete confirmation

### 4.8 Recovery preview — `feat` · [#43](https://github.com/Caff-Core/re-open/issues/43)

- [ ] App/resource tree and browser windows/tabs with stable selection during live updates
- [ ] Capability warnings, skipped schemes, missing paths, uncertain duplicates; restore button states item count

### 4.9 Restore progress — `feat` · [#44](https://github.com/Caff-Core/re-open/issues/44)

- [ ] Current action, queue, memory pause, cancel and per-item outcomes

### 4.10 Settings — `feat` · [#45](https://github.com/Caff-Core/re-open/issues/45)

- [ ] Startup, interval, exclusions, retention, paired browsers (enrollment approval), privacy controls, remove matching history

### 4.11 Accessibility and lifecycle tests — `test` · [#46](https://github.com/Caff-Core/re-open/issues/46)

- [ ] Keyboard navigation, focus, accessible names, reduced motion, dark/light
- [ ] Closing the window leaves capture active; UI crash loses no checkpoints

### Phase 4 exit criteria

- [ ] Webview close leaves capture running; selective restore and cancellation work from the UI
- [ ] Keyboard and screen-reader basics pass (Orca)
- [ ] Phase 4 issues closed

---

## Phase 5 — Development resources and memory pressure

### 5.1 VS Code adapter — `feat` · [#47](https://github.com/Caff-Core/re-open/issues/47)

- [ ] Spike observable open-folder/workspace state; record ADR
- [ ] Discover and restore with adapter-specific argument arrays (`--folder-uri` / workspace file)

### 5.2 Zed adapter — `feat` · [#48](https://github.com/Caff-Core/re-open/issues/48)

- [ ] Spike observable project state; implement only what is externally observable

### 5.3 Android Studio capability spike — `spike` · [#49](https://github.com/Caff-Core/re-open/issues/49)

- [ ] Investigate open/recent project state (`~/.config/Google/AndroidStudio*/options`) and launcher arguments
- [ ] Implement adapter only if open-project state is provable; otherwise app-only with honest label

### 5.4 Nautilus folder adapter — `spike` · [#50](https://github.com/Caff-Core/re-open/issues/50)

- [ ] Determine whether open folders are externally observable on Wayland; implement or label unsupported

### 5.5 Memory pressure during restore — `feat` · [#51](https://github.com/Caff-Core/re-open/issues/51)

- [ ] Thresholds on available memory plus PSI with hysteresis; pause/resume with explanation
- [ ] Calibrate thresholds on the 15 GiB reference machine

### 5.6 Workload profiling — `test` · [#52](https://github.com/Caff-Core/re-open/issues/52)

- [ ] Profile the Android build workload with the agent running; record measurements against the performance budget

### Phase 5 exit criteria

- [ ] Projects reopen with tested argument arrays; unsupported contexts explicit
- [ ] Workload measurements recorded; terminal replay remains excluded
- [ ] Phase 5 issues closed

---

## Phase 6 — Linux packaging and recovery hardening

### 6.1 Linux packages — `feat` · [#53](https://github.com/Caff-Core/re-open/issues/53)

- [ ] RPM and AppImage via the Tauri bundler, bundling `reopen-agent` and `reopen-host` as external binaries
- [ ] `pnpm package:linux` implemented

### 6.2 Per-user install, repair and uninstall — `feat` · [#54](https://github.com/Caff-Core/re-open/issues/54)

- [ ] Autostart entry and native-host registration repair
- [ ] Uninstall removes only owned entries and asks whether to retain history; explicit credential behaviour

### 6.3 Extension packaging — `feat` · [#55](https://github.com/Caff-Core/re-open/issues/55)

- [ ] Chromium package for Brave (Chrome Web Store compatible) with matching allowlisted ID
- [ ] Firefox XPI signed through AMO (unlisted)

### 6.4 Version compatibility — `feat` · [#56](https://github.com/Caff-Core/re-open/issues/56)

- [ ] Application, protocol, schema and extension compatibility matrix and runtime checks

### 6.5 Release workflow — `ci` · [#57](https://github.com/Caff-Core/re-open/issues/57)

- [ ] Tag-triggered GitHub Actions release publishing artifacts; unsigned artifacts labelled

### 6.6 Hardening validation — `test` · [#58](https://github.com/Caff-Core/re-open/issues/58)

- [ ] Clean user account install, login startup, `kill -9`, repeated reboot, upgrade with retained history, uninstall, offline use
- [ ] Publish actual limitations and measured resource use

### Phase 6 exit criteria

- [ ] All hardening scenarios pass on the reference machine
- [ ] Phase 6 issues closed

---

## Phase 7 — Additional platforms and browsers

Each task is an epic; break it into sub-issues when started. Order may change based on demand.

### 7.1 X11 and other Linux desktops — `feat` · [#59](https://github.com/Caff-Core/re-open/issues/59)

- [ ] X11 window discovery; KDE Plasma via KWin scripting or D-Bus

### 7.2 Additional Chromium browsers — `feat` · [#60](https://github.com/Caff-Core/re-open/issues/60)

- [ ] Chrome, Chromium and Edge native installs: host manifest paths and IDs

### 7.3 Flatpak browsers — `spike` · [#61](https://github.com/Caff-Core/re-open/issues/61)

- [ ] Investigate native messaging from Flatpak browsers (sandbox host spawning)

### 7.4 Windows 11 adapter — `feat` · [#62](https://github.com/Caff-Core/re-open/issues/62)

- [ ] `platform-windows`: visible windows, named pipes restricted to user SID, registry host registration, installer; per original Windows design

### 7.5 macOS adapter — `feat` · [#63](https://github.com/Caff-Core/re-open/issues/63)

- [ ] Window discovery, launch, Unix sockets, host registration, signed bundle

---

## Later backlog

Named workspaces, manual global shortcut, richer adapter coverage, geometry and multi-monitor restore, terminal directory integration, browser tab groups, encrypted export. Prioritise after the durable capture/restore loop is proven.

## Suggested agent task format

"Implement task N.M (#issue). Read AGENTS.md and relevant docs. Create branch `<type>/<issue#>-<slug>`. Preserve all listed invariants. Use fake providers where native execution is unavailable and label that limitation. Tick completed sub-tasks here and in the issue, update documentation, and report actual validation. Do not expand into later tasks."
