//! `DesktopVR.exe --uninstall` (and `Uninstall DesktopVR.cmd` beside it): there is no installer, so this undoes
//! what running it set up. It takes DesktopVR off SteamVR's list (no more start with SteamVR) and deletes its
//! settings. The folder it runs from is the user's to delete: a running program cannot remove itself.
//! With `--quiet` it asks nothing and shows nothing (for scripts); the exit code says how it went.
use crate::bindings::APP_KEY;
use dvr_core::state::State;
use std::path::Path;
use std::process::ExitCode;

const TITLE: &str = "Uninstall DesktopVR";

pub fn run() -> ExitCode {
    let quiet = std::env::args().any(|a| a == "--quiet");
    let error = |m: &str| {
        if quiet {
            eprintln!("{m}");
        } else {
            dvr_sys::error_dialog(TITLE, m);
        }
    };
    let Some(config) = State::default_path().and_then(|p| p.parent().map(Path::to_path_buf)) else {
        error("No settings folder found.");
        return ExitCode::FAILURE;
    };
    // `...\DesktopVR\config`: the whole `DesktopVR` folder goes, and nothing above it.
    let Some(root) = config.parent().filter(|r| r.file_name().is_some_and(|n| n == "DesktopVR")) else {
        error(&format!("Unexpected settings folder: {}", config.display()));
        return ExitCode::FAILURE;
    };
    if !quiet
        && !dvr_sys::ask_dialog(
            TITLE,
            "Remove DesktopVR from SteamVR and delete its settings and layout?\n\nAfterwards, delete the DesktopVR \
         folder to finish.",
        )
    {
        return ExitCode::SUCCESS;
    }
    // While it runs it would put itself back on SteamVR's list and write its settings again.
    let Some(_instance) = dvr_sys::single_instance() else {
        error("DesktopVR is running. Quit it first (tray icon > Quit), then uninstall.");
        return ExitCode::FAILURE;
    };
    let mut problems = Vec::new();
    if let Err(e) = dvr_vr::remove_app(&config.join("desktopvr.vrmanifest"), APP_KEY) {
        problems.push(format!("SteamVR: {e}"));
    }
    match std::fs::remove_dir_all(root) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => problems.push(format!("{}: {e}", root.display())),
        _ => {}
    }
    if problems.is_empty() {
        if quiet {
            return ExitCode::SUCCESS;
        }
        dvr_sys::info_dialog(
            TITLE,
            "DesktopVR is off SteamVR's list and its settings are deleted.\n\nDelete the DesktopVR folder to finish.",
        );
        ExitCode::SUCCESS
    } else {
        error(&format!("Not everything could be removed:\n\n{}", problems.join("\n")));
        ExitCode::FAILURE
    }
}
