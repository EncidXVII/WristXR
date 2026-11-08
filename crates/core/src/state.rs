use crate::panel::{Hand, Panel};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const APP_DIR_NAME: &str = "DesktopVR";

/// Discord and the popular Discord clients (Vesktop / Vencord, Equibop, Legcord, WebCord, Dorion), by app name.
const DISCORD_APPS: [&str; 7] = ["discord", "vesktop", "vencord", "equibop", "legcord", "webcord", "dorion"];

/// True for Discord or a Discord client; their first notification line already names the sender.
pub fn is_discord_app(app: &str) -> bool {
    let app = app.to_lowercase();
    DISCORD_APPS.iter().any(|d| app.contains(d))
}

/// Which other apps' Windows notifications show on the watch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotifyFrom {
    #[default]
    Discord,
    AllApps,
    Off,
}

impl NotifyFrom {
    pub fn next(self) -> Self {
        match self {
            Self::Discord => Self::AllApps,
            Self::AllApps => Self::Off,
            Self::Off => Self::Discord,
        }
    }

    /// Whether a notification from the app with this display name should show.
    pub fn accepts(self, app: &str) -> bool {
        match self {
            Self::Discord => is_discord_app(app),
            Self::AllApps => true,
            Self::Off => false,
        }
    }
}

/// The single auto-restored layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub dominant: Hand,
    pub panels: Vec<Panel>,
    pub chatbox: bool,
    pub typing_sounds: bool,
    /// First-run flow finished (hand, main screen, keyboard test).
    pub onboarded: bool,
    /// Defaults for newly added screens (can come from the switch-over import).
    pub default_curved: bool,
    pub default_opacity: f32,
    /// Send the current track to the VRChat chatbox when it changes.
    pub media_to_chatbox: bool,
    pub haptics: bool,
    pub update_check: bool,
    /// The watch shows only while you look at it.
    pub wrist_fade: bool,
    pub wrist_battery: bool,
    /// Game fps and CPU / GPU load on the watch.
    pub wrist_load: bool,
    pub wrist_media: bool,
    pub notify_from: NotifyFrom,
    /// Laser smoothing (tremor filter).
    pub smoothing: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            dominant: Hand::Right,
            panels: Vec::new(),
            chatbox: false,
            typing_sounds: false,
            onboarded: false,
            default_curved: false,
            default_opacity: 1.0,
            media_to_chatbox: false,
            haptics: true,
            update_check: true,
            wrist_fade: true,
            wrist_battery: true,
            wrist_load: false,
            wrist_media: true,
            notify_from: NotifyFrom::Discord,
            smoothing: true,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_clients_count_as_discord() {
        for app in ["Discord", "Discord PTB", "Vesktop", "Equibop", "Legcord", "WebCord"] {
            assert!(NotifyFrom::Discord.accepts(app), "{app}");
        }
        assert!(!NotifyFrom::Discord.accepts("Steam"));
        assert!(NotifyFrom::AllApps.accepts("Steam") && !NotifyFrom::Off.accepts("Vesktop"));
    }
}
