use std::fmt;

/// Storage failures. Messages never include stored addresses, paths or titles.
#[derive(Debug)]
pub enum StoreError {
    /// The file exists but is not a healthy SQLite database. It is left untouched;
    /// writes stay disabled until the user chooses a recovery action.
    Corrupt,
    /// The database was written by a newer build. Refuse to write.
    NewerSchema {
        found: u32,
        supported: u32,
    },
    /// The migration list or recorded version is inconsistent.
    InvalidSchemaVersion,
    /// Data failed validation before any write. `detail` names fields, never values.
    InvalidData {
        detail: String,
    },
    /// Write-ahead logging could not be enabled.
    WalUnavailable,
    /// A pre-migration backup could not be written or verified; nothing was migrated.
    BackupFailed,
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt => f.write_str("the checkpoint database is damaged; writes are disabled"),
            Self::NewerSchema { found, supported } => write!(
                f,
                "the checkpoint database uses schema {found}, newer than this version supports ({supported}); update Re-Open"
            ),
            Self::InvalidSchemaVersion => {
                f.write_str("the checkpoint database schema version is invalid")
            }
            Self::InvalidData { detail } => write!(f, "invalid data: {detail}"),
            Self::WalUnavailable => f.write_str("write-ahead logging could not be enabled"),
            Self::BackupFailed => {
                f.write_str("a verified backup could not be created before migrating")
            }
            Self::Io(error) => write!(f, "storage I/O error: {}", error.kind()),
            Self::Sqlite(error) => write!(f, "storage error: {}", sqlite_code(error)),
        }
    }
}

// Report the SQLite result code only; error text can quote SQL or values.
fn sqlite_code(error: &rusqlite::Error) -> String {
    match error.sqlite_error_code() {
        Some(code) => format!("{code:?}"),
        None => "internal".to_owned(),
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt) => {
                Self::Corrupt
            }
            _ => Self::Sqlite(error),
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
