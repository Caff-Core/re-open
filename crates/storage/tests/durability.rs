//! Durability under process death, full disks, read-only files and corruption
//! (docs/TESTING_RELEASE.md, "Critical failure cases" 1 and 12).

#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reopen_core::resource::ApplicationV1;
use reopen_core::{
    AdapterRef, CaptureReason, Resource, ResourceId, ResourcePayload, SessionId, SourceState,
    SourceStatus, Timestamp, WorkspaceId,
};
use reopen_storage::{CommitOutcome, NewCheckpoint, Store, StoreError};
use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;

const RESOURCES_PER_CHECKPOINT: u32 = 40;
const CHILD_ENV: &str = "REOPEN_DURABILITY_CHILD_DB";

fn now() -> Timestamp {
    Timestamp::parse("2026-10-23T12:00:00Z").unwrap()
}

fn checkpoint(workspace: WorkspaceId, session: SessionId, serial: u64) -> NewCheckpoint {
    NewCheckpoint {
        workspace_id: workspace,
        session_id: session,
        captured_at: now(),
        reason: CaptureReason::Manual,
        sources: vec![SourceStatus {
            source_key: "desktop.apps".to_owned(),
            state: SourceState::Complete,
            required: true,
            captured_at: Some(now()),
            last_seen_at: Some(now()),
            error: None,
        }],
        resources: (0..RESOURCES_PER_CHECKPOINT)
            .map(|ordinal| Resource {
                id: ResourceId::new(),
                adapter: AdapterRef {
                    id: "fake.apps".to_owned(),
                    version: 1,
                },
                ordinal,
                payload: ResourcePayload::ApplicationV1(ApplicationV1 {
                    app_id: format!("org.example.App{serial}.{ordinal}"),
                    display_name: "x".repeat(200),
                }),
            })
            .collect(),
    }
}

fn open_with_session(path: &Path) -> (Store, WorkspaceId, SessionId) {
    let mut store = Store::open(path, now()).unwrap();
    let workspace = store.ensure_default_workspace(now()).unwrap();
    let session = store.start_session(now()).unwrap().session_id;
    (store, workspace, session)
}

/// Every committed checkpoint must be whole: all its resources, its source and
/// its observation row, and the file must pass an integrity check.
fn assert_consistent(path: &Path) -> i64 {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let check: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(check, "ok");
    let partial: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM checkpoints c WHERE
               (SELECT COUNT(*) FROM resources r WHERE r.checkpoint_id = c.id) <> ?1
               OR (SELECT COUNT(*) FROM checkpoint_sources s WHERE s.checkpoint_id = c.id) <> 1
               OR NOT EXISTS (SELECT 1 FROM checkpoint_observations o WHERE o.checkpoint_id = c.id)",
            [RESOURCES_PER_CHECKPOINT],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(partial, 0, "found a half-written checkpoint");
    let orphans: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM resources r WHERE NOT EXISTS
               (SELECT 1 FROM checkpoints c WHERE c.id = r.checkpoint_id)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphans, 0, "found resources without a checkpoint");
    conn.query_row("SELECT COUNT(*) FROM checkpoints", [], |r| r.get(0))
        .unwrap()
}

/// Child half of the kill test: commits forever until killed. Skipped in
/// normal runs; the parent starts it with `--ignored`.
#[test]
#[ignore = "run as a child process by killing_the_writer_never_leaves_a_half_written_checkpoint"]
fn kill_child_writer() {
    let Some(path) = std::env::var_os(CHILD_ENV) else {
        return;
    };
    let (mut store, workspace, session) = open_with_session(Path::new(&path));
    for serial in 0..u64::MAX {
        store
            .commit_checkpoint(&checkpoint(workspace, session, serial))
            .unwrap();
    }
}

fn pseudo_random_millis(round: u32) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    u64::from(nanos.wrapping_mul(2_654_435_761).wrapping_add(round) % 120) + 10
}

#[test]
fn killing_the_writer_never_leaves_a_half_written_checkpoint() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("reopen.sqlite");
    drop(Store::open(&path, now()).unwrap());

    let mut committed = 0;
    for round in 0..25 {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "kill_child_writer",
                "--test-threads=1",
            ])
            .env(CHILD_ENV, &path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(pseudo_random_millis(round)));
        child.kill().unwrap(); // SIGKILL: no destructors, no clean shutdown.
        child.wait().unwrap();

        // The store must reopen (recovering the WAL) and stay consistent.
        drop(Store::open(&path, now()).unwrap());
        let count = assert_consistent(&path);
        assert!(
            count >= committed,
            "round {round}: committed checkpoints disappeared"
        );
        committed = count;
    }
    assert!(
        committed > 0,
        "the child never committed; the test proved nothing"
    );
}

#[test]
fn a_full_disk_preserves_history_and_writes_nothing() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("reopen.sqlite");
    let (mut store, workspace, session) = open_with_session(&path);
    store
        .commit_checkpoint(&checkpoint(workspace, session, 0))
        .unwrap();
    let before = assert_consistent(&path);

    let pages: i64 = Connection::open(&path)
        .unwrap()
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .unwrap();
    store
        .limit_pages_for_testing(Some(u64::try_from(pages).unwrap()))
        .unwrap();
    let mut saw_full = false;
    for serial in 1..50 {
        match store.commit_checkpoint(&checkpoint(workspace, session, serial)) {
            Ok(_) => {}
            Err(StoreError::Full) => {
                saw_full = true;
                break;
            }
            Err(other) => panic!("expected Full, got {other:?}"),
        }
    }
    assert!(saw_full, "the page limit never triggered");
    let after = assert_consistent(&path);
    assert!(after >= before);

    // Once space is available again, saving resumes.
    store.limit_pages_for_testing(None).unwrap();
    assert!(matches!(
        store
            .commit_checkpoint(&checkpoint(workspace, session, 99))
            .unwrap(),
        CommitOutcome::Created { .. }
    ));
    assert_eq!(assert_consistent(&path), after + 1);
}

fn with_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn a_read_only_database_is_refused_and_left_unchanged() {
    // Root ignores file permissions, so read-only cannot be simulated there.
    if rustix::process::geteuid().is_root() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("reopen.sqlite");
    let (mut store, workspace, session) = open_with_session(&path);
    store
        .commit_checkpoint(&checkpoint(workspace, session, 0))
        .unwrap();
    drop(store);
    let bytes = std::fs::read(&path).unwrap();

    with_mode(&path, 0o400);
    let result = Store::open(&path, now());
    with_mode(&path, 0o600);
    assert!(matches!(result, Err(StoreError::ReadOnly)), "{result:?}");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn a_damaged_page_is_reported_corrupt_and_left_untouched() {
    let dir = TempDir::new().unwrap();
    let path: PathBuf = dir.path().join("reopen.sqlite");
    let (mut store, workspace, session) = open_with_session(&path);
    for serial in 0..5 {
        store
            .commit_checkpoint(&checkpoint(workspace, session, serial))
            .unwrap();
    }
    drop(store); // Checkpoints the WAL into the main file.

    let mut bytes = std::fs::read(&path).unwrap();
    let page_size = 4096;
    assert!(bytes.len() > page_size * 4);
    // Overwrite the third page (a b-tree page) with garbage; the header stays valid.
    for byte in &mut bytes[page_size * 2..page_size * 3] {
        *byte = 0xA5;
    }
    std::fs::write(&path, &bytes).unwrap();

    assert!(matches!(
        Store::open(&path, now()),
        Err(StoreError::Corrupt)
    ));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        bytes,
        "damaged data must be preserved as evidence"
    );
}
