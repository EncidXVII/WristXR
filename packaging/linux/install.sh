#!/usr/bin/env sh
# Installs DesktopVR for the current user (Linux desktop and Steam Frame / SteamOS). No root needed.
# On first run the app registers itself with SteamVR and turns on "start with SteamVR".
# Uninstall (one line):  rm -rf ~/.local/share/desktopvr ~/.config/DesktopVR
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dest="$HOME/.local/share/desktopvr"
mkdir -p "$dest"
cp "$here/desktopvr" "$here/README.md" "$here/LICENSE" "$here/THIRD_PARTY_NOTICES.md" "$dest/"
chmod +x "$dest/desktopvr"
echo "Installed to $dest"
echo "Start SteamVR, then run: $dest/desktopvr"
echo "Self-test for bug reports: $dest/desktopvr --selftest"
