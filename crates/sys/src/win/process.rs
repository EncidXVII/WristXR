// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Process lifetime: one instance at a time, noticing when SteamVR is gone, and an exit that cannot hang.
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_TIMEOUT};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, OpenProcess, PROCESS_SYNCHRONIZE, TerminateProcess, WaitForSingleObject,
};
use windows::core::w;

/// Holds the single-instance mutex for the life of the process.
pub struct Instance(HANDLE);

/// Claims "the" WristXR instance for this user session; `None` when another copy is already running.
pub fn single_instance() -> Option<Instance> {
    // SAFETY: plain named-mutex creation; the handle is owned by the returned guard (or closed right here).
    unsafe {
        let h = CreateMutexW(None, false, w!("Local\\WristXR.instance")).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            return None;
        }
        Some(Instance(h))
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW and is closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Watches another process (SteamVR's server) so we can leave when it is gone, even if it never said goodbye.
pub struct ProcessWatch(HANDLE);

impl ProcessWatch {
    /// The first running process with this executable name (case-insensitive), e.g. "vrserver.exe".
    pub fn by_name(exe: &str) -> Option<Self> {
        // SAFETY: snapshot walk with a correctly sized entry; handles are closed on every path.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
            let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut pid = None;
            let mut ok = Process32FirstW(snap, &mut e).is_ok();
            while ok {
                let n = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
                if String::from_utf16_lossy(&e.szExeFile[..n]).eq_ignore_ascii_case(exe) {
                    pid = Some(e.th32ProcessID);
                    break;
                }
                ok = Process32NextW(snap, &mut e).is_ok();
            }
            let _ = CloseHandle(snap);
            Self::by_pid(pid?)
        }
    }

    pub fn by_pid(pid: u32) -> Option<Self> {
        // SAFETY: opening a process for waiting only; the handle is owned by the returned watch.
        unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid).ok().map(Self) }
    }

    /// Still running.
    pub fn alive(&self) -> bool {
        // SAFETY: zero-timeout wait on a handle we own.
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_TIMEOUT }
    }

    /// Backstop on its own thread: once the watched process has been gone for `grace` (time for the main loop to
    /// notice, save and leave by itself), end this process even if the main loop is stuck.
    pub fn exit_when_gone(self, grace: Duration) {
        std::thread::spawn(move || {
            while self.alive() {
                std::thread::sleep(Duration::from_secs(1));
            }
            std::thread::sleep(grace);
            terminate(0);
        });
    }
}

// SAFETY: a process handle may be waited on from any thread.
unsafe impl Send for ProcessWatch {}

impl Drop for ProcessWatch {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcess and is closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Ends the process now, skipping library teardown (WinRT / capture / SteamVR client unloading can deadlock while
/// background threads sit in system calls). Settings must already be saved.
pub fn terminate(code: u32) -> ! {
    // SAFETY: terminating our own process; nothing runs after this.
    unsafe {
        let _ = TerminateProcess(GetCurrentProcess(), code);
    }
    std::process::exit(code as i32)
}

/// After a quit: if orderly cleanup has not finished within `grace`, end the process anyway.
pub fn exit_watchdog(grace: Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(grace);
        terminate(0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_instance_until_the_first_ends() {
        let first = single_instance().expect("first instance");
        assert!(single_instance().is_none(), "second instance must be refused");
        drop(first);
        assert!(single_instance().is_some(), "after the first ends a new one may start");
    }

    #[test]
    fn notices_when_a_watched_process_ends() {
        let mut child = std::process::Command::new("ping.exe").args(["-n", "2", "127.0.0.1"]).spawn().expect("spawn");
        let watch = ProcessWatch::by_pid(child.id()).expect("watch");
        assert!(watch.alive(), "running process must read as alive");
        child.wait().expect("wait");
        assert!(!watch.alive(), "ended process must read as gone");
        assert!(ProcessWatch::by_name("ping.exe").is_none_or(|w| w.alive()), "lookup by name finds live ones only");
    }
}
