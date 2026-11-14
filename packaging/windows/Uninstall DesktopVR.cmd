@echo off
rem Takes DesktopVR off SteamVR's list and deletes its settings (asks first). Then delete this folder.
start "" "%~dp0DesktopVR.exe" --uninstall
