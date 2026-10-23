# Testing and release

## Test layers

| Layer | What to verify |
| --- | --- |
| Rust unit | Retention, identity, selection, pacing, pressure hysteresis, source freshness |
| Storage integration | Atomic writes, candidate pins, migration, corrupt/read-only/full storage |
| Contract fixtures | Rust/TypeScript agreement, bounds, versions, chunks, duplicate acknowledgements |
| Extension unit | Event coalescing, private exclusion, reconnect, worker state recovery |
| React component | Selection stability, partial/stale indicators, cancellation and error rendering |
| Native Windows integration | Visible-app discovery, current-user pipes, process launch and registration |
| End-to-end | Capture, terminate processes, relaunch, select, restore and report |

Use Rust's test framework, temporary databases, fake clocks/providers and Vitest for TypeScript. Use browser/UI automation for flows it can actually exercise. Browser extension and Windows native behavior require dedicated integration runs; a web-only mock cannot certify them.

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

Run schema drift, formatting, linting, TypeScript checks and fixture/unit tests on each change. Run Cargo workspace checks on Windows; portable core crates can also run on Linux. Build the unpacked extension artifact. Add native integration and installer validation on Windows runners or a documented local test machine. Block release if protocol/schema compatibility or critical durability cases fail.

## Release artifacts and gates

Desktop installer bundles target-specific agent and native-host binaries. Produce separate Chrome/Edge extension packages and matching allowlisted IDs. Record application version, protocol version, database schema version and extension compatibility matrix. Never publish credentials, development pairing data or personal checkpoints.

Gate release on clean install, native-host registration, login startup, no-elevation operation, offline restore, upgrade with retained history and uninstall. Uninstall asks whether history should be retained; it removes only this app's startup/host entries. A browser-store package and a locally loaded development extension have different identity requirements.

Hardware power-cut testing belongs on disposable data/devices. Automated process termination verifies transaction behavior but is not proof against every real storage failure.
