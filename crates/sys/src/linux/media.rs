// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! What is playing, and its buttons, through MPRIS: the D-Bus interface Linux media players share (Spotify,
//! browsers, VLC, mpv with its plugin, ...). Polled once a second on its own thread, like the Windows version.
use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::blocking::Connection;
use zbus::blocking::proxy::Builder;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{Array, ObjectPath, OwnedValue};

#[derive(Clone, Debug, PartialEq)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    /// Player name ("Spotify").
    pub app: String,
    pub playing: bool,
    /// Seconds into the song when `at` was taken; `duration` 0 = unknown (a stream).
    pub position: f32,
    pub duration: f32,
    pub shuffle: bool,
    pub at: Instant,
}

impl MediaInfo {
    /// Position now, counting on from `at` while playing.
    pub fn position_now(&self) -> f32 {
        let p = self.position + if self.playing { self.at.elapsed().as_secs_f32() } else { 0.0 };
        if self.duration > 0.0 { p.min(self.duration) } else { p }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
    ToggleShuffle,
    /// Seconds into the song.
    Seek(f32),
    /// The player's own volume, 0..1 (Linux has no one system mixer API).
    Volume(f32),
    ToggleMute,
}

#[derive(Default)]
struct Shared {
    media: Option<MediaInfo>,
    volume: Option<(f32, bool)>,
}

pub struct Media {
    shared: Arc<Mutex<Shared>>,
    commands: Sender<MediaCommand>,
}

const POLL: Duration = Duration::from_secs(1);
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PATH: &str = "/org/mpris/MediaPlayer2";

/// "org.mpris.MediaPlayer2.spotify" -> "Spotify"; Firefox's "firefox.instance_1_23" -> "Firefox".
pub fn app_name(id: &str) -> String {
    let short = id.strip_prefix("org.mpris.MediaPlayer2.").unwrap_or(id);
    let short = short.split('.').next().unwrap_or(short);
    let mut c = short.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

struct Player {
    name: String,
    proxy: zbus::blocking::Proxy<'static>,
    /// Volume to go back to after a mute.
    unmuted: Option<f64>,
}

fn proxy(conn: &Connection, name: &str, interface: &'static str) -> Option<zbus::blocking::Proxy<'static>> {
    Builder::new(conn)
        .destination(name.to_owned())
        .ok()?
        .path(PATH)
        .ok()?
        .interface(interface)
        .ok()?
        .cache_properties(CacheProperties::No)
        .build()
        .ok()
}

fn text(v: &OwnedValue) -> Option<String> {
    v.downcast_ref::<&str>().ok().map(str::to_owned)
}

fn texts(v: &OwnedValue) -> Option<String> {
    let a = v.downcast_ref::<Array<'_>>().ok()?;
    let parts: Vec<String> = a.iter().filter_map(|x| x.downcast_ref::<&str>().ok().map(str::to_owned)).collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn micros(v: &OwnedValue) -> Option<f32> {
    v.downcast_ref::<i64>()
        .ok()
        .map(|u| u as f32)
        .or_else(|| v.downcast_ref::<u64>().ok().map(|u| u as f32))
        .map(|u| u / 1e6)
}

/// A player's bus name, its Player interface, the song's metadata, and whether it is playing.
type Found = (String, zbus::blocking::Proxy<'static>, HashMap<String, OwnedValue>, bool);

/// The player to show: the first one playing, else the first one with a song loaded.
fn current(conn: &Connection) -> Option<Found> {
    let names = zbus::blocking::fdo::DBusProxy::new(conn).ok()?.list_names().ok()?;
    let mut paused = None;
    for n in names.iter().map(|n| n.as_str().to_owned()).filter(|n| n.starts_with("org.mpris.MediaPlayer2.")) {
        let Some(p) = proxy(conn, &n, PLAYER) else { continue };
        let status: String = p.get_property("PlaybackStatus").unwrap_or_default();
        let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").unwrap_or_default();
        if status == "Playing" {
            return Some((n, p, meta, true));
        }
        if paused.is_none() && !meta.is_empty() {
            paused = Some((n, p, meta, false));
        }
    }
    paused
}

/// The player's volume (0..1) and whether it is muted.
type Volume = Option<(f32, bool)>;

fn read(conn: &Connection) -> Option<(Player, MediaInfo, Volume)> {
    let (name, p, meta, playing) = current(conn)?;
    let title = meta.get("xesam:title").and_then(text).unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    let app = proxy(conn, &name, "org.mpris.MediaPlayer2")
        .and_then(|i| i.get_property::<String>("Identity").ok())
        .unwrap_or_else(|| app_name(&name));
    let info = MediaInfo {
        title,
        artist: meta.get("xesam:artist").and_then(texts).unwrap_or_default(),
        app,
        playing,
        position: p.get_property::<i64>("Position").map(|u| u as f32 / 1e6).unwrap_or(0.0),
        duration: meta.get("mpris:length").and_then(micros).unwrap_or(0.0),
        shuffle: p.get_property::<bool>("Shuffle").unwrap_or(false),
        at: Instant::now(),
    };
    let volume = p.get_property::<f64>("Volume").ok().map(|v| (v.clamp(0.0, 1.0) as f32, v <= 0.0));
    Some((Player { name, proxy: p, unmuted: None }, info, volume))
}

fn control(player: &mut Player, cmd: MediaCommand) {
    let p = &player.proxy;
    let r = match cmd {
        MediaCommand::PlayPause => p.call_method("PlayPause", &()).map(drop),
        MediaCommand::Next => p.call_method("Next", &()).map(drop),
        MediaCommand::Previous => p.call_method("Previous", &()).map(drop),
        MediaCommand::ToggleShuffle => {
            let on: bool = p.get_property("Shuffle").unwrap_or(false);
            p.set_property("Shuffle", !on).map_err(Into::into)
        }
        MediaCommand::Seek(s) => {
            let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").unwrap_or_default();
            match meta.get("mpris:trackid").and_then(|v| v.downcast_ref::<ObjectPath<'_>>().ok()) {
                Some(track) => p.call_method("SetPosition", &(track, (f64::from(s) * 1e6) as i64)).map(drop),
                None => Ok(()),
            }
        }
        MediaCommand::Volume(f) => p.set_property("Volume", f64::from(f.clamp(0.0, 1.0))).map_err(Into::into),
        MediaCommand::ToggleMute => {
            let now: f64 = p.get_property("Volume").unwrap_or(1.0);
            let to = if now > 0.0 {
                player.unmuted = Some(now);
                0.0
            } else {
                player.unmuted.take().unwrap_or(1.0)
            };
            p.set_property("Volume", to).map_err(Into::into)
        }
    };
    if let Err(e) = r {
        eprintln!("{}: {e}", player.name);
    }
}

impl Media {
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (commands, rx) = mpsc::channel();
        let out = shared.clone();
        let _ = std::thread::Builder::new().name("media".into()).spawn(move || {
            let Ok(conn) = Connection::session() else { return };
            let mut player: Option<Player> = None;
            loop {
                match read(&conn) {
                    Some((p, info, volume)) => {
                        // Keep the mute memory while it is the same player.
                        let unmuted = player.as_ref().filter(|old| old.name == p.name).and_then(|old| old.unmuted);
                        player = Some(Player { unmuted, ..p });
                        if let Ok(mut s) = out.lock() {
                            s.media = Some(info);
                            s.volume = volume;
                        }
                    }
                    None => {
                        player = None;
                        if let Ok(mut s) = out.lock() {
                            s.media = None;
                            s.volume = None;
                        }
                    }
                }
                match rx.recv_timeout(POLL) {
                    Ok(cmd) => {
                        if let Some(p) = player.as_mut() {
                            control(p, cmd);
                            while let Ok(more) = rx.try_recv() {
                                control(p, more);
                            }
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Self { shared, commands }
    }

    pub fn info(&self) -> Option<MediaInfo> {
        self.shared.lock().ok().and_then(|s| s.media.clone())
    }

    pub fn volume(&self) -> Option<(f32, bool)> {
        self.shared.lock().ok().and_then(|s| s.volume)
    }

    pub fn send(&self, cmd: MediaCommand) {
        if let (MediaCommand::Volume(f), Ok(mut s)) = (cmd, self.shared.lock())
            && let Some(v) = s.volume.as_mut()
        {
            v.0 = f.clamp(0.0, 1.0);
        }
        let _ = self.commands.send(cmd);
    }
}
