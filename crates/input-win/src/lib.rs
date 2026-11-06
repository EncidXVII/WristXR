//! Mouse and keyboard injection with SendInput (physical pixels, set-1 scancodes).
#![cfg(windows)]
#![allow(unsafe_code)]

use dvr_core::keyboard::EXT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VSC_TO_VK_EX, MOUSE_EVENT_FLAGS,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, MapVirtualKeyExW, SendInput,
    ToUnicodeEx, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetSystemMetrics, GetWindowThreadProcessId, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

const WHEEL_DELTA: i32 = 120;

pub struct Injector {
    last: Option<(i32, i32)>,
}

fn send(inputs: &[INPUT]) {
    // SAFETY: fully initialized INPUT records.
    unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
}

fn mouse(dx: i32, dy: i32, data: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

impl Default for Injector {
    fn default() -> Self {
        Self::new()
    }
}

impl Injector {
    pub fn new() -> Self {
        Self { last: None }
    }

    /// Absolute move over the whole virtual desktop; skipped when the pixel did not change.
    pub fn move_to(&mut self, p: (i32, i32)) {
        if self.last == Some(p) {
            return;
        }
        self.last = Some(p);
        // SAFETY: plain metric queries.
        let (vx, vy, vw, vh) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2),
                GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2),
            )
        };
        let nx = ((p.0 - vx) as i64 * 65535 / (vw - 1) as i64) as i32;
        let ny = ((p.1 - vy) as i64 * 65535 / (vh - 1) as i64) as i32;
        send(&[mouse(nx, ny, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK)]);
    }

    pub fn button(&mut self, p: (i32, i32), right: bool, down: bool) {
        self.move_to(p);
        let flags = match (right, down) {
            (false, true) => MOUSEEVENTF_LEFTDOWN,
            (false, false) => MOUSEEVENTF_LEFTUP,
            (true, true) => MOUSEEVENTF_RIGHTDOWN,
            (true, false) => MOUSEEVENTF_RIGHTUP,
        };
        send(&[mouse(0, 0, 0, flags)]);
    }

    pub fn scroll(&mut self, p: (i32, i32), notches: i32) {
        self.move_to(p);
        send(&[mouse(0, 0, notches * WHEEL_DELTA, MOUSEEVENTF_WHEEL)]);
    }

    pub fn key(&self, scancode: u16, down: bool) {
        let mut flags = KEYEVENTF_SCANCODE;
        if scancode & EXT != 0 {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        send(&[INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: scancode & 0xFF,
                    dwFlags: KEYBD_EVENT_FLAGS(flags.0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }]);
    }
}

/// Character a key produces in the foreground window's keyboard layout (unshifted).
pub fn key_label(scancode: u16) -> Option<String> {
    let code = u32::from(scancode & 0xFF) | if scancode & EXT != 0 { 0xE000 } else { 0 };
    // SAFETY: read-only layout queries; ToUnicodeEx flag 4 leaves the keyboard state untouched.
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let layout = GetKeyboardLayout(thread);
        let vk = MapVirtualKeyExW(code, MAPVK_VSC_TO_VK_EX, Some(layout));
        if vk == 0 {
            return None;
        }
        let state = [0u8; 256];
        let mut buf = [0u16; 8];
        let n = ToUnicodeEx(vk, code, &state, &mut buf, 4, Some(layout));
        (n > 0).then(|| String::from_utf16_lossy(&buf[..n as usize]))
    }
}
