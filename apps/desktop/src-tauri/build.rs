use tauri_build::{AppManifest, Attributes};

fn main() {
    // Declaring the app's commands makes each one deny-by-default: a window can
    // only call a command that a capability file grants (`allow-<command>`).
    let attributes = Attributes::new().app_manifest(AppManifest::new().commands(&["app_version"]));
    if let Err(error) = tauri_build::try_build(attributes) {
        panic!("tauri build configuration failed: {error:#}");
    }
}
