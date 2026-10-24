#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use std::path::PathBuf;

use reopen_core::resource::ApplicationV1;
use reopen_core::{
    ActionState, AdapterRef, AttemptId, CaptureReason, CheckpointId, Resource, ResourceId,
    ResourcePayload, RunState, SourceState, SourceStatus, Timestamp,
};
use reopen_storage::{CommitOutcome, NewAction, NewCheckpoint, Store, StoreError};
use rusqlite::{Connection, params};
use tempfile::TempDir;

fn at(minute: u32) -> Timestamp {
    Timestamp::parse(&format!("2026-10-24T10:{minute:02}:00Z")).unwrap()
}

struct Fixture {
    _dir: TempDir,
    path: PathBuf,
    store: Store,
    checkpoint: CheckpointId,
    resources: Vec<ResourceId>,
}

/// A checkpoint with three apps in a previous session, now a pending
/// recovery candidate of a new session.
fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("reopen.sqlite");
    let mut store = Store::open(&path, at(0)).unwrap();
    let workspace = store.ensure_default_workspace(at(0)).unwrap();
    let session = store.start_session(at(1)).unwrap().session_id;
    let resources: Vec<_> = (0..3)
        .map(|ordinal| Resource {
            id: ResourceId::new(),
            adapter: AdapterRef {
                id: "fake.apps".to_owned(),
                version: 1,
            },
            ordinal,
            payload: ResourcePayload::ApplicationV1(ApplicationV1 {
                app_id: format!("org.example.App{ordinal}.desktop"),
                display_name: format!("App {ordinal}"),
            }),
        })
        .collect();
    let ids = resources.iter().map(|r| r.id).collect();
    let outcome = store
        .commit_checkpoint(&NewCheckpoint {
            workspace_id: workspace,
            session_id: session,
            captured_at: at(2),
            reason: CaptureReason::Scheduled,
            sources: vec![SourceStatus {
                source_key: "desktop.apps".to_owned(),
                state: SourceState::Complete,
                required: true,
                captured_at: Some(at(2)),
                last_seen_at: Some(at(2)),
                error: None,
            }],
            resources,
        })
        .unwrap();
    let CommitOutcome::Created { id: checkpoint, .. } = outcome else {
        panic!()
    };
    assert_eq!(
        store.start_session(at(3)).unwrap().new_candidate,
        Some(checkpoint)
    );
    Fixture {
        _dir: dir,
        path,
        store,
        checkpoint,
        resources: ids,
    }
}

fn pending(f: &Fixture) -> Vec<NewAction> {
    f.resources
        .iter()
        .zip(0..)
        .map(|(id, ordinal)| NewAction {
            ordinal,
            resource_id: *id,
            initial: ActionState::Pending,
            result_code: None,
        })
        .collect()
}

fn pins(f: &Fixture) -> Vec<String> {
    let conn = Connection::open(&f.path).unwrap();
    let mut statement = conn
        .prepare("SELECT reason FROM checkpoint_pins WHERE checkpoint_id = ?1 ORDER BY reason")
        .unwrap();
    statement
        .query_map(params![f.checkpoint.to_string()], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn a_run_is_journaled_with_a_restore_pin() {
    let mut f = fixture();
    let actions = pending(&f);
    let (run, ids) = f
        .store
        .create_restore_run(f.checkpoint, &actions, at(4))
        .unwrap();
    assert_eq!(ids.len(), 3);
    assert_eq!(pins(&f), ["recovery_candidate", "restore_run"]);
    let record = f.store.restore_run(run).unwrap();
    assert_eq!(record.state, RunState::Queued);
    assert!(
        record
            .actions
            .iter()
            .all(|a| a.state == ActionState::Pending)
    );
}

#[test]
fn resources_from_another_checkpoint_are_refused_atomically() {
    let mut f = fixture();
    let mut actions = pending(&f);
    actions[1].resource_id = ResourceId::new();
    assert!(matches!(
        f.store.create_restore_run(f.checkpoint, &actions, at(4)),
        Err(StoreError::NotFound)
    ));
    let runs: i64 = Connection::open(&f.path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM restore_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(runs, 0);
    assert_eq!(pins(&f), ["recovery_candidate"]);
}

#[test]
fn attempt_markers_precede_outcomes_and_cannot_repeat() {
    let mut f = fixture();
    let actions = pending(&f);
    let (run, ids) = f
        .store
        .create_restore_run(f.checkpoint, &actions, at(4))
        .unwrap();
    let attempt = AttemptId::new();
    f.store.mark_action_launching(ids[0], attempt).unwrap();
    assert!(matches!(
        f.store.mark_action_launching(ids[0], AttemptId::new()),
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        f.store.finish_action(ids[0], ActionState::Launching, None),
        Err(StoreError::InvalidData { .. })
    ));
    f.store
        .finish_action(ids[0], ActionState::Restored, None)
        .unwrap();
    assert!(matches!(
        f.store.finish_action(ids[0], ActionState::Failed, None),
        Err(StoreError::NotFound)
    ));

    let record = f.store.restore_run(run).unwrap();
    assert_eq!(
        (record.actions[0].state, record.actions[0].attempt_id),
        (ActionState::Restored, Some(attempt))
    );
}

#[test]
fn a_fully_successful_run_releases_its_pin_and_resolves_the_candidate() {
    let mut f = fixture();
    let mut actions = pending(&f);
    actions[2].initial = ActionState::AlreadyOpen;
    let (run, ids) = f
        .store
        .create_restore_run(f.checkpoint, &actions, at(4))
        .unwrap();
    f.store
        .set_run_state(run, RunState::Running, at(4))
        .unwrap();
    for id in &ids[..2] {
        f.store
            .mark_action_launching(*id, AttemptId::new())
            .unwrap();
        f.store
            .finish_action(*id, ActionState::Restored, None)
            .unwrap();
    }
    f.store
        .set_run_state(run, RunState::Completed, at(5))
        .unwrap();
    assert!(pins(&f).is_empty());
    assert!(f.store.pending_candidates().unwrap().is_empty());
    assert!(f.store.restore_run(run).unwrap().completed_at.is_some());
    assert!(matches!(
        f.store.set_run_state(run, RunState::Running, at(6)),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn a_failed_action_keeps_the_candidate_protected() {
    let mut f = fixture();
    let actions = pending(&f);
    let (run, ids) = f
        .store
        .create_restore_run(f.checkpoint, &actions, at(4))
        .unwrap();
    f.store
        .finish_action(ids[0], ActionState::Restored, None)
        .unwrap();
    f.store
        .finish_action(ids[1], ActionState::Failed, Some("executable_missing"))
        .unwrap();
    f.store
        .finish_action(ids[2], ActionState::Skipped, Some("cancelled"))
        .unwrap();
    f.store
        .set_run_state(run, RunState::Completed, at(5))
        .unwrap();
    assert_eq!(
        pins(&f),
        ["recovery_candidate"],
        "the restore pin goes, the candidate stays"
    );
    assert_eq!(f.store.pending_candidates().unwrap().len(), 1);
}

#[test]
fn a_crash_mid_launch_becomes_uncertain_and_is_never_replayed() {
    let mut f = fixture();
    let actions = pending(&f);
    let (run, ids) = f
        .store
        .create_restore_run(f.checkpoint, &actions, at(4))
        .unwrap();
    f.store
        .set_run_state(run, RunState::Running, at(4))
        .unwrap();
    f.store
        .finish_action(ids[0], ActionState::Restored, None)
        .unwrap();
    f.store
        .mark_action_launching(ids[1], AttemptId::new())
        .unwrap();
    drop(f.store);

    // The agent restarts.
    let mut store = Store::open(&f.path, at(10)).unwrap();
    assert_eq!(store.reconcile_interrupted_restores(at(10)).unwrap(), 1);
    assert_eq!(
        store.reconcile_interrupted_restores(at(11)).unwrap(),
        0,
        "idempotent"
    );
    let record = store.restore_run(run).unwrap();
    assert_eq!(record.state, RunState::Interrupted);
    let states: Vec<_> = record.actions.iter().map(|a| a.state).collect();
    assert_eq!(
        states,
        [
            ActionState::Restored,
            ActionState::Uncertain,
            ActionState::Pending
        ]
    );
    assert_eq!(
        record.actions[1].result_code.as_deref(),
        Some("interrupted")
    );
    assert!(matches!(
        store.mark_action_launching(ids[1], AttemptId::new()),
        Err(StoreError::NotFound)
    ));
    f.store = store;
    assert_eq!(
        pins(&f),
        ["recovery_candidate", "restore_run"],
        "unresolved journals keep their pin"
    );
}
