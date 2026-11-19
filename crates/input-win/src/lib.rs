//! Mouse and keyboard injection with SendInput (physical pixels, set-1 scancodes).
#![cfg(windows)]
#![allow(unsafe_code)]

use dvr_core::engine::{Event, MouseButton};
use dvr_core::keyboard::EXT;
use dvr_core::panel::PixelRect;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Globalization::{LCIDToLocaleName, MAX_LOCALE_NAME};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VSC_TO_VK_EX, MOUSE_EVENT_FLAGS,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
    MOUSEINPUT, MapVirtualKeyExW, SendInput, ToUnicodeEx, VIRTUAL_KEY,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayoutList, HKL, VK_CAPITAL, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, SetCursorPos, WM_INPUTLANGCHANGEREQUEST,
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

    /// Exact physical-pixel positioning. SendInput's normalized absolute coordinates are off by several pixels
    /// (or clamp at monitor edges) on mixed-DPI desktops, so the cursor is placed with SetCursorPos instead.
    fn place(p: (i32, i32)) {
        // SAFETY: plain cursor call; the process is per-monitor DPI aware.
        unsafe {
            let _ = SetCursorPos(p.0, p.1);
        }
    }

    /// Hover: moves the cursor; skipped when the pixel did not change.
    pub fn move_to(&mut self, p: (i32, i32)) {
        if self.last != Some(p) {
            self.last = Some(p);
            Self::place(p);
        }
    }

    /// Always re-places the cursor right before the button, in case the physical mouse moved it.
    pub fn button(&mut self, p: (i32, i32), right: bool, down: bool) {
        let flags = match (right, down) {
            (false, true) => MOUSEEVENTF_LEFTDOWN,
            (false, false) => MOUSEEVENTF_LEFTUP,
            (true, true) => MOUSEEVENTF_RIGHTDOWN,
            (true, false) => MOUSEEVENTF_RIGHTUP,
        };
        self.last = Some(p);
        Self::place(p);
        send(&[mouse(0, 0, 0, flags)]);
    }

    pub fn scroll(&mut self, p: (i32, i32), notches: i32) {
        self.last = Some(p);
        Self::place(p);
        send(&[mouse(0, 0, notches * WHEEL_DELTA, MOUSEEVENTF_WHEEL)]);
    }

    /// Applies a core engine event to the OS. `rect` maps a panel id to its desktop rectangle;
    /// events on panels without one (keyboard, placeholders) are ignored. Returns false for non-input events.
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

/// Keyboard layout of the foreground window; changes when the user switches language.
pub fn layout_id() -> isize {
    foreground_layout().0 as isize
}

fn foreground_layout() -> HKL {
    // SAFETY: read-only queries.
    unsafe { GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), None)) }
}

fn short_name(layout: HKL) -> String {
    let lang = (layout.0 as usize & 0xFFFF) as u32;
    let mut buf = [0u16; MAX_LOCALE_NAME as usize];
    // SAFETY: the buffer length is passed with the buffer.
    let n = unsafe { LCIDToLocaleName(lang, Some(&mut buf), 0) };
    let name = String::from_utf16_lossy(&buf[..(n.max(1) as usize - 1).min(buf.len())]);
    name.split('-').next().filter(|s| !s.is_empty()).unwrap_or("??").to_uppercase()
}

fn layout_list() -> Vec<HKL> {
    // SAFETY: the list buffer is sized from the first call.
    unsafe {
        let count = GetKeyboardLayoutList(None).max(0) as usize;
        let mut list = vec![HKL::default(); count];
        let n = GetKeyboardLayoutList(Some(&mut list)).max(0) as usize;
        list.truncate(n);
        list
    }
}

/// Short name of the foreground window's input language, e.g. "EN", "UK", "DE".
pub fn layout_name() -> String {
    short_name(foreground_layout())
}

/// Short names of the installed input languages, in Windows' order.
pub fn layouts() -> Vec<String> {
    layout_list().into_iter().map(short_name).collect()
}

fn request(layout: HKL) -> bool {
    // SAFETY: posting a message to a window is harmless.
    unsafe {
        PostMessageW(Some(GetForegroundWindow()), WM_INPUTLANGCHANGEREQUEST, WPARAM(0), LPARAM(layout.0 as isize))
            .is_ok()
    }
}

/// Asks the foreground window to switch to the next installed input language (what Win+Space does).
/// Returns false when only one language is installed.
pub fn next_layout() -> bool {
    let list = layout_list();
    if list.len() < 2 {
        return false;
    }
    let now = foreground_layout();
    request(list.iter().position(|h| *h == now).map_or(list[0], |i| list[(i + 1) % list.len()]))
}

/// Asks the foreground window to switch to installed language `i` (as listed by [`layouts`]).
pub fn select_layout(i: usize) -> bool {
    layout_list().get(i).is_some_and(|h| request(*h))
}

fn translate(scancode: u16, shift: bool, caps: bool) -> Option<String> {
    let code = u32::from(scancode & 0xFF) | if scancode & EXT != 0 { 0xE000 } else { 0 };
    // SAFETY: read-only layout queries; ToUnicodeEx flag 4 leaves the keyboard state untouched.
    unsafe {
        let layout = foreground_layout();
        let vk = MapVirtualKeyExW(code, MAPVK_VSC_TO_VK_EX, Some(layout));
        if vk == 0 {
            return None;
        }
        let mut state = [0u8; 256];
        state[usize::from(VK_SHIFT.0)] = if shift { 0x80 } else { 0 };
        state[usize::from(VK_CAPITAL.0)] = u8::from(caps);
        let mut buf = [0u16; 8];
        let n = ToUnicodeEx(vk, code, &state, &mut buf, 4, Some(layout));
        (n > 0).then(|| String::from_utf16_lossy(&buf[..n as usize]))
    }
}

/// Character a key types with the given Shift/Caps state; dead keys and multi-char results yield `None`.
pub fn key_char(scancode: u16, shift: bool, caps: bool) -> Option<char> {
    let s = translate(scancode, shift, caps)?;
    let mut chars = s.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

/// Caps Lock is on (shown on the VR keyboard).
pub fn caps_lock() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
    // SAFETY: plain query; the low bit is the toggle state.
    unsafe { GetKeyState(i32::from(VK_CAPITAL.0)) & 1 != 0 }
}

fn elevated(process: windows::Win32::Foundation::HANDLE) -> Option<bool> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows::Win32::System::Threading::OpenProcessToken;
    let mut token = Default::default();
    let mut e = TOKEN_ELEVATION::default();
    let mut len = 0u32;
    // SAFETY: the token handle is closed below; the out-buffer is a TOKEN_ELEVATION of the size given.
    unsafe {
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some((&raw mut e).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        let _ = CloseHandle(token);
        ok.ok()?;
    }
    Some(e.TokenIsElevated != 0)
}

/// The title of the window at this desktop point when it belongs to a program running as administrator and we do
/// not: Windows then silently drops our clicks and keys on it (UIPI), so the user can be told why nothing happens.
pub fn admin_window_at(p: (i32, i32)) -> Option<String> {
    use windows::Win32::Foundation::{CloseHandle, POINT};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, GetWindowTextW, WindowFromPoint};
    // SAFETY: the pseudo-handle of this process needs no closing.
    if elevated(unsafe { GetCurrentProcess() })? {
        return None;
    }
    // SAFETY: plain window and process queries; the process handle is closed below.
    unsafe {
        let hwnd = GetAncestor(WindowFromPoint(POINT { x: p.0, y: p.1 }), GA_ROOT);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let admin = elevated(process);
        let _ = CloseHandle(process);
        if admin != Some(true) {
            return None;
        }
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(hwnd, &mut buf);
        Some(String::from_utf16_lossy(&buf[..n.max(0) as usize]))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn language_name_is_short_code() {
        let name = super::layout_name();
        assert!((2..=3).contains(&name.len()) && name.chars().all(|c| c.is_ascii_uppercase()), "{name}");
        eprintln!("input language: {name}");
    }
}
