//! System numbers for the wrist and the cost meter: CPU/RAM/GPU load, this process's CPU, local time.
#![cfg(windows)]
#![allow(unsafe_code)]

use std::time::{Duration, Instant};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW, PdhCloseQuery,
    PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};
use windows::Win32::System::SystemInformation::{GetLocalTime, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes, GetSystemTimes};
use windows::core::{PCWSTR, w};

const PDH_MORE_DATA: u32 = 0x8000_07D2;

fn ticks(f: FILETIME) -> u64 {
    (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
}

/// Kernel + user CPU time used by this process so far.
pub fn process_cpu_time() -> Duration {
    let (mut a, mut b, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: pseudo-handle of the current process; out-params are valid.
    if unsafe { GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut kernel, &mut user) }.is_err() {
        return Duration::ZERO;
    }
    Duration::from_nanos((ticks(kernel) + ticks(user)) * 100)
}

/// Local wall-clock (hour, minute).
pub fn local_time() -> (u16, u16) {
    // SAFETY: plain query.
    let t = unsafe { GetLocalTime() };
    (t.wHour, t.wMinute)
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

/// GPU 3D-engine utilization summed over processes (PDH "GPU Engine" counters).
struct GpuCounter {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
    buf: Vec<u64>,
}

impl GpuCounter {
    fn open() -> Option<Self> {
        let (mut query, mut counter) = (PDH_HQUERY::default(), PDH_HCOUNTER::default());
        // SAFETY: out-params valid; query closed in Drop.
        unsafe {
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            let path = w!("\\GPU Engine(*engtype_3D)\\Utilization Percentage");
            if PdhAddEnglishCounterW(query, path, 0, &mut counter) != 0 || PdhCollectQueryData(query) != 0 {
                PdhCloseQuery(query);
                return None;
            }
        }
        Some(Self { query, counter, buf: Vec::new() })
    }

    fn sample(&mut self) -> Option<f32> {
        // SAFETY: two-call pattern; the buffer is sized (8-byte aligned) from the first call.
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let (mut bytes, mut count) = (0u32, 0u32);
            if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, None) != PDH_MORE_DATA
            {
                return None;
            }
            self.buf.resize((bytes as usize).div_ceil(8), 0);
            let items = self.buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, Some(items)) != 0 {
                return None;
            }
            let total: f64 = std::slice::from_raw_parts(items, count as usize)
                .iter()
                .filter(|i| i.FmtValue.CStatus == 0)
                .map(|i| i.FmtValue.Anonymous.doubleValue)
                .sum();
            Some((total / 100.0).clamp(0.0, 1.0) as f32)
        }
    }
}

impl Drop for GpuCounter {
    fn drop(&mut self) {
        // SAFETY: query opened in `open`.
        unsafe { PdhCloseQuery(self.query) };
    }
}

pub struct Sampler {
    system: Option<(u64, u64)>,
    own: (Duration, Instant),
    gpu: Option<GpuCounter>,
    gpu_last: Option<f32>,
    calls: u32,
}

/// PDH's GPU engine query is the expensive part of sampling (~1% of a core at 1 Hz), so it runs every Nth sample.
const GPU_EVERY: u32 = 5;

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            system: system_times(),
            own: (process_cpu_time(), Instant::now()),
            gpu: GpuCounter::open(),
            gpu_last: None,
            calls: 0,
        }
    }

    /// Loads since the previous call. Call about once per second.
    pub fn sample(&mut self) -> Load {
        let now = system_times();
        let cpu = match (self.system, now) {
            (Some((idle0, total0)), Some((idle1, total1))) if total1 > total0 => {
                Some(1.0 - (idle1 - idle0) as f32 / (total1 - total0) as f32)
            }
            _ => None,
        };
        self.system = now;
        let (cpu0, t0) = self.own;
        let (cpu1, t1) = (process_cpu_time(), Instant::now());
        self.own = (cpu1, t1);
        let wall = (t1 - t0).as_secs_f32();
        let own_cpu_percent = if wall > 0.0 { (cpu1.saturating_sub(cpu0)).as_secs_f32() / wall * 100.0 } else { 0.0 };
        if self.calls.is_multiple_of(GPU_EVERY) {
            self.gpu_last = self.gpu.as_mut().and_then(GpuCounter::sample);
        }
        self.calls = self.calls.wrapping_add(1);
        Load { cpu, ram: memory_load(), gpu: self.gpu_last, own_cpu_percent }
    }
}

/// (idle, kernel + user) in 100 ns ticks; kernel time includes idle.
fn system_times() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: out-params valid.
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    Some((ticks(idle), ticks(kernel) + ticks(user)))
}

fn memory_load() -> Option<f32> {
    let mut m = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    // SAFETY: dwLength set.
    unsafe { GlobalMemoryStatusEx(&mut m) }.ok()?;
    Some(m.dwMemoryLoad as f32 / 100.0)
}

/// Short key click: 25 ms decaying 2 kHz tone with a soft noise attack, 16-bit mono WAV, generated once.
fn click_wav() -> &'static [u8] {
    static WAV: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    WAV.get_or_init(|| {
        const RATE: u32 = 22_050;
        let n = RATE as usize / 40;
        let mut seed = 0x1234_5678u32;
        let samples: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let noise = (seed as f32 / u32::MAX as f32 - 0.5) * (-t * 900.0).exp();
                let tone = (t * 2000.0 * std::f32::consts::TAU).sin() * (-t * 220.0).exp();
                ((tone * 0.35 + noise * 0.25) * f32::from(i16::MAX)) as i16
            })
            .collect();
        let data = (samples.len() * 2) as u32;
        let mut w = Vec::with_capacity(44 + data as usize);
        w.extend(b"RIFF");
        w.extend((36 + data).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(RATE.to_le_bytes());
        w.extend((RATE * 2).to_le_bytes());
        w.extend(2u16.to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend(data.to_le_bytes());
        samples.iter().for_each(|s| w.extend(s.to_le_bytes()));
        w
    })
}

/// Plays the key click without blocking (a new click cuts the previous one).
pub fn play_click() {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    // SAFETY: SND_MEMORY reads the static WAV buffer, which lives for the whole process.
    unsafe {
        let _ = PlaySoundW(PCWSTR(click_wav().as_ptr().cast()), None, SND_ASYNC | SND_MEMORY | SND_NODEFAULT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_are_in_range() {
        let mut s = Sampler::new();
        std::thread::sleep(Duration::from_millis(200));
        let l = s.sample();
        for v in [l.cpu, l.ram, l.gpu].into_iter().flatten() {
            assert!((0.0..=1.0).contains(&v), "{l:?}");
        }
        assert!(l.cpu.is_some() && l.ram.is_some(), "{l:?}");
        let (h, m) = local_time();
        assert!(h < 24 && m < 60);
    }
}
