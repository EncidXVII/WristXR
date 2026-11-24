// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! X11: monitors from RandR, windows from the window manager's stacking list, capture through MIT-SHM (plain
//! GetImage where SHM is missing), windows from their Composite pixmap (their own pixels, even when covered), and
//! input through XTest. With the DAMAGE extension a capture reads only after its screen or window changed.
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use wxr_core::panel::PixelRect;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::composite::{ConnectionExt as _, Redirect};
use x11rb::protocol::damage::{ConnectionExt as _, ReportLevel};
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shm::ConnectionExt as _;
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, ImageFormat, MapState, Window,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

pub struct X11 {
    pub conn: RustConnection,
    pub root: Window,
    shm: bool,
    composite: bool,
    /// The DAMAGE extension is there: captures learn when their pixels change.
    damage: bool,
    /// Damage objects that reported a change not yet read.
    damaged: Mutex<HashSet<u32>>,
    /// The XKB extension is there: the active layout can be read and set.
    xkb: bool,
    atoms: Atoms,
}

/// The keyboard as the X server maps it: keysyms per key code, the active layout and the layouts' names.
pub struct Keymap {
    min_keycode: u8,
    per_keycode: usize,
    keysyms: Vec<u32>,
    pub group: u8,
    /// Short language names of the layouts, in their order ("EN", "UK", "DE").
    pub names: Vec<String>,
}

impl Keymap {
    /// The character an evdev key types in the active layout; `None` for keys that type none, and for layouts
    /// past the second (the core mapping only lists two).
    pub fn char(&self, evdev: u16, shift: bool, caps: bool) -> Option<char> {
        if self.group > 1 {
            return None;
        }
        let keycode = usize::from(evdev) + 8;
        let row = keycode.checked_sub(usize::from(self.min_keycode))? * self.per_keycode;
        let col = usize::from(self.group) * 2;
        let sym = |level: usize| {
            self.keysyms.get(row + col + level).copied().filter(|&k| k != 0).or_else(|| {
                // A layout listing only one level for a key types the same on both.
                (level == 1).then(|| self.keysyms.get(row + col).copied()).flatten()
            })
        };
        let lower = keysym_char(sym(0)?)?;
        // Caps Lock shifts letters only.
        let shifted = shift ^ (caps && lower.is_alphabetic());
        if shifted { sym(1).and_then(keysym_char) } else { Some(lower) }
    }
}

/// `xkb_keysym_to_utf32` from libxkbcommon, which every current desktop has; `None` without it.
fn xkbcommon() -> Option<unsafe extern "C" fn(u32) -> u32> {
    static F: OnceLock<Option<unsafe extern "C" fn(u32) -> u32>> = OnceLock::new();
    *F.get_or_init(|| {
        // SAFETY: dlopen/dlsym on a system library; the symbol is `uint32_t xkb_keysym_to_utf32(xkb_keysym_t)`.
        #[allow(unsafe_code)]
        unsafe {
            let lib = libc::dlopen(c"libxkbcommon.so.0".as_ptr(), libc::RTLD_NOW);
            if lib.is_null() {
                return None;
            }
            let f = libc::dlsym(lib, c"xkb_keysym_to_utf32".as_ptr());
            (!f.is_null()).then(|| std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn(u32) -> u32>(f))
        }
    })
}

/// The printable character of a keysym.
fn keysym_char(keysym: u32) -> Option<char> {
    let code = match xkbcommon() {
        // SAFETY: a pure function of its argument.
        #[allow(unsafe_code)]
        Some(f) => unsafe { f(keysym) },
        // Without libxkbcommon: Latin-1 keysyms are their code points, and 0x01000000 + U marks Unicode U.
        None => match keysym {
            0x20..=0x7E | 0xA0..=0xFF => keysym,
            0x0100_00A0..=0x0110_FFFF => keysym - 0x0100_0000,
            _ => 0,
        },
    };
    char::from_u32(code).filter(|c| !c.is_control() && *c != '\0')
}

/// Short language names for XKB layout codes, as Windows names its input languages ("UK" is Ukrainian).
fn language(layout: &str) -> String {
    let code = layout.split(['(', ':']).next().unwrap_or(layout);
    let name = match code {
        "us" | "gb" | "au" | "ie" | "nz" | "za" => "en",
        "ua" => "uk",
        "by" => "be",
        "cz" => "cs",
        "dk" => "da",
        "se" => "sv",
        "no" => "nb",
        "jp" => "ja",
        "kr" => "ko",
        "cn" | "tw" => "zh",
        "il" => "he",
        "gr" => "el",
        "br" => "pt",
        "latam" => "es",
        "rs" => "sr",
        "kz" => "kk",
        "ee" => "et",
        "si" => "sl",
        "ge" => "ka",
        "am" => "hy",
        "ir" => "fa",
        "vn" => "vi",
        "in" => "hi",
        "ch" => "de",
        "at" => "de",
        "be" => "nl",
        "ca" => "fr",
        other => other,
    };
    name.chars().take(2).collect::<String>().to_uppercase()
}

/// With DAMAGE, a capture still reads this often when nothing reported a change (a missed event costs at most this).
const DAMAGE_REFRESH: Duration = Duration::from_secs(2);

struct Atoms {
    client_list: u32,
    wm_name: u32,
    utf8: u32,
    window_type: u32,
    type_normal: u32,
    type_dialog: u32,
    wm_pid: u32,
    active: u32,
    rules_names: u32,
    /// `_NET_WM_CM_S<screen>`: owned while a compositing manager runs.
    compositor: u32,
}

static X: OnceLock<Option<X11>> = OnceLock::new();

/// The X server, connected once; `None` when there is no X display.
pub fn get() -> Option<&'static X11> {
    X.get_or_init(|| match X11::connect() {
        Ok(x) => Some(x),
        Err(e) => {
            tracing::info!(%e, "no X11 display");
            None
        }
    })
    .as_ref()
}

type Res<T> = Result<T, String>;
fn e(err: impl std::fmt::Display) -> String {
    err.to_string()
}

impl X11 {
    fn connect() -> Res<Self> {
        let (conn, screen) = x11rb::connect(None).map_err(e)?;
        let root = conn.setup().roots[screen].root;
        let atom = |name: &[u8]| -> Res<u32> { Ok(conn.intern_atom(false, name).map_err(e)?.reply().map_err(e)?.atom) };
        let atoms = Atoms {
            client_list: atom(b"_NET_CLIENT_LIST_STACKING")?,
            wm_name: atom(b"_NET_WM_NAME")?,
            utf8: atom(b"UTF8_STRING")?,
            window_type: atom(b"_NET_WM_WINDOW_TYPE")?,
            type_normal: atom(b"_NET_WM_WINDOW_TYPE_NORMAL")?,
            type_dialog: atom(b"_NET_WM_WINDOW_TYPE_DIALOG")?,
            wm_pid: atom(b"_NET_WM_PID")?,
            active: atom(b"_NET_ACTIVE_WINDOW")?,
            rules_names: atom(b"_XKB_RULES_NAMES")?,
            compositor: atom(format!("_NET_WM_CM_S{screen}").as_bytes())?,
        };
        let shm = conn.shm_query_version().ok().and_then(|c| c.reply().ok()).is_some();
        let composite = conn.composite_query_version(0, 4).ok().and_then(|c| c.reply().ok()).is_some();
        let damage = conn.damage_query_version(1, 1).ok().and_then(|c| c.reply().ok()).is_some();
        let xkb = conn.xkb_use_extension(1, 0).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.supported);
        Ok(Self { conn, root, shm, composite, damage, damaged: Mutex::new(HashSet::new()), xkb, atoms })
    }

    /// The keyboard mapping, the active layout and the layouts' names (from `_XKB_RULES_NAMES`, which setxkbmap
    /// and the desktops keep).
    pub fn keymap(&self) -> Option<Keymap> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = self.conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
        let rules = self
            .conn
            .get_property(false, self.root, self.atoms.rules_names, AtomEnum::STRING, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.value)
            .unwrap_or_default();
        // rules, model, layouts, variants, options
        let layouts =
            rules.split(|&b| b == 0).nth(2).map(|l| String::from_utf8_lossy(l).into_owned()).unwrap_or_default();
        let names: Vec<String> = layouts.split(',').filter(|l| !l.is_empty()).map(language).collect();
        Some(Keymap {
            min_keycode: min,
            per_keycode: usize::from(map.keysyms_per_keycode),
            keysyms: map.keysyms,
            group: self.group().unwrap_or(0),
            names,
        })
    }

    /// The active layout (XKB group).
    pub fn group(&self) -> Option<u8> {
        if !self.xkb {
            return None;
        }
        let state = self.conn.xkb_get_state(xkb::ID::USE_CORE_KBD.into()).ok()?.reply().ok()?;
        Some(u8::from(state.group))
    }

    /// Switches the active layout, as the desktop's own shortcut does.
    pub fn set_group(&self, group: u8) -> bool {
        if !self.xkb || group > 3 {
            return false;
        }
        let none = 0u16.into();
        let ok = self
            .conn
            .xkb_latch_lock_state(
                xkb::ID::USE_CORE_KBD.into(),
                none,
                none,
                true,
                xkb::Group::from(group),
                none,
                false,
                0,
            )
            .is_ok();
        let _ = self.conn.flush();
        ok
    }

    fn compositing(&self) -> bool {
        self.conn
            .get_selection_owner(self.atoms.compositor)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some_and(|r| r.owner != x11rb::NONE)
    }

    /// Reads the events waiting on the connection, noting which damage objects reported a change. Only DAMAGE
    /// events are asked for on this connection, so nothing else is lost.
    fn pump(&self) {
        let Ok(mut damaged) = self.damaged.lock() else { return };
        while let Ok(Some(event)) = self.conn.poll_for_event() {
            if let Event::DamageNotify(n) = event {
                damaged.insert(n.damage);
            }
        }
    }

    /// Monitors in RandR's order, in root-window pixels.
    pub fn monitors(&self) -> Vec<(PixelRect, bool)> {
        let Ok(reply) = self.conn.randr_get_monitors(self.root, true).map_err(e).and_then(|c| c.reply().map_err(e))
        else {
            // No RandR: the whole screen is one monitor.
            return self
                .conn
                .get_geometry(self.root)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|g| vec![(PixelRect { x: 0, y: 0, w: u32::from(g.width), h: u32::from(g.height) }, true)])
                .unwrap_or_default();
        };
        reply
            .monitors
            .iter()
            .map(|m| {
                (
                    PixelRect { x: i32::from(m.x), y: i32::from(m.y), w: u32::from(m.width), h: u32::from(m.height) },
                    m.primary,
                )
            })
            .collect()
    }

    fn prop32(&self, win: Window, prop: u32, ty: impl Into<u32>) -> Vec<u32> {
        self.conn
            .get_property(false, win, prop, ty, 0, 4096)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(Iterator::collect))
            .unwrap_or_default()
    }

    fn title(&self, win: Window) -> String {
        let read = |prop: u32, ty: u32| {
            self.conn
                .get_property(false, win, prop, ty, 0, 1024)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.value).into_owned())
                .filter(|s| !s.is_empty())
        };
        read(self.atoms.wm_name, self.atoms.utf8)
            .or_else(|| read(AtomEnum::WM_NAME.into(), AtomEnum::STRING.into()))
            .unwrap_or_default()
    }

    /// Top-level windows a person would pick, front to back: mapped, normal or dialog, titled, and not ours.
    pub fn windows(&self) -> Vec<(u64, String)> {
        let me = std::process::id();
        let mut list = self.prop32(self.root, self.atoms.client_list, AtomEnum::WINDOW);
        list.reverse();
        list.into_iter()
            .filter(|&w| {
                let viewable = self
                    .conn
                    .get_window_attributes(w)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .is_some_and(|a| a.map_state == MapState::VIEWABLE);
                let types = self.prop32(w, self.atoms.window_type, AtomEnum::ATOM);
                let kind_ok = types.is_empty()
                    || types.iter().any(|t| *t == self.atoms.type_normal || *t == self.atoms.type_dialog);
                let pid = self.prop32(w, self.atoms.wm_pid, AtomEnum::CARDINAL).first().copied();
                viewable && kind_ok && pid != Some(me)
            })
            .filter_map(|w| {
                let t = self.title(w);
                (!t.is_empty()).then_some((u64::from(w), t))
            })
            .collect()
    }

    /// A window's client area in root pixels; `None` once it is gone.
    pub fn window_rect(&self, win: u64) -> Option<PixelRect> {
        let win = u32::try_from(win).ok()?;
        let g = self.conn.get_geometry(win).ok()?.reply().ok()?;
        let at = self.conn.translate_coordinates(win, self.root, 0, 0).ok()?.reply().ok()?;
        Some(PixelRect {
            x: i32::from(at.dst_x),
            y: i32::from(at.dst_y),
            w: u32::from(g.width),
            h: u32::from(g.height),
        })
    }

    pub fn window_at(&self, p: (i32, i32)) -> Option<u64> {
        let mut list = self.prop32(self.root, self.atoms.client_list, AtomEnum::WINDOW);
        list.reverse();
        list.into_iter().map(u64::from).find(|&w| {
            self.window_rect(w)
                .is_some_and(|r| p.0 >= r.x && p.1 >= r.y && p.0 < r.x + r.w as i32 && p.1 < r.y + r.h as i32)
        })
    }

    /// Asks the window manager to bring the window forward and give it the keyboard (EWMH, "from a pager").
    pub fn focus(&self, win: u64) {
        let Ok(win) = u32::try_from(win) else { return };
        let ev = ClientMessageEvent::new(32, win, self.atoms.active, [2u32, 0, 0, 0, 0]);
        let _ = self.conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ev,
        );
        let _ = self.conn.flush();
    }

    // --- Input (XTest). X key codes are evdev codes + 8. ------------------------------------------------------

    pub fn move_to(&self, p: (i32, i32)) {
        const MOTION_NOTIFY: u8 = 6;
        let _ = self.conn.xtest_fake_input(MOTION_NOTIFY, 0, 0, self.root, clamp16(p.0), clamp16(p.1), 0);
        let _ = self.conn.flush();
    }

    /// X buttons: 1 left, 3 right, 4/5 wheel up/down.
    pub fn button(&self, button: u8, down: bool) {
        const BUTTON_PRESS: u8 = 4;
        const BUTTON_RELEASE: u8 = 5;
        let kind = if down { BUTTON_PRESS } else { BUTTON_RELEASE };
        let _ = self.conn.xtest_fake_input(kind, button, 0, self.root, 0, 0, 0);
        let _ = self.conn.flush();
    }

    pub fn key(&self, evdev: u16, down: bool) {
        const KEY_PRESS: u8 = 2;
        const KEY_RELEASE: u8 = 3;
        let Ok(code) = u8::try_from(evdev + 8) else { return };
        let _ = self.conn.xtest_fake_input(if down { KEY_PRESS } else { KEY_RELEASE }, code, 0, self.root, 0, 0, 0);
        let _ = self.conn.flush();
    }
}

fn clamp16(v: i32) -> i16 {
    v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// One System V shared-memory segment attached to the X server, for MIT-SHM GetImage.
struct Shm {
    seg: u32,
    ptr: *mut u8,
    len: usize,
}

impl Shm {
    #[allow(unsafe_code)]
    fn new(x: &X11, len: usize) -> Res<Self> {
        // SAFETY: plain System V calls. The segment is marked for removal as soon as the server has attached (or
        // could not), so it goes when both we and the server detach, also if we crash: a System V segment left
        // unmarked would outlive the process.
        unsafe {
            let id = libc::shmget(libc::IPC_PRIVATE, len, libc::IPC_CREAT | 0o600);
            if id < 0 {
                return Err("shmget failed".into());
            }
            let ptr = libc::shmat(id, std::ptr::null(), 0);
            if ptr as isize == -1 {
                libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
                return Err("shmat failed".into());
            }
            let attached = x.conn.generate_id().map_err(e).and_then(|seg| {
                x.conn.shm_attach(seg, id as u32, false).map_err(e)?.check().map_err(e)?;
                Ok(seg)
            });
            libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
            match attached {
                Ok(seg) => Ok(Self { seg, ptr: ptr.cast(), len }),
                Err(err) => {
                    libc::shmdt(ptr);
                    Err(err)
                }
            }
        }
    }
}

impl Drop for Shm {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        if let Some(x) = get() {
            let _ = x.conn.shm_detach(self.seg);
            let _ = x.conn.flush();
        }
        // SAFETY: `ptr` came from shmat and is detached once.
        unsafe {
            libc::shmdt(self.ptr.cast());
        }
    }
}

/// What a capture reads: an area of the root window, or one window's own pixmap.
pub enum Target {
    Root(PixelRect),
    Window(Window),
}

pub struct XCapture {
    target: Target,
    shm: Option<Shm>,
    /// The server could not attach a segment (a remote display): plain GetImage from then on, no more tries.
    no_shm: bool,
    /// The window's Composite pixmap and the size it was named at.
    pixmap: Option<(u32, u16, u16)>,
    /// Pixels when they did not come through the shared segment (plain GetImage).
    buf: Vec<u8>,
    w: u32,
    h: u32,
    /// The DAMAGE object watching the target, and when the target was last read.
    damage: Option<u32>,
    read_at: Option<Instant>,
}

impl XCapture {
    pub fn new(target: Target) -> Res<Self> {
        let x = get().ok_or("no X11 display")?;
        if let Target::Window(w) = target
            && x.composite
        {
            // Under a compositor windows are redirected already; without one this keeps their pixels for us.
            let _ = x.conn.composite_redirect_window(w, Redirect::AUTOMATIC);
        }
        // A window's own pixels always report their damage. The root's do not under a compositing manager (it
        // paints elsewhere), so there the screen is read every frame as before.
        let (drawable, watch) = match target {
            Target::Root(_) => (x.root, !x.compositing()),
            Target::Window(w) => (w, true),
        };
        let damage = (x.damage && watch).then(|| x.conn.generate_id().ok()).flatten().filter(|&id| {
            x.conn.damage_create(id, drawable, ReportLevel::NON_EMPTY).ok().and_then(|c| c.check().ok()).is_some()
        });
        Ok(Self { target, shm: None, no_shm: !x.shm, pixmap: None, buf: Vec::new(), w: 0, h: 0, damage, read_at: None })
    }

    /// Whether the target may have changed since the last read: always without DAMAGE, on the first read and
    /// every `DAMAGE_REFRESH`; otherwise only when it reported a change. Re-arms the report when it did.
    fn changed(&mut self, x: &X11) -> bool {
        let Some(id) = self.damage else { return true };
        if self.read_at.is_none_or(|t| t.elapsed() >= DAMAGE_REFRESH) {
            let _ = x.conn.damage_subtract(id, x11rb::NONE, x11rb::NONE);
            return true;
        }
        x.pump();
        let hit = x.damaged.lock().is_ok_and(|mut d| d.remove(&id));
        if hit {
            // NON_EMPTY reports once until the damage is cleared: clear it before reading, so a change made while
            // we read reports again.
            let _ = x.conn.damage_subtract(id, x11rb::NONE, x11rb::NONE);
            let _ = x.conn.flush();
        }
        hit
    }

    /// The next `grab` reads, changed or not (the last read's pixels were not used, as after a resize).
    pub fn again(&mut self) {
        self.read_at = None;
    }

    /// Reads the latest pixels (BGRA, rows of `w * 4`); `None` when the source is gone or unreadable.
    #[allow(unsafe_code)]
    pub fn grab(&mut self) -> Option<(u32, u32, &[u8])> {
        let x = get()?;
        if !self.changed(x) {
            return None;
        }
        self.read_at = Some(Instant::now());
        let (drawable, sx, sy, w, h) = match self.target {
            Target::Root(r) => (x.root, clamp16(r.x), clamp16(r.y), r.w as u16, r.h as u16),
            Target::Window(win) => {
                let g = x.conn.get_geometry(win).ok()?.reply().ok()?;
                let drawable = if x.composite {
                    match self.pixmap {
                        Some((p, pw, ph)) if (pw, ph) == (g.width, g.height) => p,
                        _ => {
                            if let Some((p, _, _)) = self.pixmap.take() {
                                let _ = x.conn.free_pixmap(p);
                            }
                            let p = x.conn.generate_id().ok()?;
                            x.conn.composite_name_window_pixmap(win, p).ok()?.check().ok()?;
                            self.pixmap = Some((p, g.width, g.height));
                            p
                        }
                    }
                } else {
                    win
                };
                (drawable, 0, 0, g.width, g.height)
            }
        };
        if w == 0 || h == 0 {
            return None;
        }
        let len = usize::from(w) * usize::from(h) * 4;
        self.w = u32::from(w);
        self.h = u32::from(h);
        if !self.no_shm {
            if self.shm.as_ref().is_none_or(|s| s.len < len) {
                self.shm = Shm::new(x, len).inspect_err(|err| tracing::warn!(%err, "MIT-SHM unavailable")).ok();
                self.no_shm = self.shm.is_none();
            }
            if let Some(s) = &self.shm {
                let fmt = u8::from(ImageFormat::Z_PIXMAP);
                if x.conn
                    .shm_get_image(drawable, sx, sy, w, h, !0, fmt, s.seg, 0)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .is_some()
                {
                    // SAFETY: the server wrote `len` bytes into our segment, which is at least that long and ours
                    // alone until the next request; the pixels are lent from it, with no copy.
                    let px = unsafe { std::slice::from_raw_parts_mut(s.ptr, len) };
                    opaque(px);
                    return Some((self.w, self.h, px));
                }
            }
        }
        let reply = x.conn.get_image(ImageFormat::Z_PIXMAP, drawable, sx, sy, w, h, !0).ok()?.reply().ok()?;
        self.buf.resize(len, 0);
        let n = reply.data.len().min(len);
        self.buf[..n].copy_from_slice(&reply.data[..n]);
        opaque(&mut self.buf);
        Some((self.w, self.h, &self.buf))
    }
}

/// 24-bit visuals leave the fourth byte undefined; SteamVR would treat it as transparency.
fn opaque(px: &mut [u8]) {
    for p in px.as_chunks_mut::<4>().0 {
        p[3] = 0xFF;
    }
}

impl Drop for XCapture {
    fn drop(&mut self) {
        let Some(x) = get() else { return };
        if let Some((p, _, _)) = self.pixmap.take() {
            let _ = x.conn.free_pixmap(p);
        }
        if let Some(id) = self.damage.take() {
            let _ = x.conn.damage_destroy(id);
            if let Ok(mut d) = x.damaged.lock() {
                d.remove(&id);
            }
        }
        let _ = x.conn.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Key 30 (A) as the X server maps it: US in the first layout, Ukrainian (Unicode keysyms) in the second.
    fn map(group: u8) -> Keymap {
        let (min, per) = (8u8, 4usize);
        let mut keysyms = vec![0u32; (255 - usize::from(min)) * per];
        let row = (30 + 8 - usize::from(min)) * per;
        keysyms[row..row + 4].copy_from_slice(&[0x61, 0x41, 0x0100_0444, 0x0100_0424]);
        Keymap { min_keycode: min, per_keycode: per, keysyms, group, names: vec!["EN".into(), "UK".into()] }
    }

    #[test]
    fn keys_type_what_the_active_layout_says() {
        let us = map(0);
        assert_eq!(us.char(30, false, false), Some('a'));
        assert_eq!(us.char(30, true, false), Some('A'));
        assert_eq!(us.char(30, false, true), Some('A'));
        assert_eq!(us.char(30, true, true), Some('a'));
        let ua = map(1);
        assert_eq!(ua.char(30, false, false), Some('ф'));
        assert_eq!(ua.char(30, true, false), Some('Ф'));
        assert_eq!(ua.char(31, false, false), None);
        assert_eq!(map(2).char(30, false, false), None);
    }

    #[test]
    fn layouts_get_language_names() {
        assert_eq!(language("us"), "EN");
        assert_eq!(language("ua"), "UK");
        assert_eq!(language("de(nodeadkeys)"), "DE");
        assert_eq!(language("ru:2"), "RU");
    }
}
