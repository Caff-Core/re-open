# Official technical references

Reference check: 23 October 2026. Dependencies and browser behavior evolve; verify the relevant page when implementing. Project limits and architectural choices in this pack are requirements, not claims that libraries implement them automatically.

| Subject                         | Official reference                                                                                             | Use                                                                   |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| Tauri frontends                 | https://v2.tauri.app/start/frontend/                                                                           | React/Vite frontend support                                           |
| Tauri external binaries         | https://v2.tauri.app/develop/sidecar/                                                                          | Bundled Rust agent and bridge packaging                               |
| Tauri autostart                 | https://v2.tauri.app/plugin/autostart/                                                                         | Per-user login startup; verify lifetime behavior                      |
| ReUI setup                      | https://reui.io/docs/get-started                                                                               | React/Tailwind requirements                                           |
| ReUI registry                   | https://reui.io/docs/registry                                                                                  | shadcn registry integration                                           |
| shadcn/ui                       | https://ui.shadcn.com/docs                                                                                     | Base components and configuration                                     |
| sysinfo                         | https://docs.rs/sysinfo/latest/sysinfo/                                                                        | Process and memory sampling                                           |
| Native Messaging                | https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging                                 | Framing, host registration and browser transport limits               |
| Browser tabs                    | https://developer.chrome.com/docs/extensions/reference/api/tabs                                                | Tab metadata and permission model                                     |
| Firefox Native Messaging        | https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Native_messaging                        | Firefox host manifest location, `allowed_extensions`, start arguments |
| Firefox manifest keys           | https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/browser_specific_settings | Gecko extension ID                                                    |
| SQLite pragmas                  | https://www.sqlite.org/pragma.html                                                                             | Foreign keys, synchronization and timeouts                            |
| SQLite WAL                      | https://www.sqlite.org/wal.html                                                                                | WAL durability and backup constraints                                 |
| XDG Base Directory              | https://specifications.freedesktop.org/basedir-spec/latest/                                                    | Data, state and runtime directories                                   |
| XDG autostart                   | https://specifications.freedesktop.org/autostart-spec/latest/                                                  | Per-user login startup on Linux                                       |
| Desktop entries                 | https://specifications.freedesktop.org/desktop-entry-spec/latest/                                              | App identity, `Exec` field codes                                      |
| Secret Service API              | https://specifications.freedesktop.org/secret-service-spec/latest/                                             | Pairing credential storage on Linux                                   |
| Unix sockets                    | https://man7.org/linux/man-pages/man7/unix.7.html                                                              | `SO_PEERCRED`, `sun_path` length                                      |
| Pressure stall information      | https://docs.kernel.org/accounting/psi.html                                                                    | `/proc/pressure/memory` signal                                        |
| Edge Native Messaging (Phase 7) | https://learn.microsoft.com/en-us/microsoft-edge/extensions-chromium/developer-guide/native-messaging          | Edge host registration and extension identity                         |
| Windows app restart (Phase 7)   | https://learn.microsoft.com/en-us/windows/win32/recovery/registering-for-application-restart                   | Native restart support is app-specific, not universal state recovery  |

The project intentionally caps browser messages below the browser's transport ceiling. Tauri external-binary packaging does not by itself define independent agent startup, persistence or shutdown behavior; implement and test those explicitly.
