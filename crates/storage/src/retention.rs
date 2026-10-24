//! Pins and retention.
//!
//! Retention only ever deletes eligible automatic checkpoints: never manual
//! checkpoints, never pinned ones (user, restore or recovery-candidate pins),
//! never one a restore run references, and never a workspace's newest
//! checkpoint. It runs as its own bounded transaction after a successful
//! capture and reports storage pressure instead of deleting protected data.

use std::time::Duration;

use reopen_core::{CheckpointId, Timestamp};
use rusqlite::{Transaction, params};

use crate::{Store, StoreError};

/// Why a checkpoint is pinned by the user or a restore. Recovery-candidate
/// pins are managed by sessions and cannot be set or cleared here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinReason {
    /// The user chose to keep it.
    User,
    /// A restore run uses it until its journal is resolved; the reference is the run ID.
    RestoreRun,
}

impl PinReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::RestoreRun => "restore_run",
        }
    }
}

/// Retention limits (docs/PRODUCT.md, "Defaults and limits").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Automatic checkpoints last observed longer ago than this are eligible.
    pub max_age: Duration,
    /// Unpinned automatic checkpoints kept per workspace.
    pub max_automatic: u32,
    /// Used database size above which the oldest eligible checkpoints are removed.
    pub storage_budget_bytes: u64,
    /// Upper bound on deletions in one run, so a run stays short.
    pub max_deletions_per_run: u32,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_age: Duration::from_hours(24 * 7),
            max_automatic: 500,
            storage_budget_bytes: 100 * 1024 * 1024,
            max_deletions_per_run: 200,
        }
    }
}

/// What a retention run did and whether storage is under pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionReport {
    pub deleted: u32,
    /// The deletion limit was reached; run again to continue.
    pub more_pending: bool,
    /// Bytes in use by the database (pages minus free pages).
    pub used_bytes: u64,
    pub over_budget: bool,
    /// Over budget and nothing more is eligible: pins or manual checkpoints
    /// prevent compliance, so the user must prune.
    pub budget_blocked: bool,
}

/// Automatic, unpinned, unreferenced, and not the newest of its workspace.
const ELIGIBLE: &str = "
    c.reason IN ('scheduled', 'shutdown')
    AND NOT EXISTS (SELECT 1 FROM checkpoint_pins p WHERE p.checkpoint_id = c.id)
    AND NOT EXISTS (SELECT 1 FROM restore_runs r WHERE r.checkpoint_id = c.id)
    AND NOT EXISTS (
      SELECT 1 FROM recovery_candidates rc
      WHERE rc.checkpoint_id = c.id AND rc.disposition = 'pending')
    AND c.id <> (
      SELECT n.id FROM checkpoints n WHERE n.workspace_id = c.workspace_id
      ORDER BY n.captured_at DESC, n.rowid DESC LIMIT 1)";

fn used_bytes(tx: &Transaction<'_>) -> Result<u64, StoreError> {
    let (pages, free, size): (i64, i64, i64) = tx.query_row(
        "SELECT (SELECT page_count FROM pragma_page_count()),
                (SELECT freelist_count FROM pragma_freelist_count()),
                (SELECT page_size FROM pragma_page_size())",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    Ok(u64::try_from((pages - free).max(0) * size).unwrap_or(0))
}

fn delete_checkpoints(tx: &Transaction<'_>, ids: &[String]) -> Result<u32, StoreError> {
    let mut deleted = 0;
    for id in ids {
        // Closed candidate rows are history that still reference the checkpoint.
        tx.execute(
            "DELETE FROM recovery_candidates WHERE checkpoint_id = ?1 AND disposition <> 'pending'",
            params![id],
        )?;
        deleted += u32::try_from(tx.execute("DELETE FROM checkpoints WHERE id = ?1", params![id])?)
            .unwrap_or(0);
    }
    Ok(deleted)
}

fn select_ids(
    tx: &Transaction<'_>,
    sql: &str,
    args: impl rusqlite::Params,
) -> Result<Vec<String>, StoreError> {
    let mut statement = tx.prepare(sql)?;
    let rows = statement.query_map(args, |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

impl Store {
    /// Pins a checkpoint. Pinning twice with the same reason and reference is a no-op.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotFound`] if the checkpoint does not exist.
    pub fn pin_checkpoint(
        &mut self,
        checkpoint: CheckpointId,
        reason: PinReason,
        reference: &str,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let id = checkpoint.to_string();
        let exists: bool = self.conn().query_row(
            "SELECT EXISTS (SELECT 1 FROM checkpoints WHERE id = ?1)",
            params![id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StoreError::NotFound);
        }
        self.conn().execute(
            "INSERT OR IGNORE INTO checkpoint_pins (checkpoint_id, reason, reference, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, reason.as_str(), reference, now.to_string()],
        )?;
        Ok(())
    }

    /// Removes one pin; other pins on the same checkpoint remain.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotFound`] if that pin does not exist.
    pub fn unpin_checkpoint(
        &mut self,
        checkpoint: CheckpointId,
        reason: PinReason,
        reference: &str,
    ) -> Result<(), StoreError> {
        let removed = self.conn().execute(
            "DELETE FROM checkpoint_pins WHERE checkpoint_id = ?1 AND reason = ?2 AND reference = ?3",
            params![checkpoint.to_string(), reason.as_str(), reference],
        )?;
        if removed == 1 {
            Ok(())
        } else {
            Err(StoreError::NotFound)
        }
    }

    /// Deletes eligible automatic checkpoints that exceed the age or count
    /// limit, then, while over the storage budget, the oldest remaining
    /// eligible ones. One bounded transaction; call again if `more_pending`.
    ///
    /// # Errors
    ///
    /// Storage errors; on failure nothing is deleted.
    pub fn apply_retention(
        &mut self,
        policy: &RetentionPolicy,
        now: Timestamp,
    ) -> Result<RetentionReport, StoreError> {
        let cutoff = now.saturating_sub(policy.max_age).to_string();
        let limit = i64::from(policy.max_deletions_per_run);
        let tx = self.conn_mut().transaction()?;

        let expired = select_ids(
            &tx,
            &format!(
                // Rank among all unpinned automatic checkpoints (the population
                // the count limit applies to), then delete only eligible ones.
                "SELECT id FROM (
                   SELECT c.id, c.captured_at, c.rowid AS rid,
                          COALESCE(o.last_observed_at, c.captured_at) AS seen,
                          ROW_NUMBER() OVER (
                            PARTITION BY c.workspace_id ORDER BY c.captured_at DESC, c.rowid DESC) AS rank,
                          CASE WHEN {ELIGIBLE} THEN 1 ELSE 0 END AS eligible
                   FROM checkpoints c LEFT JOIN checkpoint_observations o ON o.checkpoint_id = c.id
                   WHERE c.reason IN ('scheduled', 'shutdown')
                     AND NOT EXISTS (SELECT 1 FROM checkpoint_pins p WHERE p.checkpoint_id = c.id))
                 WHERE eligible = 1 AND (seen < ?1 OR rank > ?2)
                 ORDER BY captured_at ASC, rid ASC LIMIT ?3"
            ),
            params![cutoff, policy.max_automatic, limit],
        )?;
        let mut deleted = delete_checkpoints(&tx, &expired)?;

        let mut used = used_bytes(&tx)?;
        while used > policy.storage_budget_bytes && deleted < policy.max_deletions_per_run {
            let batch = (policy.max_deletions_per_run - deleted).min(10);
            let oldest = select_ids(
                &tx,
                &format!(
                    "SELECT c.id FROM checkpoints c WHERE {ELIGIBLE} ORDER BY c.captured_at ASC, c.rowid ASC LIMIT ?1"
                ),
                params![batch],
            )?;
            if oldest.is_empty() {
                break;
            }
            deleted += delete_checkpoints(&tx, &oldest)?;
            used = used_bytes(&tx)?;
        }

        let remaining_eligible: bool = tx.query_row(
            &format!("SELECT EXISTS (SELECT 1 FROM checkpoints c WHERE {ELIGIBLE})"),
            [],
            |row| row.get(0),
        )?;
        tx.commit()?;

        let over_budget = used > policy.storage_budget_bytes;
        Ok(RetentionReport {
            deleted,
            more_pending: deleted >= policy.max_deletions_per_run,
            used_bytes: used,
            over_budget,
            budget_blocked: over_budget && !remaining_eligible,
        })
    }
}
