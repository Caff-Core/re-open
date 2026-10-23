#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use std::path::PathBuf;
use std::time::Duration;

use reopen_core::resource::ApplicationV1;
use reopen_core::{
    AdapterRef, CaptureReason, CheckpointId, Resource, ResourceId, ResourcePayload, SessionId,
    SourceState, SourceStatus, Timestamp, WorkspaceId,
};
use reopen_storage::{
    CandidateDisposition, CommitOutcome, NewCheckpoint, PinReason, RetentionPolicy, Store,
    StoreError,
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

const DAY: i64 = 86_400;
const NOW: i64 = 1_800_000_000;

fn ts(seconds: i64) -> Timestamp {
    Timestamp::from_jiff(jiff::Timestamp::from_second(seconds).unwrap())
}

struct Fixture {
    _dir: TempDir,
    path: PathBuf,
    store: Store,
    workspace: WorkspaceId,
    session: SessionId,
    serial: u32,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("reopen.sqlite");
        let mut store = Store::open(&path, ts(NOW - 30 * DAY)).unwrap();
        let workspace = store.ensure_default_workspace(ts(NOW - 30 * DAY)).unwrap();
        let session = store.start_session(ts(NOW - 30 * DAY)).unwrap().session_id;
        Self {
            _dir: dir,
            path,
            store,
            workspace,
            session,
            serial: 0,
        }
    }

    /// Commits a checkpoint with unique content at `at`.
    fn commit(&mut self, at: i64, reason: CaptureReason) -> CheckpointId {
        self.serial += 1;
        self.commit_content(at, reason, &format!("app-{}", self.serial))
    }

    fn commit_content(&mut self, at: i64, reason: CaptureReason, app: &str) -> CheckpointId {
        let checkpoint = NewCheckpoint {
            workspace_id: self.workspace,
            session_id: self.session,
            captured_at: ts(at),
            reason,
            sources: vec![SourceStatus {
                source_key: "desktop.apps".to_owned(),
                state: SourceState::Complete,
                required: true,
                captured_at: Some(ts(at)),
                last_seen_at: Some(ts(at)),
                error: None,
            }],
            resources: vec![Resource {
                id: ResourceId::new(),
                adapter: AdapterRef {
                    id: "fake.apps".to_owned(),
                    version: 1,
                },
                ordinal: 0,
                payload: ResourcePayload::ApplicationV1(ApplicationV1 {
                    app_id: app.to_owned(),
                    display_name: "x".repeat(500),
                }),
            }],
        };
        match self.store.commit_checkpoint(&checkpoint).unwrap() {
            CommitOutcome::Created { id, .. } | CommitOutcome::Unchanged { id } => id,
        }
    }

    fn exists(&self, id: CheckpointId) -> bool {
        Connection::open(&self.path)
            .unwrap()
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM checkpoints WHERE id = ?1)",
                params![id.to_string()],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn count(&self) -> i64 {
        Connection::open(&self.path)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM checkpoints", [], |r| r.get(0))
            .unwrap()
    }
}

fn generous() -> RetentionPolicy {
    RetentionPolicy {
        storage_budget_bytes: u64::MAX,
        ..RetentionPolicy::default()
    }
}

#[test]
fn old_automatic_checkpoints_expire_but_protected_ones_stay() {
    let mut f = Fixture::new();
    let old_scheduled = f.commit(NOW - 10 * DAY, CaptureReason::Scheduled);
    let old_shutdown = f.commit(NOW - 9 * DAY, CaptureReason::Shutdown);
    let old_manual = f.commit(NOW - 9 * DAY + 1, CaptureReason::Manual);
    let old_pinned = f.commit(NOW - 9 * DAY + 2, CaptureReason::Scheduled);
    f.store
        .pin_checkpoint(old_pinned, PinReason::User, "", ts(NOW))
        .unwrap();
    let recent = f.commit(NOW - DAY, CaptureReason::Scheduled);
    let expired_late_insert = f.commit(NOW - 8 * DAY, CaptureReason::Scheduled);

    let report = f.store.apply_retention(&generous(), ts(NOW)).unwrap();
    assert!(!f.exists(old_scheduled));
    assert!(!f.exists(old_shutdown));
    assert!(
        !f.exists(expired_late_insert),
        "the newest is chosen by capture time, not insertion order"
    );
    assert!(
        f.exists(old_manual),
        "manual checkpoints are kept until explicit deletion"
    );
    assert!(f.exists(old_pinned), "pinned checkpoints are exempt");
    assert!(f.exists(recent));
    assert_eq!(report.deleted, 3);
    assert!(!report.more_pending && !report.over_budget);
}

#[test]
fn the_newest_checkpoint_survives_even_when_old() {
    let mut f = Fixture::new();
    let only = f.commit(NOW - 20 * DAY, CaptureReason::Scheduled);
    f.store.apply_retention(&generous(), ts(NOW)).unwrap();
    assert!(f.exists(only));
}

#[test]
fn a_recently_observed_checkpoint_is_not_expired_by_its_capture_time() {
    let mut f = Fixture::new();
    let long_lived = f.commit_content(NOW - 10 * DAY, CaptureReason::Scheduled, "stable");
    // The same state was observed again an hour ago.
    let again = f.commit_content(NOW - 3600, CaptureReason::Scheduled, "stable");
    assert_eq!(again, long_lived);
    f.commit(NOW - 60, CaptureReason::Scheduled);

    f.store.apply_retention(&generous(), ts(NOW)).unwrap();
    assert!(f.exists(long_lived));
}

#[test]
fn count_limit_keeps_the_newest_automatic_checkpoints() {
    let mut f = Fixture::new();
    let ids: Vec<_> = (0..6)
        .map(|i| f.commit(NOW - 3600 + i, CaptureReason::Scheduled))
        .collect();
    let policy = RetentionPolicy {
        max_automatic: 3,
        ..generous()
    };
    let report = f.store.apply_retention(&policy, ts(NOW)).unwrap();
    assert_eq!(report.deleted, 3);
    assert!(ids[..3].iter().all(|id| !f.exists(*id)));
    assert!(ids[3..].iter().all(|id| f.exists(*id)));
}

#[test]
fn runs_are_bounded_and_resume() {
    let mut f = Fixture::new();
    for i in 0..5 {
        f.commit(NOW - 10 * DAY + i, CaptureReason::Scheduled);
    }
    f.commit(NOW, CaptureReason::Scheduled);
    let policy = RetentionPolicy {
        max_deletions_per_run: 2,
        ..generous()
    };

    let first = f.store.apply_retention(&policy, ts(NOW)).unwrap();
    assert_eq!((first.deleted, first.more_pending), (2, true));
    let second = f.store.apply_retention(&policy, ts(NOW)).unwrap();
    assert_eq!((second.deleted, second.more_pending), (2, true));
    let third = f.store.apply_retention(&policy, ts(NOW)).unwrap();
    assert_eq!((third.deleted, third.more_pending), (1, false));
    assert_eq!(f.count(), 1);
}

#[test]
fn storage_pressure_is_reported_when_pins_prevent_compliance() {
    let mut f = Fixture::new();
    let pinned = f.commit(NOW - 3000, CaptureReason::Scheduled);
    f.store
        .pin_checkpoint(pinned, PinReason::User, "", ts(NOW))
        .unwrap();
    let manual = f.commit(NOW - 2000, CaptureReason::Manual);
    for i in 0..4 {
        f.commit(NOW - 1000 + i, CaptureReason::Scheduled);
    }
    let newest = f.commit(NOW, CaptureReason::Scheduled);

    let policy = RetentionPolicy {
        storage_budget_bytes: 1,
        ..generous()
    };
    let report = f.store.apply_retention(&policy, ts(NOW)).unwrap();
    assert_eq!(report.deleted, 4);
    assert!(report.over_budget && report.budget_blocked);
    assert!(f.exists(pinned) && f.exists(manual) && f.exists(newest));
}

#[test]
fn pending_candidates_are_protected_and_closed_candidate_rows_are_cleaned_up() {
    let mut f = Fixture::new();
    let previous = f.commit(NOW - 10 * DAY, CaptureReason::Scheduled);
    let start = f.store.start_session(ts(NOW - 10 * DAY + 1)).unwrap();
    assert_eq!(start.new_candidate, Some(previous));
    f.session = start.session_id;
    f.commit(NOW, CaptureReason::Scheduled);

    f.store.apply_retention(&generous(), ts(NOW)).unwrap();
    assert!(f.exists(previous), "a pending candidate is never deleted");

    let [candidate] = f.store.pending_candidates().unwrap().try_into().unwrap();
    f.store
        .close_candidate(&candidate, CandidateDisposition::Dismissed)
        .unwrap();
    f.store.apply_retention(&generous(), ts(NOW)).unwrap();
    assert!(!f.exists(previous), "once dismissed it is ordinary history");
}

#[test]
fn pins_are_idempotent_and_released_individually() {
    let mut f = Fixture::new();
    let id = f.commit(NOW - 10 * DAY, CaptureReason::Scheduled);
    f.commit(NOW, CaptureReason::Scheduled);
    f.store
        .pin_checkpoint(id, PinReason::User, "", ts(NOW))
        .unwrap();
    f.store
        .pin_checkpoint(id, PinReason::User, "", ts(NOW))
        .unwrap();
    f.store
        .pin_checkpoint(id, PinReason::RestoreRun, "run-1", ts(NOW))
        .unwrap();

    f.store.unpin_checkpoint(id, PinReason::User, "").unwrap();
    assert!(matches!(
        f.store.unpin_checkpoint(id, PinReason::User, ""),
        Err(StoreError::NotFound)
    ));
    f.store.apply_retention(&generous(), ts(NOW)).unwrap();
    assert!(f.exists(id), "the restore pin still protects it");

    f.store
        .unpin_checkpoint(id, PinReason::RestoreRun, "run-1")
        .unwrap();
    f.store.apply_retention(&generous(), ts(NOW)).unwrap();
    assert!(!f.exists(id));
    assert!(matches!(
        f.store
            .pin_checkpoint(CheckpointId::new(), PinReason::User, "", ts(NOW)),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn default_policy_matches_the_product_defaults() {
    let policy = RetentionPolicy::default();
    assert_eq!(policy.max_age, Duration::from_secs(7 * 86_400));
    assert_eq!(policy.max_automatic, 500);
    assert_eq!(policy.storage_budget_bytes, 100 * 1024 * 1024);
}
