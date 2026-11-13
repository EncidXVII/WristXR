<img src="packaging/brand/desktopvr-mark.svg" width="72" alt="DesktopVR logo">

# DesktopVR: free, open-source desktop overlay for SteamVR

[![CI](https://github.com/EncidXVII/DesktopVR/actions/workflows/ci.yml/badge.svg)](https://github.com/EncidXVII/DesktopVR/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/EncidXVII/DesktopVR?include_prereleases&label=download)](https://github.com/EncidXVII/DesktopVR/releases/latest)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue)](LICENSE)

**Use your Windows desktop in VR without taking the headset off.** DesktopVR is a lightweight SteamVR overlay:
point at your screens and click, type on a two-hand VR keyboard, send messages to the VRChat chatbox, glance at
your wrist for time, batteries and load, then get back to the game.

It costs about **0.01 ms of GPU time per frame** and shows its own cost live. Everything stays out of your
way until you need it: hold one button to hide it all. No account, no telemetry, no paid tier.

[**Download for Windows**](https://github.com/EncidXVII/DesktopVR/releases/latest) ·
[Report a bug](https://github.com/EncidXVII/DesktopVR/issues/new/choose)

![DesktopVR: VR keyboard and wrist watch overlay for SteamVR](docs/img/og.png)

> **Status: early access (0.1).** The Windows build works end to end with SteamVR. Linux and Steam Frame
> (on-device) are in progress. Not yet tested on every headset: please send a self-test report (below).

## Features

- **Your desktop in VR**: every monitor as its own panel, flat or curved, at any size and distance.
  Mixed-DPI and multi-monitor setups map exactly.
- **Any window as its own panel**: pick it from the window list, or grip a screen and yank the window under
  the pointer out of it.
- **Panel toolbar**: point at a screen or window and a toolbar appears under it, starting with the panel's name:
  lock, curve, free angle, size, an opacity slider, Room / Hand / Head (a head-locked HUD that follows your
  view), reset, close.
- **Hide everything with one button**: double-tap the left menu button, like XSOverlay, to fade screens, windows
  and keyboard out; again, and they come back as arranged in front of where you face now. Hidden
  panels stop capturing, so they cost nothing.
- **Laser pointer that doesn't misclick**: trigger to click, A / X (or hold the trigger) for right click, drag,
  thumbstick to scroll.
  Hovering never clicks, and hand tremor is smoothed out without making fast aiming lag.
- **Two-hand VR keyboard** in your Windows layout, with a language key (EN, UK, DE, ... cycles your installed
  input languages), sticky Shift / Ctrl / Alt and typing sounds.
- **VRChat chatbox keyboard**: Chat mode sends what you type straight to the VRChat chatbox over OSC, plus
  "share now playing".
- **Wrist watch that stays out of the way**: the time, date and battery rings, shown only while you look at your
  wrist, plus one line for what is playing (with a pause button), a new message or a low battery. Point at the
  time to open it: seek bar, play / skip / shuffle, share the song to VRChat, Windows volume and mute, switches
  for Show overlay and Keyboard, Add (a window or the next screen, right there on the watch), the Inbox, the menu,
  and fps / load / this overlay's own cost.
- **Seven watch layouts** for the glance: Standard, Clock, No batteries, Batteries, Music (a small player with
  back / play / next), Performance (fps, CPU and GPU) and Compact (one slim bar). Messages and low batteries come
  through in every one, and opening the watch looks the same in all of them.
- **Themes**: eight accent colours (Lime, Mint, Aqua, Sky, Violet, Pink, Amber, White) in Setup > Theme.
- **Discord notifications on your wrist**: Discord messages (or, if you choose, any app's Windows notifications)
  pop up with a buzz as a banner low in your view (who, the app, and up to three lines of the message; point at
  it to keep it) and on the watch, no Discord login or bot needed; the last few wait in the menu's Inbox. Apps that send XSOverlay notifications
  (UDP 42069) work too.
- **Game input stays yours**: a hand on a panel takes from the game only the controls that do something there
  (the stick only over a screen or window), and only once you let them go; the other hand keeps playing.
- **Rebindable** in SteamVR's own binding UI. Default bindings for Valve Index, Meta Quest (Touch),
  HTC Vive, Windows Mixed Reality and Steam Frame controllers.

| Watch at a glance | Watch, pointed at |
|---|---|
| ![Watch: 21:37, battery rings for both controllers, and the song playing with a pause button](docs/img/wrist.png) ![Watch with a new Discord message from Alex](docs/img/wrist-message.png) ![Watch warning that the right controller is at 12 percent](docs/img/wrist-low.png) | ![Open watch: seek bar, play, skip, shuffle, share, volume slider, Show overlay and Keyboard switches, Add Window and Screen 2, Inbox, Menu, fps and load](docs/img/wrist-expanded.png) |

| Music layout | Performance layout | Compact layout |
|---|---|---|
| ![Music layout: time, battery rings, and a two-line player with back, pause and next](docs/img/wrist-layout-music.png) | ![Performance layout: time, battery rings, 90 fps, CPU and GPU bars](docs/img/wrist-layout-performance.png) | ![Compact layout: one bar with the time, battery numbers and the song](docs/img/wrist-layout-compact.png) |

![Toolbar under a window: its name, lock, curve, size, opacity slider, Room / Hand / Head, reset, close](docs/img/toolbar.png)

![VR keyboard with Shift lit, the language key showing EN and the laser hovering H](docs/img/keyboard.png)

## Install (3 steps)

1. Have [SteamVR](https://store.steampowered.com/app/250820/SteamVR/) installed.
2. [Download the latest release](https://github.com/EncidXVII/DesktopVR/releases/latest), unzip it and run
   `DesktopVR.exe`. There's no installer and nothing else to set up.
3. Put on your headset and follow the 4-step first run (hand, main screen, keyboard test, tips). It takes
   under a minute.

It starts with SteamVR from then on; untick **Start with SteamVR** in the tray icon to stop that.

## Switching from XSOverlay, OVR Toolkit or Desktop+

On first run DesktopVR reads your XSOverlay settings file (your own `overlaySettings.json`) and keeps your
pointing hand, typing sounds, curved screens and default opacity. Nothing in XSOverlay is changed.

| You did this before | Do this here |
|---|---|
| Open the overlay menu | Point at your watch and press Menu (or the tray icon) |
| Toggle all overlays | Double-tap the left menu button, same as XSOverlay |
| Add a window | Menu > Add window, or grip a screen and yank the window toward you |
| Window toolbar (opacity, size, close...) | Point at the window: the toolbar appears under it |
| Type into VRChat | Press **Chat** on the keyboard |
| Notifications from your apps | Apps using the XSOverlay notification API work unchanged |

Run one overlay at a time: two overlays fight over the same controller input and notification port.

## Controls

- **Trigger**: click. Hold and move: drag. Hold still ~0.6 s (a tick) and let go: right click; move instead and
  it is still a drag (Setup > Controls can turn the right click off).
- **A or X** (the lower half of the trackpad on Vive and WMR wands): right click where you point; hold and move
  to right-drag.
- **Thumbstick** on a screen: scroll. Or turn on **Drag to scroll** (Setup > Controls) and pull the page with the
  trigger, like a touchscreen (handy on wands without a stick; drag-to-select text is off while it is on).
- **Grip**: grab a panel. The spot you grabbed stays under the laser and the panel stays level, turned to face
  you, so a twist of the wrist never tips it (unless the toolbar's free angle is on: then it turns with your hand
  and stays at the angle you leave it). While holding: stick up / down pushes and pulls, stick left / right
  resizes. Both grips: resize by moving your hands apart. Locked panels ignore grip.
- **Put a panel on a hand**: hold it, bring it to your other hand and let go: it sits above that hand like a
  tablet and moves with it. Grab it away and let go in the room to put it back. The toolbar's Hand does the same
  for the hand you are not pointing with.
- **Move the watch**: grip it with your other hand and slide it along your wrist, turning that hand to tilt it; it
  stays on that wrist and keeps its spot and tilt. Menu > Reset layout puts it back.
- **Toolbar size**: - and + step through round sizes (1.0, 1.2, 1.5, 2.0 m...) and ease there.
- **Setup > Controls** lists all of this in VR; Rebind buttons there opens SteamVR's binding screen for
  DesktopVR, where every action can go on another button.
- **Point at a panel**: its toolbar appears underneath (name, lock, curve, free angle, size, opacity, Room / Hand /
  Head, reset, close). The keyboard's toolbar has your input languages and Put away.
- **Left menu button**: double-tap to hide screens, windows and keyboard; again to bring them back, as arranged,
  in front of where you face now (handy after a playspace mover).
- **Toolbar Reset** puts a panel back in front of you; **Menu > Reset layout** arranges all screens and windows
  in an arc in front of you.
- **Watch**: look at your wrist to see it; its pause button works as is. Point at the time with the other hand
  to open the player, volume and quick switches. It grows downward, so nothing moves under your laser. Add >
  Window lists your windows on the watch; pick one and it opens in front of you. Menu > Watch > Layout picks
  what the glance shows.
- **Keyboard**: use both hands at once. Shift / Ctrl / Alt light up and apply to the next key. The language
  key switches input language.
- **Menu** (side rail): Panels (add a window or screen, recenter, reset layout, Overlay / Keyboard switches, and
  everything open now with a close button), Keys (language, chat, key sounds, share songs), Watch (layout and
  what it shows), Inbox (recent messages; Discord / All apps / Off), Setup (laser hand, watch hand, vibration,
  steady laser, start with SteamVR, updates, curved screens, theme, quit).

| Panels | Setup |
|---|---|
| ![Menu, Panels page: add a window or screen, recenter, reset layout, Overlay and Keyboard switches, open panels with close buttons](docs/img/menu-panels.png) | ![Menu, Setup page: laser hand and watch hand as Left / Right choices, vibration, steady laser, start with SteamVR, updates, curved, Quit](docs/img/menu-settings.png) |
| ![Menu, Inbox page: Discord / All apps / Off, recent messages with unread dots](docs/img/menu-inbox.png) | ![Menu, Add window page: open windows with app tiles, Shown or +](docs/img/menu-windows.png) |

## Performance

Measured on the build machine (RTX 5070 Ti, Windows 11, SteamVR) with a constantly changing desktop, which is
the worst case (60 fps capture):

| Budget | Target | Measured |
|---|---|---|
| GPU per VR frame (this overlay) | < 0.3 ms | 0.005–0.02 ms |
| CPU, one core | < 3 % | 0.6–2.8 % |
| RAM | < 150 MB | 64–78 MB |

The same numbers are shown live in Setup and on the open watch, so you can check them on your own
machine.

## FAQ

**Is it free?** Yes. Free and open source under GPL-3.0, with no paid tier, account or ads.

**Which headsets work?** Anything that runs SteamVR: Valve Index, Meta Quest 2 / 3 / Pro (Link, Air Link,
Virtual Desktop or Steam Link), HTC Vive, Pico, Bigscreen Beyond, Windows Mixed Reality, Steam Frame.

**Does it work with VRChat?** Yes. It runs alongside VRChat, types into the chatbox via OSC (enable OSC in
VRChat's Action Menu), and takes from the game only the controls you're using on a panel.

**Does it work on Linux?** Not yet. Linux (Wayland screen capture, input) and Steam Frame on-device builds are
in progress; the Linux release builds and runs the self-test today.

**Does it slow down my game?** It's designed not to: about 0.01 ms GPU per frame and a few percent of one CPU
core. Hidden panels stop capturing entirely. Setup shows the real cost on your machine.

**Is Windows SmartScreen warning me?** Early builds aren't code-signed yet (see
[code signing policy](docs/code-signing-policy.md)). Every release ships a SHA-256 checksum.

## Self-test and bug reports

`DesktopVR.exe --selftest` runs the automatic checks on your machine and writes `selftest-report.zip` (results,
GPU model, monitor resolutions; no names, paths or identifiers). Attach it to a
[bug report](https://github.com/EncidXVII/DesktopVR/issues/new/choose).

## Privacy

No telemetry. Network use: a GitHub release check (no identifiers), VRChat OSC on localhost, the notification
API on localhost. Nothing else. Windows notifications (Discord by default) are read on your PC only to show them
on the watch; they are never stored on disk or sent anywhere. Switch it off in Menu > Inbox (Off).

## Building from source

Rust stable (1.85+). `cargo test --workspace`, `cargo run -p dvr-app -- --selftest`. Live checks against
SteamVR's null driver: `harness/null-driver.ps1 target/release/dvr-app.exe --selftest --vr` (backs up and
restores your SteamVR settings). See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[GPL-3.0-or-later](LICENSE). Third-party licenses: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
DesktopVR is not affiliated with Valve, VRChat, XSOverlay, OVR Toolkit or Desktop+; names are used only to
describe compatibility.
