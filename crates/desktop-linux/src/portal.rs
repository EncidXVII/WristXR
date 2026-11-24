// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wayland: one desktop-portal session for screens, windows and input together (RemoteDesktop with ScreenCast
//! sources), so the user answers one prompt. Its restore token is kept, so later runs start without asking again.
//! The session lives on its own thread; input reaches it over a channel.
//!
//! Desktops whose portal has no RemoteDesktop (the wlroots and Hyprland portals) get a ScreenCast session alone:
//! the screens show, view-only, and `Portal::view_only` says so.
use crate::pipewire::PipeWire;
use ashpd::desktop::PersistMode;
use ashpd::desktop::remote_desktop::{Axis, DeviceType, KeyState, RemoteDesktop};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType, Stream};
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
    /// Screens shared without keyboard and pointer: the desktop's portal has no RemoteDesktop.
    pub view_only: bool,
    input: Mutex<Sender<Input>>,
}

static PORTAL: OnceLock<Option<Portal>> = OnceLock::new();

/// Waylands's session, started (and the prompt shown, the first time) on first use; `None` when the portal is
/// missing or the user said no.
pub fn get() -> Option<&'static Portal> {
    PORTAL.get_or_init(start).as_ref()
}

/// Where a session's restore token is kept: `portal-token` (screens and input), `portal-token-view` (screens only).
fn token_file(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("wristxr").join(name))
}

fn read_token(path: Option<&PathBuf>) -> Option<String> {
    path.and_then(|p| std::fs::read_to_string(p).ok()).map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
}

fn keep_token(path: Option<&PathBuf>, token: Option<&str>) {
    if let (Some(path), Some(t)) = (path, token) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, t);
    }
}

/// A stale token makes the portal refuse; forgetting it lets the next run ask afresh.
fn forget_token(path: Option<&PathBuf>) {
    if let Some(p) = path {
        let _ = std::fs::remove_file(p);
    }
}

fn start() -> Option<Portal> {
    let (ready_tx, ready_rx) = channel::<Ready>();
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
        Ok(Ok((sources, view_only))) => {
            tracing::info!(n = sources.len(), view_only, "screen sharing started");
            Some(Portal { sources, view_only, input: Mutex::new(input_tx) })
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

/// What the session thread reports once: the shared sources and whether they are view-only, or why it failed.
type Ready = Result<(Vec<Source>, bool), String>;

/// Why the full session did not start: the portal lacks RemoteDesktop (share screens alone), or anything else.
enum Failed {
    NoRemoteDesktop(String),
    Other(String),
}

fn other(e: impl std::fmt::Display) -> Failed {
    Failed::Other(e.to_string())
}

fn sources(streams: &[Stream]) -> Vec<Source> {
    streams
        .iter()
        .map(|s| Source {
            node: s.pipe_wire_node_id(),
            monitor: s.source_type() != Some(SourceType::Window),
            position: s.position().unwrap_or((0, 0)),
            size: s.size().unwrap_or((0, 0)),
        })
        .collect()
}

/// WristXR draws its own laser cursor, so the shared pixels leave the system one out.
async fn cursor_mode(cast: &Screencast<'_>) -> CursorMode {
    match cast.available_cursor_modes().await {
        Ok(m) if m.contains(CursorMode::Hidden) => CursorMode::Hidden,
        _ => CursorMode::Embedded,
    }
}

async fn session(ready: Sender<Ready>, input: Receiver<Input>) {
    let token_path = token_file("portal-token");
    let token = read_token(token_path.as_ref());
    let full = async {
        // Without the interface the portal frontend refuses here, before any prompt is shown.
        let remote = RemoteDesktop::new().await.map_err(|e| Failed::NoRemoteDesktop(e.to_string()))?;
        let cast = Screencast::new().await.map_err(other)?;
        let session = remote.create_session().await.map_err(|e| Failed::NoRemoteDesktop(e.to_string()))?;
        remote
            .select_devices(
                &session,
                DeviceType::Keyboard | DeviceType::Pointer,
                token.as_deref(),
                PersistMode::ExplicitlyRevoked,
            )
            .await
            .map_err(other)?;
        let cursor = cursor_mode(&cast).await;
        cast.select_sources(
            &session,
            cursor,
            BitFlags::from(SourceType::Monitor) | SourceType::Window,
            true,
            None,
            PersistMode::DoNot,
        )
        .await
        .map_err(other)?;
        let started = remote.start(&session, None).await.map_err(other)?.response().map_err(other)?;
        keep_token(token_path.as_ref(), started.restore_token());
        let sources = sources(started.streams().unwrap_or_default());
        let fd = cast.open_pipe_wire_remote(&session).await.map_err(other)?;
        if PipeWire::connect(fd).is_none() {
            return Err(Failed::Other("PipeWire did not connect".to_owned()));
        }
        Ok((remote, session, sources))
    }
    .await;
    let (remote, session, sources) = match full {
        Ok(r) => r,
        Err(Failed::NoRemoteDesktop(e)) => {
            tracing::info!(%e, "no RemoteDesktop portal: sharing screens without keyboard and pointer");
            return view_only(ready, input).await;
        }
        Err(Failed::Other(e)) => {
            forget_token(token_path.as_ref());
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok((sources, false)));
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

/// Screens and windows alone, for a portal without RemoteDesktop; its own token, so it asks once too.
async fn view_only(ready: Sender<Ready>, input: Receiver<Input>) {
    let token_path = token_file("portal-token-view");
    let token = read_token(token_path.as_ref());
    let result = async {
        let cast = Screencast::new().await.map_err(|e| e.to_string())?;
        let session = cast.create_session().await.map_err(|e| e.to_string())?;
        let cursor = cursor_mode(&cast).await;
        cast.select_sources(
            &session,
            cursor,
            BitFlags::from(SourceType::Monitor) | SourceType::Window,
            true,
            token.as_deref(),
            PersistMode::ExplicitlyRevoked,
        )
        .await
        .map_err(|e| e.to_string())?;
        let started =
            cast.start(&session, None).await.map_err(|e| e.to_string())?.response().map_err(|e| e.to_string())?;
        keep_token(token_path.as_ref(), started.restore_token());
        let sources = sources(started.streams());
        let fd = cast.open_pipe_wire_remote(&session).await.map_err(|e| e.to_string())?;
        if PipeWire::connect(fd).is_none() {
            return Err("PipeWire did not connect".to_owned());
        }
        Ok::<_, String>((cast, session, sources))
    }
    .await;
    match result {
        Ok((_cast, _session, sources)) => {
            let _ = ready.send(Ok((sources, true)));
            // The session stays open until the app goes (the sender drops); there is nowhere to send input.
            while input.recv().is_ok() {}
        }
        Err(e) => {
            forget_token(token_path.as_ref());
            let _ = ready.send(Err(e));
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
