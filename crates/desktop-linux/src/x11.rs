//! X11: monitors from RandR, windows from the window manager's stacking list, capture through MIT-SHM (plain
//! GetImage where SHM is missing), windows from their Composite pixmap (their own pixels, even when covered), and
//! input through XTest.
use dvr_core::panel::PixelRect;
use std::sync::OnceLock;
use x11rb::connection::Connection;
use x11rb::protocol::composite::{ConnectionExt as _, Redirect};
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shm::ConnectionExt as _;
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
    atoms: Atoms,
}

struct Atoms {
    client_list: u32,
    wm_name: u32,
    utf8: u32,
    window_type: u32,
    type_normal: u32,
    type_dialog: u32,
    wm_pid: u32,
    active: u32,
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
        };
        let shm = conn.shm_query_version().ok().and_then(|c| c.reply().ok()).is_some();
        let composite = conn.composite_query_version(0, 4).ok().and_then(|c| c.reply().ok()).is_some();
        Ok(Self { conn, root, shm, composite, atoms })
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
    id: i32,
    ptr: *mut u8,
    len: usize,
}

impl Shm {
    #[allow(unsafe_code)]
    fn new(x: &X11, len: usize) -> Res<Self> {
        // SAFETY: plain System V calls; the segment is marked for removal right away, so it goes when both we and the
        // server detach (also if we crash).
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
            let seg = x.conn.generate_id().map_err(e)?;
            x.conn.shm_attach(seg, id as u32, false).map_err(e)?.check().map_err(e)?;
            libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
            Ok(Self { seg, id, ptr: ptr.cast(), len })
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
        let _ = self.id;
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
    /// The window's Composite pixmap and the size it was named at.
    pixmap: Option<(u32, u16, u16)>,
    buf: Vec<u8>,
    w: u32,
    h: u32,
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
        Ok(Self { target, shm: None, pixmap: None, buf: Vec::new(), w: 0, h: 0 })
    }

    /// Reads the latest pixels (BGRA, rows of `w * 4`); `None` when the source is gone or unreadable.
    #[allow(unsafe_code)]
    pub fn grab(&mut self) -> Option<(u32, u32, &[u8])> {
        let x = get()?;
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
        self.buf.resize(len, 0);
        let mut read = false;
        if x.shm {
            if self.shm.as_ref().is_none_or(|s| s.len < len) {
                self.shm = Shm::new(x, len).inspect_err(|err| tracing::warn!(%err, "MIT-SHM unavailable")).ok();
            }
            if let Some(s) = &self.shm {
                let fmt = u8::from(ImageFormat::Z_PIXMAP);
                if x.conn
                    .shm_get_image(drawable, sx, sy, w, h, !0, fmt, s.seg, 0)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .is_some()
                {
                    // SAFETY: the server wrote `len` bytes into our segment, which is at least that long.
                    unsafe { std::ptr::copy_nonoverlapping(s.ptr, self.buf.as_mut_ptr(), len) };
                    read = true;
                }
            }
        }
        if !read {
            let reply = x.conn.get_image(ImageFormat::Z_PIXMAP, drawable, sx, sy, w, h, !0).ok()?.reply().ok()?;
            let n = reply.data.len().min(len);
            self.buf[..n].copy_from_slice(&reply.data[..n]);
        }
        // 24-bit visuals leave the fourth byte undefined; SteamVR would treat it as transparency.
        for px in self.buf.as_chunks_mut::<4>().0 {
            px[3] = 0xFF;
        }
        self.w = u32::from(w);
        self.h = u32::from(h);
        Some((self.w, self.h, &self.buf))
    }
}

impl Drop for XCapture {
    fn drop(&mut self) {
        if let (Some(x), Some((p, _, _))) = (get(), self.pixmap.take()) {
            let _ = x.conn.free_pixmap(p);
            let _ = x.conn.flush();
        }
    }
}
