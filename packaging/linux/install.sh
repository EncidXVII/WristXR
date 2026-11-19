#!/usr/bin/env sh
# Installs DesktopVR for the current user (Linux desktop and Steam Frame / SteamOS). No root needed.
# On its first run DesktopVR registers itself with SteamVR and turns on "start with SteamVR".
# Uninstall: ./uninstall.sh (or: ~/.local/share/desktopvr/desktopvr --uninstall, then delete that folder).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dest="$HOME/.local/share/desktopvr"
mkdir -p "$dest"
cp "$here/desktopvr" "$here/README.md" "$here/LICENSE" "$here/THIRD_PARTY_NOTICES.md" "$here/uninstall.sh" "$dest/"
chmod +x "$dest/desktopvr" "$dest/uninstall.sh"
echo "Installed to $dest"
echo "Start SteamVR, then run: $dest/desktopvr"
echo "On Wayland the first run asks, on the desktop, which screens to share: answer it once."
echo "Self-test for bug reports: $dest/desktopvr --selftest"
