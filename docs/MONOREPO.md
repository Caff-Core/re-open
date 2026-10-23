# Monorepo and development tooling

## Proposed layout

This is a target layout. Only Markdown files exist in the initial pack.

```text
/
  AGENTS.md
  CLAUDE.md
  README.md
  Cargo.toml
  Cargo.lock
  package.json
  pnpm-workspace.yaml
  pnpm-lock.yaml
  rust-toolchain.toml
  apps/
    desktop/
      src/                    # React application
      src-tauri/              # Thin Rust Tauri shell; Cargo member
    extension/
      src/background/         # Manifest V3 event handling
      src/popup/              # Minimal status and pause controls
      manifests/              # Chromium (Brave) and Firefox manifest variants
    gnome-shell-extension/    # Only if selected by spike 2.1
  crates/
    agent/                    # Background binary
    core/                     # Domain types and policies
    storage/                  # rusqlite and migrations
    protocol/                 # Generated wire types and validation
    platform/                 # Traits and fake adapter
    platform-linux/           # Linux/GNOME integration (first target)
    platform-windows/         # Windows API integration (Phase 7; not created until then)
    native-host/              # Browser Native Messaging bridge
  packages/
    contracts/                # Canonical JSON Schemas + generated TypeScript
    ui/                       # Shared shadcn/ReUI primitives and tokens (ADR-012)
  fixtures/
    protocol/
    capture/
  docs/
  scripts/                    # Contract generation and platform packaging
  .github/workflows/
```

## Dependency rules

Use one root pnpm workspace for `apps/*` and `packages/*`, and one root Cargo workspace for `crates/*` plus `apps/desktop/src-tauri`. Core contains no Tauri or OS-specific dependencies. Platform crates compile only on their target OS (`cfg(target_os = ...)`); the agent selects one at build time and uses the fake adapter in tests. Only platform crates may override the workspace `unsafe_code = "forbid"` lint, with a safety comment on every `unsafe` block. Storage depends on core; platform implementations depend on platform/core; the agent composes everything. Native host depends on protocol and IPC client utilities, not storage or launch adapters.

Desktop uses `@workspace/ui` and `@workspace/contracts`. Extension uses contracts and may use a small subset of UI. Avoid bundling the entire desktop design system into the extension service worker. No imports between app source trees.

Canonical protocol schemas live in `packages/contracts/schema`; generated Rust and TypeScript are committed and checked for drift. The generator choice is recorded in [ADR-011](DECISIONS.md#adr-011-schema-generator). Enforce unknown-field and size rules explicitly; generated types alone do not validate untrusted input.

## Frontend choices

React with TypeScript strict mode and Vite. Tailwind CSS 4, shadcn/ui and ReUI with one chosen primitive family and theme. Use React state initially; add a state library only for a concrete need. Use React Router only when navigation complexity justifies it. Extension output uses its own Vite configuration and Chromium Manifest V3 manifest; no website deployment.

## Root command contract

Implement these scripts in Phase 0 (tasks 0.5–0.10); scripts for later phases fail with a message naming the phase that implements them:

| Command                   | Required behavior                                                              |
| ------------------------- | ------------------------------------------------------------------------------ |
| `pnpm dev:desktop`        | Build/locate Rust agent and launch Tauri development mode                      |
| `pnpm dev:extension`      | Watch-build unpacked browser extension                                         |
| `pnpm dev:agent`          | Run the Rust agent against an isolated development data directory              |
| `pnpm contracts:generate` | Regenerate Rust and TypeScript wire types                                      |
| `pnpm contracts:check`    | Fail if committed generated outputs differ                                     |
| `pnpm lint`               | Lint all TypeScript and frontend code                                          |
| `pnpm typecheck`          | Check desktop, extension and shared packages                                   |
| `pnpm test`               | Run TypeScript unit tests, initially Vitest                                    |
| `pnpm build`              | Build frontend packages and browser artifacts; no installer implied            |
| `pnpm package:linux`      | Build release Rust binaries and the RPM and AppImage desktop bundles (Phase 6) |

`pnpm package:windows` is deferred to the Windows adapter (Phase 7) and is not defined until then. Use Cargo commands directly for Rust checks. Platform-specific scripts must fail with useful messages on unsupported hosts. Development and test data directories must never reuse production checkpoints, pairing identities, sockets or native-host registrations.

## Toolchain policy

Select supported Node.js, pnpm and Rust versions at bootstrap, record exact versions and commit lockfiles. Verify compatibility with React, Tauri, Tailwind and the selected ReUI components. Do not use an unbounded `latest` dependency in committed manifests. No Turborepo/Nx requirement for the initial repository.
