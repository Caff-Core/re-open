#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use std::fs;
use std::path::{Path, PathBuf};

use reopen_core::Timestamp;
use reopen_storage::{MIGRATIONS, Migration, Store, StoreError};
use rusqlite::{Connection, params};
use tempfile::TempDir;

fn now() -> Timestamp {
    Timestamp::parse("2026-10-23T12:00:00Z").unwrap()
}

fn db(dir: &TempDir) -> PathBuf {
    dir.path().join("reopen.sqlite")
}

fn raw(path: &Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    conn
}

#[test]
fn fresh_database_is_durable_and_migrated() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    let store = Store::open(&path, now()).unwrap();
    assert_eq!(store.schema_version(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let wal = path.with_file_name("reopen.sqlite-wal");
        assert!(
            wal.exists(),
            "WAL file should exist while the store is open"
        );
        assert_eq!(fs::metadata(&wal).unwrap().permissions().mode() & 0o077, 0);
    }
    drop(store);

    let conn = Connection::open(&path).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn all_data_model_tables_exist() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    drop(Store::open(&path, now()).unwrap());
    let conn = raw(&path);
    for table in [
        "workspaces",
        "sessions",
        "checkpoints",
        "checkpoint_sources",
        "resources",
        "recovery_candidates",
        "checkpoint_pins",
        "browser_sources",
        "browser_captures",
        "restore_runs",
        "restore_actions",
        "settings",
        "schema_migrations",
    ] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "missing table {table}");
    }
}

#[test]
fn reopening_keeps_one_default_workspace() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    let first = Store::open(&path, now())
        .unwrap()
        .ensure_default_workspace(now())
        .unwrap();
    let second = Store::open(&path, now())
        .unwrap()
        .ensure_default_workspace(now())
        .unwrap();
    assert_eq!(first, second);
    let count: i64 = raw(&path)
        .query_row("SELECT COUNT(*) FROM workspaces", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn newer_schema_is_refused_without_changes() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    drop(Store::open(&path, now()).unwrap());
    raw(&path)
        .execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (99, 'future', 'x')",
            [],
        )
        .unwrap();

    let error = Store::open(&path, now()).unwrap_err();
    assert!(
        matches!(
            error,
            StoreError::NewerSchema {
                found: 99,
                supported: 1
            }
        ),
        "{error:?}"
    );
    assert!(error.to_string().contains("update Re-Open"));
}

#[test]
fn garbage_file_is_reported_corrupt_and_left_untouched() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    let garbage = b"this is not a sqlite database, but it is the user's data".repeat(100);
    fs::write(&path, &garbage).unwrap();

    assert!(matches!(
        Store::open(&path, now()),
        Err(StoreError::Corrupt)
    ));
    assert_eq!(fs::read(&path).unwrap(), garbage);
}

#[test]
fn empty_existing_file_is_not_treated_as_a_fresh_database() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    fs::write(&path, b"").unwrap();
    assert!(matches!(
        Store::open(&path, now()),
        Err(StoreError::Corrupt)
    ));
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
}

#[test]
fn pinned_checkpoints_cannot_be_deleted() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    let workspace = Store::open(&path, now())
        .unwrap()
        .ensure_default_workspace(now())
        .unwrap();
    let conn = raw(&path);
    conn.execute(
        "INSERT INTO sessions (id, started_at) VALUES ('s1', 't')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO checkpoints (id, workspace_id, session_id, captured_at, reason, completeness, content_hash)
         VALUES ('c1', ?1, 's1', 't', 'scheduled', 'complete', 'h')",
        params![workspace.to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO checkpoint_pins (checkpoint_id, reason, reference, created_at)
         VALUES ('c1', 'recovery_candidate', 's2', 't')",
        [],
    )
    .unwrap();

    assert!(
        conn.execute("DELETE FROM checkpoints WHERE id = 'c1'", [])
            .is_err()
    );
    conn.execute("DELETE FROM checkpoint_pins WHERE checkpoint_id = 'c1'", [])
        .unwrap();
    assert_eq!(
        conn.execute("DELETE FROM checkpoints WHERE id = 'c1'", [])
            .unwrap(),
        1
    );
}

#[test]
fn check_constraints_reject_unknown_states() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    let workspace = Store::open(&path, now())
        .unwrap()
        .ensure_default_workspace(now())
        .unwrap();
    let conn = raw(&path);
    conn.execute(
        "INSERT INTO sessions (id, started_at) VALUES ('s1', 't')",
        [],
    )
    .unwrap();
    let result = conn.execute(
        "INSERT INTO checkpoints (id, workspace_id, session_id, captured_at, reason, completeness, content_hash)
         VALUES ('c1', ?1, 's1', 't', 'scheduled', 'mostly', 'h')",
        params![workspace.to_string()],
    );
    assert!(result.is_err());
}

const V1: Migration = Migration {
    version: 1,
    name: "notes",
    sql: "CREATE TABLE notes (body TEXT NOT NULL) STRICT;",
    destructive: false,
};
const V2_DESTRUCTIVE: Migration = Migration {
    version: 2,
    name: "rename notes",
    sql: "CREATE TABLE memos (body TEXT NOT NULL) STRICT;
          INSERT INTO memos SELECT body FROM notes;
          DROP TABLE notes;",
    destructive: true,
};
const V2_BROKEN: Migration = Migration {
    version: 2,
    name: "broken",
    sql: "CREATE TABLE half (x TEXT) STRICT; THIS IS NOT SQL;",
    destructive: false,
};

#[test]
fn destructive_migration_from_prior_version_takes_a_verified_backup() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    drop(Store::open_with(&path, &[V1], now()).unwrap());
    raw(&path)
        .execute("INSERT INTO notes (body) VALUES ('kept')", [])
        .unwrap();

    let store = Store::open_with(&path, &[V1, V2_DESTRUCTIVE], now()).unwrap();
    assert_eq!(store.schema_version(), 2);
    let [backup]: [PathBuf; 1] = store.migration_backups().to_vec().try_into().unwrap();
    drop(store);

    let migrated: String = raw(&path)
        .query_row("SELECT body FROM memos", [], |r| r.get(0))
        .unwrap();
    assert_eq!(migrated, "kept");
    let saved: String = raw(&backup)
        .query_row("SELECT body FROM notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(saved, "kept");
    let saved_version: i64 = raw(&backup)
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(saved_version, 1);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn existing_database_permissions_are_tightened() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    drop(Store::open(&path, now()).unwrap());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    drop(Store::open(&path, now()).unwrap());
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let wal = path.with_file_name("reopen.sqlite-wal");
    if wal.exists() {
        assert_eq!(fs::metadata(&wal).unwrap().permissions().mode() & 0o077, 0);
    }
}

#[test]
fn failed_migration_rolls_back_to_the_previous_version() {
    let dir = TempDir::new().unwrap();
    let path = db(&dir);
    drop(Store::open_with(&path, &[V1], now()).unwrap());

    assert!(Store::open_with(&path, &[V1, V2_BROKEN], now()).is_err());
    let conn = raw(&path);
    let version: i64 = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(version, 1);
    let half: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'half'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(half, 0);
}

#[test]
fn gaps_in_the_migration_list_are_rejected() {
    let dir = TempDir::new().unwrap();
    let gap = Migration {
        version: 3,
        ..V2_DESTRUCTIVE
    };
    assert!(matches!(
        Store::open_with(&db(&dir), &[V1, gap], now()),
        Err(StoreError::InvalidSchemaVersion)
    ));
}

#[test]
fn shipped_migrations_are_sequential_and_end_at_latest() {
    assert!(
        MIGRATIONS
            .iter()
            .zip(1..)
            .all(|(m, v): (&Migration, u32)| m.version == v)
    );
}
