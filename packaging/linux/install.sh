#!/usr/bin/env sh
# Installs DesktopVR for the current user (Linux desktop and Steam Frame / SteamOS). No root needed.
# The Linux build is early: screen capture and input aren't done yet, so for now it runs the self-test only
# (it doesn't register with SteamVR or write any settings).
# Uninstall (one line):  rm -rf ~/.local/share/desktopvr ~/.config/desktopvr
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dest="$HOME/.local/share/desktopvr"
mkdir -p "$dest"
cp "$here/desktopvr" "$here/README.md" "$here/LICENSE" "$here/THIRD_PARTY_NOTICES.md" "$dest/"
chmod +x "$dest/desktopvr"
echo "Installed to $dest"
echo "The Linux build runs the self-test only for now (the VR overlay is Windows-only so far)."
echo "Self-test for bug reports: $dest/desktopvr --selftest"
