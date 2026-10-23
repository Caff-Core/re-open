# Product requirements

## Problem

Memory-heavy Android builds can freeze the user's PC, requiring a forced shutdown. Reopening applications, projects and browser tabs manually interrupts work. The product reduces reconstruction effort; it does not solve memory exhaustion or recover running builds.

## First-release outcomes

- Capture visible, supported desktop applications and browser workspaces periodically.
- Keep bounded checkpoint history with a protected previous-session candidate.
- Preview a checkpoint and select individual apps, resources or tabs to reopen.
- Restore gradually, detect duplicates where evidence supports it, and report uncertain outcomes.
- Run unobtrusively at login and continue capture when the interface is closed.

## Scope

| First release | Later |
| --- | --- |
| Linux adapter: Fedora 44, GNOME 50 on Wayland | X11, KDE Plasma, Windows 11 and macOS adapters |
| Brave extension, then Firefox; separate profiles | Chrome, Chromium, Edge and Flatpak browsers |
| App identity and executable validation | Broader packaged-app support |
| Browser addresses, tab order, pinned state | Tab groups and richer layouts |
| Nautilus folder and VS Code/Zed project adapters where proven | Android Studio (after spike) and terminal adapters |
| Snapshot preview, selection and result report | Named workspaces and global shortcuts |
| Staged launches and conservative memory warning | Window geometry and multi-monitor restoration |
| Local-only history and privacy exclusions | Optional encrypted exports |

VS Code, Zed and Nautilus are the user's confirmed tools and the proposed initial resource adapters; each still needs proof that its open state is externally observable on GNOME Wayland. Android Studio is high-value but requires a capability spike; do not assume its open-project state is externally discoverable.

## User flows

### Setup

Explain recovery limits. Enable login startup explicitly. Show capture interval, exclusions and retention. Install the browser extension and display connection/pairing state per profile. No administrator permission for normal operation.

### Normal operation

Tray shows active, paused or degraded capture. Default checkpoint interval: 60 seconds. “Save now” requests immediate capture. Display the last successful checkpoint and separate warnings for unavailable providers.

### After restart

Pin the previous session's latest useful checkpoint before recording the new session. Show a recovery notice with timestamp and browser freshness. Let the user inspect, restore selected resources, postpone, or dismiss. Continue recording new-session history separately.

### Restore

Preview proposed actions and unsupported items. User selects resources and confirms once. Launch sequentially with memory checks, progress and cancellation. Never rerun terminal commands or builds. Show restored, already open, failed, skipped and uncertain outcomes.

## Defaults and limits

Defaults are configurable policy, not performance guarantees. Start with 60-second checkpoints, 5-second memory sampling, a 2-second minimum launch gap, seven-day history and 500 automatic checkpoints per workspace. Use the tighter applicable history limit; pinned checkpoints are exempt. Warn when storage reaches a configurable 100 MiB budget and request pruning if pins prevent compliance.

At most 2,000 tabs per browser source are supported initially. Exceeding this limit marks capture partial; never silently truncate. Memory-pressure thresholds use available physical memory with hysteresis; on Linux, memory pressure stall information (`/proc/pressure/memory`) is an additional signal. Calibrate on the 15 GiB reference machine under the Android build workload. Do not automatically kill apps, change swap or zram settings or initiate shutdown.

## Acceptance criteria

1. Killing the agent during persistence leaves either the prior committed checkpoint or the new committed checkpoint, never a half-written one.
2. A new empty session cannot remove or replace the protected previous-session candidate.
3. A disconnected browser is shown as stale/unavailable; zero tabs requires a successful complete capture.
4. Selecting five of ten restorable items launches only those five, subject to explicit skips and duplicate checks.
5. Closing the desktop window does not stop capture. Explicit “Quit background agent” explains that capture will stop.
6. Unsupported apps remain visible with honest capability labels.
7. The engine stays bounded in memory and storage under the documented test workload.
8. No browsing addresses or document paths appear in default diagnostic logs.
