#!/usr/bin/env sh
# Copyright (C) 2026 EncidXVII
# SPDX-License-Identifier: GPL-3.0-or-later

# Installs WristXR for the current user (Linux desktop and Steam Frame / SteamOS). No root needed.
# On its first run WristXR registers itself with SteamVR and turns on "start with SteamVR".
# Uninstall: ./uninstall.sh (or: ~/.local/share/wristxr/wristxr --uninstall, then delete that folder).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dest="$HOME/.local/share/wristxr"
mkdir -p "$dest"
cp "$here/wristxr" "$here/README.md" "$here/LICENSE" "$here/NOTICE" "$here/THIRD_PARTY_NOTICES.md" "$here/uninstall.sh" "$dest/"
chmod +x "$dest/wristxr" "$dest/uninstall.sh"
echo "Installed to $dest"
echo "Start SteamVR, then run: $dest/wristxr"
echo "On Wayland the first run asks, on the desktop, which screens to share: answer it once."
echo "Self-test for bug reports: $dest/wristxr --selftest"
