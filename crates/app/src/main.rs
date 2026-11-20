// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

// No console window when started by double-click or by SteamVR; --selftest re-attaches to the caller's console.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod bindings;
mod gates;
#[cfg(windows)]
mod gates_input;
#[cfg(any(windows, target_os = "linux"))]
mod gates_menu;
#[cfg(windows)]
mod gates_vr;
mod import;
#[cfg(windows)]
#[path = "plat_win.rs"]
mod plat;
#[cfg(target_os = "linux")]
#[path = "plat_linux.rs"]
mod plat;
#[cfg(any(windows, target_os = "linux"))]
mod run;
#[cfg(any(windows, target_os = "linux"))]
mod session;
#[cfg(windows)]
mod tray;
#[cfg(target_os = "linux")]
#[path = "tray_linux.rs"]
mod tray;
#[cfg(any(windows, target_os = "linux"))]
mod uninstall;
mod update;

use std::process::ExitCode;

const REPORT: &str = "selftest-report.json";

fn main() -> ExitCode {
    // Physical pixels everywhere (capture, input, monitor rects) — must precede any window or monitor call.
    #[cfg(windows)]
    wxr_capture_win::init_process();
    if std::env::args().any(|a| a == "--selftest") {
        #[cfg(windows)]
        wxr_sys::attach_parent_console();
        return selftest();
    }
    #[cfg(any(windows, target_os = "linux"))]
    if std::env::args().any(|a| a == "--uninstall") {
        return uninstall::run();
    }
    // A second copy would fight the first over the controllers and the notification port.
    #[cfg(any(windows, target_os = "linux"))]
    let Some(_instance) = wxr_sys::single_instance() else {
        fatal("WristXR is already running. Use its tray icon to open the menu or quit.");
        return ExitCode::FAILURE;
    };
    #[cfg(any(windows, target_os = "linux"))]
    let result = run::run();
    #[cfg(not(any(windows, target_os = "linux")))]
    let result: Result<(), String> = Err("WristXR runs on Windows and Linux.".into());
    let code = match result {
        Ok(()) => 0,
        Err(e) => {
            fatal(&e);
            1
        }
    };
    // Settings are saved; skip library teardown, which can hang with background threads still running.
    #[cfg(any(windows, target_os = "linux"))]
    wxr_sys::terminate(code);
    #[cfg(not(any(windows, target_os = "linux")))]
    ExitCode::from(code as u8)
}

/// A startup failure the user must see (e.g. SteamVR missing): a dialog (on Linux through zenity or kdialog when
/// installed), and stderr.
fn fatal(message: &str) {
    eprintln!("{message}");
    #[cfg(any(windows, target_os = "linux"))]
    wxr_sys::error_dialog("WristXR", message);
}

fn selftest() -> ExitCode {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut results: Vec<_> = gates::GATES.iter().map(|(name, gate)| (*name, gate())).collect();
    #[cfg(any(windows, target_os = "linux"))]
    results.push(("4c first run + menu", gates_menu::first_run_and_menu()));
    #[cfg(windows)]
    if std::env::args().any(|a| a == "--vr") {
        results.push(("1+2+5+P live overlay", gates_vr::overlay_live()));
    }
    #[cfg(windows)]
    if let Some(minutes) = std::env::args().skip_while(|a| a != "--soak").nth(1).and_then(|m| m.parse().ok()) {
        results.push(("8+P soak S1", gates_vr::soak(minutes)));
    }
    #[cfg(windows)]
    if std::env::args().any(|a| a == "--input") {
        results.push(("3+4+5b live input + pop-out", gates_input::input_live()));
    }
    for (name, r) in &results {
        match r {
            Ok(()) => println!("PASS  {name}"),
            Err(e) => println!("FAIL  {name}: {e}"),
        }
    }
    let report = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "system": system_info(),
        "gates": results
            .iter()
            .map(|(n, r)| serde_json::json!({ "gate": n, "ok": r.is_ok(), "detail": r.as_ref().err() }))
            .collect::<Vec<_>>(),
    });
    let written = serde_json::to_vec_pretty(&report)
        .map_err(|e| e.to_string())
        .and_then(|b| std::fs::write(REPORT, b).map_err(|e| e.to_string()));
    match written.and_then(|()| zip_report()) {
        Ok(path) => println!("report: {path}"),
        Err(e) => {
            eprintln!("could not write {REPORT}: {e}");
            return ExitCode::FAILURE;
        }
    }
    if results.iter().all(|(_, r)| r.is_ok()) { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Hardware facts that help triage a report (no user names, paths or identifiers).
fn system_info() -> serde_json::Value {
    #[cfg(windows)]
    let (gpus, monitors) = (
        plat::adapter_names(),
        plat::monitors().iter().map(|m| format!("{}x{}", m.rect.w, m.rect.h)).collect::<Vec<_>>(),
    );
    // Linux: no monitor list here, since on Wayland asking for one opens the screen-sharing prompt.
    #[cfg(target_os = "linux")]
    let (gpus, monitors): (Vec<String>, Vec<String>) = (plat::adapter_names(), Vec::new());
    #[cfg(not(any(windows, target_os = "linux")))]
    let (gpus, monitors): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    serde_json::json!({ "gpus": gpus, "monitors": monitors, "cpus": std::thread::available_parallelism().map_or(0, |n| n.get()) })
}

/// Packs the report as `selftest-report.zip` for attaching to an issue. Falls back to the JSON if zipping fails.
fn zip_report() -> Result<String, String> {
    const ZIP: &str = "selftest-report.zip";
    // Windows: System32's bsdtar writes real zips (`-a` by extension); a GNU tar earlier on PATH would not.
    #[cfg(windows)]
    let zipped = std::env::var_os("SystemRoot").is_some_and(|root| {
        std::process::Command::new(
            std::path::Path::new(&root).join(["System32", "tar.exe"].iter().collect::<std::path::PathBuf>()),
        )
        .args(["-a", "-c", "-f", ZIP, REPORT])
        .status()
        .is_ok_and(|s| s.success())
    });
    #[cfg(not(windows))]
    let zipped = std::process::Command::new("zip").args(["-q", ZIP, REPORT]).status().is_ok_and(|s| s.success());
    if zipped {
        let _ = std::fs::remove_file(REPORT);
        Ok(ZIP.to_owned())
    } else {
        Ok(REPORT.to_owned())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_gates_pass() {
        let failures: Vec<String> =
            crate::gates::GATES.iter().filter_map(|(n, g)| g().err().map(|e| format!("{n}: {e}"))).collect();
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
