# Desktop interface

## Design system

React + TypeScript + Vite inside Tauri 2. Use Tailwind CSS 4 and a single shadcn/ui configuration; add ReUI components through its supported registry. Select one compatible primitive family during bootstrap, use shared theme tokens, and verify component APIs before installation. No Next.js or server rendering is required.

Use shadcn/ui for buttons, dialogs, menus, checkboxes, tooltips and basic form controls. Use ReUI selectively for checkpoint tables, resource filtering and richer layouts. Install only components that are used. Check licenses before selecting premium components.

## Screens

| Screen | Contents |
| --- | --- |
| Overview | Capture status, last saved time, recovery candidate, memory status, Save now |
| Checkpoints | Timestamp, session, completeness, source freshness, pins and preview |
| Recovery preview | App/resource tree, browser windows/tabs, selection and capability warnings |
| Restore progress | Current action, queue, memory pause, cancel and per-item outcomes |
| Settings | Startup, interval, exclusions, retention, paired browsers and privacy controls |

Named workspaces are later functionality. The schema begins with a Default workspace so this can be added without rewriting historical identity.

## Interaction rules

- Display capture time separately from last observation and browser source time.
- Distinguish paused, disconnected, partial and unsupported states in text, not colour alone.
- A recovery candidate remains visible until restored successfully or dismissed explicitly.
- Restore buttons state the selected item count. Show skipped schemes, missing paths and uncertain duplicates before launch.
- Preserve selection during live status updates; a changing checkpoint list must not silently change the selected plan.
- Deleting a protected checkpoint requires an explicit action identifying what protection will be removed.
- Settings apply through validated Rust commands. The frontend cannot bypass restore policy.

## Accessibility and performance

Keyboard navigation, visible focus, accessible names, reduced motion, clear empty states and readable dark/light themes. Virtualize large tab/resource lists where needed. Avoid animations that obscure state transitions. Do not continuously chart every process or keep the webview alive solely for memory sampling.

The tray offers Open, Save now, Pause/Resume capture and Quit.

**GNOME tray limitation:** stock GNOME Shell has no system tray. Tauri's Linux tray uses AppIndicator/StatusNotifierItem, which appears only when the user has installed an AppIndicator extension (for example Fedora's `gnome-shell-extension-appindicator`). Do not require it. Task 4.1 decides the fallback (background-app portal, persistent notification or window-only mode) and records an ADR; until then, every tray action must also be reachable from the desktop window, and capture status must never depend on the tray being visible. Closing the interface keeps the background agent active. “Quit background agent” is a separate explicit action with a clear consequence.
