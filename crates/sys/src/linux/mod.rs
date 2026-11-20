//! Linux: system numbers for the watch and the cost meter (from /proc and sysfs), local time, one instance,
//! SteamVR's server watch, dialogs (zenity or kdialog when installed), media (MPRIS) and app notifications
//! (a D-Bus monitor of the notification service).

mod media;
mod process;
mod toasts;

pub use media::{Media, MediaCommand, MediaInfo, app_name};
pub use process::{Instance, ProcessWatch, exit_watchdog, single_instance, terminate};
pub use toasts::{Access as ToastAccess, AppToast, AppToasts};

use std::time::{Duration, Instant};

fn rusage() -> libc::rusage {
    // SAFETY: getrusage fills the struct we own.
    unsafe {
        let mut u: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut u);
        u
    }
}

/// Kernel + user CPU time used by this process so far.
pub fn process_cpu_time() -> Duration {
    let u = rusage();
    let t = |tv: libc::timeval| Duration::new(tv.tv_sec.max(0) as u64, (tv.tv_usec.max(0) as u32) * 1000);
    t(u.ru_utime) + t(u.ru_stime)
}

fn local() -> libc::tm {
    // SAFETY: time + localtime_r into a struct we own.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm
    }
}

/// (weekday 0 = Sunday, day of month, month 1-12), as on Windows.
pub fn local_date() -> (u16, u16, u16) {
    let t = local();
    (t.tm_wday as u16, t.tm_mday as u16, (t.tm_mon + 1) as u16)
}

pub fn local_time() -> (u16, u16) {
    let t = local();
    (t.tm_hour as u16, t.tm_min as u16)
}

/// Linux desktops have no one switch for it; motion stays on.
pub fn animations_enabled() -> bool {
    true
}

/// Loads in 0..1; `None` when the source is unavailable.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Load {
    pub cpu: Option<f32>,
    pub ram: Option<f32>,
    pub gpu: Option<f32>,
    /// This process, percent of one core since the previous sample.
    pub own_cpu_percent: f32,
}

pub struct Sampler {
    cpu: Option<(u64, u64)>,
    own: (Duration, Instant),
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

/// (busy, total) jiffies from /proc/stat's first line.
fn cpu_jiffies() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/stat").ok()?;
    let line = text.lines().next()?.strip_prefix("cpu ")?;
    let v: Vec<u64> = line.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    let idle = v.get(3)? + v.get(4).copied().unwrap_or(0);
    let total: u64 = v.iter().sum();
    Some((total - idle, total))
}

fn ram() -> Option<f32> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb = |key: &str| -> Option<f32> {
        text.lines().find(|l| l.starts_with(key))?.split_whitespace().nth(1)?.parse().ok()
    };
    let (total, avail) = (kb("MemTotal:")?, kb("MemAvailable:")?);
    (total > 0.0).then(|| 1.0 - avail / total)
}

/// AMD and Intel (xe) GPUs say how busy they are in sysfs; NVIDIA's driver does not.
fn gpu() -> Option<f32> {
    let dir = std::fs::read_dir("/sys/class/drm").ok()?;
    dir.filter_map(Result::ok)
        .filter(|e| {
            e.file_name().to_string_lossy().starts_with("card") && !e.file_name().to_string_lossy().contains('-')
        })
        .find_map(|e| std::fs::read_to_string(e.path().join("device/gpu_busy_percent")).ok())
        .and_then(|t| t.trim().parse::<f32>().ok())
        .map(|p| p / 100.0)
}

impl Sampler {
    pub fn new() -> Self {
        Self { cpu: cpu_jiffies(), own: (process_cpu_time(), Instant::now()) }
    }

    pub fn sample(&mut self) -> Load {
        let now = cpu_jiffies();
        let cpu = match (self.cpu, now) {
            // The kernel's iowait count can step back, so busy time is not strictly rising.
            (Some((b0, t0)), Some((b1, t1))) if t1 > t0 => {
                Some((b1.saturating_sub(b0) as f32 / (t1 - t0) as f32).min(1.0))
            }
            _ => None,
        };
        self.cpu = now;
        let (own0, at0) = self.own;
        let own = process_cpu_time();
        let wall = at0.elapsed().as_secs_f32();
        self.own = (own, Instant::now());
        let own_cpu_percent = if wall > 0.0 { (own - own0).as_secs_f32() / wall * 100.0 } else { 0.0 };
        Load { cpu, ram: ram(), gpu: gpu(), own_cpu_percent }
    }
}

/// No key click on Linux: there is no sound API every desktop has.
pub fn play_click() {}

/// Nothing to pump: Linux needs no window message loop for what DesktopVR uses.
pub fn pump_messages() {}

/// Started from a terminal, output already goes there.
pub fn attach_parent_console() {}

pub fn light_taskbar() -> bool {
    false
}

pub fn small_icon_px() -> u32 {
    22
}

fn have(tool: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(tool).is_file()))
}

/// A dialog through zenity or kdialog when one is there; true when the user said yes (or for a plain message).
fn dialog(kind: &str, title: &str, message: &str) -> bool {
    let run = |cmd: &mut std::process::Command| cmd.status().ok().map(|s| s.success());
    let shown = if have("zenity") {
        run(std::process::Command::new("zenity").args([kind, "--title", title, "--text", message, "--no-markup"]))
    } else if have("kdialog") {
        let k = match kind {
            "--error" => "--error",
            "--question" => "--yesno",
            _ => "--msgbox",
        };
        run(std::process::Command::new("kdialog").args(["--title", title, k, message]))
    } else {
        None
    };
    shown.unwrap_or_else(|| {
        eprintln!("{title}: {message}");
        kind != "--question"
    })
}

pub fn error_dialog(title: &str, message: &str) {
    dialog("--error", title, message);
}

pub fn info_dialog(title: &str, message: &str) {
    dialog("--info", title, message);
}

/// Yes/no. Without a dialog tool the question goes to the terminal it was started from; with neither, the answer
/// is no.
pub fn ask_dialog(title: &str, message: &str) -> bool {
    use std::io::IsTerminal;
    if have("zenity") || have("kdialog") {
        return dialog("--question", title, message);
    }
    if !std::io::stdin().is_terminal() {
        return false;
    }
    eprint!("{title}: {message} [y/N] ");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).is_ok() && matches!(answer.trim(), "y" | "Y" | "yes")
}

/// Resident memory of this process.
pub fn working_set_bytes() -> u64 {
    let pages = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|t| t.split_whitespace().nth(1)?.parse::<u64>().ok())
        .unwrap_or(0);
    // SAFETY: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(0) as u64;
    pages * page
}
