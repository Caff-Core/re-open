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

**Status:** proposed implementation choice. Capture survives UI/browser exits. A tray shell provides login startup and opens a webview on demand. No elevated service; desktop-session discovery and user-specific resource access belong to the logged-in user context.

## ADR-006: Native Messaging plus local pipes

**Status:** proposed implementation choice. Extension talks to a restricted native bridge, then the agent. Avoid public localhost endpoints, browser debugging ports and permission-heavy page inspection. Additional binaries require installer/registration tests.

## ADR-007: Explicit restore and durable action journal

**Status:** proposed implementation choice. The first release restores only after user selection. Capture retries are idempotent; external launch effects cannot be guaranteed exactly once. Interrupted actions are reconciled or labelled uncertain, never blindly retried.

## ADR-008: Windows and Chromium first

**Status:** provisional, awaiting user confirmation. Targeting one OS/browser family makes a useful first release achievable. Platform interfaces preserve extension points without claiming cross-platform support before implementation.

## Open decisions

- Confirm target OS, browser profiles, editor, RAM and available storage.
- Choose final product name, identifiers and signing/distribution approach.
- Verify editor/project discovery APIs for the user's actual workflow.
- Choose schema generator and one shadcn/ReUI primitive family after compatibility verification.
- Calibrate memory thresholds and resource budgets on the target machine.
- Decide later whether encrypted export or additional platforms justify their complexity.
