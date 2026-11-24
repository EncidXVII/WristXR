// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux: screens, windows and input for WristXR, the counterpart of `wxr-capture-win` and `wxr-input-win`.
//!
//! On a Wayland session everything goes through one desktop-portal session (screens and windows the user shares,
//! keyboard and pointer), with frames from PipeWire; if the portal is missing or refused it falls back to X11, which
//! then sees only X11 apps. On X11 it talks to the server directly.
//!
//! The portal does not say where a shared window sits on the desktop, so each one gets its own area of desktop
//! coordinates far outside the screens (`WINDOW_AREA`); input that lands there goes to that window's stream.
#![cfg(target_os = "linux")]
#![allow(unsafe_code)]

mod keys;
mod pipewire;
mod portal;
mod x11;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use wxr_core::engine::{Event, MouseButton};
use wxr_core::keyboard::CAPS;
use wxr_core::panel::PixelRect;

pub use keys::evdev;
pub use pipewire::{Crop, DmaFrame, set_dmabuf_modifiers};

/// Shared Wayland windows are numbered from here, so their ids never meet X11 window ids (29 bits).
const PORTAL_WINDOW: u64 = 1 << 40;
/// Desktop coordinates for shared Wayland windows start here, one `WINDOW_STEP` apart.
const WINDOW_AREA: i32 = 1_000_000;
const WINDOW_STEP: i32 = 20_000;
/// X11 capture has no "new frame" signal; it reads at most this often.
const X11_FRAME: Duration = Duration::from_millis(15);

enum Backend {
    Portal(&'static portal::Portal),
    X11(&'static x11::X11),
    None,
}

fn backend() -> Backend {
    if portal::wayland()
        && let Some(p) = portal::get()
    {
        return Backend::Portal(p);
    }
    match x11::get() {
        Some(x) => Backend::X11(x),
        None => Backend::None,
    }
}

#[derive(Clone, Copy, Debug)]
enum MonitorSource {
    X11,
    Portal(u32),
}

#[derive(Clone, Copy, Debug)]
pub struct Monitor {
    pub rect: PixelRect,
    pub primary: bool,
    source: MonitorSource,
}

/// Screens, in a stable order (RandR's, or the order the user shared them in).
pub fn monitors() -> Vec<Monitor> {
    match backend() {
        Backend::Portal(p) => p
            .sources
            .iter()
            .filter(|s| s.monitor)
            .enumerate()
            .map(|(i, s)| Monitor {
                rect: PixelRect {
                    x: s.position.0,
                    y: s.position.1,
                    w: s.size.0.max(1) as u32,
                    h: s.size.1.max(1) as u32,
                },
                primary: i == 0,
                source: MonitorSource::Portal(s.node),
            })
            .collect(),
        Backend::X11(x) => x
            .monitors()
            .into_iter()
            .map(|(rect, primary)| Monitor { rect, primary, source: MonitorSource::X11 })
            .collect(),
        Backend::None => Vec::new(),
    }
}

/// Shared Wayland windows: (id, node, desktop rectangle).
fn portal_windows(p: &portal::Portal) -> impl Iterator<Item = (u64, u32, PixelRect)> + '_ {
    p.sources.iter().filter(|s| !s.monitor).enumerate().map(|(i, s)| {
        let rect = PixelRect {
            x: WINDOW_AREA + i as i32 * WINDOW_STEP,
            y: 0,
            w: s.size.0.max(1) as u32,
            h: s.size.1.max(1) as u32,
        };
        (PORTAL_WINDOW | u64::from(s.node), s.node, rect)
    })
}

/// Windows the user can add, front to back. On Wayland: the windows shared in the portal prompt (it gives no
/// titles, so they are numbered).
pub fn windows() -> Vec<(u64, String)> {
    match backend() {
        Backend::Portal(p) => {
            portal_windows(p).enumerate().map(|(i, (id, _, _))| (id, format!("Shared window {}", i + 1))).collect()
        }
        Backend::X11(x) => x.windows(),
        Backend::None => Vec::new(),
    }
}

pub fn window_rect(id: u64) -> Option<PixelRect> {
    match backend() {
        Backend::Portal(p) => portal_windows(p).find(|(w, _, _)| *w == id).map(|(_, _, r)| r),
        Backend::X11(x) => x.window_rect(id),
        Backend::None => None,
    }
}

/// The window under a desktop pixel (X11 only: Wayland does not tell).
pub fn window_at(p: (i32, i32)) -> Option<u64> {
    match backend() {
        Backend::X11(x) => x.window_at(p),
        _ => None,
    }
}

/// Brings a window forward (X11 only; on Wayland the compositor decides).
pub fn focus(id: u64) {
    if let Backend::X11(x) = backend() {
        x.focus(id);
    }
}

enum CaptureInner {
    X { cap: x11::XCapture, at: Option<Instant> },
    Pw { stream: pipewire::Stream, seen: u64 },
}

/// A new frame, lent for the length of `Capture::grab`'s closure.
pub enum Pixels<'a> {
    /// BGRA rows of `w * 4` bytes.
    Bgra(&'a [u8]),
    /// A frame still in the GPU (Wayland, with DMA-BUF modifiers set): the GPU copies it.
    DmaBuf(&'a DmaFrame),
}

/// A live capture of one screen or window.
pub struct Capture(CaptureInner);

impl Capture {
    pub fn monitor(m: &Monitor) -> Result<Self, String> {
        match m.source {
            MonitorSource::X11 => {
                Ok(Self(CaptureInner::X { cap: x11::XCapture::new(x11::Target::Root(m.rect))?, at: None }))
            }
            MonitorSource::Portal(node) => Self::stream(node),
        }
    }

    pub fn window(id: u64) -> Result<Self, String> {
        if id & PORTAL_WINDOW != 0 {
            return Self::stream((id & 0xFFFF_FFFF) as u32);
        }
        let win = u32::try_from(id).map_err(|e| e.to_string())?;
        Ok(Self(CaptureInner::X { cap: x11::XCapture::new(x11::Target::Window(win))?, at: None }))
    }

    fn stream(node: u32) -> Result<Self, String> {
        let pw = pipewire::PipeWire::get().ok_or("PipeWire not connected")?;
        Ok(Self(CaptureInner::Pw { stream: pipewire::Stream::new(pw, node)?, seen: 0 }))
    }

    /// The newest frame, when there is a new one since the last call: `f` gets its size and pixels. The pixels
    /// are lent, not copied, so `f` should copy them out.
    pub fn grab<R>(&mut self, f: impl FnOnce(u32, u32, Pixels<'_>) -> R) -> Option<R> {
        match &mut self.0 {
            CaptureInner::X { cap, at } => {
                if at.is_some_and(|t| t.elapsed() < X11_FRAME) {
                    return None;
                }
                *at = Some(Instant::now());
                let (w, h, px) = cap.grab()?;
                Some(f(w, h, Pixels::Bgra(px)))
            }
            CaptureInner::Pw { stream, seen } => {
                let latest = stream.latest.lock().ok()?;
                if latest.seq == *seen {
                    return None;
                }
                *seen = latest.seq;
                let pixels = match &latest.pixels {
                    pipewire::Pixels::None => return None,
                    pipewire::Pixels::Bgra(px) => Pixels::Bgra(px),
                    pipewire::Pixels::Dma(frame) => Pixels::DmaBuf(frame),
                };
                Some(f(latest.w, latest.h, pixels))
            }
        }
    }

    /// The GPU could not import this capture's DMA-BUFs: it asks for shared memory from now on.
    pub fn dmabuf_failed(&self) {
        if let CaptureInner::Pw { stream, .. } = &self.0 {
            stream.shared_memory_only();
        }
    }
}

// --- Input -----------------------------------------------------------------------------------------------------------

static CAPS_ON: AtomicBool = AtomicBool::new(false);

/// Mouse and keyboard into the desktop, from WristXR's engine events (desktop pixels, set-1 scancodes).
#[derive(Default)]
pub struct Injector {
    last: Option<(i32, i32)>,
}

impl Injector {
    pub fn new() -> Self {
        Self { last: None }
    }

    fn place(&self, p: (i32, i32)) {
        match backend() {
            Backend::Portal(portal) => {
                let target = portal
                    .sources
                    .iter()
                    .filter(|s| s.monitor)
                    .map(|s| {
                        (
                            s.node,
                            PixelRect {
                                x: s.position.0,
                                y: s.position.1,
                                w: s.size.0.max(1) as u32,
                                h: s.size.1.max(1) as u32,
                            },
                        )
                    })
                    .chain(portal_windows(portal).map(|(_, node, r)| (node, r)))
                    .find(|(_, r)| p.0 >= r.x && p.1 >= r.y && p.0 < r.x + r.w as i32 && p.1 < r.y + r.h as i32);
                if let Some((node, r)) = target {
                    portal.send(portal::Input::Move { node, x: f64::from(p.0 - r.x), y: f64::from(p.1 - r.y) });
                }
            }
            Backend::X11(x) => x.move_to(p),
            Backend::None => {}
        }
    }

    pub fn move_to(&mut self, p: (i32, i32)) {
        if self.last != Some(p) {
            self.last = Some(p);
            self.place(p);
        }
    }

    pub fn button(&mut self, p: (i32, i32), right: bool, down: bool) {
        self.last = Some(p);
        self.place(p);
        match backend() {
            Backend::Portal(portal) => {
                portal.send(portal::Input::Button { code: if right { 0x111 } else { 0x110 }, down })
            }
            Backend::X11(x) => x.button(if right { 3 } else { 1 }, down),
            Backend::None => {}
        }
    }

    pub fn scroll(&mut self, p: (i32, i32), notches: i32) {
        self.last = Some(p);
        self.place(p);
        match backend() {
            Backend::Portal(portal) => portal.send(portal::Input::Scroll(notches)),
            Backend::X11(x) => {
                let button = if notches > 0 { 4 } else { 5 };
                for _ in 0..notches.unsigned_abs() {
                    x.button(button, true);
                    x.button(button, false);
                }
            }
            Backend::None => {}
        }
    }

    /// Applies an engine event to the desktop; false for events that are not input.
    pub fn apply(&mut self, event: &Event, rect: impl Fn(u32) -> Option<PixelRect>) -> bool {
        match *event {
            Event::Hover { panel, uv, cursor: true, .. } => {
                if let Some(r) = rect(panel) {
                    self.move_to(r.pixel_at(uv));
                }
            }
            Event::Mouse { panel, uv, button, down } => {
                if let Some(r) = rect(panel) {
                    self.button(r.pixel_at(uv), button == MouseButton::Right, down);
                }
            }
            Event::Scroll { panel, uv, notches } => {
                if let Some(r) = rect(panel) {
                    self.scroll(r.pixel_at(uv), notches);
                }
            }
            Event::Key { scancode, down } => self.key(scancode, down),
            _ => return false,
        }
        true
    }

    pub fn key(&self, scancode: u16, down: bool) {
        let Some(code) = keys::evdev(scancode) else { return };
        if scancode == CAPS && down {
            CAPS_ON.fetch_xor(true, Ordering::Relaxed);
        }
        match backend() {
            Backend::Portal(p) => p.send(portal::Input::Key { code: i32::from(code), down }),
            Backend::X11(x) => x.key(code, down),
            Backend::None => {}
        }
    }
}

// --- Keyboard layout ---------------------------------------------------------------------------------------------
// Keys go out as key codes, so every app gets the user's own layout. On X11 the labels and the VRChat chat line
// follow the active layout too, read from the X server, and the layout can be switched from VR. Wayland has no
// common way to read or set it, so there they follow a US layout.

/// How long a read keyboard mapping is reused (labels ask for it once per key per redraw).
const KEYMAP_FRESH: Duration = Duration::from_millis(500);

static KEYMAP: std::sync::Mutex<Option<(Instant, std::sync::Arc<x11::Keymap>)>> = std::sync::Mutex::new(None);

fn keymap() -> Option<std::sync::Arc<x11::Keymap>> {
    let Backend::X11(x) = backend() else { return None };
    let mut cached = KEYMAP.lock().ok()?;
    if let Some((at, map)) = cached.as_ref()
        && at.elapsed() < KEYMAP_FRESH
    {
        return Some(map.clone());
    }
    let map = std::sync::Arc::new(x.keymap()?);
    *cached = Some((Instant::now(), map.clone()));
    Some(map)
}

fn forget_keymap() {
    if let Ok(mut cached) = KEYMAP.lock() {
        *cached = None;
    }
}

/// Changes when the active layout does.
pub fn layout_id() -> isize {
    match backend() {
        Backend::X11(x) => x.group().map_or(0, isize::from),
        _ => 0,
    }
}

pub fn layout_name() -> String {
    keymap().and_then(|m| m.names.get(usize::from(m.group)).cloned()).unwrap_or_else(|| "EN".to_owned())
}

pub fn layouts() -> Vec<String> {
    keymap().map(|m| m.names.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| vec!["EN".to_owned()])
}

/// The next layout, on X11; on Wayland switching is the desktop's job.
pub fn next_layout() -> bool {
    let Some(map) = keymap() else { return false };
    let n = map.names.len().min(4);
    if n < 2 {
        return false;
    }
    select_layout((usize::from(map.group) + 1) % n)
}

pub fn select_layout(i: usize) -> bool {
    let Backend::X11(x) = backend() else { return false };
    let ok = u8::try_from(i).is_ok_and(|g| x.set_group(g));
    forget_keymap();
    ok
}

pub fn key_char(scancode: u16, shift: bool, caps: bool) -> Option<char> {
    if let Some(map) = keymap() {
        let from_layout = keys::evdev(scancode).and_then(|code| map.char(code, shift, caps));
        if from_layout.is_some() || map.group <= 1 {
            return from_layout;
        }
    }
    keys::us_char(scancode, shift, caps)
}

/// Caps Lock as WristXR last set it (Linux has no common way to read it on Wayland).
pub fn caps_lock() -> bool {
    CAPS_ON.load(Ordering::Relaxed)
}

/// Windows' "administrator window" problem does not exist here.
pub fn admin_window_at(_p: (i32, i32)) -> Option<String> {
    None
}

/// The screens are shared without keyboard and pointer: the desktop's portal has no RemoteDesktop (the wlroots
/// and Hyprland portals), so clicks and keys go nowhere.
pub fn view_only() -> bool {
    matches!(backend(), Backend::Portal(p) if p.view_only)
}
