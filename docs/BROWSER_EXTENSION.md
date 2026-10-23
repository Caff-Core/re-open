# Browser extension

## First target and permissions

Build Manifest V3 for Brave first (native RPM install on Fedora), then Firefox from the same source. Chrome, Chromium and Edge share the Chromium build but are later targets (Phase 7). Flatpak and Snap browsers are out of scope: their sandboxes do not spawn host-side native messaging binaries without extra portal work (task 7.3). Use a background worker and a small popup; no content scripts. Request only `tabs`, `storage`, `nativeMessaging` and `alarms` when used for reconciliation. No broad host permissions, browsing history, cookies or debugger access. Explain that `tabs` permits reading sensitive tab metadata.

Private/incognito capture is unsupported and excluded at every stage, even if a user manually enables the extension in that mode. The agent validates incoming private flags as well; private records are rejected rather than persisted.

## Capture strategy

Listen for tab creation, update, removal, move, attach/detach and relevant window changes. Event listeners mark state dirty; debounce to a full browser-state enumeration and send a chunked complete capture. Events are hints, not the durable source of truth. Periodic reconciliation repairs missed events.

Do not rely on indefinite worker timers or in-memory state. On worker activation/reconnect, reconcile windows/tabs and load persisted source identity, pause settings and pending job metadata. Use browser alarms within supported timing limits. Closing all browser windows or worker termination is not authoritative evidence that the prior browser workspace should be erased.

Store only the latest pending browser capture locally, replace superseded data, and enforce the same tab/byte limits as the host. Clear private or newly excluded data immediately. Browser state caches contain sensitive addresses and must not accumulate unbounded history.

## Profile identity and pairing

Generate a source UUID in extension-local storage for each browser profile. This labels the source, not a discovered filesystem profile path. Browser profile labels are user editable. Never guess profiles from process names or browser configuration directories.

First connection requires enrollment shown in the desktop UI. The native host verifies the caller's extension origin against the installed manifest allowlist and startup origin argument. Bind enrollment credentials to browser family, source UUID and installation. Protect credentials in per-user storage; do not log them. Unknown sources can request pairing but cannot submit durable captures or receive restore jobs until approved.

If a cloned profile presents a duplicate source UUID, detect conflicting active sessions and require re-enrollment. A connection lease and fresh epoch prevent older connections overwriting current state. Offline profiles remain visible; never redirect their restore jobs to another profile.

## Native host lifecycle

The browser starts the Rust native host when the extension connects. The host forwards validated, bounded frames to the agent's browser-only endpoint. It does not own history, launch arbitrary applications or keep the background agent alive. On host/agent failure the extension shows disconnected and retries with bounded backoff.

Native-host registration is per user. Use separate explicit allowed extension IDs for local development and each distribution channel. Development registration uses its own host name (`com.caffcore.reopen.dev.host`) and must not replace production. Installer repairs and uninstall remove only entries owned by this application.

## Browser differences

|                         | Brave (first)                                                                              | Firefox (second)                                                            |
| ----------------------- | ------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| Per-user host manifest  | `~/.config/BraveSoftware/Brave-Browser/NativeMessagingHosts/com.caffcore.reopen.host.json` | `~/.mozilla/native-messaging-hosts/com.caffcore.reopen.host.json`           |
| Caller allowlist field  | `allowed_origins`: `chrome-extension://<id>/` entries                                      | `allowed_extensions`: add-on IDs                                            |
| Host arguments at start | Caller origin                                                                              | Path to the host manifest, then the add-on ID                               |
| Extension identity      | ID derived from the packaging key; pin the development key so the ID is stable             | `browser_specific_settings.gecko.id` in the manifest                        |
| Background context      | `background.service_worker`                                                                | `background.scripts` (event page); Firefox does not run MV3 service workers |
| API namespace           | `chrome.*`                                                                                 | `browser.*` (promise-based); `chrome.*` partly aliased                      |
| Distribution            | Chrome Web Store compatible package, or unpacked for development                           | AMO-signed XPI (unlisted); unsigned only in development profiles            |

The host `path` must be absolute. Both manifests point at the same `reopen-host` binary; the host identifies the browser family from its start arguments and verifies the caller against the allowlist before forwarding anything. Isolate API differences in one small browser-API module rather than branching throughout the extension. Verify these paths and fields against current browser documentation when implementing tasks 3.7 and 3.9.

## Restore behavior

Only execute agent-issued jobs from a user-approved plan. Require a connected matching source. Reconstruct windows/order and pinned state where supported, pacing new tabs and avoiding focus stealing until the user-selected foreground action. Acknowledge per-tab results and retain job deduplication metadata across worker restarts.

Do not auto-open a browser under a guessed profile. If no matching source is connected, ask the user to open that browser profile and retry. Explicit profile-launch integration is a separate future adapter.

## Popup

Show connected/disconnected, profile label, last accepted capture time, pause/resume and “Open desktop app” only if a deliberately implemented safe integration exists. Otherwise instruct the user to open the desktop app manually. Avoid promising a generic native command that launches arbitrary programs.
