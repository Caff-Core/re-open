//! Atomic checkpoint persistence.

use std::collections::HashSet;

use reopen_core::{
    CaptureReason, CheckpointId, Completeness, Resource, SessionId, SourceStatus, Timestamp,
    WorkspaceId,
};
use rusqlite::{OptionalExtension as _, params};
use sha2::{Digest as _, Sha256};

use crate::{Store, StoreError};

/// A fully collected capture, ready to commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCheckpoint {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub captured_at: Timestamp,
    pub reason: CaptureReason,
    pub sources: Vec<SourceStatus>,
    pub resources: Vec<Resource>,
}

/// What a commit did. Returned only after the transaction has committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    /// A new immutable checkpoint was written.
    Created {
        id: CheckpointId,
        completeness: Completeness,
        content_hash: String,
    },
    /// The state matched the session's latest checkpoint; only its observation
    /// freshness was updated.
    Unchanged { id: CheckpointId },
}

fn enum_name<T: serde::Serialize>(value: &T) -> Result<String, StoreError> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => Ok(name),
        _ => Err(StoreError::InvalidData {
            detail: "enum did not serialise to a name".to_owned(),
        }),
    }
}

fn invalid(detail: impl Into<String>) -> StoreError {
    StoreError::InvalidData {
        detail: detail.into(),
    }
}

/// Validates a capture before any write. Details name fields, never values.
fn validate(checkpoint: &NewCheckpoint) -> Result<(), StoreError> {
    let mut keys = HashSet::new();
    for source in &checkpoint.sources {
        if source.source_key.is_empty() || !keys.insert(source.source_key.as_str()) {
            return Err(invalid("source keys must be non-empty and unique"));
        }
        source
            .validate()
            .map_err(|error| invalid(format!("source status: {error:?}")))?;
    }
    let mut ordinals = HashSet::new();
    for resource in &checkpoint.resources {
        if !ordinals.insert(resource.ordinal) {
            return Err(invalid("resource ordinals must be unique"));
        }
        resource
            .payload
            .validate()
            .map_err(|error| invalid(format!("resource payload: {error}")))?;
    }
    Ok(())
}

/// SHA-256 over the meaningful state: completeness, source states and the
/// resources in order. Timestamps are excluded so an identical capture taken
/// later hashes the same.
fn content_hash(
    checkpoint: &NewCheckpoint,
    completeness: Completeness,
) -> Result<String, StoreError> {
    let mut sources: Vec<_> = checkpoint
        .sources
        .iter()
        .map(|source| {
            (
                &source.source_key,
                source.state,
                source.required,
                source.error,
            )
        })
        .collect();
    sources.sort_by(|a, b| a.0.cmp(b.0));
    let mut resources: Vec<_> = checkpoint
        .resources
        .iter()
        .map(|resource| (resource.ordinal, &resource.adapter, &resource.payload))
        .collect();
    resources.sort_by_key(|resource| resource.0);

    let canonical = serde_json::to_vec(&(completeness, sources, resources))
        .map_err(|_| invalid("capture could not be encoded"))?;
    let digest = Sha256::digest(&canonical);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

impl Store {
    /// Validates and commits a capture in one transaction: the checkpoint, its
    /// source statuses and its resources become visible together or not at all.
    ///
    /// A scheduled or shutdown capture identical to the latest checkpoint of the
    /// same session only refreshes that checkpoint's observation time. Manual
    /// captures ("Save now") always create a checkpoint.
    ///
    /// # Errors
    ///
    /// [`StoreError::InvalidData`] before any write for an invalid capture;
    /// storage errors otherwise, in which case nothing was written.
    pub fn commit_checkpoint(
        &mut self,
        checkpoint: &NewCheckpoint,
    ) -> Result<CommitOutcome, StoreError> {
        validate(checkpoint)?;
        let completeness = Completeness::evaluate(&checkpoint.sources);
        let hash = content_hash(checkpoint, completeness)?;
        let captured_at = checkpoint.captured_at.to_string();
        let session = checkpoint.session_id.to_string();
        let workspace = checkpoint.workspace_id.to_string();

        let tx = self.conn_mut().transaction()?;

        if checkpoint.reason != CaptureReason::Manual {
            let latest: Option<(String, String)> = tx
                .query_row(
                    "SELECT id, content_hash FROM checkpoints
                     WHERE workspace_id = ?1 AND session_id = ?2
                     ORDER BY captured_at DESC, rowid DESC LIMIT 1",
                    params![workspace, session],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((id, latest_hash)) = latest
                && latest_hash == hash
            {
                tx.execute(
                    "UPDATE checkpoint_observations
                     SET last_observed_at = ?2, observation_count = observation_count + 1
                     WHERE checkpoint_id = ?1",
                    params![id, captured_at],
                )?;
                tx.commit()?;
                let id = id
                    .parse()
                    .map_err(|_| invalid("stored checkpoint id is not a UUID"))?;
                return Ok(CommitOutcome::Unchanged { id });
            }
        }

        let id = CheckpointId::new();
        let id_text = id.to_string();
        tx.execute(
            "INSERT INTO checkpoints
               (id, workspace_id, session_id, captured_at, reason, completeness, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id_text,
                workspace,
                session,
                captured_at,
                enum_name(&checkpoint.reason)?,
                enum_name(&completeness)?,
                hash
            ],
        )?;
        {
            let mut insert_source = tx.prepare(
                "INSERT INTO checkpoint_sources
                   (checkpoint_id, source_key, state, required, captured_at, last_seen_at, error_code)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for source in &checkpoint.sources {
                insert_source.execute(params![
                    id_text,
                    source.source_key,
                    source.state.as_str(),
                    source.required,
                    source.captured_at.map(|t| t.to_string()),
                    source.last_seen_at.map(|t| t.to_string()),
                    source.error.as_ref().map(enum_name).transpose()?,
                ])?;
            }
            let mut insert_resource = tx.prepare(
                "INSERT INTO resources
                   (id, checkpoint_id, kind, adapter_id, adapter_version, identity_key, ordinal, validated_payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for resource in &checkpoint.resources {
                let payload = serde_json::to_string(&resource.payload)
                    .map_err(|_| invalid("resource payload could not be encoded"))?;
                insert_resource.execute(params![
                    resource.id.to_string(),
                    id_text,
                    enum_name(&resource.kind())?,
                    resource.adapter.id,
                    resource.adapter.version,
                    resource.payload.identity_key(),
                    resource.ordinal,
                    payload,
                ])?;
            }
        }
        tx.execute(
            "INSERT INTO checkpoint_observations (checkpoint_id, last_observed_at, observation_count)
             VALUES (?1, ?2, 1)",
            params![id_text, captured_at],
        )?;
        tx.commit()?;
        Ok(CommitOutcome::Created {
            id,
            completeness,
            content_hash: hash,
        })
    }
}
