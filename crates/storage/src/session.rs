//! Agent sessions and previous-session recovery protection.

use reopen_core::{CheckpointId, SessionId, Timestamp};
use rusqlite::{OptionalExtension as _, Transaction, params};

use crate::{Store, StoreError};

/// The session that ran before this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviousSession {
    pub id: SessionId,
    /// `false` means no clean-exit marker: an unclean exit, not proof of a system failure.
    pub clean_exit: bool,
}

/// Result of recording a new session at agent startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStart {
    pub session_id: SessionId,
    pub previous: Option<PreviousSession>,
    /// The previous session's checkpoint now protected as a recovery candidate.
    pub new_candidate: Option<CheckpointId>,
}

/// An unresolved previous-session checkpoint offered for recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryCandidate {
    pub session_id: SessionId,
    pub checkpoint_id: CheckpointId,
    pub created_at: Timestamp,
}

/// How a candidate stopped being pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateDisposition {
    /// The user chose not to recover it.
    Dismissed,
    /// It was restored successfully.
    Resolved,
}

fn parse<T: std::str::FromStr>(text: &str, what: &'static str) -> Result<T, StoreError> {
    text.parse().map_err(|_| StoreError::InvalidData {
        detail: format!("stored {what} is invalid"),
    })
}

/// The previous session's latest useful checkpoint: one with at least one
/// resource, preferring complete over partial. A capture with no resources is
/// valid history but never displaces useful work, so it yields no candidate.
fn latest_useful_checkpoint(
    tx: &Transaction<'_>,
    session: &str,
) -> Result<Option<String>, StoreError> {
    Ok(tx
        .query_row(
            "SELECT c.id FROM checkpoints c
             WHERE c.session_id = ?1
               AND EXISTS (SELECT 1 FROM resources r WHERE r.checkpoint_id = c.id)
             ORDER BY (c.completeness = 'complete') DESC, c.captured_at DESC, c.rowid DESC
             LIMIT 1",
            params![session],
            |row| row.get(0),
        )
        .optional()?)
}

impl Store {
    /// Records a new session. In the same transaction, protects the previous
    /// session's latest useful checkpoint as a pending recovery candidate with
    /// its own pin, so no capture or retention in the new session can remove it.
    /// Call this before enabling capture.
    ///
    /// # Errors
    ///
    /// Storage errors; on failure nothing is recorded.
    pub fn start_session(&mut self, now: Timestamp) -> Result<SessionStart, StoreError> {
        let session_id = SessionId::new();
        let now_text = now.to_string();
        let tx = self.conn_mut().transaction()?;

        let previous: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT id, clean_exit_at FROM sessions ORDER BY started_at DESC, rowid DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        tx.execute(
            "INSERT INTO sessions (id, started_at, previous_session_id) VALUES (?1, ?2, ?3)",
            params![
                session_id.to_string(),
                now_text,
                previous.as_ref().map(|p| &p.0)
            ],
        )?;

        let mut new_candidate = None;
        if let Some((previous_id, _)) = &previous
            && let Some(checkpoint) = latest_useful_checkpoint(&tx, previous_id)?
        {
            tx.execute(
                "INSERT INTO recovery_candidates (session_id, checkpoint_id, disposition, created_at)
                 VALUES (?1, ?2, 'pending', ?3)",
                params![previous_id, checkpoint, now_text],
            )?;
            tx.execute(
                "INSERT INTO checkpoint_pins (checkpoint_id, reason, reference, created_at)
                 VALUES (?1, 'recovery_candidate', ?2, ?3)",
                params![checkpoint, previous_id, now_text],
            )?;
            new_candidate = Some(parse(&checkpoint, "checkpoint id")?);
        }
        tx.commit()?;

        let previous = previous
            .map(|(id, clean_exit_at)| {
                Ok::<_, StoreError>(PreviousSession {
                    id: parse(&id, "session id")?,
                    clean_exit: clean_exit_at.is_some(),
                })
            })
            .transpose()?;
        Ok(SessionStart {
            session_id,
            previous,
            new_candidate,
        })
    }

    /// Marks a session as cleanly exited on graceful shutdown.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotFound`] if the session does not exist; storage errors otherwise.
    pub fn mark_clean_exit(
        &mut self,
        session: SessionId,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let updated = self.conn().execute(
            "UPDATE sessions SET clean_exit_at = ?2 WHERE id = ?1",
            params![session.to_string(), now.to_string()],
        )?;
        if updated == 1 {
            Ok(())
        } else {
            Err(StoreError::NotFound)
        }
    }

    /// Pending recovery candidates, newest first. Several can exist after
    /// repeated restarts; each stays protected until it is resolved or dismissed.
    ///
    /// # Errors
    ///
    /// Storage errors.
    pub fn pending_candidates(&self) -> Result<Vec<RecoveryCandidate>, StoreError> {
        let mut statement = self.conn().prepare(
            "SELECT session_id, checkpoint_id, created_at FROM recovery_candidates
             WHERE disposition = 'pending' ORDER BY created_at DESC, rowid DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (session, checkpoint, created_at) = row?;
            Ok(RecoveryCandidate {
                session_id: parse(&session, "session id")?,
                checkpoint_id: parse(&checkpoint, "checkpoint id")?,
                created_at: Timestamp::parse(&created_at).map_err(|_| StoreError::InvalidData {
                    detail: "stored timestamp is invalid".to_owned(),
                })?,
            })
        })
        .collect()
    }

    /// Ends one pending candidate and releases only that candidate's pin. Other
    /// candidates and any user or restore pins on the same checkpoint remain.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotFound`] if no such pending candidate exists.
    pub fn close_candidate(
        &mut self,
        candidate: &RecoveryCandidate,
        disposition: CandidateDisposition,
    ) -> Result<(), StoreError> {
        let session = candidate.session_id.to_string();
        let checkpoint = candidate.checkpoint_id.to_string();
        let name = match disposition {
            CandidateDisposition::Dismissed => "dismissed",
            CandidateDisposition::Resolved => "resolved",
        };
        let tx = self.conn_mut().transaction()?;
        let updated = tx.execute(
            "UPDATE recovery_candidates SET disposition = ?3
             WHERE session_id = ?1 AND checkpoint_id = ?2 AND disposition = 'pending'",
            params![session, checkpoint, name],
        )?;
        if updated != 1 {
            return Err(StoreError::NotFound);
        }
        tx.execute(
            "DELETE FROM checkpoint_pins
             WHERE checkpoint_id = ?1 AND reason = 'recovery_candidate' AND reference = ?2",
            params![checkpoint, session],
        )?;
        tx.commit()?;
        Ok(())
    }
}
