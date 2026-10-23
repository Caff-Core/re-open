# Official technical references

Reference check: 23 October 2026. Dependencies and browser behavior evolve; verify the relevant page when implementing. Project limits and architectural choices in this pack are requirements, not claims that libraries implement them automatically.

| Subject | Official reference | Use |
| --- | --- | --- |
| Tauri frontends | https://v2.tauri.app/start/frontend/ | React/Vite frontend support |
| Tauri external binaries | https://v2.tauri.app/develop/sidecar/ | Bundled Rust agent and bridge packaging |
| Tauri autostart | https://v2.tauri.app/plugin/autostart/ | Per-user login startup; verify lifetime behavior |
| ReUI setup | https://reui.io/docs/get-started | React/Tailwind requirements |
| ReUI registry | https://reui.io/docs/registry | shadcn registry integration |
| shadcn/ui | https://ui.shadcn.com/docs | Base components and configuration |
| sysinfo | https://docs.rs/sysinfo/latest/sysinfo/ | Process and memory sampling |
| Native Messaging | https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging | Framing, host registration and browser transport limits |
| Browser tabs | https://developer.chrome.com/docs/extensions/reference/api/tabs | Tab metadata and permission model |
| Edge Native Messaging | https://learn.microsoft.com/en-us/microsoft-edge/extensions-chromium/developer-guide/native-messaging | Edge host registration and extension identity |
| SQLite pragmas | https://www.sqlite.org/pragma.html | Foreign keys, synchronization and timeouts |
| SQLite WAL | https://www.sqlite.org/wal.html | WAL durability and backup constraints |
| Windows app restart | https://learn.microsoft.com/en-us/windows/win32/recovery/registering-for-application-restart | Native restart support is app-specific, not universal state recovery |

The project intentionally caps browser messages below the browser's transport ceiling. Tauri external-binary packaging does not by itself define independent agent startup, persistence or shutdown behavior; implement and test those explicitly.
