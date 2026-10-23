# Browser extension

## First target and permissions

Build Chromium Manifest V3 for Chrome and Edge. Use a service worker and a small popup; no content scripts. Request only `tabs`, `storage`, `nativeMessaging` and `alarms` when used for reconciliation. No broad host permissions, browsing history, cookies or debugger access. Explain that `tabs` permits reading sensitive tab metadata.

Private/incognito capture is unsupported and excluded at every stage, even if a user manually enables the extension in that mode. The agent validates incoming private flags as well; private records are rejected rather than persisted.

## Capture strategy

Listen for tab creation, update, removal, move, attach/detach and relevant window changes. Event listeners mark state dirty; debounce to a full browser-state enumeration and send a chunked complete capture. Events are hints, not the durable source of truth. Periodic reconciliation repairs missed events.

Do not rely on indefinite worker timers or in-memory state. On worker activation/reconnect, reconcile windows/tabs and load persisted source identity, pause settings and pending job metadata. Use browser alarms within supported timing limits. Closing all browser windows or worker termination is not authoritative evidence that the prior browser workspace should be erased.

Store only the latest pending browser capture locally, replace superseded data, and enforce the same tab/byte limits as the host. Clear private or newly excluded data immediately. Browser state caches contain sensitive addresses and must not accumulate unbounded history.

## Profile identity and pairing

Generate a source UUID in extension-local storage for each browser profile. This labels the source, not a discovered filesystem profile path. Browser profile labels are user editable. Never guess profiles from process names or Chromium installation directories.

First connection requires enrollment shown in the desktop UI. The native host verifies the caller's extension origin against the installed manifest allowlist and startup origin argument. Bind enrollment credentials to browser family, source UUID and installation. Protect credentials in per-user storage; do not log them. Unknown sources can request pairing but cannot submit durable captures or receive restore jobs until approved.

If a cloned profile presents a duplicate source UUID, detect conflicting active sessions and require re-enrollment. A connection lease and fresh epoch prevent older connections overwriting current state. Offline profiles remain visible; never redirect their restore jobs to another profile.

## Native host lifecycle

The browser starts the Rust native host when the extension connects. The host forwards validated, bounded frames to the agent's browser-only endpoint. It does not own history, launch arbitrary applications or keep the background agent alive. On host/agent failure the extension shows disconnected and retries with bounded backoff.

Native-host registration is per user. Use separate explicit allowed extension IDs for local development, Chrome distribution and Edge distribution. Development registration must not replace production. Installer repairs and uninstall remove only entries owned by this application.

## Restore behavior

Only execute agent-issued jobs from a user-approved plan. Require a connected matching source. Reconstruct windows/order and pinned state where supported, pacing new tabs and avoiding focus stealing until the user-selected foreground action. Acknowledge per-tab results and retain job deduplication metadata across worker restarts.

Do not auto-open a browser under a guessed profile. If no matching source is connected, ask the user to open that browser profile and retry. Explicit profile-launch integration is a separate future adapter.

## Popup

Show connected/disconnected, profile label, last accepted capture time, pause/resume and “Open desktop app” only if a deliberately implemented safe integration exists. Otherwise instruct the user to open the desktop app manually. Avoid promising a generic native command that launches arbitrary programs.
