//! Live gates against a running SteamVR (real headset or the null driver). `--selftest --vr`.
#![allow(unsafe_code)]

use crate::gates::GateResult;
use dvr_capture_win::Capture;
use dvr_core::backend::VrBackend;
use dvr_core::engine::FrameInput;
use dvr_core::panel::{Panel, Source};
use dvr_gfx_d3d11::{Gpu, raw};
use dvr_vr::OpenVr;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

const RUN: Duration = Duration::from_secs(3);

/// Kernel + user CPU time of this process.
fn cpu_time() -> Duration {
    let (mut a, mut b, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: pseudo-handle of the current process; out-params are valid.
    if unsafe { GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut kernel, &mut user) }.is_err() {
        return Duration::ZERO;
    }
    let ticks = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    Duration::from_nanos((ticks(kernel) + ticks(user)) * 100)
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Gate 1 + 2 + P (partial): overlay shows the captured primary monitor; loop is paced by the compositor;
/// CPU stays under budget.
pub fn overlay_live() -> GateResult {
    let dir = std::env::temp_dir().join(format!("dvr-selftest-vr-{}", std::process::id()));
    let manifest = crate::bindings::install(&dir).map_err(err)?;
    let result = run(&manifest);
    // Best effort: a leftover temp dir is harmless.
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run(manifest: &std::path::Path) -> GateResult {
    let mut vr = OpenVr::init(manifest)?;
    let gpu = Gpu::new(vr.adapter_index()).map_err(err)?;
    let head = vr.head_pose().ok_or("HMD pose not valid")?;
    let monitors = dvr_capture_win::monitors();
    let (index, monitor) = monitors.iter().enumerate().find(|(_, m)| m.primary).ok_or("no primary monitor")?;
    let panel =
        Panel::in_front_of(1, Source::Monitor(index as u32), head, monitor.rect.h as f32 / monitor.rect.w as f32);
    let tex = gpu.texture(monitor.rect.w, monitor.rect.h, None).map_err(err)?;
    let capture = Capture::monitor(&gpu.device, monitor.handle).map_err(err)?;

    let mut input = FrameInput::default();
    let (mut frames, mut captured) = (0u32, 0u32);
    let (cpu0, t0) = (cpu_time(), Instant::now());
    while t0.elapsed() < RUN {
        if !vr.poll(t0.elapsed().as_millis() as u64, &mut input) {
            return Err("runtime asked to quit".into());
        }
        vr.place_panel(&panel, panel.pose);
        if capture.copy_latest(&gpu.context, &tex).map_err(err)? {
            gpu.flush();
            vr.set_texture_d3d11(1, raw(&tex))?;
            captured += 1;
        }
        vr.wait_frame(20);
        frames += 1;
    }
    let wall = t0.elapsed();
    let cpu = (cpu_time() - cpu0).as_secs_f64() / wall.as_secs_f64() * 100.0;
    let hz = f64::from(frames) / wall.as_secs_f64();
    println!("      vr: {hz:.0} loop Hz, {captured} captured frames, CPU {cpu:.2}% of one core");
    if hz < 45.0 {
        return Err(format!("loop not paced by the compositor: {hz:.0} Hz"));
    }
    if captured == 0 {
        return Err("no desktop frame captured".into());
    }
    if cpu >= 3.0 {
        return Err(format!("CPU {cpu:.2}% of one core exceeds the 3% budget"));
    }
    Ok(())
}
