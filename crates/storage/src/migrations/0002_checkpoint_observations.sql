-- When a capture matches the latest checkpoint of the same session, the agent records
-- that it observed the same state again here instead of rewriting immutable history.
CREATE TABLE checkpoint_observations (
  checkpoint_id TEXT PRIMARY KEY REFERENCES checkpoints (id) ON DELETE CASCADE,
  last_observed_at TEXT NOT NULL,
  observation_count INTEGER NOT NULL CHECK (observation_count >= 1)
) STRICT;
