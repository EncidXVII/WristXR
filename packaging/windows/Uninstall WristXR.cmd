@echo off
rem Takes WristXR off SteamVR's list and deletes its settings (asks first). Then delete this folder.
start "" "%~dp0WristXR.exe" --uninstall
