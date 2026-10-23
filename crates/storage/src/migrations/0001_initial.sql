-- Schema v1. See docs/DATA_MODEL.md. IDs are UUID text; timestamps are UTC RFC 3339 text.
-- Pins and recovery candidates reference checkpoints without ON DELETE CASCADE, so a
-- protected checkpoint cannot be deleted while any protection exists (invariant 1).

CREATE TABLE workspaces (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  created_at TEXT NOT NULL
) STRICT;

CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  started_at TEXT NOT NULL,
  clean_exit_at TEXT,
  previous_session_id TEXT REFERENCES sessions (id)
) STRICT;

CREATE TABLE checkpoints (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces (id),
  session_id TEXT NOT NULL REFERENCES sessions (id),
  captured_at TEXT NOT NULL,
  reason TEXT NOT NULL CHECK (reason IN ('scheduled', 'manual', 'shutdown')),
  completeness TEXT NOT NULL CHECK (completeness IN ('complete', 'partial')),
  content_hash TEXT NOT NULL
) STRICT;
CREATE INDEX checkpoints_by_workspace_time ON checkpoints (workspace_id, captured_at);
CREATE INDEX checkpoints_by_session ON checkpoints (session_id);

CREATE TABLE checkpoint_sources (
  checkpoint_id TEXT NOT NULL REFERENCES checkpoints (id) ON DELETE CASCADE,
  source_key TEXT NOT NULL,
  state TEXT NOT NULL CHECK (
    state IN ('complete', 'partial', 'unavailable', 'stale', 'excluded', 'unsupported')
  ),
  required INTEGER NOT NULL CHECK (required IN (0, 1)),
  captured_at TEXT,
  last_seen_at TEXT,
  error_code TEXT,
  PRIMARY KEY (checkpoint_id, source_key)
) STRICT;

CREATE TABLE resources (
  id TEXT PRIMARY KEY,
  checkpoint_id TEXT NOT NULL REFERENCES checkpoints (id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('application', 'project', 'folder', 'browser_tab')),
  adapter_id TEXT NOT NULL,
  adapter_version INTEGER NOT NULL,
  identity_key TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  validated_payload TEXT NOT NULL,
  UNIQUE (checkpoint_id, ordinal)
) STRICT;

CREATE TABLE recovery_candidates (
  session_id TEXT NOT NULL REFERENCES sessions (id),
  checkpoint_id TEXT NOT NULL REFERENCES checkpoints (id),
  disposition TEXT NOT NULL CHECK (disposition IN ('pending', 'dismissed', 'resolved')),
  created_at TEXT NOT NULL,
  PRIMARY KEY (session_id, checkpoint_id)
) STRICT;

-- `reference` identifies the protecting object (a session for candidate pins, a restore
-- run for restore pins, empty for user pins) so releasing one protection leaves others.
CREATE TABLE checkpoint_pins (
  checkpoint_id TEXT NOT NULL REFERENCES checkpoints (id),
  reason TEXT NOT NULL CHECK (reason IN ('recovery_candidate', 'user', 'restore_run')),
  reference TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  PRIMARY KEY (checkpoint_id, reason, reference)
) STRICT;

CREATE TABLE browser_sources (
  source_id TEXT PRIMARY KEY,
  browser_family TEXT NOT NULL,
  display_label TEXT NOT NULL,
  enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
  pairing_key_reference TEXT
) STRICT;

CREATE TABLE browser_captures (
  source_id TEXT NOT NULL REFERENCES browser_sources (source_id),
  capture_id TEXT NOT NULL,
  source_epoch TEXT NOT NULL,
  sequence INTEGER NOT NULL,
  captured_at TEXT NOT NULL,
  received_at TEXT NOT NULL,
  state TEXT NOT NULL,
  PRIMARY KEY (source_id, capture_id)
) STRICT;

CREATE TABLE restore_runs (
  id TEXT PRIMARY KEY,
  checkpoint_id TEXT NOT NULL REFERENCES checkpoints (id),
  state TEXT NOT NULL CHECK (
    state IN ('queued', 'running', 'paused', 'completed', 'cancelled', 'interrupted')
  ),
  created_at TEXT NOT NULL,
  completed_at TEXT
) STRICT;

CREATE TABLE restore_actions (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES restore_runs (id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  resource_id TEXT NOT NULL REFERENCES resources (id),
  state TEXT NOT NULL CHECK (
    state IN ('pending', 'launching', 'restored', 'already_open', 'failed', 'skipped', 'uncertain')
  ),
  attempt_id TEXT,
  result_code TEXT,
  UNIQUE (run_id, ordinal)
) STRICT;

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  version INTEGER NOT NULL,
  validated_value TEXT NOT NULL
) STRICT;
