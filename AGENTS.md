# Coding-agent instructions

## Purpose and authority

Build the product described in [README.md](README.md) and [docs/PRODUCT.md](docs/PRODUCT.md). These are project instructions for AI coding agents, including Claude Code and GPT-Astra. User instructions take precedence. Read the architecture, implementation plan and documents relevant to your change before editing.

The repository is being bootstrapped; check [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) for what exists. Do not represent proposed modules, scripts or tests as existing. Work on the next open task unless instructed otherwise.

## Fixed decisions

- Rust engine; Tauri 2; React + TypeScript + Vite; Tailwind CSS 4; shadcn/ui + ReUI.
- One monorepo for desktop app, browser extension, native bridge and shared crates/contracts.
- SQLite, with one background-agent writer. No hosted backend, cloud sync or telemetry by default.
- The Rust agent continues running without an open desktop window or browser.
- Product name Re-Open; identifiers `com.caffcore.reopen`, `com.caffcore.reopen.host`, binaries `reopen-agent` and `reopen-host` (ADR-010).
- First target (ADR-009): Fedora 44 x86_64, GNOME 50 on Wayland, Brave then Firefox, VS Code then Zed, Nautilus, 15 GiB RAM. Windows, macOS, X11/KDE, other Chromium browsers and Flatpak browsers are Phase 7 adapters; do not build them earlier.
- Local IPC on Linux: per-user Unix domain sockets in `$XDG_RUNTIME_DIR/reopen/`, caller UID checked with `SO_PEERCRED`. Storage under XDG directories. Pairing credentials in the Secret Service.
- Use an ordinary per-user process at login (XDG autostart), never root, a system service or elevation.

## Invariants

1. Never delete or overwrite a protected recovery checkpoint automatically.
2. Never advertise incomplete capture as a complete checkpoint.
3. Never execute stored arbitrary shell commands or replay a terminal's history.
4. Never restore anything automatically on login in the first release. Require an explicit user restore action.
5. Browser messages cannot launch executables, read arbitrary files or create restore plans.
6. Persist a checkpoint atomically; acknowledge browser submissions only after commit.
7. Keep raw browser addresses, document paths, window titles and command arguments out of logs.
8. Exclude private browsing; do not collect page content, passwords, cookies or screenshots.
9. Preserve missing, unknown and stale data as distinct states. A disconnected browser is not an empty browser.
10. Report unsupported capabilities honestly. A process list does not reveal every open document or browser tab.

## Implementation habits

- Keep business logic in Rust crates, not React components or Tauri command handlers.
- Keep Tauri commands narrow and typed; never expose generic shell execution.
- Define protocol schemas first, generate matching Rust/TypeScript representations, and test shared fixtures.
- Use composition and platform adapters. Isolate unsafe OS calls in platform crates (`platform-linux` first; `platform-windows` in Phase 7) with explicit safety comments and bounded resource lifetimes.
- Implement the smallest complete vertical slice, then extend. Do not generate every module as empty scaffolding.
- Follow existing conventions; pin dependency versions and commit lockfiles when bootstrapping. Verify current official documentation rather than guessing dependency APIs.
- Add meaningful tests for persistence, protocol validation and restore side effects. Use fake adapters for most tests.
- Change the relevant docs when behavior or contracts change. Record new architectural decisions in `docs/DECISIONS.md`.
- Do not add cloud services, browser debugging ports, page content scripts, elevation, command replay or automatic process killing without an explicit product decision.

## Workflow and completion

1. Inspect the repository and the current phase/task status in the plan and its GitHub issue.
2. Identify affected invariants and any unresolved platform assumption.
3. Implement one useful slice, including error paths.
4. Run checks appropriate to the slice; document environmental blockers precisely.
5. Tick completed sub-tasks in both the plan file and the issue; update relevant documentation.
6. Report what changed, actual checks run, remaining limitations and the next concrete step.

### Git and GitHub

- One task = one issue = one branch = one PR. Branch `<type>/<issue#>-<slug>` (`feat`, `fix`, `chore`, `docs`, `ci`, `refactor`, `test`; spikes use `docs`).
- Conventional Commits. No co-author trailers in commit messages.
- PR title is a Conventional Commit (it becomes the squash commit) and the body contains `Closes #<issue>`. `main` is protected; never push to it directly.
- Before starting, check for an existing branch or open PR for the issue so parallel sessions do not duplicate work.

Never claim tests passed unless executed. Do not fabricate working native integrations from mocks. No requirement to delegate to subagents.

## Intended checks after bootstrap

The root scripts defined in [docs/MONOREPO.md](docs/MONOREPO.md) must implement these commands before they are used:

```sh
pnpm lint
pnpm typecheck
pnpm test
pnpm build
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run native Linux integration and package checks on the Fedora 44 / GNOME Wayland reference machine; CI containers cannot exercise a desktop session, real browser profiles or the Secret Service. Cross-platform fixture tests are not substitutes for native validation.

<!-- BEGIN:reui-agent-rules -->
## ReUI design rules (every UI edit, follow-ups included)

This project uses an existing design system: ReUI on shadcn/ui. Keep its look. Generic frontend advice (vary fonts or themes, gradients, page-load or staggered reveals, "2-3 intentional motions", Framer Motion) does not apply here, and other design skills lose where they disagree.

1. Use installed components first (`components/ui`, `components/reui`): tables `data-grid`, filtering `filters`, boards `kanban`, modals `Dialog` or `AlertDialog`, toasts `sonner`. Never set height, radius, padding, shadow or motion on them; use `size`, `variant`, `spacing`.
2. Color: semantic tokens. State: `<Badge variant="success-light">` from `@/components/reui/badge` at default size, or Alert variants. Raw palette only for categorical identity, with its `dark:` pair.
3. Spacing: `gap-*` on the parent: `gap-1` to `gap-2` in a row, `gap-3` to `gap-4` between groups, `gap-6` to `gap-8` between sections. Type: `text-sm` body, `text-xs` meta, nothing smaller; never resize FrameTitle or CardTitle.
4. Motion: installed parts already animate; never retime them. Yours: hover `transition-colors`; reveal `duration-200 ease-[cubic-bezier(0.22,1,0.36,1)]`; transform and opacity only; never `transition-all`, bounce, or a stagger on table rows or lists of 20+; at most 1 entrance per view; `MotionConfig reducedMotion="user"` plus `motion-reduce:`.
5. Every data region: empty state with an action, `Skeleton` loading, inline error with Retry.
6. Before UI code, write the plan from `.agents/skills/reui/SKILL.md` in your reply. Before you finish, run its self-check on the files you changed and fix every hit (2 rounds at most). Full rules: `.agents/skills/reui/SKILL.md`. If that folder is missing, call the `reui` MCP tool `get_agent_skill`.

Managed by the ReUI installer. Edits between the markers are replaced on the next run.
<!-- END:reui-agent-rules -->
