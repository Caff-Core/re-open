//! Visible-application discovery on GNOME (ADR-016).
//!
//! Primary: the Re-Open window-list GNOME Shell extension over the session
//! bus. Fallback: apps identified from systemd user units, which shows what
//! is running but not which windows are open, so it is always reported as
//! partial. The matching logic is pure; the D-Bus calls are thin.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;

use reopen_core::ResourcePayload;
use reopen_core::resource::ApplicationV1;
use zbus::{OwnedObjectPath, OwnedVariant as OwnedValue};

use crate::catalog::{Catalog, ExclusionPolicy};

pub const EXTENSION_BUS_NAME: &str = "com.caffcore.ReOpen.Windows";
pub const EXTENSION_PATH: &str = "/com/caffcore/ReOpen/Windows";
pub const EXTENSION_INTERFACE: &str = "com.caffcore.ReOpen.Windows1";
/// The `GetWindows` API version this build understands.
pub const EXTENSION_API_VERSION: u32 = 1;

/// One window reported by the extension. Titles are never part of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    /// The `.desktop` ID GNOME associated with the window, or empty / `window:N`
    /// when it could not associate one.
    pub app_id: String,
    pub pid: i32,
}

/// Apps found, in first-seen order, and how many items could not be matched
/// to a capturable, launchable installed application.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Discovered {
    pub apps: Vec<ResourcePayload>,
    pub unidentified: usize,
}

/// Turns IDs into one application resource per installed, allowed and
/// launchable app. Excluded apps (helpers, shell components, user
/// exclusions) are dropped silently; unknown or unlaunchable ones are counted.
pub fn match_apps<'a>(
    ids: impl IntoIterator<Item = &'a str>,
    catalog: &Catalog,
    policy: &ExclusionPolicy,
    path_var: Option<&OsString>,
) -> Discovered {
    let mut seen = BTreeSet::new();
    let mut found = Discovered::default();
    for id in ids {
        let Some(app) = (!id.is_empty()).then(|| catalog.find(id)).flatten() else {
            found.unidentified += 1;
            continue;
        };
        if !policy.allows(app) || !seen.insert(app.desktop_id.clone()) {
            continue;
        }
        if app.validate_exec(path_var).is_err() {
            found.unidentified += 1;
            continue;
        }
        found
            .apps
            .push(ResourcePayload::ApplicationV1(ApplicationV1 {
                app_id: app.desktop_id.clone(),
                display_name: app.name.clone(),
            }));
    }
    found
}

/// Reverses systemd unit-name escaping (`\x2d` → `-`).
fn unescape_unit(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find("\\x") {
        out.push_str(&rest[..index]);
        let hex = rest.get(index + 2..index + 4);
        match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
            Some(byte) => {
                out.push(char::from(byte));
                rest = &rest[index + 4..];
            }
            None => {
                out.push_str("\\x");
                rest = &rest[index + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The application ID encoded in a systemd user unit name, if any:
/// `app-<launcher>-<id>-<n>.scope|.service` (launchers `gnome`, `flatpak`,
/// `reopen`), `app-<id>-<n>.scope` and D-Bus-activated
/// `dbus-<bus>-<id>@<n>.service`. Template instances such as
/// `app-gnome-<id>@autostart.service` are autostarted helpers and are skipped.
#[must_use]
pub fn unit_app_id(unit: &str) -> Option<String> {
    if let Some(service) = unit.strip_suffix(".service")
        && let Some(activated) = service.strip_prefix("dbus-")
    {
        let (_bus, activated) = activated.split_once('-')?;
        let id = activated.split('@').next()?;
        return (!id.is_empty()).then(|| unescape_unit(id));
    }
    let name = unit
        .strip_suffix(".scope")
        .or_else(|| unit.strip_suffix(".service"))?;
    if name.contains('@') {
        return None;
    }
    let rest = name.strip_prefix("app-")?;
    let rest = ["gnome-", "flatpak-", "reopen-"]
        .iter()
        .find_map(|launcher| rest.strip_prefix(launcher))
        .unwrap_or(rest);
    let (id, instance) = rest.rsplit_once('-')?;
    if id.is_empty() || instance.is_empty() || !instance.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(unescape_unit(id))
}

/// Why the extension could not be used; any of these selects the fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionError {
    /// No session bus, or the extension is not installed or not enabled.
    NotAvailable,
    /// The extension speaks a different API version.
    UnsupportedVersion(u32),
    /// The reply did not have the expected shape.
    InvalidReply,
}

fn session() -> Result<zbus::Connection, ExtensionError> {
    zbus::block_on(zbus::Connection::session()).map_err(|_| ExtensionError::NotAvailable)
}

/// Reads normal windows from the extension.
///
/// # Errors
///
/// [`ExtensionError`] when the extension is absent, a different version, or
/// replies unexpectedly.
pub fn extension_windows() -> Result<Vec<WindowInfo>, ExtensionError> {
    let connection = session()?;
    zbus::block_on(async {
        let version = connection
            .call_method(
                Some(EXTENSION_BUS_NAME),
                EXTENSION_PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(EXTENSION_INTERFACE, "Version"),
            )
            .await
            .map_err(|_| ExtensionError::NotAvailable)?;
        let (version,): (OwnedValue,) = version
            .body()
            .deserialize()
            .map_err(|_| ExtensionError::InvalidReply)?;
        let version = u32::try_from(version).map_err(|_| ExtensionError::InvalidReply)?;
        if version != EXTENSION_API_VERSION {
            return Err(ExtensionError::UnsupportedVersion(version));
        }

        let reply = connection
            .call_method(
                Some(EXTENSION_BUS_NAME),
                EXTENSION_PATH,
                Some(EXTENSION_INTERFACE),
                "GetWindows",
                &(),
            )
            .await
            .map_err(|_| ExtensionError::NotAvailable)?;
        let (windows,): (Vec<HashMap<String, OwnedValue>>,) = reply
            .body()
            .deserialize()
            .map_err(|_| ExtensionError::InvalidReply)?;
        windows
            .into_iter()
            .map(|window| {
                let app_id = window
                    .get("app_id")
                    .and_then(|value| String::try_from(value.try_clone().ok()?).ok())
                    .ok_or(ExtensionError::InvalidReply)?;
                let pid = window
                    .get("pid")
                    .and_then(|value| i32::try_from(value).ok())
                    .ok_or(ExtensionError::InvalidReply)?;
                Ok(WindowInfo { app_id, pid })
            })
            .collect()
    })
}

type UnitRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    OwnedObjectPath,
    u32,
    String,
    OwnedObjectPath,
);

/// Lists active app scopes and D-Bus-activated services of the user manager.
///
/// # Errors
///
/// `None` when the user systemd manager is unreachable.
#[must_use]
pub fn running_app_units() -> Option<Vec<String>> {
    let connection = zbus::block_on(zbus::Connection::session()).ok()?;
    zbus::block_on(async {
        let reply = connection
            .call_method(
                Some("org.freedesktop.systemd1"),
                "/org/freedesktop/systemd1",
                Some("org.freedesktop.systemd1.Manager"),
                "ListUnitsByPatterns",
                &(
                    vec!["active"],
                    vec!["app-*.scope", "app-*.service", "dbus-*.service"],
                ),
            )
            .await
            .ok()?;
        let (units,): (Vec<UnitRow>,) = reply.body().deserialize().ok()?;
        Some(units.into_iter().map(|unit| unit.0).collect())
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    use super::*;

    #[test]
    fn unit_names_yield_app_ids() {
        assert_eq!(
            unit_app_id(r"app-gnome-brave\x2dbrowser-160758.scope").as_deref(),
            Some("brave-browser")
        );
        assert_eq!(
            unit_app_id("app-gnome-org.gnome.TextEditor-42.scope").as_deref(),
            Some("org.gnome.TextEditor")
        );
        assert_eq!(
            unit_app_id("app-flatpak-com.microsoft.Edge-641628974.scope").as_deref(),
            Some("com.microsoft.Edge")
        );
        assert_eq!(
            unit_app_id("app-org.chromium.Chromium-160765.scope").as_deref(),
            Some("org.chromium.Chromium")
        );
        assert_eq!(
            unit_app_id("dbus-:1.2-org.gnome.Nautilus@5.service").as_deref(),
            Some("org.gnome.Nautilus")
        );
        assert_eq!(unit_app_id("session-2.scope"), None);
        assert_eq!(unit_app_id("app-.scope"), None);
        assert_eq!(unit_app_id("pipewire.service"), None);
        assert_eq!(
            unit_app_id(r"app-reopen-org.gnome.Calculator-12340.service").as_deref(),
            Some("org.gnome.Calculator")
        );
        assert_eq!(
            unit_app_id("app-gnome-org.kde.kdeconnect.daemon@autostart.service"),
            None
        );
        assert_eq!(
            unit_app_id("app-gnome-xdg-user-dirs-update.service"),
            None,
            "instance must be numeric"
        );
    }

    #[test]
    fn unescaping_ignores_malformed_sequences() {
        assert_eq!(unescape_unit(r"a\x2db\x2"), r"a-b\x2");
        assert_eq!(unescape_unit(r"\xzz"), r"\xzz");
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn matching_dedupes_excludes_and_counts_unknown_or_unlaunchable_apps() {
        let root = tempfile::TempDir::new().unwrap();
        let bin = root.path().join("bin");
        write(&bin.join("editor"), "#!/bin/sh\n");
        std::fs::set_permissions(bin.join("editor"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let apps = root.path().join("data/applications");
        let entry = |name: &str, exec: &str, extra: &str| {
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n{extra}")
        };
        write(
            &apps.join("org.example.Editor.desktop"),
            &entry("Editor", "editor %F", ""),
        );
        write(
            &apps.join("org.example.Gone.desktop"),
            &entry("Gone", "missing-binary", ""),
        );
        write(
            &apps.join("org.example.Daemon.desktop"),
            &entry("Daemon", "editor", "NoDisplay=true\n"),
        );
        let catalog = Catalog::load(&[root.path().join("data")]);
        let path = OsString::from(bin.as_os_str());

        let found = match_apps(
            [
                "org.example.Editor.desktop",
                "org.example.Editor",
                "org.example.Daemon.desktop",
                "org.example.Gone.desktop",
                "window:17",
                "",
            ],
            &catalog,
            &ExclusionPolicy::default(),
            Some(&path),
        );
        assert_eq!(
            found.apps,
            [ResourcePayload::ApplicationV1(ApplicationV1 {
                app_id: "org.example.Editor.desktop".to_owned(),
                display_name: "Editor".to_owned()
            })]
        );
        assert_eq!(
            found.unidentified, 3,
            "missing binary, window:17 and an empty ID"
        );
    }
}
