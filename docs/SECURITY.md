# Security and privacy

## Threat model

Protect against malformed browser input, unintended command execution, cross-profile restore mistakes, accidental sensitive logging and access by other local users. A compromised process already running as the same user can often read or modify that user's data; this product is not a security boundary against same-user malware.

No cloud synchronization, remote telemetry, screenshot capture, keystroke collection, clipboard collection, page scraping, password capture or cookie export. Do not collect unsaved document contents or full process command lines.

## Trust boundaries

- Treat stored snapshots and extension payloads as untrusted at execution time.
- Validate schemas, lengths, enum values, identifiers and cross-references before persistence or side effects.
- Current-user pipe permissions and installation credentials limit access; browser role routing cannot grant desktop authority.
- Browser source IDs are identifiers, not authentication secrets. Enrollment binds the source to a permitted browser connection.
- Native host stdout is framed protocol only. All logging uses a redaction layer.
- Tauri capabilities must permit only required commands. Deny generic shell/file access, remote web content and arbitrary navigation.

## Launch protection

Only trusted adapters translate approved resource types to launch actions. Never execute snapshot-provided command strings. Validate executable references and construct argument arrays. Revalidate old checkpoints after upgrades. Browser messages cannot create desktop restore plans, alter executable references or invoke system APIs.

Initial browser restoration permits HTTP/HTTPS resources and controlled blank tabs only. Exclude executable schemes and local files. Never automatically rerun a build or terminal history.

## Local data

SQLite contents include private addresses and file paths. Use per-user application storage with restrictive access. SQLite is not encrypted by default; describe that honestly. Full-disk protection is external to the app. Optional database/export encryption is a future decision, not a first-release claim.

Paired-source credentials use an operating-system-protected secret store, with records separated from checkpoint data. Support revoke/re-enroll. A fresh install and an uninstall-with-data-retained must have explicit credential behavior.

Logs contain stable event codes, timings, counts and opaque IDs only. No addresses, titles, paths, raw payloads or credentials. Use bounded rotation, initially three files of 1 MiB each. Diagnostic exports require explicit selection and redaction review.

## Exclusions and deletion

Apply exclusions both before capture and before restore. When an exclusion changes, immediately stop future capture and invalidate affected queued actions. Offer a separate “Remove matching saved history” operation; do not imply disabling capture erases older checkpoints. Bulk history deletion must include protected candidate handling explicitly. Never export history automatically.

## Installer and updates

Install and register only per-user components by default. Do not request elevation for routine capture. Release signing keys and update credentials never enter source control. Automatic updating is deferred until signed verification, compatibility and rollback behavior are implemented. Do not auto-resume interrupted restores after upgrades.
