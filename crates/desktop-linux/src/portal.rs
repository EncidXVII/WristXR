// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wayland: one desktop-portal session for screens, windows and input together (RemoteDesktop with ScreenCast
//! sources), so the user answers one prompt. Its restore token is kept, so later runs start without asking again.
//! The session lives on its own thread; input reaches it over a channel.
use crate::pipewire::PipeWire;
use ashpd::desktop::PersistMode;
use ashpd::desktop::remote_desktop::{Axis, DeviceType, KeyState, RemoteDesktop};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::enumflags2::BitFlags;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// A shared screen or window as the portal describes it, in its own logical pixels.
#[derive(Clone, Debug)]
pub struct Source {
    pub node: u32,
    pub monitor: bool,
    pub position: (i32, i32),
    pub size: (i32, i32),
}

pub enum Input {
    /// Absolute pointer position inside one stream, in that stream's logical pixels.
    Move { node: u32, x: f64, y: f64 },
    /// Linux button codes (BTN_LEFT 0x110, BTN_RIGHT 0x111).
    Button { code: i32, down: bool },
    /// Wheel notches, positive up.
    Scroll(i32),
    /// evdev key code.
    Key { code: i32, down: bool },
}

pub struct Portal {
    pub sources: Vec<Source>,
    input: Mutex<Sender<Input>>,
}

static PORTAL: OnceLock<Option<Portal>> = OnceLock::new();

/// Waylands's session, started (and the prompt shown, the first time) on first use; `None` when the portal is
/// missing or the user said no.
pub fn get() -> Option<&'static Portal> {
    PORTAL.get_or_init(start).as_ref()
}

fn token_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("wristxr").join("portal-token"))
}

fn start() -> Option<Portal> {
    let (ready_tx, ready_rx) = channel::<Result<Vec<Source>, String>>();
    let (input_tx, input_rx) = channel::<Input>();
    let spawned = std::thread::Builder::new()
        .name("portal".into())
        .spawn(move || futures_lite::future::block_on(session(ready_tx, input_rx)));
    if let Err(e) = spawned {
        tracing::warn!(%e, "portal thread not started");
        return None;
    }
    // The prompt waits for the user; long enough for them to take the headset off and answer.
    match ready_rx.recv_timeout(Duration::from_secs(300)) {
        Ok(Ok(sources)) => {
            tracing::info!(n = sources.len(), "screen sharing started");
            Some(Portal { sources, input: Mutex::new(input_tx) })
        }
        Ok(Err(e)) => {
            tracing::warn!(%e, "screen sharing not started");
            None
        }
        Err(_) => {
            tracing::warn!("screen sharing prompt not answered");
            None
        }
    }
}

async fn session(ready: Sender<Result<Vec<Source>, String>>, input: Receiver<Input>) {
    let token_path = token_file();
    let token = token_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).filter(|t| !t.trim().is_empty());
    let result = async {
        let remote = RemoteDesktop::new().await.map_err(|e| e.to_string())?;
        let cast = Screencast::new().await.map_err(|e| e.to_string())?;
        let session = remote.create_session().await.map_err(|e| e.to_string())?;
        remote
            .select_devices(
                &session,
                DeviceType::Keyboard | DeviceType::Pointer,
                token.as_deref().map(str::trim),
                PersistMode::ExplicitlyRevoked,
            )
            .await
            .map_err(|e| e.to_string())?;
        // WristXR draws its own laser cursor, so the shared pixels leave the system one out.
        let cursor = match cast.available_cursor_modes().await {
            Ok(m) if m.contains(CursorMode::Hidden) => CursorMode::Hidden,
            _ => CursorMode::Embedded,
        };
        cast.select_sources(
            &session,
            cursor,
            BitFlags::from(SourceType::Monitor) | SourceType::Window,
            true,
            None,
            PersistMode::DoNot,
        )
        .await
        .map_err(|e| e.to_string())?;
        let started =
            remote.start(&session, None).await.map_err(|e| e.to_string())?.response().map_err(|e| e.to_string())?;
        if let (Some(path), Some(t)) = (&token_path, started.restore_token()) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, t);
        }
        let sources: Vec<Source> = started
            .streams()
            .unwrap_or_default()
            .iter()
            .map(|s| Source {
                node: s.pipe_wire_node_id(),
                monitor: s.source_type() != Some(SourceType::Window),
                position: s.position().unwrap_or((0, 0)),
                size: s.size().unwrap_or((0, 0)),
            })
            .collect();
        let fd = cast.open_pipe_wire_remote(&session).await.map_err(|e| e.to_string())?;
        if PipeWire::connect(fd).is_none() {
            return Err("PipeWire did not connect".to_owned());
        }
        Ok::<_, String>((remote, session, sources))
    }
    .await;
    let (remote, session, sources) = match result {
        Ok(r) => r,
        Err(e) => {
            // A stale token makes the portal refuse; forget it so the next run asks afresh.
            if let Some(p) = &token_path {
                let _ = std::fs::remove_file(p);
            }
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(sources));
    // Input requests until the app goes (the sender drops); the session ends with this thread.
    while let Ok(cmd) = input.recv() {
        let state = |down: bool| if down { KeyState::Pressed } else { KeyState::Released };
        let r = match cmd {
            Input::Move { node, x, y } => remote.notify_pointer_motion_absolute(&session, node, x, y).await,
            Input::Button { code, down } => remote.notify_pointer_button(&session, code, state(down)).await,
            // Wayland's discrete steps run the other way from the wheel: positive is down.
            Input::Scroll(n) => remote.notify_pointer_axis_discrete(&session, Axis::Vertical, -n).await,
            Input::Key { code, down } => remote.notify_keyboard_keycode(&session, code, state(down)).await,
        };
        if let Err(e) = r {
            tracing::debug!(%e, "portal input dropped");
        }
    }
}

impl Portal {
    pub fn send(&self, input: Input) {
        if let Ok(tx) = self.input.lock() {
            let _ = tx.send(input);
        }
    }
}

/// True when this is a Wayland session, where X11 cannot see (or reach) native windows.
pub fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
}
