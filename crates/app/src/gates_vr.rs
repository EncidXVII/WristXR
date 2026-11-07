//! Live gates against a running SteamVR (real headset or the null driver). `--selftest --vr`.
//! Uses the same meters as the wrist cost readout (dvr_sys::Sampler, GpuTimer), so gate P and the UI agree.

use crate::gates::GateResult;
use dvr_capture_win::{Capture, Frame};
use dvr_core::backend::VrBackend;
use dvr_core::engine::FrameInput;
use dvr_core::math::Vec3;
use dvr_core::panel::{Anchor, Hand, Panel, Source};
use dvr_gfx_d3d11::{Gpu, GpuTimer, raw};
use dvr_ui::{WRIST_SIZE, Wrist, WristInfo};
use dvr_vr::OpenVr;
use std::time::{Duration, Instant};

/// Startup (SteamVR settling, first PDH collection, font loading) is excluded from the measurement.
const WARMUP: Duration = Duration::from_secs(2);
const RUN: Duration = Duration::from_secs(5);
/// Budgets from the task (scenario S1 is stricter; this is one monitor + wrist).
const GPU_BUDGET_MS: f32 = 0.3;
const CPU_BUDGET_PERCENT: f32 = 3.0;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Gates 1 + 2 + 5 + P (partial): captured primary monitor and a wrist panel are shown; the loop is paced by
/// the compositor; our GPU and CPU cost stay under budget.
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
    // Gate C (tray / autostart): register the app with SteamVR and round-trip auto-launch.
    let dir = manifest.parent().ok_or("manifest has no folder")?;
    let exe = std::env::current_exe().map_err(err)?;
    let app = crate::bindings::install_vrmanifest(dir, &exe).map_err(err)?;
    vr.register_app(&app, crate::bindings::APP_KEY)?;
    for on in [true, false] {
        vr.set_auto_launch(crate::bindings::APP_KEY, on)?;
        if vr.auto_launch(crate::bindings::APP_KEY) != on {
            return Err(format!("auto-launch did not become {on}"));
        }
    }
    let gpu = Gpu::new(vr.adapter_index()).map_err(err)?;
    let head = vr.head_pose().ok_or("HMD pose not valid")?;
    let monitors = dvr_capture_win::monitors();
    let (index, monitor) = monitors.iter().enumerate().find(|(_, m)| m.primary).ok_or("no primary monitor")?;
    let panel =
        Panel::in_front_of(1, Source::Monitor(index as u32), head, monitor.rect.h as f32 / monitor.rect.w as f32);
    // No controllers on the null driver: show the wrist in the world, next to the monitor panel.
    let mut wrist_panel = Panel::on_wrist(2, Hand::Left, WRIST_SIZE.1 as f32 / WRIST_SIZE.0 as f32);
    wrist_panel.anchor = Anchor::World;
    wrist_panel.pose = panel.pose;
    wrist_panel.pose.pos = panel.pose.pos + Vec3::new(0.0, -0.45, 0.2);
    let tex = gpu.texture(monitor.rect.w, monitor.rect.h, None).map_err(err)?;
    let wrist_tex = gpu.texture(WRIST_SIZE.0, WRIST_SIZE.1, None).map_err(err)?;
    let mut capture = Capture::monitor(&gpu.device, monitor.handle).map_err(err)?;
    let mut wrist = Wrist::new()?;
    let mut timer = GpuTimer::new(&gpu).map_err(err)?;
    let mut sampler = dvr_sys::Sampler::new();

    let mut input = FrameInput::default();
    // Per-hand overlay priority (game input blocked for a hand on a panel) must be accepted by SteamVR.
    vr.set_exclusive([true, false]);
    vr.poll(0, &mut input);
    if vr.input_error() != 0 {
        return Err(format!("UpdateActionState with per-hand priority: EVRInputError {}", vr.input_error()));
    }
    vr.set_exclusive([false, false]);
    let (mut frames, mut captured, mut wrist_frames) = (0u32, 0u32, 0u32);
    let start = Instant::now();
    let mut tick = start;
    let mut measured: Option<(Instant, std::time::Duration)> = None;
    while measured.is_none_or(|(t0, _)| t0.elapsed() < RUN) {
        if measured.is_none() && start.elapsed() >= WARMUP {
            measured = Some((Instant::now(), dvr_sys::process_cpu_time()));
            (frames, captured, wrist_frames) = (0, 0, 0);
        }
        if !vr.poll(start.elapsed().as_millis() as u64, &mut input) {
            return Err("runtime asked to quit".into());
        }
        if tick.elapsed() >= Duration::from_secs(1) {
            tick = Instant::now();
            let load = sampler.sample();
            let (hour, minute) = dvr_sys::local_time();
            wrist.set(&WristInfo {
                hour,
                minute,
                cpu: load.cpu,
                ram: load.ram,
                gpu: load.gpu,
                own_gpu_ms: timer.last_ms(),
                own_cpu_percent: load.own_cpu_percent,
                ..WristInfo::default()
            });
        }
        vr.place_panel(&panel, panel.pose);
        vr.place_panel(&wrist_panel, wrist_panel.pose);
        // All GPU work back to back inside the timer, then one flush, then hand textures to SteamVR.
        timer.begin(&gpu);
        let wrist_new = match wrist.render() {
            Some(px) => {
                gpu.update(&wrist_tex, px, WRIST_SIZE.0);
                true
            }
            None => false,
        };
        let capture_new = capture.copy_latest(&gpu.context, &tex).map_err(err)? == Frame::Copied;
        timer.end(&gpu);
        gpu.flush();
        if wrist_new {
            vr.set_texture_d3d11(2, raw(&wrist_tex))?;
            wrist_frames += 1;
        }
        if capture_new {
            vr.set_texture_d3d11(1, raw(&tex))?;
            captured += 1;
        }
        vr.wait_frame(20);
        frames += 1;
    }
    let (t0, cpu0) = measured.ok_or("no measurement window")?;
    let wall = t0.elapsed();
    let cpu = (dvr_sys::process_cpu_time() - cpu0).as_secs_f32() / wall.as_secs_f32() * 100.0;
    let gpu_ms = timer.last_ms();
    let hz = f64::from(frames) / wall.as_secs_f64();
    println!(
        "      vr: {hz:.0} loop Hz, {captured} captured, {wrist_frames} wrist redraws, \
         GPU {gpu_ms:.3} ms/frame, CPU {cpu:.2}% of one core"
    );
    if hz < 45.0 {
        return Err(format!("loop not paced by the compositor: {hz:.0} Hz"));
    }
    if captured == 0 {
        return Err("no desktop frame captured".into());
    }
    // Stats change once per second; anything more means redrawing without changes.
    if wrist_frames == 0 || wrist_frames > RUN.as_secs() as u32 + 2 {
        return Err(format!("wrist must redraw only on change: {wrist_frames} redraws in {RUN:?}"));
    }
    if gpu_ms >= GPU_BUDGET_MS {
        return Err(format!("GPU {gpu_ms:.3} ms per frame exceeds {GPU_BUDGET_MS} ms"));
    }
    if cpu >= CPU_BUDGET_PERCENT {
        return Err(format!("CPU {cpu:.2}% of one core exceeds {CPU_BUDGET_PERCENT}%"));
    }
    Ok(())
}
