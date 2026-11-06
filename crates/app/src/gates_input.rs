//! Live input gate (`--selftest --input`): scripted pointer/keyboard through the real SendInput path into a
//! topmost test window owned by this process. Briefly moves the mouse; restores it afterwards.
#![allow(unsafe_code)]

use crate::gates::{GateResult, HEAD, Rig, err, key_sc, press_key};
use dvr_core::keyboard;
use dvr_core::math::{Pose, Quat, Vec3};
use dvr_core::panel::{Hand, Panel, PixelRect, Source};
use dvr_input_win::Injector;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED, MSLLHOOKSTRUCT, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetForegroundWindow, GetMessageW, MSG,
    PostMessageW, PostQuitMessage, RegisterClassW, SetCursorPos, TranslateMessage, WM_CHAR, WM_CLOSE, WM_DESTROY,
    WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_SETFOCUS, WNDCLASSW, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};
use windows::core::w;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Seen {
    Down { x: i32, y: i32, right: bool },
    Wheel(i32),
    Char(char),
    Focus,
}

static SEEN: Mutex<Vec<Seen>> = Mutex::new(Vec::new());
/// Set when real (non-injected) mouse or keyboard input happens during the gate: the result is then inconclusive.
static PHYSICAL_INPUT: AtomicBool = AtomicBool::new(false);

extern "system" fn mouse_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: for WH_MOUSE_LL, lParam points at an MSLLHOOKSTRUCT for the duration of the call.
        let info = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
        // Moves are ignored: SetCursorPos produces non-injected moves, and the cursor is re-placed before clicks.
        if info.flags & LLMHF_INJECTED == 0 && wp.0 as u32 != WM_MOUSEMOVE {
            PHYSICAL_INPUT.store(true, Ordering::Relaxed);
        }
    }
    // SAFETY: pass the event on unchanged.
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

extern "system" fn keyboard_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: for WH_KEYBOARD_LL, lParam points at a KBDLLHOOKSTRUCT for the duration of the call.
        let info = unsafe { &*(lp.0 as *const KBDLLHOOKSTRUCT) };
        if info.flags.0 & LLKHF_INJECTED.0 == 0 {
            PHYSICAL_INPUT.store(true, Ordering::Relaxed);
        }
    }
    // SAFETY: pass the event on unchanged.
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

fn seen() -> Vec<Seen> {
    SEEN.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default()
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let low = |v: usize| (v & 0xFFFF) as u16 as i16 as i32;
    let event = match msg {
        WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
            Some(Seen::Down { x: low(lp.0 as usize), y: low(lp.0 as usize >> 16), right: msg == WM_RBUTTONDOWN })
        }
        WM_MOUSEWHEEL => Some(Seen::Wheel(low(wp.0 >> 16))),
        WM_CHAR => char::from_u32(wp.0 as u32).map(Seen::Char),
        WM_SETFOCUS => Some(Seen::Focus),
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            None
        }
        _ => None,
    };
    if let (Some(e), Ok(mut log)) = (event, SEEN.lock()) {
        log.push(e);
    }
    // SAFETY: default handling for a window this thread owns.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// Topmost borderless window on its own message-loop thread; closed on drop.
struct TestWindow {
    hwnd: isize,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TestWindow {
    fn open(r: PixelRect) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                lpszClassName: w!("dvr-selftest-target"),
                ..Default::default()
            };
            // SAFETY: class and window are owned by this thread, which pumps their messages until WM_QUIT.
            unsafe {
                RegisterClassW(&class);
                let hwnd = CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                    w!("dvr-selftest-target"),
                    w!("DesktopVR self-test"),
                    WS_POPUP | WS_VISIBLE,
                    r.x,
                    r.y,
                    r.w as i32,
                    r.h as i32,
                    None,
                    None,
                    None,
                    None,
                );
                let _ = tx.send(hwnd.map(|h| h.0 as isize).map_err(|e| e.to_string()));
                // Low-level hooks run on this thread's message loop and only observe (never block) input.
                let hooks = [
                    SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0).ok(),
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0).ok(),
                ];
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                for hook in hooks.into_iter().flatten() {
                    let _ = UnhookWindowsHookEx(hook);
                }
            }
        });
        let hwnd = rx.recv_timeout(Duration::from_secs(2)).map_err(err)??;
        Ok(Self { hwnd, thread: Some(thread) })
    }

    fn handle(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: posting to our own window; its thread destroys it and exits the loop.
        let _ = unsafe { PostMessageW(Some(self.handle()), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn settle() -> Vec<Seen> {
    std::thread::sleep(Duration::from_millis(250));
    seen()
}

pub fn input_live() -> GateResult {
    let monitors = dvr_capture_win::monitors();
    let (index, m) = monitors.iter().enumerate().find(|(_, m)| m.primary).ok_or("no primary monitor")?;
    let screen = m.rect;
    let win = PixelRect { x: screen.x + screen.w as i32 / 4, y: screen.y + screen.h as i32 / 4, w: 640, h: 400 };
    let mut saved = POINT::default();
    // SAFETY: plain cursor query.
    let _ = unsafe { GetCursorPos(&mut saved) };
    PHYSICAL_INPUT.store(false, Ordering::Relaxed);
    let result = run(index as u32, screen, win);
    if result.is_err() && PHYSICAL_INPUT.load(Ordering::Relaxed) {
        return Err("inconclusive: real mouse/keyboard input during the test; rerun hands-off".into());
    }
    // SAFETY: restores the user's cursor position.
    let _ = unsafe { SetCursorPos(saved.x, saved.y) };
    result
}

fn run(monitor: u32, screen: PixelRect, win: PixelRect) -> GateResult {
    let window = TestWindow::open(win)?;
    settle();
    const KB: u32 = 1;
    const DESK: u32 = 2;
    let aspect = screen.h as f32 / screen.w as f32;
    let desk =
        Panel::new(DESK, Source::Monitor(monitor), Pose::new(Vec3::new(0.0, 1.5, -1.0), Quat::IDENTITY), 1.2, aspect);
    let mut rig = Rig::new(vec![Panel::keyboard_below(KB, HEAD), desk]);
    let mut injector = Injector::new();
    let rect = |panel: u32| (panel == DESK).then_some(screen);
    let mut apply = |rig: &mut Rig| {
        for e in rig.take() {
            injector.apply(&e, rect);
        }
    };
    // Pixel centers: uv for desktop pixel (px, py).
    let uv = |px: i32, py: i32| {
        [((px - screen.x) as f32 + 0.5) / screen.w as f32, ((py - screen.y) as f32 + 0.5) / screen.h as f32]
    };
    let r = Hand::Right;

    let (cx, cy) = (200, 150);
    rig.aim(r, DESK, uv(win.x + cx, win.y + cy))?;
    rig.frames(2);
    rig.tap(r);
    apply(&mut rig);
    let got = settle();
    let hit = got
        .iter()
        .any(|s| matches!(*s, Seen::Down { x, y, right: false } if (x - cx).abs() <= 1 && (y - cy).abs() <= 1));
    if !hit {
        let under = dvr_capture_win::window_at((win.x + cx, win.y + cy));
        return Err(format!(
            "left click did not land at ({cx},{cy}) in the test window {}: saw {got:?}, window under target {under:?}",
            window.hwnd
        ));
    }

    rig.hold_ms(500);
    rig.aim(r, DESK, uv(win.x + 300, win.y + 200))?;
    rig.frames(1);
    rig.hand(r).trigger = true;
    rig.hold_ms(700);
    rig.hand(r).trigger = false;
    rig.frames(1);
    rig.hand(r).stick = [0.0, 1.0];
    rig.hold_ms(300);
    rig.hand(r).stick = [0.0; 2];
    rig.frames(1);
    apply(&mut rig);
    let got = settle();
    let right = got
        .iter()
        .any(|s| matches!(*s, Seen::Down { x, y, right: true } if (x - 300).abs() <= 1 && (y - 200).abs() <= 1));
    let stray = got.iter().any(|s| matches!(*s, Seen::Down { right: false, .. }));
    if !right || stray {
        return Err(format!("long press did not right-click at (300,200): {got:?}"));
    }
    if !got.iter().any(|s| matches!(*s, Seen::Wheel(d) if d > 0)) {
        return Err(format!("scroll did not reach the window: {got:?}"));
    }

    // Typing only when our window has focus, so keys can never land in another app.
    // SAFETY: plain query.
    if unsafe { GetForegroundWindow() } != window.handle() {
        return Err("test window did not take focus; typing skipped".into());
    }
    let word = ["h", "e", "l", "l", "o"];
    let mut expected = String::new();
    for (i, l) in word.iter().enumerate() {
        let sc = key_sc(l)?;
        expected.extend(dvr_input_win::key_char(sc, false, false));
        press_key(&mut rig, if i % 2 == 0 { Hand::Left } else { r }, sc)?;
    }
    press_key(&mut rig, r, key_sc("Shift")?)?;
    press_key(&mut rig, Hand::Left, key_sc("a")?)?;
    expected.extend(dvr_input_win::key_char(key_sc("a")?, true, false));
    press_key(&mut rig, r, keyboard::CHAT)?;
    apply(&mut rig);
    let typed: String = settle().iter().filter_map(|s| if let Seen::Char(c) = s { Some(*c) } else { None }).collect();
    if typed != expected {
        return Err(format!("typed {typed:?}, expected {expected:?} (current layout)"));
    }

    // Gate 5b live: yank the test window out of the desktop panel; it must resolve to our window and capture.
    rig.aim(r, DESK, uv(win.x + 320, win.y + 200))?;
    rig.frames(1);
    rig.take();
    rig.hand(r).grip = true;
    rig.frames(1);
    let toward = -rig.hands[1].pose.forward();
    for _ in 0..5 {
        rig.hand(r).pose.pos = rig.hands[1].pose.pos + toward * 0.05;
        rig.frames(1);
    }
    rig.hand(r).grip = false;
    rig.frames(1);
    let pop = rig.take().into_iter().find_map(|e| match e {
        dvr_core::engine::Event::PopOut { panel, uv, .. } => Some((panel, uv)),
        _ => None,
    });
    let (panel, at) = pop.ok_or("yank did not pop out")?;
    let hwnd = (panel == DESK).then(|| dvr_capture_win::window_at(screen.pixel_at(at))).flatten();
    if hwnd != Some(window.hwnd) {
        return Err(format!("popped window {hwnd:?} is not the test window {}", window.hwnd));
    }
    let gpu = dvr_gfx_d3d11::Gpu::new(None).map_err(err)?;
    let rect = dvr_capture_win::window_rect(window.hwnd).ok_or("test window has no rectangle")?;
    if (rect.w, rect.h) != (win.w, win.h) {
        return Err(format!("window rectangle {rect:?} does not match {win:?}"));
    }
    let mut tex = gpu.texture(rect.w, rect.h, None).map_err(err)?;
    let mut capture = dvr_capture_win::Capture::window(&gpu.device, window.hwnd).map_err(err)?;
    for _ in 0..100 {
        match capture.copy_latest(&gpu.context, &tex).map_err(err)? {
            dvr_capture_win::Frame::Copied => return Ok(()),
            dvr_capture_win::Frame::Resized(w, h) => tex = gpu.texture(w, h, None).map_err(err)?,
            dvr_capture_win::Frame::Unchanged => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    Err("popped-out window produced no capture frame".into())
}
