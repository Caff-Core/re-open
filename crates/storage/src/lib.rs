//! SQLite storage for Re-Open.
//!
//! The background agent is the only accessor; it opens one [`Store`] and owns
//! all writes. Opening validates the file, enables durable settings and applies
//! migrations. See `docs/DATA_MODEL.md`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use reopen_core::{Timestamp, WorkspaceId};
use rusqlite::{Connection, OpenFlags, params};

mod checkpoint;
mod error;
pub mod migrate;
mod session;
pub mod writer;

pub use checkpoint::{CommitOutcome, NewCheckpoint};
pub use error::StoreError;
pub use migrate::{MIGRATIONS, Migration};
pub use session::{CandidateDisposition, PreviousSession, RecoveryCandidate, SessionStart};
pub use writer::{WriterError, WriterHandle};

/// How long a statement waits on a lock before failing.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// Name of the initial workspace.
pub const DEFAULT_WORKSPACE_NAME: &str = "Default";

/// An open, migrated checkpoint database.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
    path: PathBuf,
    /// Verified backups written by migrations during this open.
    backups: Vec<PathBuf>,
}

impl Store {
    /// Opens (or, if the file does not exist, creates) the database and
    /// migrates it to the latest schema.
    ///
    /// # Errors
    ///
    /// [`StoreError::Corrupt`] for a damaged file, which is left untouched;
    /// [`StoreError::NewerSchema`] when a newer build wrote it; other errors for
    /// I/O, WAL or migration failures.
    pub fn open(path: &Path, now: Timestamp) -> Result<Self, StoreError> {
        let store = Self::open_with(path, MIGRATIONS, now)?;
        store.ensure_default_workspace(now)?;
        Ok(store)
    }

    /// [`Store::open`] with an explicit migration list (for tests and tooling).
    ///
    /// # Errors
    ///
    /// See [`Store::open`].
    pub fn open_with(
        path: &Path,
        migrations: &[Migration],
        now: Timestamp,
    ) -> Result<Self, StoreError> {
        let creating = !path.exists();
        // SQLite would silently initialise an empty file; an existing zero-byte
        // database means data was lost, so never treat it as a fresh start.
        if !creating && std::fs::metadata(path)?.len() == 0 {
            return Err(StoreError::Corrupt);
        }
        // Create the file ourselves with owner-only permissions so it is never
        // readable by others, even briefly; SQLite gives -wal and -shm files the
        // database file's mode. Existing files are tightened if needed.
        if creating {
            create_private_file(path)?;
        } else {
            restrict_existing_permissions(path)?;
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut conn = Connection::open_with_flags(path, flags)?;

        // Fails with NOTADB or CORRUPT on a damaged file before anything is written.
        let check: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if check != "ok" {
            return Err(StoreError::Corrupt);
        }

        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(StoreError::WalUnavailable);
        }
        conn.pragma_update(None, "synchronous", "FULL")?;

        migrate::ensure_migrations_table(&conn)?;
        let backups = migrate::apply(&mut conn, path, migrations, now)?;
        Ok(Self {
            conn,
            path: path.to_owned(),
            backups,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn schema_version(&self) -> u32 {
        migrate::current_version(&self.conn).unwrap_or(0)
    }

    #[must_use]
    pub fn migration_backups(&self) -> &[PathBuf] {
        &self.backups
    }

    /// The connection, for storage modules in this crate.
    pub(crate) const fn conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) const fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Returns the Default workspace, creating it on first open.
    ///
    /// # Errors
    ///
    /// Returns a storage error if the query or insert fails.
    pub fn ensure_default_workspace(&self, now: Timestamp) -> Result<WorkspaceId, StoreError> {
        let existing: Option<String> = self
            .conn()
            .query_row(
                "SELECT id FROM workspaces WHERE name = ?1 ORDER BY created_at LIMIT 1",
                params![DEFAULT_WORKSPACE_NAME],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        if let Some(id) = existing {
            return id.parse().map_err(|_| StoreError::InvalidSchemaVersion);
        }
        let id = WorkspaceId::new();
        self.conn().execute(
            "INSERT INTO workspaces (id, name, created_at) VALUES (?1, ?2, ?3)",
            params![id.to_string(), DEFAULT_WORKSPACE_NAME, now.to_string()],
        )?;
        Ok(id)
    }
}

/// Creates a new empty file readable and writable only by the owner.
/// Fails if the file already exists.
#[cfg(unix)]
pub(crate) fn create_private_file(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn create_private_file(path: &Path) -> Result<(), StoreError> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_existing_permissions(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(path)?.permissions().mode();
    if mode & 0o077 != 0 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o700))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_existing_permissions(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}
