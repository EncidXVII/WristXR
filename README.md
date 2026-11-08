# DesktopVR (working name)

**Use your desktop in VR without taking the headset off. Free, lighter than anything else, Windows and Linux.**

Point at a screen and click, type on a two-hand VR keyboard, glance at your wrist, get back to the game.

> Status: early. The Windows build works end to end with SteamVR; Linux capture/input and Steam Frame (on-device)
> are in progress. Not yet tested on every headset; please send a self-test report (below).

## Install (3 steps)

1. Have SteamVR installed.
2. Download `DesktopVR.exe` (Releases) and run it. No installer.
3. Put on your headset and follow the 4-step first run (hand, main screen, keyboard test, tips).

It starts with SteamVR from then on; untick **Start with SteamVR** in the tray icon to stop that.

## Switch in 2 minutes

Coming from XSOverlay? On first run DesktopVR reads your XSOverlay settings file (your own
`overlaySettings.json`, plain JSON) and keeps your pointing hand, typing sounds, curved screens and default
opacity. Nothing in XSOverlay is changed. Then:

| You did this in XSOverlay | Do this here |
|---|---|
| Open the menu | Double-tap the left menu button |
| Grab a window off the desktop | Grip a screen and yank it toward you |
| Type into VRChat | Press **Chat** on the keyboard (Chat mode) |
| Notifications from your apps | They just work: compatible with the XSOverlay notification API (UDP 42069) |

Run only one of the two overlays at a time (both want the notification port).

## Controls

- **Trigger**: click. Hold ~0.6 s: right click. Hold and move: drag. Hovering never clicks.
- **Thumbstick** on a screen: scroll. While holding a panel: push / pull.
- **Grip**: move a panel. Both hands: resize and turn. Locked panels ignore grip.
- **Keyboard**: both hands at once. Shift / Ctrl / Alt light up and apply to the next key. Uses your Windows layout.
- **Wrist** (other hand): time, game fps, batteries, CPU/RAM/GPU load, this overlay's cost, media controls, notifications.
- While a hand points at or holds a panel, the game does not receive that hand's input; the other hand keeps playing.

Rebind anything in SteamVR's own controller binding UI (default bindings for Index, Quest Touch, Vive, WMR and
Steam Frame controllers).

## Performance

Measured on the build machine (RTX 5070 Ti, Windows 11), SteamVR null driver, with a continuously changing
desktop (worst case: 60 fps capture):

| Budget | Target | Measured |
|---|---|---|
| GPU per VR frame (this overlay) | < 0.3 ms | 0.005–0.02 ms |
| CPU, one core | < 3 % | 0.6–2.8 % |
| RAM | < 150 MB | 64–78 MB |

The same numbers are shown live on the wrist and in Settings ("This overlay: … ms GPU · …% CPU").
Comparison against other overlays and a 60-minute soak are in progress.

## Self-test

`DesktopVR.exe --selftest` runs the automatic checks on your machine and writes `selftest-report.zip`
(results, GPU model, monitor resolutions; no names, paths or identifiers). Attach it to bug reports.

## Privacy

No telemetry. Network use: GitHub release check (no identifiers), VRChat OSC on localhost, the notification
API on localhost. Nothing else.

## Building

Rust stable. `cargo test --workspace`, `cargo run -p dvr-app -- --selftest`. Live checks against SteamVR's
null driver: `harness/null-driver.ps1 target/release/dvr-app.exe --selftest --vr` (backs up and restores your
SteamVR settings).

## License

GPL-3.0-or-later (`LICENSE`). Third-party licenses: `THIRD_PARTY_NOTICES.md`.
