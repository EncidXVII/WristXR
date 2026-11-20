// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! `WristXR.exe --uninstall` (and `Uninstall WristXR.cmd` beside it): there is no installer, so this undoes
//! what running it set up. It takes WristXR off SteamVR's list (no more start with SteamVR) and deletes its
//! settings. The folder it runs from is the user's to delete: a running program cannot remove itself.
//! With `--quiet` it asks nothing and shows nothing (for scripts); the exit code says how it went.
use crate::bindings::APP_KEY;
use std::path::Path;
use std::process::ExitCode;
use wxr_core::state::State;

const TITLE: &str = "Uninstall WristXR";

pub fn run() -> ExitCode {
    let quiet = std::env::args().any(|a| a == "--quiet");
    let error = |m: &str| {
        if quiet {
            eprintln!("{m}");
        } else {
            wxr_sys::error_dialog(TITLE, m);
        }
    };
    let Some(config) = State::default_path().and_then(|p| p.parent().map(Path::to_path_buf)) else {
        error("No settings folder found.");
        return ExitCode::FAILURE;
    };
    // Windows: `...\WristXR\config`, so the whole `WristXR` folder goes; Linux: `~/.config/wristxr` itself.
    // Nothing above it, ever.
    let root = if cfg!(windows) { config.parent() } else { Some(config.as_path()) };
    let name = if cfg!(windows) { "WristXR" } else { "wristxr" };
    let Some(root) = root.filter(|r| r.file_name().is_some_and(|n| n == name)) else {
        error(&format!("Unexpected settings folder: {}", config.display()));
        return ExitCode::FAILURE;
    };
    if !quiet
        && !wxr_sys::ask_dialog(
            TITLE,
            "Remove WristXR from SteamVR and delete its settings and layout?\n\nAfterwards, delete the WristXR \
         folder to finish.",
        )
    {
        return ExitCode::SUCCESS;
    }
    // While it runs it would put itself back on SteamVR's list and write its settings again.
    let Some(_instance) = wxr_sys::single_instance() else {
        error("WristXR is running. Quit it first (tray icon > Quit), then uninstall.");
        return ExitCode::FAILURE;
    };
    let mut problems = Vec::new();
    if let Err(e) = wxr_vr::remove_app(&config.join("wristxr.vrmanifest"), APP_KEY) {
        problems.push(format!("SteamVR: {e}"));
    }
    match std::fs::remove_dir_all(root) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => problems.push(format!("{}: {e}", root.display())),
        _ => {}
    }
    // Linux: the screen-sharing permission the portal gave (asked again on the next install).
    #[cfg(target_os = "linux")]
    if let Some(state) = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
    {
        let _ = std::fs::remove_dir_all(state.join("wristxr"));
    }
    if problems.is_empty() {
        if quiet {
            return ExitCode::SUCCESS;
        }
        wxr_sys::info_dialog(
            TITLE,
            "WristXR is off SteamVR's list and its settings are deleted.\n\nDelete the WristXR folder to finish.",
        );
        ExitCode::SUCCESS
    } else {
        error(&format!("Not everything could be removed:\n\n{}", problems.join("\n")));
        ExitCode::FAILURE
    }
}
