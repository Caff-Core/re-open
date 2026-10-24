//! The `desktop.apps` capture provider for GNOME.

use std::ffi::OsString;
use std::path::PathBuf;

use reopen_core::checkpoint::ProviderError;
use reopen_core::{AdapterRef, Capability, CapabilitySet};
use reopen_platform::{CaptureProvider, CaptureState, ProviderCapture};

use crate::catalog::{Catalog, ExclusionPolicy};
use crate::windows::{Discovered, extension_windows, match_apps, running_app_units, unit_app_id};

pub const GNOME_SHELL_ADAPTER: &str = "linux.gnome-shell";
pub const SYSTEMD_UNITS_ADAPTER: &str = "linux.systemd-units";

/// Visible applications: from the GNOME Shell extension when it is enabled,
/// otherwise from systemd user units, reported as partial.
#[derive(Debug, Clone)]
pub struct GnomeAppsProvider {
    pub data_dirs: Vec<PathBuf>,
    pub path_var: Option<OsString>,
    pub exclusions: ExclusionPolicy,
}

impl GnomeAppsProvider {
    /// Uses the process environment for data directories and `PATH`.
    #[must_use]
    pub fn from_env(exclusions: ExclusionPolicy) -> Self {
        Self {
            data_dirs: crate::catalog::data_dirs(|key| std::env::var_os(key)),
            path_var: std::env::var_os("PATH"),
            exclusions,
        }
    }
}

/// Builds the capture for discovered apps. `limited` marks the fallback.
#[must_use]
pub fn capture_from(found: Discovered, adapter: &str, limited: bool) -> ProviderCapture {
    let state = if limited {
        CaptureState::Partial(ProviderError::LimitedVisibility)
    } else if found.unidentified > 0 {
        CaptureState::Partial(ProviderError::UnidentifiedWindows)
    } else {
        CaptureState::Complete
    };
    ProviderCapture {
        state,
        resources: found.apps,
        adapter: Some(AdapterRef {
            id: adapter.to_owned(),
            version: 1,
        }),
    }
}

impl CaptureProvider for GnomeAppsProvider {
    fn source_key(&self) -> &str {
        "desktop.apps"
    }

    fn required(&self) -> bool {
        true
    }

    fn adapter(&self) -> AdapterRef {
        AdapterRef {
            id: GNOME_SHELL_ADAPTER.to_owned(),
            version: 1,
        }
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::empty().with(Capability::DiscoverApp)
    }

    fn capture(&self) -> Result<ProviderCapture, ProviderError> {
        let catalog = Catalog::load(&self.data_dirs);
        let path = self.path_var.as_ref();
        if let Ok(windows) = extension_windows() {
            let found = match_apps(
                windows.iter().map(|w| w.app_id.as_str()),
                &catalog,
                &self.exclusions,
                path,
            );
            return Ok(capture_from(found, GNOME_SHELL_ADAPTER, false));
        }
        let units = running_app_units().ok_or(ProviderError::Unavailable)?;
        let ids: Vec<String> = units.iter().filter_map(|unit| unit_app_id(unit)).collect();
        let mut found = match_apps(
            ids.iter().map(String::as_str),
            &catalog,
            &self.exclusions,
            path,
        );
        // Units without a matching app are daemons and services, not windows.
        found.unidentified = 0;
        Ok(capture_from(found, SYSTEMD_UNITS_ADAPTER, true))
    }
}

#[cfg(test)]
mod tests {
    use reopen_core::ResourcePayload;
    use reopen_core::resource::FolderV1;

    use super::*;

    fn apps(n: usize) -> Vec<ResourcePayload> {
        (0..n)
            .map(|i| {
                ResourcePayload::FolderV1(FolderV1 {
                    path: format!("/{i}"),
                })
            })
            .collect()
    }

    #[test]
    fn extension_capture_is_complete_only_when_every_window_is_identified() {
        let complete = capture_from(
            Discovered {
                apps: apps(2),
                unidentified: 0,
            },
            GNOME_SHELL_ADAPTER,
            false,
        );
        assert_eq!(complete.state, CaptureState::Complete);
        assert_eq!(complete.adapter.unwrap().id, GNOME_SHELL_ADAPTER);

        let partial = capture_from(
            Discovered {
                apps: apps(2),
                unidentified: 1,
            },
            GNOME_SHELL_ADAPTER,
            false,
        );
        assert_eq!(
            partial.state,
            CaptureState::Partial(ProviderError::UnidentifiedWindows)
        );
        assert_eq!(partial.resources.len(), 2);
    }

    #[test]
    fn the_fallback_is_always_partial_with_limited_visibility() {
        let fallback = capture_from(
            Discovered {
                apps: apps(3),
                unidentified: 0,
            },
            SYSTEMD_UNITS_ADAPTER,
            true,
        );
        assert_eq!(
            fallback.state,
            CaptureState::Partial(ProviderError::LimitedVisibility)
        );
        assert_eq!(fallback.adapter.unwrap().id, SYSTEMD_UNITS_ADAPTER);
    }
}
