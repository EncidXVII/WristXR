# Contributing to WristXR

Thanks for helping. The most useful contributions right now, in order:

1. **Headset test reports.** Run `WristXR.exe --selftest`, use it in VR for a few minutes and open a
   [bug report](https://github.com/EncidXVII/WristXR/issues/new/choose) with `selftest-report.zip` attached,
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
cargo run -p wxr-app -- --selftest
cargo run -p wxr-app --release              # run it (Windows, SteamVR)
```

Live checks against SteamVR's null driver, no headset needed (backs up and restores your SteamVR settings):

```
harness/null-driver.ps1 target/release/wxr-app.exe --selftest --vr
```

Regenerate the README screenshots from the real UI:

```
cargo test -p wxr-app --release screenshots -- --ignored
```

## Before you open a pull request

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` pass.
- Performance is a feature: changes in the per-frame path must keep the overlay under 0.3 ms GPU per frame and
  3 % of one CPU core (`--selftest` checks this).
- `unsafe` lives only in the FFI / OS crates (`vr`, `gfx-d3d11`, `capture-win`, `input-win`, `sys`), each block
  with a `// SAFETY:` comment.
- Every user-visible string goes through the Fluent file (`crates/ui/locales/en-US.ftl`).
- One topic per pull request; describe what you tested (headset or null driver).

## Rights in what you contribute

WristXR is the work of [EncidXVII](https://github.com/EncidXVII), published under GPL-3.0-or-later with the terms in [NOTICE](NOTICE). So that it stays one
work with one owner who can answer for all of it:

- You keep the copyright in what you write.
- By opening a pull request you license your contribution to the project under GPL-3.0-or-later, and you give EncidXVII a
  perpetual, worldwide, royalty-free right to use, change and relicense it as part of WristXR (for example to move to a
  later license version or to publish in a store that needs other terms).
- You confirm that you wrote it or have the right to submit it, as in the
  [Developer Certificate of Origin 1.1](https://developercertificate.org/): add `Signed-off-by: Your Name <email>`
  to your commits (`git commit -s`).
- Contributors are named in the changelog and the release notes.
