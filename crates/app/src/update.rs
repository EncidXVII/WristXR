// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Update check for GitHub builds: latest release vs this version. Never installs anything; the tray offers a
//! one-click download link. Network only to api.github.com, generic User-Agent, no identifiers.
use serde::Deserialize;

/// `owner/name` of the release repository; empty disables the check.
#[cfg(any(windows, target_os = "linux"))]
pub const REPO: &str = "EncidXVII/WristXR";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

#[derive(Deserialize)]
struct Latest {
    tag_name: String,
    html_url: String,
}

fn parts(v: &str) -> Vec<u64> {
    v.trim_start_matches('v').split(['.', '-']).map_while(|p| p.parse().ok()).collect()
}

/// True when `candidate` is a newer version than `current` (numeric, `v` prefix allowed).
pub fn newer(candidate: &str, current: &str) -> bool {
    parts(candidate) > parts(current)
}

/// Parses the GitHub "latest release" response; `Some` only when it is newer than `current`.
pub fn parse(json: &str, current: &str) -> Option<Release> {
    let l: Latest = serde_json::from_str(json).ok()?;
    newer(&l.tag_name, current)
        .then(|| Release { version: l.tag_name.trim_start_matches('v').to_owned(), url: l.html_url })
}

/// Checks once on a background thread; the receiver yields a release only when one is newer.
#[cfg(any(windows, target_os = "linux"))]
pub fn check_in_background() -> std::sync::mpsc::Receiver<Release> {
    let (tx, rx) = std::sync::mpsc::channel();
    if !REPO.is_empty() {
        std::thread::spawn(move || {
            let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
            // curl ships with Windows 10+ and with every Linux desktop; no HTTP client crate needed.
            let mut cmd = std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
            cmd.args(["-sfL", "--max-time", "10", "-H", "User-Agent: WristXR-update-check", &url]);
            // CREATE_NO_WINDOW: the app has no console, so without it curl would flash one at startup.
            #[cfg(windows)]
            std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
            let out = cmd.output();
            if let Some(r) =
                out.ok().and_then(|o| parse(&String::from_utf8_lossy(&o.stdout), env!("CARGO_PKG_VERSION")))
            {
                let _ = tx.send(r);
            }
        });
    }
    rx
}

/// Opens the release page in the default browser.
#[cfg(any(windows, target_os = "linux"))]
pub fn open(url: &str) {
    if url.starts_with("https://github.com/") {
        browse(url);
    }
}

/// Opens one of the app's own links (Setup > About) in the default browser.
#[cfg(any(windows, target_os = "linux"))]
pub fn browse(url: &str) {
    let _ = std::process::Command::new(if cfg!(windows) { "explorer.exe" } else { "xdg-open" }).arg(url).spawn();
}
