//! Update check for GitHub builds: latest release vs this version. Never installs anything; the tray offers a
//! one-click download link. Network only to api.github.com, generic User-Agent, no identifiers.
use serde::Deserialize;

/// `owner/name` of the release repository; empty disables the check (set when the repo exists).
#[cfg(windows)]
pub const REPO: &str = "";

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
#[cfg(windows)]
pub fn check_in_background() -> std::sync::mpsc::Receiver<Release> {
    let (tx, rx) = std::sync::mpsc::channel();
    if !REPO.is_empty() {
        std::thread::spawn(move || {
            let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
            // curl.exe ships with Windows 10+; no HTTP client crate needed.
            let out = std::process::Command::new("curl.exe")
                .args(["-sfL", "--max-time", "10", "-H", "User-Agent: DesktopVR-update-check", &url])
                .output();
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
#[cfg(windows)]
pub fn open(url: &str) {
    if url.starts_with("https://github.com/") {
        let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
    }
}
