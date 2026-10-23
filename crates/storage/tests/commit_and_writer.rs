#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use reopen_core::checkpoint::ProviderError;
use reopen_core::resource::{ApplicationV1, FolderV1};
use reopen_core::{
    AdapterRef, CaptureReason, Completeness, Resource, ResourceId, ResourcePayload, SessionId,
    SourceState, SourceStatus, Timestamp, WorkspaceId,
};
use reopen_storage::writer::{self, DEFAULT_QUEUE_CAPACITY};
use reopen_storage::{CommitOutcome, NewCheckpoint, Store, StoreError, WriterError};
use rusqlite::{Connection, params};
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

struct Fixture {
    _dir: TempDir,
    path: PathBuf,
    store: Store,
    workspace: WorkspaceId,
    session: SessionId,
}

fn insert_session(path: &Path, session: SessionId) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO sessions (id, started_at) VALUES (?1, '2026-10-23T11:00:00Z')",
            params![session.to_string()],
        )
        .unwrap();
}

fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("reopen.sqlite");
    let store = Store::open(&path, at("2026-10-23T11:00:00Z")).unwrap();
    let workspace = store
        .ensure_default_workspace(at("2026-10-23T11:00:00Z"))
        .unwrap();
    let session = SessionId::new();
    insert_session(&path, session);
    Fixture {
        _dir: dir,
        path,
        store,
        workspace,
        session,
    }
}

fn app(ordinal: u32, app_id: &str) -> Resource {
    Resource {
        id: ResourceId::new(),
        adapter: AdapterRef {
            id: "fake.apps".to_owned(),
            version: 1,
        },
        ordinal,
        payload: ResourcePayload::ApplicationV1(ApplicationV1 {
            app_id: app_id.to_owned(),
            display_name: app_id.to_owned(),
        }),
    }
}

fn source(state: SourceState, captured_at: &str) -> SourceStatus {
    SourceStatus {
        source_key: "desktop.apps".to_owned(),
        state,
        required: true,
        captured_at: Some(at(captured_at)),
        last_seen_at: Some(at(captured_at)),
        error: None,
    }
}

fn capture(f: &Fixture, captured_at: &str, reason: CaptureReason, apps: &[&str]) -> NewCheckpoint {
    NewCheckpoint {
        workspace_id: f.workspace,
        session_id: f.session,
        captured_at: at(captured_at),
        reason,
        sources: vec![source(SourceState::Complete, captured_at)],
        resources: apps
            .iter()
            .zip(0..)
            .map(|(id, ordinal)| app(ordinal, id))
            .collect(),
    }
}

fn count(path: &Path, table: &str) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn commit_writes_checkpoint_sources_and_resources_together() {
    let mut f = fixture();
    let outcome = f
        .store
        .commit_checkpoint(&capture(
            &f,
            "2026-10-23T12:00:00Z",
            CaptureReason::Scheduled,
            &["code", "brave"],
        ))
        .unwrap();
    let CommitOutcome::Created {
        completeness,
        content_hash,
        ..
    } = outcome
    else {
        panic!("expected a new checkpoint, got {outcome:?}")
    };
    assert_eq!(completeness, Completeness::Complete);
    assert_eq!(content_hash.len(), 64);
    assert_eq!(count(&f.path, "checkpoints"), 1);
    assert_eq!(count(&f.path, "checkpoint_sources"), 1);
    assert_eq!(count(&f.path, "resources"), 2);

    let payload: String = Connection::open(&f.path)
        .unwrap()
        .query_row(
            "SELECT validated_payload FROM resources WHERE ordinal = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let payload: ResourcePayload = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload, app(1, "brave").payload);
}

#[test]
fn partial_sources_are_stored_as_partial() {
    let mut f = fixture();
    let mut checkpoint = capture(
        &f,
        "2026-10-23T12:00:00Z",
        CaptureReason::Scheduled,
        &["code"],
    );
    checkpoint.sources.push(SourceStatus {
        source_key: "browser:a".to_owned(),
        state: SourceState::Unavailable,
        required: true,
        captured_at: None,
        last_seen_at: None,
        error: Some(ProviderError::Unavailable),
    });
    let outcome = f.store.commit_checkpoint(&checkpoint).unwrap();
    assert!(matches!(
        outcome,
        CommitOutcome::Created {
            completeness: Completeness::Partial,
            ..
        }
    ));
    let stored: String = Connection::open(&f.path)
        .unwrap()
        .query_row("SELECT completeness FROM checkpoints", [], |row| row.get(0))
        .unwrap();
    assert_eq!(stored, "partial");
}

#[test]
fn identical_state_later_only_refreshes_observation() {
    let mut f = fixture();
    let first = f
        .store
        .commit_checkpoint(&capture(
            &f,
            "2026-10-23T12:00:00Z",
            CaptureReason::Scheduled,
            &["code"],
        ))
        .unwrap();
    let CommitOutcome::Created { id, .. } = first else {
        panic!("{first:?}")
    };

    let second = f
        .store
        .commit_checkpoint(&capture(
            &f,
            "2026-10-23T12:01:00Z",
            CaptureReason::Scheduled,
            &["code"],
        ))
        .unwrap();
    assert_eq!(second, CommitOutcome::Unchanged { id });
    assert_eq!(count(&f.path, "checkpoints"), 1);

    let (observed, times): (String, i64) = Connection::open(&f.path)
        .unwrap()
        .query_row(
            "SELECT last_observed_at, observation_count FROM checkpoint_observations",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((observed.as_str(), times), ("2026-10-23T12:01:00Z", 2));
    let captured: String = Connection::open(&f.path)
        .unwrap()
        .query_row("SELECT captured_at FROM checkpoints", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        captured, "2026-10-23T12:00:00Z",
        "history must not be rewritten"
    );
}

#[test]
fn changed_state_manual_capture_and_new_session_create_checkpoints() {
    let mut f = fixture();
    f.store
        .commit_checkpoint(&capture(
            &f,
            "2026-10-23T12:00:00Z",
            CaptureReason::Scheduled,
            &["code"],
        ))
        .unwrap();

    let changed = capture(
        &f,
        "2026-10-23T12:01:00Z",
        CaptureReason::Scheduled,
        &["code", "brave"],
    );
    assert!(matches!(
        f.store.commit_checkpoint(&changed).unwrap(),
        CommitOutcome::Created { .. }
    ));

    let manual = capture(
        &f,
        "2026-10-23T12:02:00Z",
        CaptureReason::Manual,
        &["code", "brave"],
    );
    assert!(matches!(
        f.store.commit_checkpoint(&manual).unwrap(),
        CommitOutcome::Created { .. }
    ));

    let next_session = SessionId::new();
    insert_session(&f.path, next_session);
    let mut same_in_new_session = capture(
        &f,
        "2026-10-23T12:03:00Z",
        CaptureReason::Scheduled,
        &["code", "brave"],
    );
    same_in_new_session.session_id = next_session;
    assert!(matches!(
        f.store.commit_checkpoint(&same_in_new_session).unwrap(),
        CommitOutcome::Created { .. }
    ));
    assert_eq!(count(&f.path, "checkpoints"), 4);
}

#[test]
fn invalid_capture_writes_nothing() {
    let mut f = fixture();
    let mut checkpoint = capture(
        &f,
        "2026-10-23T12:00:00Z",
        CaptureReason::Scheduled,
        &["code"],
    );
    checkpoint.resources.push(Resource {
        payload: ResourcePayload::FolderV1(FolderV1 {
            path: "relative/secret".to_owned(),
        }),
        ..app(1, "x")
    });
    let error = f.store.commit_checkpoint(&checkpoint).unwrap_err();
    assert!(matches!(error, StoreError::InvalidData { .. }));
    assert!(!error.to_string().contains("secret"));

    let mut duplicate = capture(
        &f,
        "2026-10-23T12:00:00Z",
        CaptureReason::Scheduled,
        &["a", "b"],
    );
    duplicate.resources[1].ordinal = 0;
    assert!(matches!(
        f.store.commit_checkpoint(&duplicate),
        Err(StoreError::InvalidData { .. })
    ));
    assert_eq!(count(&f.path, "checkpoints"), 0);
    assert_eq!(count(&f.path, "resources"), 0);
}

#[test]
fn failure_mid_transaction_leaves_no_partial_rows() {
    let mut f = fixture();
    let mut checkpoint = capture(
        &f,
        "2026-10-23T12:00:00Z",
        CaptureReason::Scheduled,
        &["a", "b"],
    );
    // The second resource reuses the first one's primary key, so its insert
    // fails after the checkpoint, source and first resource rows were written.
    checkpoint.resources[1].id = checkpoint.resources[0].id;
    assert!(f.store.commit_checkpoint(&checkpoint).is_err());
    for table in [
        "checkpoints",
        "checkpoint_sources",
        "resources",
        "checkpoint_observations",
    ] {
        assert_eq!(count(&f.path, table), 0, "{table}");
    }
}

#[test]
fn writer_runs_jobs_in_order_and_returns_results() {
    let f = fixture();
    let checkpoint = capture(
        &f,
        "2026-10-23T12:00:00Z",
        CaptureReason::Scheduled,
        &["code"],
    );
    let path = f.path.clone();
    let (handle, thread) = writer::spawn(f.store, DEFAULT_QUEUE_CAPACITY).unwrap();

    let outcome = handle
        .call(move |store| store.commit_checkpoint(&checkpoint))
        .unwrap()
        .unwrap();
    assert!(matches!(outcome, CommitOutcome::Created { .. }));
    assert_eq!(handle.call(|store| store.schema_version()).unwrap(), 2);

    drop(handle);
    let store = thread.join().unwrap();
    assert_eq!(store.path(), path);
}

#[test]
fn full_queue_is_reported_instead_of_growing() {
    let f = fixture();
    let (handle, thread) = writer::spawn(f.store, 1).unwrap();
    let (started_tx, started) = mpsc::channel();
    let (release, gate) = mpsc::channel::<()>();

    let blocking = handle
        .submit(move |_| {
            started_tx.send(()).unwrap();
            gate.recv().unwrap();
        })
        .unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let queued = handle.submit(|_| ()).unwrap();
    assert_eq!(handle.submit(|_| ()).unwrap_err(), WriterError::Busy);

    release.send(()).unwrap();
    blocking.blocking_recv().unwrap();
    queued.blocking_recv().unwrap();
    drop(handle);
    thread.join().unwrap();
}

#[test]
fn a_panicking_job_does_not_stop_the_writer() {
    let f = fixture();
    let (handle, thread) = writer::spawn(f.store, DEFAULT_QUEUE_CAPACITY).unwrap();
    assert_eq!(
        handle.call(|_| -> () { panic!("job failure") }),
        Err(WriterError::Lost)
    );
    assert_eq!(handle.call(|store| store.schema_version()).unwrap(), 2);
    drop(handle);
    thread.join().unwrap();
}
