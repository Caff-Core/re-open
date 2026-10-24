#!/usr/bin/env bash
# Loads the Re-Open window-list extension into a private headless GNOME Shell
# (own session bus, own config and data dirs), opens test apps and prints the
# extension's GetWindows reply. Never touches the user's running session.
#
# Usage: scripts/test-gnome-extension.sh [app ...]   (default: gnome-text-editor gnome-calculator)
set -euo pipefail

if ! command -v gnome-shell >/dev/null; then
  echo "gnome-shell is not installed; this check needs a GNOME host." >&2
  exit 2
fi

repo="$(cd "$(dirname "$0")/.." && pwd)"
uuid="reopen-windows@caffcore.com"
apps=("$@")
[ ${#apps[@]} -eq 0 ] && apps=(gnome-text-editor gnome-calculator)

work="$(mktemp -d "${TMPDIR:-/tmp}/reopen-gnome-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT
export XDG_CONFIG_HOME="$work/config" XDG_DATA_HOME="$work/data" XDG_CACHE_HOME="$work/cache"
mkdir -p "$XDG_DATA_HOME/gnome-shell/extensions" "$XDG_CACHE_HOME"
cp -r "$repo/apps/gnome-shell-extension/$uuid" "$XDG_DATA_HOME/gnome-shell/extensions/"
display="reopen-test-$$"

dbus-run-session -- bash -s "$display" "$uuid" "${apps[@]}" <<'SESSION'
set -euo pipefail
display="$1"; uuid="$2"; shift 2
gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "['$uuid']"
gnome-shell --headless --virtual-monitor 1280x720 --wayland-display "$display" >"$XDG_CACHE_HOME/shell.log" 2>&1 &
shell=$!
trap 'kill $shell 2>/dev/null || true; wait 2>/dev/null || true' EXIT

call() {
  gdbus call --session --dest com.caffcore.ReOpen.Windows \
    --object-path /com/caffcore/ReOpen/Windows --method com.caffcore.ReOpen.Windows1.GetWindows
}
for _ in $(seq 1 60); do call >/dev/null 2>&1 && break; sleep 0.5; done
if ! call >/dev/null 2>&1; then
  echo "extension did not appear on the bus; shell log:" >&2
  tail -40 "$XDG_CACHE_HOME/shell.log" >&2
  exit 1
fi

echo "before apps: $(call)"
for app in "$@"; do
  WAYLAND_DISPLAY="$display" GDK_BACKEND=wayland "$app" >/dev/null 2>&1 &
done
sleep 6
echo "with apps:   $(call)"
gdbus call --session --dest com.caffcore.ReOpen.Windows --object-path /com/caffcore/ReOpen/Windows \
  --method org.freedesktop.DBus.Properties.Get com.caffcore.ReOpen.Windows1 Version
SESSION
