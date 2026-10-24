//! Loads the real application catalog of the machine running the tests.

#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use reopen_platform_linux::{Catalog, ExclusionPolicy, data_dirs};

#[test]
fn the_host_catalog_loads_and_every_entry_is_launchable_as_an_argv() {
    let dirs = data_dirs(|key| std::env::var_os(key));
    let catalog = Catalog::load(&dirs);
    if !std::path::Path::new("/usr/share/applications").exists() {
        return;
    }
    assert!(!catalog.is_empty(), "no applications found in {dirs:?}");
    for app in catalog.iter() {
        assert!(
            !app.exec.is_empty(),
            "{} has an empty command",
            app.desktop_id
        );
        assert!(app.desktop_id.ends_with(".desktop"));
    }
    let policy = ExclusionPolicy::default();
    assert!(
        catalog.iter().any(|app| policy.allows(app)),
        "some app should be capturable"
    );
}

#[test]
fn the_host_session_is_captured_with_an_honest_state() {
    use reopen_core::checkpoint::ProviderError;
    use reopen_platform::{CaptureProvider, CaptureState};
    use reopen_platform_linux::GnomeAppsProvider;

    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        return; // No desktop session (CI): nothing to observe.
    }
    let provider = GnomeAppsProvider::from_env(ExclusionPolicy::default());
    let Ok(capture) = provider.capture() else {
        return; // A session bus without a user systemd manager.
    };
    let adapter = capture.adapter.clone().unwrap().id;
    eprintln!(
        "adapter={adapter} state={:?} apps={}",
        capture.state,
        capture.resources.len()
    );
    match adapter.as_str() {
        "linux.systemd-units" => {
            assert_eq!(
                capture.state,
                CaptureState::Partial(ProviderError::LimitedVisibility)
            );
        }
        "linux.gnome-shell" => assert!(matches!(
            capture.state,
            CaptureState::Complete | CaptureState::Partial(ProviderError::UnidentifiedWindows)
        )),
        other => panic!("unexpected adapter {other}"),
    }
}
