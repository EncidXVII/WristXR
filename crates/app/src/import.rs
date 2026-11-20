// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Switch-over import: reads the user's own XSOverlay settings file (plain JSON) on first run.
//! Only keys that map to our features are read; identifiers and analytics fields are ignored.
//! Format observed from a user settings file (VersionNumber 680); never from the app's binaries.
use serde::Deserialize;
use wxr_core::panel::Hand;
use wxr_core::state::State;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Imported {
    pub dominant: Option<Hand>,
    pub typing_sounds: Option<bool>,
    pub curved: Option<bool>,
    pub opacity: Option<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XsSettings {
    /// Observed default 0 for a right-handed user; 1 is taken as left. First run still asks.
    dominant_hand: Option<i64>,
    keyboard_typing_sounds: Option<bool>,
    curved_overlays: Option<bool>,
    overlay_default_opacity: Option<f32>,
}

/// `%USERPROFILE%\AppData\LocalLow\Xiexe\XSOverlay\config\overlaySettings.json` (XSOverlay is Windows-only).
#[cfg(windows)]
pub fn xsoverlay_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("USERPROFILE")?;
    Some(std::path::PathBuf::from(home).join(r"AppData\LocalLow\Xiexe\XSOverlay\config\overlaySettings.json"))
}

pub fn xsoverlay(json: &str) -> Option<Imported> {
    let s: XsSettings = serde_json::from_str(json).ok()?;
    Some(Imported {
        dominant: s.dominant_hand.and_then(|h| match h {
            0 => Some(Hand::Right),
            1 => Some(Hand::Left),
            _ => None,
        }),
        typing_sounds: s.keyboard_typing_sounds,
        curved: s.curved_overlays,
        opacity: s.overlay_default_opacity.filter(|o| (0.1..=1.0).contains(o)),
    })
}

/// Reads the user's XSOverlay settings if present and readable.
#[cfg(windows)]
pub fn detect() -> Option<Imported> {
    xsoverlay(&std::fs::read_to_string(xsoverlay_path()?).ok()?)
}

/// XSOverlay is Windows-only: nothing to import elsewhere.
#[cfg(not(windows))]
pub fn detect() -> Option<Imported> {
    None
}

pub fn apply(state: &mut State, imp: &Imported) {
    if let Some(h) = imp.dominant {
        state.dominant = h;
    }
    if let Some(t) = imp.typing_sounds {
        state.typing_sounds = t;
    }
    if let Some(c) = imp.curved {
        state.default_curved = c;
    }
    if let Some(o) = imp.opacity {
        state.default_opacity = o;
    }
}
