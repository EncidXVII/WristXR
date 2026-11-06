use crate::panel::{Hand, Panel};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const APP_DIR_NAME: &str = "DesktopVR";

/// The single auto-restored layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub dominant: Hand,
    pub panels: Vec<Panel>,
    pub chatbox: bool,
    pub typing_sounds: bool,
}

impl Default for State {
    fn default() -> Self {
        Self { dominant: Hand::Right, panels: Vec::new(), chatbox: false, typing_sounds: false }
    }
}

impl State {
    pub fn default_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", APP_DIR_NAME).map(|d| d.config_dir().join("state.json"))
    }

    pub fn next_id(&self) -> u32 {
        self.panels.iter().map(|p| p.id + 1).max().unwrap_or(1)
    }

    /// Missing or corrupt file yields defaults; the app must always start.
    pub fn load(path: &Path) -> Self {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!(%e, "state file unreadable, using defaults");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Atomic: write a temp file, then rename over the old one.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(io::Error::other)?)?;
        fs::rename(tmp, path)
    }
}
