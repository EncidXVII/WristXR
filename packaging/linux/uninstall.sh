#!/usr/bin/env sh
# Copyright (C) 2026 EncidXVII
# SPDX-License-Identifier: GPL-3.0-or-later

# Undoes install.sh: takes WristXR off SteamVR's list, deletes its settings, layout and screen-sharing
# permission, then the installed program. Quit WristXR first (tray icon > Quit).
set -eu
dest="$HOME/.local/share/wristxr"
here=$(cd "$(dirname "$0")" && pwd)
bin="$dest/wristxr"
[ -x "$bin" ] || bin="$here/wristxr"
# --quiet: running this script is the answer to "are you sure?", and a desktop without zenity or kdialog could
# not ask. A failure (WristXR still running) stops here, with the reason, and leaves the program in place.
"$bin" --uninstall --quiet
rm -rf "$dest"
echo "WristXR is uninstalled."
