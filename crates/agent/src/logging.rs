//! Bounded diagnostic logs.
//!
//! Log events carry stable event codes, counts, durations and opaque IDs only:
//! never addresses, paths, titles, payloads or credentials (AGENTS.md
//! invariant 7). Files rotate by size: `agent.log`, `.1` and `.2`, 1 MiB each.

use std::fs::File;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// Total files kept, including the active one.
pub const LOG_FILES: usize = 3;

/// An append-only log file that rotates when it would exceed [`MAX_LOG_BYTES`].
#[derive(Debug)]
pub struct RotatingLog {
    dir: PathBuf,
    file: File,
    size: u64,
    max_bytes: u64,
}

fn open_append(path: &Path) -> io::Result<File> {
    File::options()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

impl RotatingLog {
    /// # Errors
    ///
    /// I/O errors opening `agent.log` in `dir`.
    pub fn open(dir: &Path) -> io::Result<Self> {
        Self::with_limit(dir, MAX_LOG_BYTES)
    }

    /// # Errors
    ///
    /// I/O errors opening the active log file.
    pub fn with_limit(dir: &Path, max_bytes: u64) -> io::Result<Self> {
        let file = open_append(&dir.join("agent.log"))?;
        let size = file.metadata()?.len();
        Ok(Self {
            dir: dir.to_owned(),
            file,
            size,
            max_bytes,
        })
    }

    fn path(&self, index: usize) -> PathBuf {
        if index == 0 {
            self.dir.join("agent.log")
        } else {
            self.dir.join(format!("agent.log.{index}"))
        }
    }

    fn rotate(&mut self) -> io::Result<()> {
        for index in (1..LOG_FILES).rev() {
            let from = self.path(index - 1);
            if from.exists() {
                std::fs::rename(from, self.path(index))?;
            }
        }
        self.file = open_append(&self.path(0))?;
        self.size = 0;
        Ok(())
    }
}

impl Write for RotatingLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if self.size > 0 && self.size.saturating_add(len) > self.max_bytes {
            self.rotate()?;
        }
        self.file.write_all(buf)?;
        self.size = self.size.saturating_add(len);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Installs the global subscriber writing to `dir/agent.log`.
///
/// # Errors
///
/// I/O errors opening the log, or if a subscriber is already installed.
pub fn init(dir: &Path) -> io::Result<()> {
    let log = RotatingLog::open(dir)?;
    tracing_subscriber::fmt()
        .with_writer(Mutex::new(log))
        .with_ansi(false)
        .with_target(false)
        .try_init()
        .map_err(|_| io::Error::other("logging already initialised"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_keeps_three_bounded_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut log = RotatingLog::with_limit(dir.path(), 100).unwrap();
        for i in 0..20 {
            writeln!(log, "{i:040}").unwrap();
        }
        let mut files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(files, ["agent.log", "agent.log.1", "agent.log.2"]);
        for file in files {
            assert!(std::fs::metadata(dir.path().join(file)).unwrap().len() <= 100);
        }
    }
}
