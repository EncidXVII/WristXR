# Changelog

All notable changes to DesktopVR. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
versions: [Semantic Versioning](https://semver.org/).

## [0.1.0] - unreleased

First public early-access release (Windows).

### Added
- New look ("Night Lime"): side-rail menu with grouped lists, switches and Left / Right choices; Open now list
  to find and close any panel; Inbox page with unread dots and a count on the rail.
- Watch: date and battery rings at a glance, one line for the song (with a pause button that works without
  opening the watch), a new message or a low battery. Pointed at: seek bar, shuffle, share the song to VRChat,
  Windows volume and mute, Keyboard / Screens switches, Inbox. It grows only downward.
- Toolbar: the panel's name first, opacity slider, Room / Hand / Head as one choice, keyboard languages and
  Put away on the keyboard's toolbar.
- Motion: the watch grows down from the time and shrinks back the same way, its contents fading in as they are
  revealed; switches slide; buttons and rows answer on trigger-down; the menu fades in instead of flashing into
  view. All of it follows Windows' "Animation effects" setting (off = instant).
- Desktop panels for every monitor, flat or curved, exact mapping on mixed-DPI multi-monitor setups.
- Add any open window as its own panel from a window list.
- Toolbar under the pointed-at panel: lock, curve, smaller / bigger, opacity, anchor, close.
- Head-locked panels (HUD): the anchor button cycles world, hand and head; head-locked panels ride on the
  headset in the compositor, so they never lag.
- Laser smoothing: tremor and tracking noise are filtered, fast aiming passes straight through.
- Notification history (Menu > Notifications).
- Discord notifications on the watch, read from Windows notifications (no login); source Discord / all apps / off.
- Menu Close button; adding a screen or window closes the menu so the new panel is in view.
- Windows that close remove their panel; the keyboard's toolbar sits above it; a buzz confirms hide / show.
- Double-tap the left menu button (as in XSOverlay) to hide or show screens, windows and keyboard; hidden panels
  stop capturing. Hide all / Show all buttons on the watch and in the menu show the current state.
- Toolbar Reset and Menu > Reset layout (screens and windows in an arc in front of you).
- Compact menu: 10 buttons per page; laser smoothing can be switched off in Settings.
- Laser pointer: click, hold for right click, drag, thumbstick scroll; hovering never clicks.
- Grab-and-yank window pop-out into its own panel; panels can be locked or attached to a hand.
- Two-hand VR keyboard in the Windows layout with sticky modifiers and typing sounds.
- VRChat chatbox Chat mode over OSC and "share now playing".
- Minimal wrist watch (time, batteries) that shows only while you look at it; pointing at it opens Menu,
  Keyboard, Hide all, media controls and fps / load / own cost. Each part can be switched off.
- Keyboard language key cycling the installed Windows input languages; function row (Esc, F1–F12, Del), Win key
  (Win + key, or Win twice for Start) and arrow keys.
- Settings: pointing hand, start with SteamVR, vibration, curved screens by default, update check.
- XSOverlay notification API compatibility (UDP 42069) and XSOverlay settings import on first run.
- Four-step first run, menu (Panels, Keyboard, Wrist, Settings), tray icon, start with SteamVR.
- Per-hand input exclusivity: only the hand using the overlay is taken from the game.
- Default bindings for Index, Quest Touch, Vive, WMR and Steam Frame controllers.
- Release check against GitHub (no identifiers), `--selftest` report.

[0.1.0]: https://github.com/EncidXVII/DesktopVR/releases/tag/v0.1.0
