//! Numbered, transactional migrations with verified backups before destructive steps.

use std::path::{Path, PathBuf};
use std::time::Duration;

use reopen_core::Timestamp;
use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags, params};

use crate::error::StoreError;

/// One schema step. Versions start at 1 and increase by exactly 1.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
    /// Drops or rewrites existing data; a verified backup is taken first.
    pub destructive: bool,
}

/// The application's migrations, in order.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: include_str!("migrations/0001_initial.sql"),
        destructive: false,
    },
    Migration {
        version: 2,
        name: "checkpoint observations",
        sql: include_str!("migrations/0002_checkpoint_observations.sql"),
        destructive: false,
    },
];

/// Newest schema version this build understands.
#[must_use]
pub fn latest_version(migrations: &[Migration]) -> u32 {
    migrations.last().map_or(0, |migration| migration.version)
}

pub(crate) fn ensure_migrations_table(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           version INTEGER PRIMARY KEY,
           name TEXT NOT NULL,
           applied_at TEXT NOT NULL
         ) STRICT;",
    )?;
    Ok(())
}

/// Highest applied version, or 0 for a new database.
pub(crate) fn current_version(conn: &Connection) -> Result<u32, StoreError> {
    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    u32::try_from(version).map_err(|_| StoreError::InvalidSchemaVersion)
}

/// Applies pending migrations in order. Each step runs in its own transaction
/// together with its `schema_migrations` row, so a crash leaves the database at
/// the previous or the new version, never in between.
pub(crate) fn apply(
    conn: &mut Connection,
    db_path: &Path,
    migrations: &[Migration],
    now: Timestamp,
) -> Result<Vec<PathBuf>, StoreError> {
    check_sequence(migrations)?;
    let current = current_version(conn)?;
    let latest = latest_version(migrations);
    if current > latest {
        return Err(StoreError::NewerSchema {
            found: current,
            supported: latest,
        });
    }

    let mut backups = Vec::new();
    for migration in migrations
        .iter()
        .filter(|migration| migration.version > current)
    {
        // A new database has no data to protect.
        if migration.destructive && current_version(conn)? > 0 {
            backups.push(verified_backup(conn, db_path, migration.version, now)?);
        }
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
            params![migration.version, migration.name, now.to_string()],
        )?;
        tx.commit()?;
    }
    Ok(backups)
}

fn check_sequence(migrations: &[Migration]) -> Result<(), StoreError> {
    let in_order = migrations
        .iter()
        .zip(1..)
        .all(|(migration, expected): (&Migration, u32)| migration.version == expected);
    if in_order {
        Ok(())
    } else {
        Err(StoreError::InvalidSchemaVersion)
    }
}

/// Copies the live database (including WAL content) with the SQLite backup API,
/// then reopens the copy read-only and checks it before returning its path.
fn verified_backup(
    conn: &Connection,
    db_path: &Path,
    next_version: u32,
    now: Timestamp,
) -> Result<PathBuf, StoreError> {
    let stamp = now.as_jiff().as_second();
    let file_name = format!(
        "{}.pre-v{next_version}-{stamp}.bak",
        db_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("reopen.sqlite")
    );
    let backup_path = db_path.with_file_name(file_name);
    // create_new fails if a file is already there, and the copy is owner-only.
    crate::create_private_file(&backup_path).map_err(|_| StoreError::BackupFailed)?;

    {
        let mut target =
            Connection::open_with_flags(&backup_path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let backup = Backup::new(conn, &mut target)?;
        backup.run_to_completion(256, Duration::from_millis(10), None)?;
    }

    let copy = Connection::open_with_flags(&backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let check: String = copy.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    let copied_version = current_version(&copy)?;
    if check != "ok" || copied_version != current_version(conn)? {
        return Err(StoreError::BackupFailed);
    }
    Ok(backup_path)
}
