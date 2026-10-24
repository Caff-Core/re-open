//! The restore journal: runs, actions, attempt markers and reconciliation.
//!
//! An action is marked `launching` (with a fresh attempt ID) and committed
//! before its side effect, then given its result. After a crash, `launching`
//! actions become `uncertain` and are never replayed. A run pins its
//! checkpoint until its journal is resolved.

use reopen_core::{
    ActionState, AttemptId, CheckpointId, ResourceId, RestoreActionId, RestoreRunId, RunState,
    Timestamp,
};
use rusqlite::{OptionalExtension as _, params};

use crate::{Store, StoreError};

/// An action to journal when a run starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAction {
    pub ordinal: u32,
    pub resource_id: ResourceId,
    /// `Pending` for actions to execute; `AlreadyOpen` or `Skipped` for ones
    /// decided at planning time.
    pub initial: ActionState,
    pub result_code: Option<String>,
}

/// One journaled action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRecord {
    pub id: RestoreActionId,
    pub ordinal: u32,
    pub resource_id: ResourceId,
    pub state: ActionState,
    pub attempt_id: Option<AttemptId>,
    pub result_code: Option<String>,
}

/// A run and its actions in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub id: RestoreRunId,
    pub checkpoint_id: CheckpointId,
    pub state: RunState,
    pub created_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    pub actions: Vec<ActionRecord>,
}

fn invalid(what: &str) -> StoreError {
    StoreError::InvalidData {
        detail: format!("stored {what} is invalid"),
    }
}

fn state_name<T: serde::Serialize>(state: &T) -> Result<String, StoreError> {
    match serde_json::to_value(state) {
        Ok(serde_json::Value::String(name)) => Ok(name),
        _ => Err(invalid("state")),
    }
}

fn parse_state<T: serde::de::DeserializeOwned>(name: String) -> Result<T, StoreError> {
    serde_json::from_value(serde_json::Value::String(name)).map_err(|_| invalid("state"))
}

impl Store {
    /// Journals a new run and its actions, and pins the checkpoint for the
    /// run, in one transaction.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotFound`] if the checkpoint or a resource does not exist.
    pub fn create_restore_run(
        &mut self,
        checkpoint: CheckpointId,
        actions: &[NewAction],
        now: Timestamp,
    ) -> Result<(RestoreRunId, Vec<RestoreActionId>), StoreError> {
        let run = RestoreRunId::new();
        let checkpoint_text = checkpoint.to_string();
        let now_text = now.to_string();
        let tx = self.conn_mut().transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM checkpoints WHERE id = ?1)",
            params![checkpoint_text],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StoreError::NotFound);
        }
        tx.execute(
            "INSERT INTO restore_runs (id, checkpoint_id, state, created_at) VALUES (?1, ?2, 'queued', ?3)",
            params![run.to_string(), checkpoint_text, now_text],
        )?;
        let mut ids = Vec::with_capacity(actions.len());
        for action in actions {
            let belongs: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM resources WHERE id = ?1 AND checkpoint_id = ?2)",
                params![action.resource_id.to_string(), checkpoint_text],
                |row| row.get(0),
            )?;
            if !belongs {
                return Err(StoreError::NotFound);
            }
            let id = RestoreActionId::new();
            tx.execute(
                "INSERT INTO restore_actions (id, run_id, ordinal, resource_id, state, result_code)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id.to_string(),
                    run.to_string(),
                    action.ordinal,
                    action.resource_id.to_string(),
                    state_name(&action.initial)?,
                    action.result_code
                ],
            )?;
            ids.push(id);
        }
        tx.execute(
            "INSERT OR IGNORE INTO checkpoint_pins (checkpoint_id, reason, reference, created_at)
             VALUES (?1, 'restore_run', ?2, ?3)",
            params![checkpoint_text, run.to_string(), now_text],
        )?;
        tx.commit()?;
        Ok((run, ids))
    }

    /// Records the attempt marker. Must commit before the side effect starts.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotFound`] unless the action is `pending`.
    pub fn mark_action_launching(
        &mut self,
        action: RestoreActionId,
        attempt: AttemptId,
    ) -> Result<(), StoreError> {
        let updated = self.conn().execute(
            "UPDATE restore_actions SET state = 'launching', attempt_id = ?2 WHERE id = ?1 AND state = 'pending'",
            params![action.to_string(), attempt.to_string()],
        )?;
        if updated == 1 {
            Ok(())
        } else {
            Err(StoreError::NotFound)
        }
    }

    /// Records an action's outcome.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotFound`] unless the action is `pending` or `launching`;
    /// [`StoreError::InvalidData`] for a non-final state.
    pub fn finish_action(
        &mut self,
        action: RestoreActionId,
        state: ActionState,
        result_code: Option<&str>,
    ) -> Result<(), StoreError> {
        if !state.is_final() {
            return Err(StoreError::InvalidData {
                detail: "an outcome must be a final state".to_owned(),
            });
        }
        let updated = self.conn().execute(
            "UPDATE restore_actions SET state = ?2, result_code = ?3
             WHERE id = ?1 AND state IN ('pending', 'launching')",
            params![action.to_string(), state_name(&state)?, result_code],
        )?;
        if updated == 1 {
            Ok(())
        } else {
            Err(StoreError::NotFound)
        }
    }

    /// Moves a run to a new state. On a terminal state, its restore pin is
    /// released once no action is `uncertain`, and a completed run in which
    /// every action restored or was already open resolves the checkpoint's
    /// pending recovery candidates.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotFound`] if the run does not exist or is already terminal.
    pub fn set_run_state(
        &mut self,
        run: RestoreRunId,
        state: RunState,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let run_text = run.to_string();
        let tx = self.conn_mut().transaction()?;
        let completed_at = state.is_terminal().then(|| now.to_string());
        let updated = tx.execute(
            "UPDATE restore_runs SET state = ?2, completed_at = ?3
             WHERE id = ?1 AND state NOT IN ('completed', 'cancelled', 'interrupted')",
            params![run_text, state_name(&state)?, completed_at],
        )?;
        if updated != 1 {
            return Err(StoreError::NotFound);
        }
        if state.is_terminal() {
            let checkpoint: String = tx.query_row(
                "SELECT checkpoint_id FROM restore_runs WHERE id = ?1",
                params![run_text],
                |row| row.get(0),
            )?;
            let (uncertain, unsuccessful): (i64, i64) = tx.query_row(
                "SELECT COUNT(*) FILTER (WHERE state = 'uncertain'),
                        COUNT(*) FILTER (WHERE state NOT IN ('restored', 'already_open'))
                 FROM restore_actions WHERE run_id = ?1",
                params![run_text],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if uncertain == 0 {
                tx.execute(
                    "DELETE FROM checkpoint_pins WHERE checkpoint_id = ?1 AND reason = 'restore_run' AND reference = ?2",
                    params![checkpoint, run_text],
                )?;
            }
            if state == RunState::Completed && unsuccessful == 0 {
                tx.execute(
                    "DELETE FROM checkpoint_pins WHERE checkpoint_id = ?1 AND reason = 'recovery_candidate'
                       AND reference IN (SELECT session_id FROM recovery_candidates
                                         WHERE checkpoint_id = ?1 AND disposition = 'pending')",
                    params![checkpoint],
                )?;
                tx.execute(
                    "UPDATE recovery_candidates SET disposition = 'resolved'
                     WHERE checkpoint_id = ?1 AND disposition = 'pending'",
                    params![checkpoint],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// At startup, before any new restore: unfinished runs become
    /// `interrupted` and `launching` actions become `uncertain`. Their pins
    /// remain until the user reviews them. Returns the runs reconciled.
    ///
    /// # Errors
    ///
    /// Storage errors; on failure nothing changes.
    pub fn reconcile_interrupted_restores(&mut self, now: Timestamp) -> Result<u32, StoreError> {
        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "UPDATE restore_actions SET state = 'uncertain', result_code = 'interrupted'
             WHERE state = 'launching'",
            [],
        )?;
        let runs = tx.execute(
            "UPDATE restore_runs SET state = 'interrupted', completed_at = ?1
             WHERE state IN ('queued', 'running', 'paused')",
            params![now.to_string()],
        )?;
        tx.commit()?;
        u32::try_from(runs).map_err(|_| invalid("run count"))
    }

    /// A run with its actions in order.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotFound`] if the run does not exist.
    pub fn restore_run(&self, run: RestoreRunId) -> Result<RunRecord, StoreError> {
        let run_text = run.to_string();
        let (checkpoint, state, created_at, completed_at): (String, String, String, Option<String>) = self
            .conn()
            .query_row(
                "SELECT checkpoint_id, state, created_at, completed_at FROM restore_runs WHERE id = ?1",
                params![run_text],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
            .ok_or(StoreError::NotFound)?;
        let mut statement = self.conn().prepare(
            "SELECT id, ordinal, resource_id, state, attempt_id, result_code
             FROM restore_actions WHERE run_id = ?1 ORDER BY ordinal",
        )?;
        let actions = statement
            .query_map(params![run_text], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .map(|row| {
                let (id, ordinal, resource, state, attempt, result_code) = row?;
                Ok(ActionRecord {
                    id: id.parse().map_err(|_| invalid("action id"))?,
                    ordinal,
                    resource_id: resource.parse().map_err(|_| invalid("resource id"))?,
                    state: parse_state(state)?,
                    attempt_id: attempt
                        .map(|a| a.parse().map_err(|_| invalid("attempt id")))
                        .transpose()?,
                    result_code,
                })
            })
            .collect::<Result<_, StoreError>>()?;
        let timestamp = |text: &str| Timestamp::parse(text).map_err(|_| invalid("timestamp"));
        Ok(RunRecord {
            id: run,
            checkpoint_id: checkpoint.parse().map_err(|_| invalid("checkpoint id"))?,
            state: parse_state(state)?,
            created_at: timestamp(&created_at)?,
            completed_at: completed_at.as_deref().map(timestamp).transpose()?,
            actions,
        })
    }
}
