//! Per-user singleton lock.

use std::fs::{File, TryLockError};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

/// Held for the agent's lifetime; released when dropped or the process exits.
#[derive(Debug)]
pub struct AgentLock {
    _file: File,
}

#[derive(Debug)]
pub enum LockError {
    /// Another agent for this user holds the lock.
    AlreadyRunning,
    Io(std::io::Error),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning => {
                f.write_str("another Re-Open agent is already running for this user")
            }
            Self::Io(error) => write!(f, "cannot take the agent lock: {}", error.kind()),
        }
    }
}

impl std::error::Error for LockError {}

impl AgentLock {
    /// Takes the exclusive lock without waiting. The lock file itself is never
    /// truncated or removed; only the advisory lock matters.
    ///
    /// # Errors
    ///
    /// [`LockError::AlreadyRunning`] when another process holds it.
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(LockError::Io)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(LockError::AlreadyRunning),
            Err(TryLockError::Error(error)) => Err(LockError::Io(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_holder_is_refused_until_the_first_releases() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("agent.lock");
        let first = AgentLock::acquire(&path).unwrap();
        assert!(matches!(
            AgentLock::acquire(&path),
            Err(LockError::AlreadyRunning)
        ));
        drop(first);
        AgentLock::acquire(&path).unwrap();
    }
}
