# DesktopVR (working name)

Use your desktop in VR without taking the headset off. Free, light, Windows and Linux.

Status: early development. The interaction core (pointer, keyboard, grab, layout) and its automated gates are in place; the SteamVR backend and screen capture are next.

```
cargo test --workspace
cargo run -p dvr-app -- --selftest
```

Notifications: compatible with the XSOverlay notification API (UDP, port 42069).

License: GPL-3.0-or-later.
