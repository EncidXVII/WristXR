#!/usr/bin/env sh
# Undoes install.sh: takes DesktopVR off SteamVR's list, deletes its settings, layout and screen-sharing
# permission, then the installed program. Quit DesktopVR first (tray icon > Quit).
set -eu
dest="$HOME/.local/share/desktopvr"
here=$(cd "$(dirname "$0")" && pwd)
bin="$dest/desktopvr"
[ -x "$bin" ] || bin="$here/desktopvr"
"$bin" --uninstall
rm -rf "$dest"
echo "DesktopVR is uninstalled."
