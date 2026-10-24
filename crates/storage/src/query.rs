//! Read queries for status and history.

use reopen_core::checkpoint::ProviderError;
use reopen_core::{
    AdapterRef, CaptureReason, CheckpointId, Completeness, Resource, ResourcePayload, SessionId,
    SourceState, SourceStatus, Timestamp,
};
use rusqlite::{OptionalExtension as _, Row, params};

use crate::{Store, StoreError};

/// One checkpoint in history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointSummary {
    pub id: CheckpointId,
    pub session_id: SessionId,
    pub captured_at: Timestamp,
    pub reason: CaptureReason,
    pub completeness: Completeness,
    pub resource_count: u32,
    /// Pinned or a pending recovery candidate.
    pub protected: bool,
}

/// A checkpoint with its sources and resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointDetail {
    pub summary: CheckpointSummary,
    pub sources: Vec<SourceStatus>,
    pub resources: Vec<Resource>,
}

fn invalid(what: &str) -> StoreError {
    StoreError::InvalidData {
        detail: format!("stored {what} is invalid"),
    }
}

fn from_name<T: serde::de::DeserializeOwned>(name: &str, what: &str) -> Result<T, StoreError> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).map_err(|_| invalid(what))
}

fn timestamp(text: &str) -> Result<Timestamp, StoreError> {
    Timestamp::parse(text).map_err(|_| invalid("timestamp"))
}

const SUMMARY_COLUMNS: &str = "
    c.id, c.session_id, c.captured_at, c.reason, c.completeness,
    (SELECT COUNT(*) FROM resources r WHERE r.checkpoint_id = c.id),
    EXISTS (SELECT 1 FROM checkpoint_pins p WHERE p.checkpoint_id = c.id)";

type RawSummary = (String, String, String, String, String, i64, bool);

fn raw_summary(row: &Row<'_>) -> rusqlite::Result<RawSummary> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

fn summary(raw: RawSummary) -> Result<CheckpointSummary, StoreError> {
    let (id, session, captured_at, reason, completeness, count, protected) = raw;
    Ok(CheckpointSummary {
        id: id.parse().map_err(|_| invalid("checkpoint id"))?,
        session_id: session.parse().map_err(|_| invalid("session id"))?,
        captured_at: timestamp(&captured_at)?,
        reason: from_name(&reason, "reason")?,
        completeness: from_name(&completeness, "completeness")?,
        resource_count: u32::try_from(count).map_err(|_| invalid("resource count"))?,
        protected,
    })
}

impl Store {
    /// Newest checkpoints first, at most `limit`.
    ///
    /// # Errors
    ///
    /// Storage errors or invalid stored data.
    pub fn list_checkpoints(&self, limit: u32) -> Result<Vec<CheckpointSummary>, StoreError> {
        let mut statement = self.conn().prepare(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM checkpoints c ORDER BY c.captured_at DESC, c.rowid DESC LIMIT ?1"
        ))?;
        let rows = statement.query_map(params![limit], raw_summary)?;
        rows.map(|row| summary(row?)).collect()
    }

    /// The newest checkpoint, if any.
    ///
    /// # Errors
    ///
    /// Storage errors or invalid stored data.
    pub fn latest_checkpoint(&self) -> Result<Option<CheckpointSummary>, StoreError> {
        Ok(self.list_checkpoints(1)?.into_iter().next())
    }

    /// One checkpoint with its sources and resources in order.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotFound`] if it does not exist; storage errors or invalid
    /// stored data otherwise. Stored payloads are revalidated on read.
    pub fn checkpoint_detail(&self, id: CheckpointId) -> Result<CheckpointDetail, StoreError> {
        let id_text = id.to_string();
        let raw = self
            .conn()
            .query_row(
                &format!("SELECT {SUMMARY_COLUMNS} FROM checkpoints c WHERE c.id = ?1"),
                params![id_text],
                raw_summary,
            )
            .optional()?
            .ok_or(StoreError::NotFound)?;
        let summary = summary(raw)?;

        let mut statement = self.conn().prepare(
            "SELECT source_key, state, required, captured_at, last_seen_at, error_code
             FROM checkpoint_sources WHERE checkpoint_id = ?1 ORDER BY source_key",
        )?;
        let sources = statement
            .query_map(params![id_text], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .map(|row| {
                let (source_key, state, required, captured_at, last_seen_at, error) = row?;
                Ok(SourceStatus {
                    source_key,
                    state: state
                        .parse::<SourceState>()
                        .map_err(|_| invalid("source state"))?,
                    required,
                    captured_at: captured_at.as_deref().map(timestamp).transpose()?,
                    last_seen_at: last_seen_at.as_deref().map(timestamp).transpose()?,
                    error: error
                        .as_deref()
                        .map(|e| from_name::<ProviderError>(e, "error code"))
                        .transpose()?,
                })
            })
            .collect::<Result<_, StoreError>>()?;

        let mut statement = self.conn().prepare(
            "SELECT id, adapter_id, adapter_version, ordinal, validated_payload
             FROM resources WHERE checkpoint_id = ?1 ORDER BY ordinal",
        )?;
        let resources = statement
            .query_map(params![id_text], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .map(|row| {
                let (resource_id, adapter_id, version, ordinal, payload) = row?;
                let payload: ResourcePayload =
                    serde_json::from_str(&payload).map_err(|_| invalid("resource payload"))?;
                payload
                    .validate()
                    .map_err(|_| invalid("resource payload"))?;
                Ok(Resource {
                    id: resource_id.parse().map_err(|_| invalid("resource id"))?,
                    adapter: AdapterRef {
                        id: adapter_id,
                        version,
                    },
                    ordinal,
                    payload,
                })
            })
            .collect::<Result<_, StoreError>>()?;

        Ok(CheckpointDetail {
            summary,
            sources,
            resources,
        })
    }

    /// Number of pending recovery candidates.
    ///
    /// # Errors
    ///
    /// Storage errors.
    pub fn pending_candidate_count(&self) -> Result<u32, StoreError> {
        let count: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM recovery_candidates WHERE disposition = 'pending'",
            [],
            |row| row.get(0),
        )?;
        u32::try_from(count).map_err(|_| invalid("candidate count"))
    }
}
