//! Per-user directories (docs/DATA_MODEL.md, "Directories on Linux").

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

/// Where the agent keeps data, logs, its lock and its sockets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPaths {
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub runtime_dir: PathBuf,
}

/// Why the directories cannot be used. Messages name the directory role, not its path.
#[derive(Debug)]
pub enum PathError {
    /// `XDG_RUNTIME_DIR` is unset or not absolute; there is no safe fallback.
    NoRuntimeDir,
    /// `HOME` is needed for a default and is unset or not absolute.
    NoHome,
    /// An override variable is not an absolute path.
    InvalidOverride(&'static str),
    /// A directory exists but is not owned by this user or is accessible to others.
    Insecure(&'static str),
    Io(&'static str, std::io::Error),
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRuntimeDir => f.write_str(
                "XDG_RUNTIME_DIR is not set to an absolute path; refusing to create sockets elsewhere",
            ),
            Self::NoHome => f.write_str("HOME is not set to an absolute path"),
            Self::InvalidOverride(name) => write!(f, "{name} must be an absolute path"),
            Self::Insecure(role) => write!(f, "the {role} directory is not private to this user"),
            Self::Io(role, error) => {
                write!(f, "cannot prepare the {role} directory: {}", error.kind())
            }
        }
    }
}

impl std::error::Error for PathError {}

fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

impl AgentPaths {
    /// Resolves directories from the environment. `REOPEN_DATA_DIR`,
    /// `REOPEN_STATE_DIR` and `REOPEN_RUNTIME_DIR` override everything (tests
    /// and isolated runs); otherwise XDG variables apply, and development
    /// builds use the `reopen-dev` name so they never touch production data.
    ///
    /// # Errors
    ///
    /// [`PathError`] when a required variable is missing or not absolute.
    pub fn resolve(dev: bool, var: impl Fn(&str) -> Option<OsString>) -> Result<Self, PathError> {
        let name = if dev { "reopen-dev" } else { "reopen" };
        let override_of = |key: &'static str| -> Result<Option<PathBuf>, PathError> {
            match var(key) {
                None => Ok(None),
                Some(value) => absolute(Some(value))
                    .map(Some)
                    .ok_or(PathError::InvalidOverride(key)),
            }
        };
        let home = || absolute(var("HOME")).ok_or(PathError::NoHome);
        let xdg = |key: &str, default: &str| -> Result<PathBuf, PathError> {
            match absolute(var(key)) {
                Some(path) => Ok(path.join(name)),
                None => Ok(home()?.join(default).join(name)),
            }
        };
        Ok(Self {
            data_dir: match override_of("REOPEN_DATA_DIR")? {
                Some(path) => path,
                None => xdg("XDG_DATA_HOME", ".local/share")?,
            },
            state_dir: match override_of("REOPEN_STATE_DIR")? {
                Some(path) => path,
                None => xdg("XDG_STATE_HOME", ".local/state")?,
            },
            runtime_dir: match override_of("REOPEN_RUNTIME_DIR")? {
                Some(path) => path,
                None => absolute(var("XDG_RUNTIME_DIR"))
                    .ok_or(PathError::NoRuntimeDir)?
                    .join(name),
            },
        })
    }

    #[must_use]
    pub fn database(&self) -> PathBuf {
        self.data_dir.join("reopen.sqlite")
    }

    #[must_use]
    pub fn log_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }

    #[must_use]
    pub fn lock_file(&self) -> PathBuf {
        self.runtime_dir.join("agent.lock")
    }

    #[must_use]
    pub fn desktop_socket(&self) -> PathBuf {
        self.runtime_dir.join("desktop.sock")
    }

    /// Creates the directories with mode 0700. Data and log directories with
    /// broader modes are tightened; a runtime directory that is not private,
    /// or any directory owned by someone else, is refused.
    ///
    /// # Errors
    ///
    /// [`PathError::Insecure`] or [`PathError::Io`].
    pub fn prepare(&self) -> Result<(), PathError> {
        prepare_dir(&self.data_dir, "data", true)?;
        prepare_dir(&self.state_dir, "state", true)?;
        prepare_dir(&self.log_dir(), "log", true)?;
        prepare_dir(&self.runtime_dir, "runtime", false)
    }
}

fn prepare_dir(path: &Path, role: &'static str, may_tighten: bool) -> Result<(), PathError> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};

    let io = |error| PathError::Io(role, error);
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(io)?;
    }
    let metadata = std::fs::symlink_metadata(path).map_err(io)?;
    if !metadata.is_dir() || metadata.uid() != rustix::process::getuid().as_raw() {
        return Err(PathError::Insecure(role));
    }
    if metadata.mode() & 0o077 != 0 {
        if !may_tighten {
            return Err(PathError::Insecure(role));
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).into()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn xdg_defaults_and_dev_isolation() {
        let vars = env(&[("HOME", "/home/u"), ("XDG_RUNTIME_DIR", "/run/user/1000")]);
        let prod = AgentPaths::resolve(false, &vars).unwrap();
        assert_eq!(prod.data_dir, Path::new("/home/u/.local/share/reopen"));
        assert_eq!(prod.state_dir, Path::new("/home/u/.local/state/reopen"));
        assert_eq!(
            prod.desktop_socket(),
            Path::new("/run/user/1000/reopen/desktop.sock")
        );
        let dev = AgentPaths::resolve(true, &vars).unwrap();
        assert_eq!(dev.data_dir, Path::new("/home/u/.local/share/reopen-dev"));
        assert_eq!(dev.runtime_dir, Path::new("/run/user/1000/reopen-dev"));
    }

    #[test]
    fn relative_xdg_values_fall_back_to_defaults() {
        let vars = env(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "relative"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]);
        assert_eq!(
            AgentPaths::resolve(false, vars).unwrap().data_dir,
            Path::new("/home/u/.local/share/reopen")
        );
    }

    #[test]
    fn missing_runtime_dir_is_refused() {
        assert!(matches!(
            AgentPaths::resolve(false, env(&[("HOME", "/home/u")])),
            Err(PathError::NoRuntimeDir)
        ));
        let relative = env(&[("HOME", "/home/u"), ("XDG_RUNTIME_DIR", "run")]);
        assert!(matches!(
            AgentPaths::resolve(false, relative),
            Err(PathError::NoRuntimeDir)
        ));
    }

    #[test]
    fn overrides_must_be_absolute() {
        let vars = env(&[
            ("REOPEN_DATA_DIR", "data"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("HOME", "/h"),
        ]);
        assert!(matches!(
            AgentPaths::resolve(false, vars),
            Err(PathError::InvalidOverride("REOPEN_DATA_DIR"))
        ));
    }

    #[test]
    fn prepare_creates_private_dirs_and_refuses_a_shared_runtime_dir() {
        let root = tempfile::TempDir::new().unwrap();
        let paths = AgentPaths {
            data_dir: root.path().join("data/reopen"),
            state_dir: root.path().join("state/reopen"),
            runtime_dir: root.path().join("run/reopen"),
        };
        paths.prepare().unwrap();
        for dir in [
            &paths.data_dir,
            &paths.state_dir,
            &paths.runtime_dir,
            &paths.log_dir(),
        ] {
            assert_eq!(
                std::fs::metadata(dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }

        let shared = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&paths.data_dir, shared.clone()).unwrap();
        std::fs::set_permissions(&paths.runtime_dir, shared).unwrap();
        assert!(matches!(
            paths.prepare(),
            Err(PathError::Insecure("runtime"))
        ));
        assert_eq!(
            std::fs::metadata(&paths.data_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700,
            "data dir is tightened"
        );
    }
}
