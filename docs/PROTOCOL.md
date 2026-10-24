# Protocols and contracts

## Channels and authority

| Channel                  | Transport                                        | Authority                                            |
| ------------------------ | ------------------------------------------------ | ---------------------------------------------------- |
| React to Tauri           | Tauri commands/events                            | Typed UI requests                                    |
| Desktop shell to agent   | Current-user Unix socket `desktop.sock`          | Preview, settings, capture, approved restore         |
| Extension to native host | Browser Native Messaging                         | Browser capture and browser restore acknowledgements |
| Native host to agent     | Separate current-user Unix socket `browser.sock` | Same browser-only operations                         |

Authenticate role and installation context from the endpoint and credentials; do not trust a JSON `role` value. Use distinct endpoints, explicit operation allowlists and installation credentials protected with user-level access. Same-user malware remains outside the protection boundary.

## Local sockets on Linux

| Path                                   | Mode                      | Purpose                                          |
| -------------------------------------- | ------------------------- | ------------------------------------------------ |
| `$XDG_RUNTIME_DIR/reopen/`             | `0700`, owned by the user | Runtime directory; created by the agent          |
| `$XDG_RUNTIME_DIR/reopen/agent.lock`   | `0600`                    | Agent singleton lock                             |
| `$XDG_RUNTIME_DIR/reopen/desktop.sock` | `0600`                    | Desktop-role endpoint                            |
| `$XDG_RUNTIME_DIR/reopen/browser.sock` | `0600`                    | Browser-role endpoint used only by `reopen-host` |

Development uses `$XDG_RUNTIME_DIR/reopen-dev/` with the same names. If `XDG_RUNTIME_DIR` is unset, the agent refuses to start its endpoints; it never falls back to a shared directory such as `/tmp`. Refuse to use an existing directory or socket that is not owned by the current UID or has broader permissions. Remove a stale socket only while holding the singleton lock.

On accept, read `SO_PEERCRED` and reject the connection unless the peer UID equals the agent's UID. The peer PID is a diagnostic hint, not an authority: it can be reused and same-user processes can impersonate each other. Keep socket paths short; `sun_path` is limited to 108 bytes, which is why tests use short temporary directories.

Unix sockets are the Linux transport only. The Windows adapter uses named pipes restricted to the user's security identifier ([Later platform adapters](ARCHITECTURE.md#later-platform-adapters)); the envelope and framing below are identical across transports.

## Canonical envelope

```json
{
  "protocol_version": 1,
  "request_id": "9ddac584-c8a5-49ca-af3f-4f93fcc82c8d",
  "kind": "browser.capture.begin",
  "payload": {
    "source_id": "3ee947ea-c562-42fd-a8df-64d1ca9ce311",
    "capture_id": "8070bdd9-cdb6-456e-878b-e957c7baf04e",
    "source_epoch": "f38432e7-cd5a-420c-bdef-8071d92e04d3",
    "sequence": 42,
    "chunk_count": 2,
    "tab_count": 12
  }
}
```

The handshake establishes protocol version, source identity, lease/epoch and supported capabilities. Unsupported major versions fail closed with `VERSION_UNSUPPORTED`. Request IDs correlate responses; capture IDs identify idempotent submissions. Credentials are handshake material, never part of logged envelopes.

## Operations

| Desktop operations                                       | Browser operations                           |
| -------------------------------------------------------- | -------------------------------------------- |
| `status.get`, `settings.get`, `settings.update`          | `browser.hello`, `browser.heartbeat`         |
| `capture.request`, `capture.pause`, `capture.resume`     | `browser.capture.begin`, `.chunk`, `.commit` |
| `checkpoints.list`, `checkpoints.get`                    | `browser.restore.poll`, `.ack`               |
| `recovery.dismiss`, `checkpoint.pin`, `checkpoint.unpin` | `browser.disconnect`                         |
| `restore.preview`, `restore.start`, `restore.cancel`     | No desktop launch or generic execution       |

Desktop `restore.preview` produces a plan ID, resource selections, policy warnings and expiry. `restore.start` accepts that plan ID and a request ID. Changes to exclusions, resources or source identity invalidate stale plans. Recheck path existence and memory at execution time. Cancelling stops future actions, not apps already launched.

## Responses

Every frame is an envelope. A successful response repeats the request's `request_id` and uses kind `<operation>.result`, with the payload defined in `packages/contracts/schema/desktop.schema.json` (for example `status.get` → `status.get.result` with `StatusResult`). A failure uses kind `error` with an `ErrorResponse`-shaped payload. If a request is too malformed to read its `request_id`, the error carries the nil UUID. Both sides validate payloads against the schema definition for the operation, in addition to the envelope schema; the agent also validates its own responses before sending them.

Implemented desktop operations: `status.get`, `capture.request`, `checkpoints.list`, `checkpoints.get` (task 1.8) and `restore.preview`, `restore.start`, `restore.status`, `restore.cancel` (task 2.5). Unknown operations return `UNSUPPORTED_CAPABILITY`. Each connection is served one request at a time, so at most one request is outstanding; the agent accepts at most 8 concurrent connections and closes a connection idle for 60 seconds.

## Framing and limits

Native Messaging uses a four-byte native-endian length followed by UTF-8 JSON. Standard output contains protocol frames only; diagnostics go to standard error with redaction. Internal agent socket framing uses an explicitly documented four-byte little-endian length followed by the same envelope, on every platform.

Project limits: 256 KiB per encoded message, 64 chunks, 8 MiB per browser capture, 2,000 tabs per source, 30-second staging expiry and one staged capture per source. Reject oversize frames before allocating their declared length. Limit outstanding requests to 16 per connection. These are application limits below browser transport ceilings.

Chunks include capture ID, zero-based index and tab/window data. Commit requires all declared chunks, unique indices, exact counts and valid cross-references. Staging expiry/disconnect discards incomplete data while retaining the previous accepted capture. Sequence numbers must increase within the active epoch. New connections invalidate old epochs; they must not accept old pending frames.

## Errors and retries

Errors contain `request_id`, stable code, retryable flag and a redacted user-safe message. Initial codes: `VERSION_UNSUPPORTED`, `UNAUTHORIZED`, `INVALID_PAYLOAD`, `LIMIT_EXCEEDED`, `SOURCE_UNAVAILABLE`, `STALE_PLAN`, `MEMORY_PRESSURE`, `STORAGE_UNAVAILABLE`, `TIMEOUT`, `UNSUPPORTED_CAPABILITY`, `NOT_FOUND`, `RESTORE_IN_PROGRESS`.

Capture retries reuse capture IDs; commit acknowledgements include the persisted capture ID. Reconnect uses exponential backoff capped at 60 seconds with jitter. Unsent state coalesces to the newest full capture. Never blindly retry launch actions after an ambiguous timeout.

## Browser restore dispatch

The agent creates browser actions only from a user-approved persisted plan and addresses them to an enrolled source ID. The extension polls over its connected native port; no inbound web server exists. A restore job has an ID, attempt ID, expiry and bounded tab batch. The extension deduplicates jobs using extension storage and acknowledges individual tab outcomes. If it crashes between opening a tab and acknowledging, report uncertain and reconcile before another attempt. Exactly-once side effects cannot be guaranteed across process failure.

Schemas cover every envelope and payload. Share fixtures for valid, malformed, missing-field, unknown-version, duplicate, oversized and out-of-order messages. Never make TypeScript interfaces the only runtime validation.
