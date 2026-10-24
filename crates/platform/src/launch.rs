//! Application launching.
//!
//! Only trusted adapters turn an approved resource into a launch. The input is
//! an application identity (for example a desktop ID), never a stored command
//! line; the adapter resolves it against what is installed now and refuses
//! anything missing or changed rather than guessing.

use std::sync::{Mutex, PoisonError};

/// Why a launch did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchError {
    /// No installed application has this identity any more.
    NotInstalled,
    /// The application is installed but its program is missing or not executable.
    ExecutableMissing,
    /// The application is excluded from capture and restore.
    Excluded,
    /// The launch mechanism is not available on this system.
    Unsupported,
    /// The launch was attempted and failed.
    Failed,
}

impl LaunchError {
    /// Stable, log-safe name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::ExecutableMissing => "executable_missing",
            Self::Excluded => "excluded",
            Self::Unsupported => "unsupported",
            Self::Failed => "failed",
        }
    }
}

/// Evidence of a started launch, for duplicate detection and reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchReceipt {
    /// An adapter-specific handle, for example a systemd unit name.
    pub handle: String,
}

/// Starts applications by identity.
pub trait AppLauncher: Send + Sync {
    /// Launches the application once. May block briefly; run it on a worker.
    ///
    /// # Errors
    ///
    /// [`LaunchError`] when the app cannot be launched safely.
    fn launch(&self, app_id: &str) -> Result<LaunchReceipt, LaunchError>;
}

/// Records launches and returns scripted results; for tests.
#[derive(Debug, Default)]
pub struct FakeLauncher {
    failures: Mutex<Vec<(String, LaunchError)>>,
    launched: Mutex<Vec<String>>,
}

impl FakeLauncher {
    /// Makes launches of `app_id` fail with `error`.
    pub fn fail(&self, app_id: &str, error: LaunchError) {
        self.failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((app_id.to_owned(), error));
    }

    #[must_use]
    pub fn launched(&self) -> Vec<String> {
        self.launched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl AppLauncher for FakeLauncher {
    fn launch(&self, app_id: &str) -> Result<LaunchReceipt, LaunchError> {
        let failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, error)) = failures.iter().find(|(id, _)| id == app_id) {
            return Err(*error);
        }
        drop(failures);
        let mut launched = self.launched.lock().unwrap_or_else(PoisonError::into_inner);
        launched.push(app_id.to_owned());
        Ok(LaunchReceipt {
            handle: format!("fake-{}", launched.len()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_records_launches_and_scripted_failures() {
        let launcher = FakeLauncher::default();
        launcher.fail("gone.desktop", LaunchError::NotInstalled);
        assert_eq!(launcher.launch("a.desktop").unwrap().handle, "fake-1");
        assert_eq!(
            launcher.launch("gone.desktop"),
            Err(LaunchError::NotInstalled)
        );
        assert_eq!(launcher.launched(), ["a.desktop"]);
        assert_eq!(
            LaunchError::ExecutableMissing.as_str(),
            "executable_missing"
        );
    }
}
