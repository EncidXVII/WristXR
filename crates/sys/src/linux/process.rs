//! One DesktopVR per user, SteamVR's server watch, and ending the process.
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::Duration;

/// Holds the lock file for the life of the process.
pub struct Instance(#[allow(dead_code)] File);

fn lock_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(d) => PathBuf::from(d).join("desktopvr.lock"),
        // SAFETY: getuid has no preconditions.
        None => std::env::temp_dir().join(format!("desktopvr-{}.lock", unsafe { libc::getuid() })),
    }
}

/// Claims "the" DesktopVR for this user; `None` when another copy holds it. The kernel drops the lock when the
/// process ends, however it ends.
pub fn single_instance() -> Option<Instance> {
    let file = File::options().create(true).truncate(false).write(true).open(lock_path()).ok()?;
    // SAFETY: flock on a file we own.
    let ok = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    ok.then_some(Instance(file))
}

/// Watches another process (SteamVR's server) so we can leave when it is gone, even if it never said goodbye.
pub struct ProcessWatch(u32);

fn running(pid: u32) -> bool {
    // A zombie has ended; only its entry is left.
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| s.rsplit_once(") ").map(|(_, rest)| !rest.starts_with('Z')))
        .unwrap_or(false)
}

impl ProcessWatch {
    /// The first running process with this name. A Windows name ("vrserver.exe") is matched without ".exe".
    pub fn by_name(exe: &str) -> Option<Self> {
        let name = exe.strip_suffix(".exe").unwrap_or(exe);
        // /proc/<pid>/comm holds at most 15 characters.
        let want: String = name.chars().take(15).collect();
        std::fs::read_dir("/proc")
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .find(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == want))
            .map(Self)
    }

    pub fn by_pid(pid: u32) -> Option<Self> {
        running(pid).then_some(Self(pid))
    }

    /// Still running.
    pub fn alive(&self) -> bool {
        running(self.0)
    }

    /// Backstop on its own thread: once the watched process has been gone for `grace`, end this process even if
    /// the main loop is stuck.
    pub fn exit_when_gone(self, grace: Duration) {
        let _ = std::thread::Builder::new().name("steamvr-watch".into()).spawn(move || {
            while self.alive() {
                std::thread::sleep(Duration::from_secs(1));
            }
            std::thread::sleep(grace);
            terminate(0);
        });
    }
}

/// Ends the process now, skipping library teardown. Settings must already be saved.
pub fn terminate(code: u32) -> ! {
    std::process::exit(code as i32)
}

/// After a quit: if orderly cleanup has not finished within `grace`, end the process anyway.
pub fn exit_watchdog(grace: Duration) {
    let _ = std::thread::Builder::new().name("exit-watchdog".into()).spawn(move || {
        std::thread::sleep(grace);
        terminate(0);
    });
}
