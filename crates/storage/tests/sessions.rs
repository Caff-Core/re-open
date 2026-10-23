#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use std::path::PathBuf;

use reopen_core::checkpoint::ProviderError;
use reopen_core::resource::ApplicationV1;
use reopen_core::{
    AdapterRef, CaptureReason, CheckpointId, Resource, ResourceId, ResourcePayload, SessionId,
    SourceState, SourceStatus, Timestamp, WorkspaceId,
};
use reopen_storage::{CandidateDisposition, CommitOutcome, NewCheckpoint, Store, StoreError};
use rusqlite::{Connection, params};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    path: PathBuf,
    store: Store,
    workspace: WorkspaceId,
    minute: u32,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("reopen.sqlite");
        let store = Store::open(&path, Timestamp::parse("2026-10-23T08:00:00Z").unwrap()).unwrap();
        let workspace = store
            .ensure_default_workspace(Timestamp::parse("2026-10-23T08:00:00Z").unwrap())
            .unwrap();
        Self {
            _dir: dir,
            path,
            store,
            workspace,
            minute: 0,
        }
    }

    /// A strictly increasing clock so ordering by time is deterministic.
    fn tick(&mut self) -> Timestamp {
        self.minute += 1;
        Timestamp::parse(&format!(
            "2026-10-23T{:02}:{:02}:00Z",
            9 + self.minute / 60,
            self.minute % 60
        ))
        .unwrap()
    }

    fn start(&mut self) -> reopen_storage::SessionStart {
        let now = self.tick();
        self.store.start_session(now).unwrap()
    }

    fn capture(&mut self, session: SessionId, state: SourceState, apps: &[&str]) -> CheckpointId {
        let now = self.tick();
        let checkpoint = NewCheckpoint {
            workspace_id: self.workspace,
            session_id: session,
            captured_at: now,
            reason: CaptureReason::Manual,
            sources: vec![SourceStatus {
                source_key: "desktop.apps".to_owned(),
                state,
                required: true,
                captured_at: Some(now),
                last_seen_at: Some(now),
                error: (state != SourceState::Complete).then_some(ProviderError::Timeout),
            }],
            resources: apps
                .iter()
                .zip(0..)
                .map(|(app, ordinal)| Resource {
                    id: ResourceId::new(),
                    adapter: AdapterRef {
                        id: "fake.apps".to_owned(),
                        version: 1,
                    },
                    ordinal,
                    payload: ResourcePayload::ApplicationV1(ApplicationV1 {
                        app_id: (*app).to_owned(),
                        display_name: (*app).to_owned(),
                    }),
                })
                .collect(),
        };
        match self.store.commit_checkpoint(&checkpoint).unwrap() {
            CommitOutcome::Created { id, .. } | CommitOutcome::Unchanged { id } => id,
        }
    }

    fn pins(&self, checkpoint: CheckpointId) -> Vec<(String, String)> {
        let conn = Connection::open(&self.path).unwrap();
        let mut statement = conn
            .prepare("SELECT reason, reference FROM checkpoint_pins WHERE checkpoint_id = ?1 ORDER BY reference")
            .unwrap();
        statement
            .query_map(params![checkpoint.to_string()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
}

#[test]
fn first_session_has_no_previous_session_or_candidate() {
    let mut f = Fixture::new();
    let start = f.start();
    assert_eq!(start.previous, None);
    assert_eq!(start.new_candidate, None);
}

#[test]
fn unclean_previous_session_protects_its_latest_useful_checkpoint() {
    let mut f = Fixture::new();
    let s1 = f.start().session_id;
    f.capture(s1, SourceState::Complete, &["code"]);
    let latest = f.capture(s1, SourceState::Complete, &["code", "brave"]);

    let start = f.start();
    let previous = start.previous.unwrap();
    assert_eq!(previous.id, s1);
    assert!(!previous.clean_exit);
    assert_eq!(start.new_candidate, Some(latest));
    assert_eq!(
        f.pins(latest),
        [("recovery_candidate".to_owned(), s1.to_string())]
    );
}

#[test]
fn clean_exit_is_recorded_and_still_offers_recovery() {
    let mut f = Fixture::new();
    let s1 = f.start().session_id;
    let checkpoint = f.capture(s1, SourceState::Complete, &["code"]);
    let now = f.tick();
    f.store.mark_clean_exit(s1, now).unwrap();

    let start = f.start();
    assert!(start.previous.unwrap().clean_exit);
    assert_eq!(start.new_candidate, Some(checkpoint));
    assert!(matches!(
        f.store.mark_clean_exit(SessionId::new(), now),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn complete_is_preferred_over_a_newer_partial_and_partial_is_used_when_alone() {
    let mut f = Fixture::new();
    let s1 = f.start().session_id;
    let complete = f.capture(s1, SourceState::Complete, &["code"]);
    f.capture(s1, SourceState::Partial, &["code", "brave"]);
    assert_eq!(f.start().new_candidate, Some(complete));

    let mut g = Fixture::new();
    let s1 = g.start().session_id;
    let partial = g.capture(s1, SourceState::Partial, &["code"]);
    assert_eq!(g.start().new_candidate, Some(partial));
}

#[test]
fn an_empty_capture_never_becomes_the_candidate() {
    let mut f = Fixture::new();
    let s1 = f.start().session_id;
    let useful = f.capture(s1, SourceState::Complete, &["code"]);
    f.capture(s1, SourceState::Complete, &[]);
    assert_eq!(f.start().new_candidate, Some(useful));

    let mut g = Fixture::new();
    let s1 = g.start().session_id;
    g.capture(s1, SourceState::Complete, &[]);
    assert_eq!(g.start().new_candidate, None);
}

#[test]
fn repeated_restarts_keep_every_pending_candidate_protected() {
    let mut f = Fixture::new();
    let s1 = f.start().session_id;
    let work = f.capture(s1, SourceState::Complete, &["code", "studio"]);

    // Crash, restart: s2 protects s1's work, then captures an empty desktop.
    let s2 = f.start().session_id;
    f.capture(s2, SourceState::Complete, &[]);
    // Crash again: s2 had nothing useful, so no new candidate, and s1's stays.
    let s3 = f.start();
    assert_eq!(s3.new_candidate, None);
    let pending = f.store.pending_candidates().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].checkpoint_id, work);

    // s3 does useful work, then crash: now two candidates are pending.
    let more = f.capture(s3.session_id, SourceState::Complete, &["code"]);
    f.start();
    let pending: Vec<_> = f
        .store
        .pending_candidates()
        .unwrap()
        .into_iter()
        .map(|c| c.checkpoint_id)
        .collect();
    assert_eq!(pending, [more, work]);
    assert_eq!(f.pins(work).len(), 1);
    assert_eq!(f.pins(more).len(), 1);
}

#[test]
fn closing_a_candidate_releases_only_its_own_pin() {
    let mut f = Fixture::new();
    let s1 = f.start().session_id;
    let checkpoint = f.capture(s1, SourceState::Complete, &["code"]);
    f.start();
    let now = f.tick().to_string();
    let conn = Connection::open(&f.path).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    conn.execute(
        "INSERT INTO checkpoint_pins (checkpoint_id, reason, reference, created_at) VALUES (?1, 'user', '', ?2)",
        params![checkpoint.to_string(), now],
    )
    .unwrap();

    let [candidate] = f.store.pending_candidates().unwrap().try_into().unwrap();
    f.store
        .close_candidate(&candidate, CandidateDisposition::Dismissed)
        .unwrap();
    assert!(f.store.pending_candidates().unwrap().is_empty());
    assert_eq!(f.pins(checkpoint), [("user".to_owned(), String::new())]);
    assert!(
        conn.execute(
            "DELETE FROM checkpoints WHERE id = ?1",
            params![checkpoint.to_string()]
        )
        .is_err()
    );

    assert!(matches!(
        f.store
            .close_candidate(&candidate, CandidateDisposition::Resolved),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn a_pending_candidate_cannot_be_deleted() {
    let mut f = Fixture::new();
    let s1 = f.start().session_id;
    let checkpoint = f.capture(s1, SourceState::Complete, &["code"]);
    f.start();
    let conn = Connection::open(&f.path).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    assert!(
        conn.execute(
            "DELETE FROM checkpoints WHERE id = ?1",
            params![checkpoint.to_string()]
        )
        .is_err()
    );
}
