//! SteamVR action manifest and default bindings, embedded so the app runs from any folder.
use std::io;
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = include_str!("../../../bindings/actions.json");

pub const FILES: &[(&str, &str)] = &[
    ("actions.json", MANIFEST),
    ("bindings_knuckles.json", include_str!("../../../bindings/bindings_knuckles.json")),
    ("bindings_oculus_touch.json", include_str!("../../../bindings/bindings_oculus_touch.json")),
    ("bindings_vive_controller.json", include_str!("../../../bindings/bindings_vive_controller.json")),
    ("bindings_holographic_controller.json", include_str!("../../../bindings/bindings_holographic_controller.json")),
    ("bindings_frame_controller.json", include_str!("../../../bindings/bindings_frame_controller.json")),
];

/// Writes the files into `dir` (only when changed) and returns the manifest path.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn install(dir: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    for (name, text) in FILES {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(*text) {
            std::fs::write(&path, text)?;
        }
    }
    Ok(dir.join("actions.json"))
}
