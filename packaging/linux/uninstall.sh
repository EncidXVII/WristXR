#!/usr/bin/env sh
# Undoes install.sh: takes DesktopVR off SteamVR's list, deletes its settings, layout and screen-sharing
# permission, then the installed program. Quit DesktopVR first (tray icon > Quit).
set -eu
dest="$HOME/.local/share/desktopvr"
here=$(cd "$(dirname "$0")" && pwd)
bin="$dest/desktopvr"
[ -x "$bin" ] || bin="$here/desktopvr"
# --quiet: running this script is the answer to "are you sure?", and a desktop without zenity or kdialog could
# not ask. A failure (DesktopVR still running) stops here, with the reason, and leaves the program in place.
"$bin" --uninstall --quiet
rm -rf "$dest"
echo "DesktopVR is uninstalled."
