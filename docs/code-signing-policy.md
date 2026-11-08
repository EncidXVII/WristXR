# Code signing policy

Windows release binaries are signed through the SignPath Foundation program for open-source projects.

- **What is signed:** only release builds produced by this repository's CI from a tagged commit. Local and
  pull-request builds are never signed.
- **Who can release:** maintainers listed in the repository settings, each with multi-factor authentication
  enabled on GitHub and SignPath.
- **Approval:** every signing request is approved manually by a maintainer after CI (format, lint, tests,
  license check, self-test) passed for that commit.
- **What the binary does:** see `README.md`. It needs no administrator rights, installs no services and
  sends no telemetry. Network use: GitHub release check, localhost OSC (VRChat) and localhost notifications.
- **Third-party code:** listed in `THIRD_PARTY_NOTICES.md`; licenses are checked in CI with `cargo deny`.
- **Reporting:** suspected misuse of the signing certificate or a compromised release can be reported through
  a GitHub security advisory on this repository.

Free code signing provided by SignPath.io, certificate by SignPath Foundation.
