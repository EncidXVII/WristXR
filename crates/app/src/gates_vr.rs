// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Live gates against a running SteamVR (real headset or the null driver). `--selftest --vr`.
//! Uses the same meters as the wrist cost readout (wxr_sys::Sampler, GpuTimer), so gate P and the UI agree.

use crate::gates::GateResult;
use std::time::{Duration, Instant};
use wxr_capture_win::{Capture, Frame};
use wxr_core::backend::VrBackend;
use wxr_core::engine::{Claims, FrameInput};
use wxr_core::math::Vec3;
use wxr_core::panel::{Anchor, Hand, Panel, Source};
use wxr_gfx_d3d11::{Detail, Gpu, GpuTimer, Sharpen, raw};
use wxr_ui::{WRIST_SIZE, Wrist, WristInfo, WristLoad};
use wxr_vr::OpenVr;

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
    let dir = std::env::temp_dir().join(format!("wxr-selftest-vr-{}", std::process::id()));
    let manifest = crate::bindings::install(&dir).map_err(err)?;
    let result = run(&manifest);
    // Best effort: a leftover temp dir is harmless.
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn run(manifest: &std::path::Path) -> GateResult {
    let mut vr = OpenVr::connect()?;
    // Gate C (tray / autostart): register the app with SteamVR (before the actions, as the app does) and
    // round-trip auto-launch.
    let dir = manifest.parent().ok_or("manifest has no folder")?;
    let exe = std::env::current_exe().map_err(err)?;
    let app = crate::bindings::install_vrmanifest(dir, &exe).map_err(err)?;
    vr.register_app(&app, crate::bindings::APP_KEY)?;
    vr.load_actions(manifest)?;
    for on in [true, false] {
        vr.set_auto_launch(crate::bindings::APP_KEY, on)?;
        if vr.auto_launch(crate::bindings::APP_KEY) != on {
            return Err(format!("auto-launch did not become {on}"));
        }
    }
    let gpu = Gpu::new(vr.adapter_index()).map_err(err)?;
    let head = vr.head_pose().ok_or("HMD pose not valid")?;
    let monitors = wxr_capture_win::monitors();
    let (index, monitor) = monitors.iter().enumerate().find(|(_, m)| m.primary).ok_or("no primary monitor")?;
    let panel =
        Panel::in_front_of(1, Source::Monitor(index as u32), head, monitor.rect.h as f32 / monitor.rect.w as f32);
    // No controllers on the null driver: show the wrist in the world, next to the monitor panel.
    let mut wrist_panel = Panel::on_wrist(2, Hand::Left, WRIST_SIZE.1 as f32 / WRIST_SIZE.0 as f32);
    wrist_panel.anchor = Anchor::World;
    wrist_panel.pose = panel.pose;
    wrist_panel.pose.pos = panel.pose.pos + Vec3::new(0.0, -0.45, 0.2);
    // As the app shows a screen, at its worst case for the budget: full size, so the detail pass draws
    // every pixel of every captured frame.
    let sharpen = Sharpen::new(&gpu).map_err(err)?;
    let detail = Detail::new(&gpu, monitor.rect.w, monitor.rect.h, monitor.rect.w).map_err(err)?;
    let wrist_tex = gpu.texture(WRIST_SIZE.0, WRIST_SIZE.1, None).map_err(err)?;
    let mut capture = Capture::monitor(&gpu.device, monitor.handle).map_err(err)?;
    let mut wrist = Wrist::new()?;
    let mut timer = GpuTimer::new(&gpu).map_err(err)?;
    let mut sampler = wxr_sys::Sampler::new();

    let mut input = FrameInput::default();
    // Per-hand, per-group overlay priority (some of a hand's controls taken from the game) must be accepted.
    let all = Claims { point: true, stick: true, right: true };
    vr.set_claims([all, Claims { stick: true, ..Claims::default() }]);
    vr.poll(0, &mut input);
    if vr.input_error() != 0 {
        return Err(format!("UpdateActionState with per-hand priority: EVRInputError {}", vr.input_error()));
    }
    vr.set_claims([Claims::default(); 2]);
    let (mut frames, mut captured, mut wrist_frames) = (0u32, 0u32, 0u32);
    let start = Instant::now();
    let mut tick = start;
    let mut measured: Option<(Instant, std::time::Duration)> = None;
    while measured.is_none_or(|(t0, _)| t0.elapsed() < RUN) {
        if measured.is_none() && start.elapsed() >= WARMUP {
            measured = Some((Instant::now(), wxr_sys::process_cpu_time()));
            (frames, captured, wrist_frames) = (0, 0, 0);
        }
        if !vr.poll(start.elapsed().as_millis() as u64, &mut input) {
            return Err("runtime asked to quit".into());
        }
        if tick.elapsed() >= Duration::from_secs(1) {
            tick = Instant::now();
            let load = sampler.sample();
            let (hour, minute) = wxr_sys::local_time();
            wrist.set(&WristInfo {
                hour,
                minute,
                // Worst case for the budget: expanded with the load line (redrawn every second).
                expanded: true,
                load: Some(WristLoad {
                    game_fps: None,
                    target_fps: None,
                    cpu: load.cpu,
                    gpu: load.gpu,
                    own_gpu_ms: timer.last_ms(),
                }),
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
        let capture_new = capture.copy_latest(&gpu.context, &detail.src).map_err(err)? == Frame::Copied;
        if capture_new {
            sharpen.new_frame(&gpu, &detail);
            sharpen.draw(&gpu, &detail);
        }
        timer.end(&gpu);
        gpu.flush();
        if wrist_new {
            vr.set_texture_d3d11(2, raw(&wrist_tex))?;
            wrist_frames += 1;
        }
        if capture_new {
            vr.set_texture_d3d11(1, raw(&detail.out))?;
            captured += 1;
        }
        vr.wait_frame(20);
        frames += 1;
    }
    let (t0, cpu0) = measured.ok_or("no measurement window")?;
    let wall = t0.elapsed();
    let cpu = (wxr_sys::process_cpu_time() - cpu0).as_secs_f32() / wall.as_secs_f32() * 100.0;
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

/// One 90 Hz frame: our per-frame work longer than this would cost the user a frame.
const FRAME_BUDGET: Duration = Duration::from_micros(11_111);
const SOAK_WARMUP: Duration = Duration::from_secs(30);
const MEMORY_GROWTH_BUDGET: u64 = 10 * 1024 * 1024;
const RAM_BUDGET: u64 = 150 * 1024 * 1024;

/// Gate 8 + P (S1): every monitor captured, wrist and keyboard rendering, for `minutes`. Checks errors, memory
/// growth, frames where our own work overran a 90 Hz frame, and the CPU/GPU/RAM budgets.
pub fn soak(minutes: u64) -> GateResult {
    let dir = std::env::temp_dir().join(format!("wxr-soak-{}", std::process::id()));
    let manifest = crate::bindings::install(&dir).map_err(err)?;
    let result = soak_run(&manifest, Duration::from_secs(minutes * 60));
    // Best effort: a leftover temp dir is harmless.
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn soak_run(manifest: &std::path::Path, length: Duration) -> GateResult {
    let mut vr = OpenVr::init(manifest)?;
    let gpu = Gpu::new(vr.adapter_index()).map_err(err)?;
    let head = vr.head_pose().ok_or("HMD pose not valid")?;
    let monitors = wxr_capture_win::monitors();
    // S1: one panel per monitor, side by side in front of the user.
    let mut screens = Vec::new();
    for (i, m) in monitors.iter().enumerate() {
        let aspect = m.rect.h as f32 / m.rect.w as f32;
        let mut p = Panel::in_front_of(i as u32 + 1, Source::Monitor(i as u32), head, aspect);
        p.pose.pos = p.pose.pos + Vec3::new((i as f32 - 0.5) * 1.25, 0.0, 0.0);
        let tex = gpu.texture(m.rect.w, m.rect.h, None).map_err(err)?;
        let capture = Capture::monitor(&gpu.device, m.handle).map_err(err)?;
        screens.push((p, tex, capture));
    }
    let keyboard_panel = Panel::keyboard_below(50, head);
    let mut wrist_panel = Panel::on_wrist(51, Hand::Left, WRIST_SIZE.1 as f32 / WRIST_SIZE.0 as f32);
    wrist_panel.anchor = Anchor::World;
    wrist_panel.pose.pos = head.pos + Vec3::new(0.5, -0.4, -0.6);
    let kb_tex = gpu.texture(wxr_ui::KEYBOARD_SIZE.0, wxr_ui::KEYBOARD_SIZE.1, None).map_err(err)?;
    let wrist_tex = gpu.texture(WRIST_SIZE.0, WRIST_SIZE.1, None).map_err(err)?;
    let mut keyboard = wxr_ui::Keyboard::new()?;
    let mut wrist = Wrist::new()?;
    let mut timer = GpuTimer::new(&gpu).map_err(err)?;
    let mut sampler = wxr_sys::Sampler::new();
    let faces: Vec<wxr_ui::KeyFace> = wxr_core::keyboard::ROWS
        .iter()
        .flat_map(|r| r.iter())
        .map(|k| wxr_ui::KeyFace { label: k.label.to_owned(), ..wxr_ui::KeyFace::default() })
        .collect();

    let mut input = FrameInput::default();
    let start = Instant::now();
    let mut tick = start;
    let mut baseline: Option<(Instant, Duration, u64)> = None;
    let (mut frames, mut overruns, mut worst, mut peak_ram) = (0u64, 0u64, Duration::ZERO, 0u64);
    let mut fresh: Vec<u32> = Vec::with_capacity(8);
    while baseline.is_none_or(|(t0, _, _)| t0.elapsed() < length) {
        if baseline.is_none() && start.elapsed() >= SOAK_WARMUP {
            baseline = Some((Instant::now(), wxr_sys::process_cpu_time(), wxr_sys::working_set_bytes()));
            (frames, overruns, worst) = (0, 0, Duration::ZERO);
        }
        if !vr.poll(start.elapsed().as_millis() as u64, &mut input) {
            return Err("runtime asked to quit".into());
        }
        let work = Instant::now();
        if tick.elapsed() >= Duration::from_secs(1) {
            tick = Instant::now();
            let load = sampler.sample();
            let (hour, minute) = wxr_sys::local_time();
            wrist.set(&WristInfo {
                hour,
                minute,
                // Worst case for the budget: expanded with the load line (redrawn every second).
                expanded: true,
                load: Some(WristLoad {
                    game_fps: None,
                    target_fps: None,
                    cpu: load.cpu,
                    gpu: load.gpu,
                    own_gpu_ms: timer.last_ms(),
                }),
                ..WristInfo::default()
            });
            peak_ram = peak_ram.max(wxr_sys::working_set_bytes());
        }
        keyboard.set(&faces);
        for (p, _, _) in &screens {
            vr.place_panel(p, p.pose);
        }
        vr.place_panel(&keyboard_panel, keyboard_panel.pose);
        vr.place_panel(&wrist_panel, wrist_panel.pose);
        timer.begin(&gpu);
        let wrist_new = wrist.render().map(|px| gpu.update(&wrist_tex, px, WRIST_SIZE.0)).is_some();
        let kb_new = keyboard.render().map(|px| gpu.update(&kb_tex, px, wxr_ui::KEYBOARD_SIZE.0)).is_some();
        fresh.clear();
        for (p, tex, capture) in &mut screens {
            if capture.copy_latest(&gpu.context, tex).map_err(err)? == Frame::Copied {
                fresh.push(p.id);
            }
        }
        timer.end(&gpu);
        gpu.flush();
        if wrist_new {
            vr.set_texture_d3d11(51, raw(&wrist_tex))?;
        }
        if kb_new {
            vr.set_texture_d3d11(50, raw(&kb_tex))?;
        }
        for (p, tex, _) in &screens {
            if fresh.contains(&p.id) {
                vr.set_texture_d3d11(u64::from(p.id), raw(tex))?;
            }
        }
        let spent = work.elapsed();
        worst = worst.max(spent);
        if baseline.is_some() && spent > FRAME_BUDGET {
            overruns += 1;
        }
        frames += 1;
        vr.wait_frame(20);
    }
    let (t0, cpu0, ram0) = baseline.ok_or("no measurement window")?;
    let wall = t0.elapsed();
    let ram1 = wxr_sys::working_set_bytes();
    let growth = ram1.saturating_sub(ram0);
    let cpu = (wxr_sys::process_cpu_time() - cpu0).as_secs_f32() / wall.as_secs_f32() * 100.0;
    let gpu_ms = timer.last_ms();
    let mb = |b: u64| b as f64 / 1_048_576.0;
    println!(
        "      soak S1 ({} screens + keyboard + wrist), {:.0} min: {frames} frames, {overruns} overruns > 11.1 ms (worst {:.1} ms), CPU {cpu:.2}%, GPU {gpu_ms:.3} ms/frame, RAM {:.0} MB (peak {:.0}), growth {:.1} MB",
        screens.len(),
        wall.as_secs_f64() / 60.0,
        worst.as_secs_f64() * 1000.0,
        mb(ram1),
        mb(peak_ram),
        mb(growth),
    );
    let mut failures = Vec::new();
    if overruns > 0 {
        failures.push(format!("{overruns} frames where our work exceeded 11.1 ms"));
    }
    if growth >= MEMORY_GROWTH_BUDGET {
        failures.push(format!("memory grew {:.1} MB", mb(growth)));
    }
    if peak_ram >= RAM_BUDGET {
        failures.push(format!("RAM peak {:.0} MB", mb(peak_ram)));
    }
    if cpu >= CPU_BUDGET_PERCENT {
        failures.push(format!("CPU {cpu:.2}%"));
    }
    if gpu_ms >= GPU_BUDGET_MS {
        failures.push(format!("GPU {gpu_ms:.3} ms"));
    }
    if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
}
