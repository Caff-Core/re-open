//! Launching applications on Linux.
//!
//! An app is launched by desktop ID: the current catalog entry is resolved,
//! its program validated, and its argument vector (field codes removed) is
//! started through `systemd-run --user` as a transient
//! `app-reopen-<desktop-id>-<n>.service`. The app then belongs to the user's
//! systemd manager, not to the agent: it outlives the agent, leaves no zombie
//! behind, and follows GNOME's `app-…` unit naming so it can be found again.
//! Nothing passes through a shell.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use reopen_platform::launch::{AppLauncher, LaunchError, LaunchReceipt};

use crate::catalog::{AppIdentity, Catalog, ExclusionPolicy, ExecError, resolve_program};

/// How long `systemd-run` may take to register the unit.
const SYSTEMD_RUN_TIMEOUT: Duration = Duration::from_secs(10);

/// Escapes a string for use inside a systemd unit name component: `-` is
/// the component separator, so it and anything outside `[A-Za-z0-9:_.]`
/// becomes `\xHH`.
#[must_use]
pub fn escape_unit_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'.') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("\\x{byte:02x}"));
        }
    }
    out
}

/// The full `systemd-run` argument vector for launching `app` via `program`.
#[must_use]
pub fn systemd_run_args(
    app: &AppIdentity,
    program: &Path,
    unit: &str,
    home: Option<&Path>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--user",
        "--collect",
        "--quiet",
        "--no-block",
        "--service-type=exec",
        "--property=ExitType=cgroup",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    args.push(format!("--unit={unit}").into());
    args.push(format!("--description=Re-Open restore of {}", app.desktop_id).into());
    if let Some(home) = home {
        let mut working_directory = OsString::from("--working-directory=");
        working_directory.push(home);
        args.push(working_directory);
    }
    args.push("--".into());
    args.push(program.as_os_str().to_owned());
    args.extend(app.exec.iter().skip(1).map(OsString::from));
    args
}

/// Launches GNOME/freedesktop applications by desktop ID.
#[derive(Debug)]
pub struct LinuxLauncher {
    pub data_dirs: Vec<PathBuf>,
    pub path_var: Option<OsString>,
    pub home: Option<PathBuf>,
    pub exclusions: ExclusionPolicy,
    /// `systemd-run`; resolved from `PATH` when not set.
    pub systemd_run: Option<PathBuf>,
    counter: AtomicU64,
}

impl LinuxLauncher {
    #[must_use]
    pub fn from_env(exclusions: ExclusionPolicy) -> Self {
        Self {
            data_dirs: crate::catalog::data_dirs(|key| std::env::var_os(key)),
            path_var: std::env::var_os("PATH"),
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_absolute()),
            exclusions,
            systemd_run: None,
            counter: AtomicU64::new(0),
        }
    }

    fn unit_name(&self, desktop_id: &str) -> String {
        let id = desktop_id.strip_suffix(".desktop").unwrap_or(desktop_id);
        let serial = self.counter.fetch_add(1, Ordering::Relaxed);
        format!(
            "app-reopen-{}-{}{serial}.service",
            escape_unit_component(id),
            std::process::id()
        )
    }
}

fn run_bounded(mut command: Command) -> Result<(), LaunchError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| LaunchError::Unsupported)?;
    let deadline = Instant::now() + SYSTEMD_RUN_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) | Err(_) => return Err(LaunchError::Failed),
            Ok(None) if Instant::now() >= deadline => {
                // Stops only systemd-run itself, never an application.
                let _ = child.kill();
                let _ = child.wait();
                return Err(LaunchError::Failed);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

impl AppLauncher for LinuxLauncher {
    fn launch(&self, app_id: &str) -> Result<LaunchReceipt, LaunchError> {
        let catalog = Catalog::load(&self.data_dirs);
        let app = catalog.find(app_id).ok_or(LaunchError::NotInstalled)?;
        if !self.exclusions.allows(app) {
            return Err(LaunchError::Excluded);
        }
        let program = app
            .validate_exec(self.path_var.as_ref())
            .map_err(|error| match error {
                ExecError::Missing | ExecError::UnexpectedRunner => LaunchError::ExecutableMissing,
            })?;
        let systemd_run = match &self.systemd_run {
            Some(path) => Some(path.clone()),
            None => resolve_program("systemd-run", self.path_var.as_ref()),
        }
        .ok_or(LaunchError::Unsupported)?;

        let unit = self.unit_name(&app.desktop_id);
        let mut command = Command::new(systemd_run);
        command.args(systemd_run_args(app, &program, &unit, self.home.as_deref()));
        run_bounded(command)?;
        Ok(LaunchReceipt { handle: unit })
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn executable(path: &Path, text: &str) {
        write(path, text);
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn unit_components_escape_separators_and_unsafe_bytes() {
        assert_eq!(
            escape_unit_component("org.gnome.TextEditor"),
            "org.gnome.TextEditor"
        );
        assert_eq!(escape_unit_component("brave-browser"), r"brave\x2dbrowser");
        assert_eq!(escape_unit_component("a b/c"), r"a\x20b\x2fc");
        assert_eq!(
            crate::windows::unit_app_id(&format!(
                "app-reopen-{}-7.service",
                escape_unit_component("brave-browser")
            ))
            .as_deref(),
            Some("brave-browser"),
            "units Re-Open starts are recognised again by the fallback"
        );
    }

    struct Setup {
        _root: tempfile::TempDir,
        launcher: LinuxLauncher,
        record: PathBuf,
        bin: PathBuf,
    }

    /// A catalog in a temp dir and a fake `systemd-run` that records its argv,
    /// one argument per line, to prove nothing is shell-interpreted.
    fn setup(exit_code: i32) -> Setup {
        let root = tempfile::TempDir::new().unwrap();
        let bin = root.path().join("bin");
        let record = root.path().join("argv");
        executable(&bin.join("editor"), "#!/bin/sh\n");
        executable(
            &bin.join("systemd-run"),
            &format!(
                "#!/bin/sh\nfor arg in \"$@\"; do printf '%s\\n' \"$arg\" >> '{}'; done\nexit {exit_code}\n",
                record.display()
            ),
        );
        let apps = root.path().join("data/applications");
        let entry = |name: &str, exec: &str, extra: &str| {
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n{extra}")
        };
        write(
            &apps.join("org.example.Editor.desktop"),
            &entry("Editor", r#"editor --title "a; rm -rf ~" %F"#, ""),
        );
        write(
            &apps.join("org.example.Gone.desktop"),
            &entry("Gone", "missing-binary", ""),
        );
        write(
            &apps.join("org.example.Helper.desktop"),
            &entry("Helper", "editor", "NoDisplay=true\n"),
        );
        let launcher = LinuxLauncher {
            data_dirs: vec![root.path().join("data")],
            path_var: Some(bin.clone().into_os_string()),
            home: Some(root.path().to_owned()),
            exclusions: ExclusionPolicy::default(),
            systemd_run: Some(bin.join("systemd-run")),
            counter: AtomicU64::new(0),
        };
        Setup {
            _root: root,
            launcher,
            record,
            bin,
        }
    }

    #[test]
    fn launches_the_argument_vector_through_systemd_run_without_a_shell() {
        let s = setup(0);
        let receipt = s.launcher.launch("org.example.Editor").unwrap();
        assert!(
            receipt.handle.starts_with("app-reopen-org.example.Editor-"),
            "{}",
            receipt.handle
        );
        assert!(
            std::path::Path::new(&receipt.handle)
                .extension()
                .is_some_and(|ext| ext == "service")
        );

        let argv: Vec<String> = std::fs::read_to_string(&s.record)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        let separator = argv.iter().position(|arg| arg == "--").unwrap();
        assert!(argv[..separator].contains(&"--user".to_owned()));
        assert!(argv[..separator].contains(&format!("--unit={}", receipt.handle)));
        assert_eq!(
            argv[separator + 1..],
            [
                s.bin.join("editor").display().to_string(),
                "--title".to_owned(),
                "a; rm -rf ~".to_owned()
            ],
            "the program is resolved, arguments are kept verbatim and field codes are gone"
        );
    }

    #[test]
    fn refuses_missing_excluded_and_unlaunchable_apps_without_running_anything() {
        let s = setup(0);
        assert_eq!(
            s.launcher.launch("org.example.Nope.desktop"),
            Err(LaunchError::NotInstalled)
        );
        assert_eq!(
            s.launcher.launch("org.example.Gone.desktop"),
            Err(LaunchError::ExecutableMissing)
        );
        assert_eq!(
            s.launcher.launch("org.example.Helper.desktop"),
            Err(LaunchError::Excluded)
        );
        assert!(!s.record.exists(), "systemd-run must not be invoked");
    }

    #[test]
    fn a_failing_systemd_run_is_reported() {
        let s = setup(1);
        assert_eq!(
            s.launcher.launch("org.example.Editor.desktop"),
            Err(LaunchError::Failed)
        );
    }

    #[test]
    fn missing_systemd_run_is_unsupported() {
        let mut s = setup(0);
        s.launcher.systemd_run = None;
        s.launcher.path_var = Some(OsString::from("/nonexistent"));
        // With PATH pointing nowhere the program is missing first.
        assert_eq!(
            s.launcher.launch("org.example.Editor.desktop"),
            Err(LaunchError::ExecutableMissing)
        );
    }
}
