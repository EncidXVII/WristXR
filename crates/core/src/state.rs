// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::math::Pose;
use crate::panel::{Hand, Panel};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const APP_DIR_NAME: &str = "WristXR";

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

/// Accent colour (Setup > Theme). Only the accent changes; greys, fonts and the warning red stay. Each is light
/// enough for dark text on it at 7:1 or better.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Accent {
    #[default]
    Lime,
    Mint,
    Aqua,
    Sky,
    Violet,
    Pink,
    Amber,
    White,
}

impl Accent {
    pub const ALL: [Self; 8] =
        [Self::Lime, Self::Mint, Self::Aqua, Self::Sky, Self::Violet, Self::Pink, Self::Amber, Self::White];

    /// 0xRRGGBB.
    pub fn rgb(self) -> u32 {
        match self {
            Self::Lime => 0xC6FF1A,
            Self::Mint => 0x5CF2A0,
            Self::Aqua => 0x3DF2E0,
            Self::Sky => 0x5CC8FF,
            Self::Violet => 0xB49AFF,
            Self::Pink => 0xFF7ADB,
            Self::Amber => 0xFFB627,
            Self::White => 0xF5F5F7,
        }
    }

    /// Fluent id of its name.
    pub fn name_id(self) -> &'static str {
        match self {
            Self::Lime => "theme-lime",
            Self::Mint => "theme-mint",
            Self::Aqua => "theme-aqua",
            Self::Sky => "theme-sky",
            Self::Violet => "theme-violet",
            Self::Pink => "theme-pink",
            Self::Amber => "theme-amber",
            Self::White => "theme-white",
        }
    }
}

/// What the closed watch shows (Watch > Layout). The open watch is the same in every layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchLayout {
    /// Time, L and R batteries, the song line.
    #[default]
    Standard,
    /// Time and date.
    Clock,
    /// Time, date and the song line.
    NoBatteries,
    /// Time and every battery as a number (L, R, lowest tracker).
    Batteries,
    /// Time, batteries, and a small two-line player.
    Music,
    /// Time, batteries, and game fps with CPU and GPU load.
    Performance,
    /// One slim bar: time, battery numbers, the song.
    Compact,
    /// Time, date, and game fps with CPU and GPU load: Performance with the date where the batteries were.
    Game,
    /// Time, date, and the small two-line player: Music with the date where the batteries were.
    Player,
}

impl WatchLayout {
    /// In the order Watch > Layout lists them.
    pub const ALL: [Self; 9] = [
        Self::Standard,
        Self::Clock,
        Self::NoBatteries,
        Self::Batteries,
        Self::Music,
        Self::Player,
        Self::Performance,
        Self::Game,
        Self::Compact,
    ];

    /// Its number in the watch view (see `WristView.layout`), fixed whatever the menu order.
    pub fn view_index(self) -> i32 {
        match self {
            Self::Standard => 0,
            Self::Clock => 1,
            Self::NoBatteries => 2,
            Self::Batteries => 3,
            Self::Music => 4,
            Self::Performance => 5,
            Self::Compact => 6,
            Self::Game => 7,
            Self::Player => 8,
        }
    }

    /// Shows fps, CPU and GPU when closed (whatever the FPS and load switch says).
    pub fn shows_load(self) -> bool {
        matches!(self, Self::Performance | Self::Game)
    }

    /// Shows the two-line player (back, play, next) when closed.
    pub fn shows_player(self) -> bool {
        matches!(self, Self::Music | Self::Player)
    }

    /// Batteries only when open; the date sits where they would be.
    pub fn date_beside(self) -> bool {
        matches!(self, Self::Clock | Self::NoBatteries | Self::Game | Self::Player)
    }

    /// Fluent ids of its name and one-line description.
    pub fn ids(self) -> (&'static str, &'static str) {
        match self {
            Self::Standard => ("layout-standard", "layout-standard-hint"),
            Self::Clock => ("layout-clock", "layout-clock-hint"),
            Self::NoBatteries => ("layout-no-batteries", "layout-no-batteries-hint"),
            Self::Batteries => ("layout-batteries", "layout-batteries-hint"),
            Self::Music => ("layout-music", "layout-music-hint"),
            Self::Performance => ("layout-performance", "layout-performance-hint"),
            Self::Compact => ("layout-compact", "layout-compact-hint"),
            Self::Game => ("layout-game", "layout-game-hint"),
            Self::Player => ("layout-player", "layout-player-hint"),
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
    pub wrist_layout: WatchLayout,
    pub accent: Accent,
    pub notify_from: NotifyFrom,
    /// Laser smoothing (tremor filter).
    pub smoothing: bool,
    /// Holding the trigger right-clicks too (A/X always does).
    pub hold_right_click: bool,
    /// Trigger-drag on a screen scrolls it (off: it drags, to select text).
    pub drag_scroll: bool,
    /// The watch was moved or tilted on its wrist: keep it as it is. Saved under its own name, so a spot kept by
    /// an older build (on the controller's top, or without its tilt) is dropped once.
    #[serde(rename = "watch_placed")]
    pub wrist_moved: bool,
    /// Where the head was when the layout was last saved. The next start brings the layout, as arranged, to where
    /// you then face: SteamVR may come back with the playspace moved (a playspace mover's offset is gone).
    pub layout_head: Option<Pose>,
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
            wrist_layout: WatchLayout::Standard,
            accent: Accent::Lime,
            notify_from: NotifyFrom::Discord,
            smoothing: true,
            hold_right_click: true,
            drag_scroll: false,
            wrist_moved: false,
            layout_head: None,
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
