# Releasing WristXR

## The repository page (once)

What search engines and assistants read first is the repository's own page, so it says what the README says:

1. **Name**: `WristXR` (GitHub redirects the old address after a rename).
2. **About**:
   - Description: `Free, open-source VR desktop overlay for SteamVR with a wrist clock: screens and windows in VR, laser pointer, VR keyboard, VRChat chatbox, Discord notifications. Windows and Linux. Under 0.1 ms GPU per frame.`
   - Topics (20 at most): `vr`, `virtual-reality`, `steamvr`, `openvr`, `vr-overlay`, `desktop-overlay`, `vrchat`, `vrchat-osc`,
     `xsoverlay-alternative`, `wrist-watch`, `vr-keyboard`, `discord-notifications`, `valve-index`, `meta-quest`,
     `steam-frame`, `linux`, `wayland`, `rust`, `slint`, `open-source`.
3. **Settings > General > Social preview**: upload `docs/img/og.png` (the card shown when the link is shared).
4. **Security**: turn on Private vulnerability reporting (`SECURITY.md` sends reports there) and Dependabot alerts.
5. Features: Issues on, Discussions on (Q&A, Show and tell), Wiki off.

## Each release

1. Everything committed, the changelog's top section dated, the version the same in every manifest.
2. The checks in [CONTRIBUTING.md](CONTRIBUTING.md) pass, and the build has been used in a headset.
3. Build the packages with the packaging scripts; each carries `LICENSE`, `NOTICE` and `THIRD_PARTY_NOTICES.md`.
4. Tag `vX.Y.Z` and push the tag: the `release` workflow builds the packages and their SHA-256 files. Publish them
   from your own account, with the changelog section as the notes:
   `gh run download <run id> -D assets`, then
   `gh release create vX.Y.Z assets/*/* --title "WristXR X.Y.Z" --notes-file notes.md`.
5. Check the README's download links and the app's update check against the new release.
6. In `flake.nix`, set `version` and each Linux file's `hash` (its `.sha256` in SRI form: `nix hash convert --hash-algo
   sha256 <hex>`), then push: the Nix workflow builds it.
