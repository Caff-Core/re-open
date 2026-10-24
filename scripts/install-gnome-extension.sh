#!/usr/bin/env bash
# Installs the Re-Open window-list extension for the current user (development).
# It only copies files into the user's extension directory; enabling is left to
# the user, and GNOME on Wayland loads a new extension after the next login.
set -euo pipefail

uuid="reopen-windows@caffcore.com"
repo="$(cd "$(dirname "$0")/.." && pwd)"
source_dir="$repo/apps/gnome-shell-extension/$uuid"
target_root="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions"
target="$target_root/$uuid"

mkdir -p "$target_root"
mkdir -p "$target"
# Only this extension's own directory is written.
cp "$source_dir/metadata.json" "$source_dir/extension.js" "$target/"

cat <<EOF
Installed $uuid to $target

Next steps:
  1. Log out and log back in (GNOME on Wayland loads new extensions at login).
  2. gnome-extensions enable $uuid
  3. Check it: gdbus call --session --dest com.caffcore.ReOpen.Windows \\
       --object-path /com/caffcore/ReOpen/Windows --method com.caffcore.ReOpen.Windows1.GetWindows

Without the extension, Re-Open still captures running apps but marks them as
limited visibility (partial).
EOF
