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
