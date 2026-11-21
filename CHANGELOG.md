# Changelog

All notable changes to WristXR. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
versions: [Semantic Versioning](https://semver.org/).

## [0.1.0] - unreleased

First public early-access release (Windows).

### Changed
- Screens and windows look sharper in the headset, far ones most: each is handed to SteamVR at about 2.5x the
  size it is shown, so text no longer shimmers or smears when the compositor shrinks it and stays readable at mid
  and far distances. Screens shrunk 2.5x or more are resampled with a Kaiser-windowed filter (as SimulaVR filters
  its windows) from the nearest of a set of smaller copies, which keeps letter edges smooth with no sharpening
  halos; close ones get a light sharpen. Detail changes in steps of about a tenth as a screen moves,
  and each screen keeps one texture for good, with two picture slots: a new detail level is drawn into the
  slot SteamVR is not showing, then shown, so moving or resizing a screen never blinks or flashes a wrong
  picture. Under 0.35 ms of GPU per captured frame.
- The open watch stays open 2.5 seconds after the laser leaves it (was 1.5).
- Pointing at a panel takes from the game only the controls that do something there. The stick is taken only over
  a screen or window (it scrolls) or while holding a panel; A/X only over a screen or window; the menu button
  never. So over the keyboard, the toolbar or the watch, you still turn and walk in the game. A control changes
  hands only once you let it go: a stick you are turning with, or a grip holding something in the game, stays the
  game's until released, so the game no longer keeps turning you when the laser lands on a panel. Custom SteamVR
  bindings for WristXR need setting again (the controls are in new groups).
- The watch sits on the back of the wrist, facing out of it, where a real watch is (it was on the controller's top,
  facing your chest when you turned the wrist). Grip it with the other hand to slide it along the wrist and, by
  turning that hand, tilt it; it keeps the spot and the tilt after a restart. A spot kept by an older build is
  dropped once.
- The watch shows from a wider look: within about 52 degrees of where you look (was 45), and stays to 70 (was
  63). The wrist's turn is unchanged.
- Holding the trigger no longer gets in the way of dragging: held still, a tick says a right click is ready and
  letting go gives it; moving instead is an ordinary drag, as before the hold.
- Pointing at a panel takes its toolbar even while your other laser rests on another one: the toolbar goes to the
  panel a laser came onto last (a laser resting on the keyboard used to keep a screen's toolbar from showing, and
  the other way round).
- New logo: a screen on a stand, leaning forward, in the same family as SweatXR's and PortXR's. It is the app
  icon (Explorer, taskbar), the tray icon (lime on a dark taskbar, dark on a light one, switching with Windows)
  and the picture SteamVR shows next to WristXR. Every icon is drawn from one script,
  packaging/brand/marks.py.
- A held panel keeps the spot you grabbed under the laser and turns to face you, level, easing there instead of
  copying every twist of the wrist; two hands resize it without rolling it.
- The toolbar's - and + step through round sizes (0.5, 0.6, 0.8, 1.0, 1.2, 1.5, 2 m...) and ease to them,
  instead of x1.25 jumps; the readout shows the size it is going to.
- The toolbar's Hand puts the panel in your free hand, above it and facing you, instead of leaving it floating
  where it was and following that hand from afar.
- The toolbar rides with a head-locked panel and moves in the same frame as its panel, so it no longer trails it.
- With nothing playing, the watch keeps the time where it is and gives the date its own line under it.
- The watch needs a quarter less turn of the wrist: its roll around the forearm must be within about 50 degrees
  of your eyes to show and 61 to stay. Where your arm is counts as before (face within 66 degrees of your eyes,
  stays to 81).
- Grip the watch with your other hand to move it along your wrist: it stays on that wrist, and the spot is kept
  after a restart. Menu > Reset layout (or changing the watch hand) puts it back.

### Added
- Keyboard: Print Screen, and an Fn key (as on a laptop): Fn + arrows are Home / End / Page Up / Page Down,
  Fn + Del is Insert, and the keys show it while Fn is lit. Combos of modifiers alone: tap a lit modifier again
  to send the lit ones (Alt, Shift, Shift = Alt+Shift), as Win, Win already opened Start.
- The song's cover on the watch (Music and Player layouts, and the open watch), read from Windows' media session,
  in place of the note. Linux keeps the note for now.
- Linux: WristXR runs on Linux with SteamVR for Linux, on X11 and on Wayland. On Wayland, screens, windows,
  keyboard and pointer go through the desktop portal (asked once, remembered after); frames come from PipeWire.
  Panels reach SteamVR through Vulkan. The tray, Discord and other app notifications on the watch, media controls
  (MPRIS), the update check and `--uninstall` work there too. New, and not yet tested on hardware.
- Uninstall: `Uninstall WristXR.cmd` in the folder (or `WristXR.exe --uninstall`) takes WristXR off
  SteamVR's list, so it no longer starts with SteamVR, and deletes its settings and layout, after asking. It
  works with SteamVR running or not, and never starts it.
- Two watch layouts: Game (time, date, FPS, CPU and GPU) and Player (time, date and the small player), for
  Performance and Music without the batteries.
- Notification banner: a message also shows low in your view (riding with your head), with the app, the sender and
  up to three lines of text. It rises and fades in and goes back down the same way after about 6 seconds; lasers
  pass through it to whatever is behind. Without Windows' animations it simply appears and goes.
- The watch's playing mark moves while a song plays and you look at the watch: four bars rising and falling on slow
  waves of their own. Paused, they rest.
- After a SteamVR restart, the layout comes to where you are when you put the headset on, as you arranged it
  around you. Screens no longer appear across the room when a playspace mover had moved you.
- Free angle (toolbar): a held screen or window turns with your hand and keeps the angle you leave it at, instead
  of turning to face you, level.
- Setup > Controls > Rebind buttons opens SteamVR's binding screen for WristXR.
- Screens and windows fade out as you lean into them (from 30 cm, gone at 8 cm) instead of being cut off by the
  headset's near clip.
- Right click on A / X (the lower half of the trackpad on Vive and WMR wands), held for right-drag. Holding the
  trigger still right-clicks; Setup > Controls can turn that off.
- Setup > Controls: every control in one page, pointing and holding.
- Drag to scroll (Setup > Controls, off by default): trigger-drag on a screen or window scrolls it like a
  touchscreen, as in OVR Toolkit.
- Grip + stick left / right resizes the held panel smoothly.
- Bring a held panel to your other hand and let go: it sits above that hand and moves with it (the hand feels it
  arrive). Let it go in the room to take it off.
- Watch layouts (Menu > Watch > Layout): Standard, Clock (time and date), No batteries (time, date, song),
  Batteries (every battery as a number, with L / R / T under it), Music (a two-line player with back, play and
  next on the closed watch), Performance (fps, red under 90 % of the headset's rate, and CPU / GPU bars, red
  above 90 %) and Compact (one slim bar). Messages and low batteries show in every layout; the top row never
  moves when the watch opens, and the open watch is the same in all of them.
- Add from the watch: the open watch has an Add row with Window and the next screen. Window lists your windows
  on the watch (front to back, shown ones marked); picking one opens it in front of you, without the menu.
- Themes (Setup > Theme): Lime, Mint, Aqua, Sky, Violet, Pink, Amber or White as the accent, on the menu, watch,
  keyboard, toolbar and laser dot. Warning red stays red; dark text on every accent is at least 7:1.
- New look ("Night Lime"): side-rail menu with grouped lists, switches and Left / Right choices; Open now list
  to find and close any panel; Inbox page with unread dots and a count on the rail.
- Watch: date and battery rings at a glance, one line for the song (with a pause button that works without
  opening the watch), a new message or a low battery. Pointed at: seek bar, shuffle, share the song to VRChat,
  the player's volume and mute, Keyboard / Screens switches, Inbox. It grows only downward.
- Toolbar: the panel's name first, opacity slider, Room / Hand / Head as one choice, keyboard languages and
  Put away on the keyboard's toolbar.
- Hide all and show again (double-tap left menu, or Show overlay) brings screens, windows and keyboard back as
  arranged, in front of where you face now: after a playspace mover or a walk, one double-tap puts everything back.
- The watch's first switch is Show overlay (screens, windows and keyboard together); the menu calls it Overlay.
- The watch shows at a wider glance and no longer flickers at the edge (it holds once shown and needs a clear look
  away to go); hiding fades out in a tenth of a second.
- The toolbar stays while the laser crosses the gap between a panel and it (or a panel seen through that gap).
- Rounded menu and keyboard, rounded keys.
- Battery rings: every connected controller gets one, an empty ring with a dim L / R when it reports no level;
  controllers are found by type when SteamVR assigns no hands; a charging controller is never shown as low.
- Neutral graphite greys in place of the warm ones; Anton for headings and the time, Barlow for all other text,
  both bundled, so the look no longer depends on Windows' fonts.
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
- Notifications on the watch from any app (Telegram, Discord, ...), read from Windows notifications (no login);
  Inbox > Windows notifications from: All apps (the default) / Discord only / Off. VRCX always comes through.
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

[0.1.0]: https://github.com/EncidXVII/WristXR/releases/tag/v0.1.0

### Fixed
- The watch's volume slider and mute act on the playing app's own volume (its Volume mixer entry) wherever it
  plays. They set Windows' default device, which did nothing for a player sent to another device (Spotify to an
  audio interface while the headset is the default) or a device that takes volume itself (Virtual Desktop's).
- Clicking a window of a program that runs as administrator now explains, once, why nothing happens (Windows drops
  clicks from other apps there) instead of failing silently.
- Opening a window or a screen while everything is hidden shows everything again: a double-tap of the menu button
  in a game could hide it all unnoticed, and the new window was added hidden.
- The toolbar's size readout follows a panel sized in hand (grip and stick, or both hands), not just its buttons.
- The very first launch gets its controller buttons: SteamVR now learns which app this is before the controls are
  loaded (they used to work only after a restart). If a tracked controller's buttons still never arrive, the
  controls are loaded again once.
