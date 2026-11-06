# DesktopVR (working name)

Use your desktop in VR without taking the headset off. Free, light, Windows and Linux.

Status: early development. On Windows it already shows your primary monitor in SteamVR with laser pointer, click/drag/scroll, a two-hand VR keyboard and grab/scale. Linux backends are next.

```
cargo test --workspace
cargo run -p dvr-app -- --selftest
```

Notifications: compatible with the XSOverlay notification API (UDP, port 42069).

License: GPL-3.0-or-later.
