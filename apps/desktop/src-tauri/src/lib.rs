//! Re-Open desktop shell.
//!
//! The shell hosts the React interface and forwards narrow, typed commands.
//! It holds no business logic and no storage; see `docs/ARCHITECTURE.md`.

use tauri::{AppHandle, Url, WebviewUrl, WebviewWindowBuilder};

const MAIN_WINDOW: &str = "main";
/// Must match `build.devUrl` in `tauri.conf.json` and the Vite server port.
const DEV_SERVER_PORT: u16 = 1420;

/// Returns the application version from the package metadata.
#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Allows navigation only to the bundled app or, in development, the Vite server.
fn is_app_url(url: &Url, dev: bool) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" if url.host_str() == Some("tauri.localhost") => true,
        "http" if dev => url.host_str() == Some("localhost") && url.port() == Some(DEV_SERVER_PORT),
        _ => false,
    }
}

/// Builds and runs the desktop shell.
///
/// # Errors
///
/// Returns an error if the Tauri runtime or the main window cannot start.
pub fn run() -> Result<(), tauri::Error> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_version])
        .setup(|app| {
            let dev = tauri::is_dev();
            WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::default())
                .title("Re-Open")
                .inner_size(1100.0, 720.0)
                .min_inner_size(800.0, 560.0)
                .on_navigation(move |url| is_app_url(url, dev))
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(value: &str) -> Url {
        Url::parse(value).unwrap()
    }

    #[test]
    fn bundled_app_urls_are_allowed() {
        assert!(is_app_url(&url("tauri://localhost/index.html"), false));
        assert!(is_app_url(&url("http://tauri.localhost/"), false));
    }

    #[test]
    fn dev_server_is_allowed_only_in_development() {
        let dev_url = url("http://localhost:1420/");
        assert!(is_app_url(&dev_url, true));
        assert!(!is_app_url(&dev_url, false));
        assert!(!is_app_url(&url("http://localhost:8080/"), true));
    }

    #[test]
    fn external_and_local_file_urls_are_blocked() {
        for blocked in [
            "https://example.com/",
            "http://localhost.example.com:1420/",
            "file:///etc/passwd",
            "tauri://evil.example/",
            "javascript:alert(1)",
        ] {
            assert!(
                !is_app_url(&url(blocked), true),
                "{blocked} should be blocked"
            );
        }
    }
}
