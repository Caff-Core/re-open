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
      manifests/              # Chrome/Edge build variations
  crates/
    agent/                    # Background binary
    core/                     # Domain types and policies
    storage/                  # rusqlite and migrations
    protocol/                 # Generated wire types and validation
    platform/                 # Traits and fake adapter
    platform-windows/         # Windows API integration
    native-host/              # Browser Native Messaging bridge
  packages/
    contracts/                # Canonical JSON Schemas + generated TypeScript
    ui/                       # Shared shadcn/ReUI primitives and tokens
  fixtures/
    protocol/
    capture/
  docs/
  scripts/                    # Contract generation and platform packaging
  .github/workflows/
```

## Dependency rules

Use one root pnpm workspace for `apps/*` and `packages/*`, and one root Cargo workspace for `crates/*` plus `apps/desktop/src-tauri`. Core contains no Tauri or Windows dependencies. Storage depends on core; platform implementations depend on platform/core; the agent composes everything. Native host depends on protocol and IPC client utilities, not storage or launch adapters.

Desktop uses `@workspace/ui` and `@workspace/contracts`. Extension uses contracts and may use a small subset of UI. Avoid bundling the entire desktop design system into the extension service worker. No imports between app source trees.

Canonical protocol schemas live in `packages/contracts/schema`; generated Rust and TypeScript are committed and checked for drift. Pick and record a generator during bootstrap. Enforce unknown-field and size rules explicitly; generated types alone do not validate untrusted input.

## Frontend choices

React with TypeScript strict mode and Vite. Tailwind CSS 4, shadcn/ui and ReUI with one chosen primitive family and theme. Use React state initially; add a state library only for a concrete need. Use React Router only when navigation complexity justifies it. Extension output uses its own Vite configuration and Chromium Manifest V3 manifest; no website deployment.

## Root command contract

Implement these scripts in milestone 0; they are not currently executable:

| Command | Required behavior |
| --- | --- |
| `pnpm dev:desktop` | Build/locate Rust agent and launch Tauri development mode |
| `pnpm dev:extension` | Watch-build unpacked browser extension |
| `pnpm dev:agent` | Run the Rust agent against an isolated development data directory |
| `pnpm contracts:generate` | Regenerate Rust and TypeScript wire types |
| `pnpm contracts:check` | Fail if committed generated outputs differ |
| `pnpm lint` | Lint all TypeScript and frontend code |
| `pnpm typecheck` | Check desktop, extension and shared packages |
| `pnpm test` | Run TypeScript unit tests, initially Vitest |
| `pnpm build` | Build frontend packages and browser artifacts; no installer implied |
| `pnpm package:windows` | Build release Rust binaries and Windows desktop installer |

Use Cargo commands directly for Rust checks. Platform-specific scripts must fail with useful messages on unsupported hosts. Development and test data directories must never reuse production checkpoints, pairing identities or registry entries.

## Toolchain policy

Select supported Node.js, pnpm and Rust versions at bootstrap, record exact versions and commit lockfiles. Verify compatibility with React, Tauri, Tailwind and the selected ReUI components. Do not use an unbounded `latest` dependency in committed manifests. No Turborepo/Nx requirement for the initial repository.
