# Contributing to DesktopVR

Thanks for helping. The most useful contributions right now, in order:

1. **Headset test reports.** Run `DesktopVR.exe --selftest`, use it in VR for a few minutes and open a
   [bug report](https://github.com/EncidXVII/DesktopVR/issues/new/choose) with `selftest-report.zip` attached,
   even if everything worked. Say which headset, controllers and streaming method (Link, Air Link, Virtual
   Desktop, Steam Link) you used.
2. **Linux testers** (KDE / GNOME on Wayland) and **Steam Frame** testers: use the "Steam Frame test report"
   template.
3. **Translations**: only English ships in 0.1, but every string is in `crates/ui/locales/en-US.ftl`. Open an
   issue to claim a language; loading other locales is the next step. Keep strings short: they're read at arm's
   length in a headset.
4. **Code**: bugs labelled `good first issue` or `help wanted`.

## Building

Rust stable 1.85+. On Linux also `libfontconfig1-dev pkg-config`.

```
cargo test --workspace
cargo run -p dvr-app -- --selftest
cargo run -p dvr-app --release              # run it (Windows, SteamVR)
```

Live checks against SteamVR's null driver, no headset needed (backs up and restores your SteamVR settings):

```
harness/null-driver.ps1 target/release/dvr-app.exe --selftest --vr
```

Regenerate the README screenshots from the real UI:

```
cargo test -p dvr-app --release screenshots -- --ignored
```

## Before you open a pull request

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- Performance is a feature: changes in the per-frame path must keep the overlay under 0.3 ms GPU per frame and
  3 % of one CPU core (`--selftest` checks this).
- `unsafe` lives only in the FFI / OS crates (`vr`, `gfx-d3d11`, `capture-win`, `input-win`, `sys`), each block
  with a `// SAFETY:` comment.
- Every user-visible string goes through the Fluent file (`crates/ui/locales/en-US.ftl`).
- One topic per pull request; describe what you tested (headset or null driver).

By contributing you agree your contribution is licensed under GPL-3.0-or-later.
