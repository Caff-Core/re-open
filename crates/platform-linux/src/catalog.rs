//! The installed-application catalog: desktop IDs resolved across the XDG data
//! directories, executable validation and capture exclusions.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use crate::desktop::{DesktopEntry, parse_entry};

/// Upper bounds on catalog scanning, so a pathological directory cannot stall capture.
const MAX_DEPTH: usize = 4;
const MAX_ENTRIES: usize = 10_000;

/// How an application is installed, which decides how it is launched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppKind {
    Native,
    Flatpak { app_id: String },
    Snap { name: String },
}

/// One application identified by its desktop ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdentity {
    /// The desktop file ID, for example `org.gnome.Nautilus.desktop`.
    pub desktop_id: String,
    pub name: String,
    pub kind: AppKind,
    /// The command line without field codes; launched as an argument vector.
    pub exec: Vec<String>,
    pub try_exec: Option<String>,
    /// Shown in menus (`NoDisplay` is false). Helpers and daemons are not.
    pub user_visible: bool,
    /// The file that defined it; diagnostics only, never logged.
    pub source: PathBuf,
}

/// The XDG data directories in precedence order: `$XDG_DATA_HOME` (default
/// `~/.local/share`) then `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`).
/// Relative entries are ignored, as the spec requires.
#[must_use]
pub fn data_dirs(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let absolute = |path: PathBuf| path.is_absolute().then_some(path);
    let mut dirs = Vec::new();
    let home = var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .and_then(absolute)
        .or_else(|| {
            var("HOME")
                .map(PathBuf::from)
                .and_then(absolute)
                .map(|home| home.join(".local/share"))
        });
    dirs.extend(home);
    let system = var("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(std::env::split_paths(&system).filter_map(absolute));
    dirs.dedup();
    dirs
}

/// Installed applications by desktop ID.
#[derive(Debug, Default)]
pub struct Catalog {
    apps: BTreeMap<String, AppIdentity>,
    /// IDs deleted by a higher-precedence `Hidden=true` entry.
    masked: BTreeSet<String>,
    /// Desktop files that could not be read or parsed.
    pub skipped: usize,
}

fn kind(entry: &DesktopEntry) -> AppKind {
    if let Some(app_id) = &entry.flatpak_id {
        return AppKind::Flatpak {
            app_id: app_id.clone(),
        };
    }
    if let Some(name) = &entry.snap_name {
        return AppKind::Snap { name: name.clone() };
    }
    match entry
        .exec
        .first()
        .and_then(|program| program.strip_prefix("/snap/bin/"))
    {
        Some(name) if !name.is_empty() => AppKind::Snap {
            name: name.to_owned(),
        },
        _ => AppKind::Native,
    }
}

impl Catalog {
    /// Loads `<dir>/applications` for each directory. The first directory that
    /// defines a desktop ID wins, including a `Hidden=true` entry, which
    /// deletes that ID for lower-precedence directories.
    #[must_use]
    pub fn load(dirs: &[PathBuf]) -> Self {
        let mut catalog = Self::default();
        let mut seen = 0;
        for dir in dirs {
            let root = dir.join("applications");
            let mut files = Vec::new();
            collect(&root, &root, 0, &mut files, &mut seen);
            for (desktop_id, path) in files {
                if catalog.apps.contains_key(&desktop_id) || catalog.masked.contains(&desktop_id) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    catalog.skipped += 1;
                    continue;
                };
                match parse_entry(&text) {
                    Ok(entry) if entry.hidden => {
                        catalog.masked.insert(desktop_id);
                    }
                    Ok(entry) => {
                        let identity = AppIdentity {
                            kind: kind(&entry),
                            name: entry.name,
                            exec: entry.exec,
                            try_exec: entry.try_exec,
                            user_visible: !entry.no_display,
                            source: path,
                            desktop_id: desktop_id.clone(),
                        };
                        catalog.apps.insert(desktop_id, identity);
                    }
                    Err(_) => catalog.skipped += 1,
                }
            }
        }
        catalog
    }

    #[must_use]
    pub fn get(&self, desktop_id: &str) -> Option<&AppIdentity> {
        self.apps.get(desktop_id)
    }

    /// Looks up an ID with or without the `.desktop` suffix.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&AppIdentity> {
        self.get(id).or_else(|| self.get(&format!("{id}.desktop")))
    }

    pub fn iter(&self) -> impl Iterator<Item = &AppIdentity> {
        self.apps.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.apps.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }
}

/// Collects `(desktop_id, path)` in sorted order; subdirectories contribute
/// `subdir-name.desktop` IDs per the spec.
fn collect(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<(String, PathBuf)>,
    seen: &mut usize,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    for path in paths {
        if *seen >= MAX_ENTRIES {
            return;
        }
        if path.is_dir() {
            collect(root, &path, depth + 1, out, seen);
        } else if path.extension().is_some_and(|ext| ext == "desktop") {
            *seen += 1;
            if let Some(id) = path
                .strip_prefix(root)
                .ok()
                .and_then(|relative| relative.to_str())
            {
                out.push((id.replace('/', "-"), path));
            }
        }
    }
}

/// Why an application cannot be launched as recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecError {
    /// The program (or `TryExec`) does not exist or is not executable.
    Missing,
    /// A Flatpak or Snap entry does not launch through its expected runner.
    UnexpectedRunner,
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// Resolves a program the way `execvp` would: an absolute path as is, a bare
/// name through `PATH`. Never searches elsewhere for a same-named file.
#[must_use]
pub fn resolve_program(program: &str, path_var: Option<&OsString>) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);
        return (path.is_absolute() && is_executable(&path)).then_some(path);
    }
    let search = path_var?;
    std::env::split_paths(search)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

impl AppIdentity {
    /// Checks that the recorded launcher still exists and matches the
    /// install kind, returning the resolved program path.
    ///
    /// # Errors
    ///
    /// [`ExecError`] when the program is missing or the runner is wrong.
    pub fn validate_exec(&self, path_var: Option<&OsString>) -> Result<PathBuf, ExecError> {
        if let Some(try_exec) = &self.try_exec
            && resolve_program(try_exec, path_var).is_none()
        {
            return Err(ExecError::Missing);
        }
        let program = self.exec.first().ok_or(ExecError::Missing)?;
        let resolved = resolve_program(program, path_var).ok_or(ExecError::Missing)?;
        let file_name = resolved
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        match &self.kind {
            AppKind::Flatpak { app_id }
                if file_name != "flatpak" || !self.exec.contains(app_id) =>
            {
                Err(ExecError::UnexpectedRunner)
            }
            AppKind::Snap { .. } if !resolved.starts_with("/snap/bin") => {
                Err(ExecError::UnexpectedRunner)
            }
            _ => Ok(resolved),
        }
    }

    /// A stable, user-facing app ID: the Flatpak or Snap ID when there is one,
    /// otherwise the desktop ID.
    #[must_use]
    pub fn app_id(&self) -> &str {
        match &self.kind {
            AppKind::Flatpak { app_id } => app_id,
            AppKind::Snap { name } => name,
            AppKind::Native => &self.desktop_id,
        }
    }
}

/// Desktop IDs never captured: the shell, session daemons, portals and Re-Open itself.
const BUILT_IN_EXCLUSIONS: &[&str] = &[
    "org.gnome.Shell",
    "org.gnome.SettingsDaemon",
    "org.freedesktop.",
    "xdg-desktop-portal",
    "com.caffcore.reopen",
];

/// Which applications are captured.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExclusionPolicy {
    /// Desktop IDs the user excluded.
    pub desktop_ids: BTreeSet<String>,
}

impl ExclusionPolicy {
    /// Whether an app may be captured: user-visible, not built-in excluded and
    /// not excluded by the user.
    #[must_use]
    pub fn allows(&self, app: &AppIdentity) -> bool {
        app.user_visible
            && !BUILT_IN_EXCLUSIONS
                .iter()
                .any(|prefix| app.desktop_id.starts_with(prefix))
            && !self.desktop_ids.contains(&app.desktop_id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn entry(name: &str, exec: &str, extra: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n{extra}")
    }

    fn executable(path: &Path) {
        write(path, "#!/bin/sh\n");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn data_dirs_follow_xdg_precedence() {
        let vars: HashMap<&str, &str> = [
            ("HOME", "/home/u"),
            ("XDG_DATA_DIRS", "/opt/share:relative:/usr/share"),
        ]
        .into();
        let dirs = data_dirs(|key| vars.get(key).map(Into::into));
        assert_eq!(
            dirs,
            [
                PathBuf::from("/home/u/.local/share"),
                "/opt/share".into(),
                "/usr/share".into()
            ]
        );
        let defaults = data_dirs(|key| (key == "HOME").then(|| "/h".into()));
        assert_eq!(
            defaults,
            [
                PathBuf::from("/h/.local/share"),
                "/usr/local/share".into(),
                "/usr/share".into()
            ]
        );
    }

    #[test]
    fn user_entries_override_and_hidden_masks_system_entries() {
        let root = tempfile::TempDir::new().unwrap();
        let user = root.path().join("user");
        let system = root.path().join("system");
        write(
            &system.join("applications/editor.desktop"),
            &entry("System Editor", "editor", ""),
        );
        write(
            &user.join("applications/editor.desktop"),
            &entry("My Editor", "editor --mine", ""),
        );
        write(
            &system.join("applications/gone.desktop"),
            &entry("Gone", "gone", ""),
        );
        write(
            &user.join("applications/gone.desktop"),
            &entry("Gone", "gone", "Hidden=true\n"),
        );
        write(
            &system.join("applications/kde/okular.desktop"),
            &entry("Okular", "okular %U", ""),
        );
        write(
            &system.join("applications/broken.desktop"),
            "not a desktop file",
        );

        let catalog = Catalog::load(&[user, system]);
        assert_eq!(catalog.get("editor.desktop").unwrap().name, "My Editor");
        assert!(
            catalog.get("gone.desktop").is_none(),
            "Hidden=true deletes the system entry"
        );
        assert_eq!(catalog.find("kde-okular").unwrap().exec, ["okular"]);
        assert_eq!(catalog.skipped, 1);
        assert_eq!(catalog.len(), 2);
    }

    #[test]
    fn executables_resolve_by_path_or_absolute_only() {
        let root = tempfile::TempDir::new().unwrap();
        let bin = root.path().join("bin");
        executable(&bin.join("editor"));
        write(&bin.join("not-executable"), "data");
        let path = OsString::from(bin.as_os_str());

        let app = |exec: &[&str]| AppIdentity {
            desktop_id: "editor.desktop".to_owned(),
            name: "Editor".to_owned(),
            kind: AppKind::Native,
            exec: exec.iter().map(|arg| (*arg).to_owned()).collect(),
            try_exec: None,
            user_visible: true,
            source: PathBuf::new(),
        };
        assert_eq!(
            app(&["editor"]).validate_exec(Some(&path)),
            Ok(bin.join("editor"))
        );
        let absolute = bin.join("editor").to_string_lossy().into_owned();
        assert!(app(&[&absolute]).validate_exec(None).is_ok());
        assert_eq!(
            app(&["missing"]).validate_exec(Some(&path)),
            Err(ExecError::Missing)
        );
        assert_eq!(
            app(&["not-executable"]).validate_exec(Some(&path)),
            Err(ExecError::Missing)
        );
        assert_eq!(
            app(&["bin/editor"]).validate_exec(Some(&path)),
            Err(ExecError::Missing),
            "relative paths are never resolved"
        );

        let mut with_try = app(&["editor"]);
        with_try.try_exec = Some("missing-helper".to_owned());
        assert_eq!(with_try.validate_exec(Some(&path)), Err(ExecError::Missing));
    }

    #[test]
    fn flatpak_and_snap_kinds_need_their_runner() {
        let root = tempfile::TempDir::new().unwrap();
        let bin = root.path().join("bin");
        executable(&bin.join("flatpak"));
        executable(&bin.join("impostor"));
        let path = OsString::from(bin.as_os_str());
        let flatpak = |exec: &[&str]| AppIdentity {
            desktop_id: "com.example.App.desktop".to_owned(),
            name: "App".to_owned(),
            kind: AppKind::Flatpak {
                app_id: "com.example.App".to_owned(),
            },
            exec: exec.iter().map(|arg| (*arg).to_owned()).collect(),
            try_exec: None,
            user_visible: true,
            source: PathBuf::new(),
        };
        assert!(
            flatpak(&["flatpak", "run", "com.example.App"])
                .validate_exec(Some(&path))
                .is_ok()
        );
        assert_eq!(
            flatpak(&["impostor", "com.example.App"]).validate_exec(Some(&path)),
            Err(ExecError::UnexpectedRunner)
        );
        assert_eq!(
            flatpak(&["flatpak", "run", "com.other"]).validate_exec(Some(&path)),
            Err(ExecError::UnexpectedRunner)
        );
        assert_eq!(flatpak(&["flatpak"]).app_id(), "com.example.App");

        let snap = kind(&parse_entry(&entry("S", "/snap/bin/spotify", "")).unwrap());
        assert_eq!(
            snap,
            AppKind::Snap {
                name: "spotify".to_owned()
            }
        );
    }

    #[test]
    fn exclusions_cover_hidden_helpers_shell_components_and_user_choices() {
        let app = |id: &str, visible: bool| AppIdentity {
            desktop_id: id.to_owned(),
            name: id.to_owned(),
            kind: AppKind::Native,
            exec: vec!["x".to_owned()],
            try_exec: None,
            user_visible: visible,
            source: PathBuf::new(),
        };
        let mut policy = ExclusionPolicy::default();
        assert!(policy.allows(&app("org.gnome.Nautilus.desktop", true)));
        assert!(
            !policy.allows(&app("org.gnome.Nautilus.desktop", false)),
            "NoDisplay helpers are excluded"
        );
        assert!(!policy.allows(&app("org.gnome.Shell.Extensions.desktop", true)));
        assert!(!policy.allows(&app(
            "org.gnome.SettingsDaemon.DiskUtilityNotify.desktop",
            true
        )));
        assert!(!policy.allows(&app("com.caffcore.reopen.desktop", true)));
        policy
            .desktop_ids
            .insert("org.gnome.Nautilus.desktop".to_owned());
        assert!(!policy.allows(&app("org.gnome.Nautilus.desktop", true)));
    }
}
