# Testing and release

## Test layers

| Layer | What to verify |
| --- | --- |
| Rust unit | Retention, identity, selection, pacing, pressure hysteresis, source freshness |
| Storage integration | Atomic writes, candidate pins, migration, corrupt/read-only/full storage |
| Contract fixtures | Rust/TypeScript agreement, bounds, versions, chunks, duplicate acknowledgements |
| Extension unit | Event coalescing, private exclusion, reconnect, worker state recovery |
| React component | Selection stability, partial/stale indicators, cancellation and error rendering |
| Native Linux integration | GNOME Wayland app discovery, current-user sockets and `SO_PEERCRED`, desktop-entry launch, Secret Service, host-manifest and autostart registration |
| End-to-end | Capture, terminate processes, relaunch, select, restore and report |

Use Rust's test framework, temporary databases, fake clocks/providers and Vitest for TypeScript. Use browser/UI automation for flows it can actually exercise. Browser extension and Linux native behavior require dedicated integration runs; a web-only mock cannot certify them.

## Reference machine

Native validation runs on the development machine: Fedora 44 x86_64, GNOME 50 on Wayland, 15 GiB RAM, Brave and Firefox from native RPMs. Record the kernel, GNOME Shell, Brave and Firefox versions with each native test run and each performance measurement. Results from CI containers or other desktops do not substitute for this machine.

## Critical failure cases

1. Terminate the writer at transaction boundaries; the prior committed checkpoint survives.
2. Restart repeatedly without dismissing recovery; all pending candidates remain protected.
3. Disconnect browser, send a partial capture or terminate worker; last known state stays labelled stale.
4. Retry a committed capture; no duplicate accepted capture appears.
5. Deliver stale connection/epoch messages; current state is unchanged.
6. Exceed frame/tab/staging limits; bounded memory use and useful errors result.
7. Crash after launch but before acknowledgement; action becomes uncertain and requires reconciliation.
8. Raise memory pressure mid-restore; new actions pause, cancellation remains responsive.
9. Remove/rename an executable or project; preview/execution reports unavailable safely.
10. Forge browser launch commands, source IDs or unknown message kinds; reject them.
11. Change exclusions; subsequent capture and queued actions respect them.
12. Corrupt database or fail a migration; no silent empty-history replacement occurs.

## Performance budget

Initial engineering targets, to be measured rather than promised:

- Agent idle resident memory below 50 MiB under a reference workload of 30 supported app windows and 200 tabs.
- Idle agent CPU below 1% average over ten minutes on the documented reference machine.
- No webview process while the recovery window is closed, subject to verified Tauri/platform behavior.
- Capture completes within the 10-second cycle deadline; provider timeouts are visible.
- Queue, staging, retention and logs remain bounded at the 2,000-tab limit and during host outages.

Measure agent, native host, tray shell and open UI separately, then total idle footprint. Record machine specifications, build mode, interval and workload. Revisit budgets using evidence; do not hide overhead in a different process.

## Continuous integration

Run schema drift, formatting, linting, TypeScript checks and fixture/unit tests on each change. CI runs on GitHub-hosted Linux runners (`ubuntu-latest`) with the Tauri WebKitGTK system dependencies installed; Cargo workspace checks, including `platform-linux`, run there. Build the unpacked extension artifacts. CI cannot exercise a GNOME Wayland session, a real browser profile or the Secret Service, so native integration and package validation run on the reference machine and are recorded in the relevant issue. Windows and macOS runners are added only with their Phase 7 adapters. Block release if protocol/schema compatibility or critical durability cases fail.

## Release artifacts and gates

Linux desktop packages (RPM and AppImage) bundle target-specific agent and native-host binaries. Produce a Chromium extension package for Brave and an AMO-signed Firefox XPI, each with matching allowlisted IDs. Record application version, protocol version, database schema version and extension compatibility matrix. Never publish credentials, development pairing data or personal checkpoints.

Gate release on clean install in a fresh user account, native-host registration, login startup, no-root operation, offline restore, upgrade with retained history and uninstall. Uninstall asks whether history should be retained; it removes only this app's startup/host entries. A browser-store package and a locally loaded development extension have different identity requirements.

Hardware power-cut testing belongs on disposable data/devices. Automated process termination verifies transaction behavior but is not proof against every real storage failure.
