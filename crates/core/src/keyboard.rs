// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keyboard geometry and modifier logic. Keys are PC set-1 scancodes so the OS layout
//! (US, UA, DE, ...) decides the produced character. Linux evdev codes equal set-1 for
//! the main block; extended keys carry the `EXT` flag.
use crate::engine::Event;

pub const EXT: u16 = 0xE000;
pub const ESC: u16 = 0x01;
pub const BACKSPACE: u16 = 0x0E;
pub const TAB: u16 = 0x0F;
pub const ENTER: u16 = 0x1C;
pub const LCTRL: u16 = 0x1D;
pub const LSHIFT: u16 = 0x2A;
pub const RSHIFT: u16 = 0x36;
pub const LALT: u16 = 0x38;
pub const SPACE: u16 = 0x39;
pub const CAPS: u16 = 0x3A;
pub const UP: u16 = EXT | 0x48;
pub const LEFT: u16 = EXT | 0x4B;
pub const RIGHT: u16 = EXT | 0x4D;
pub const DOWN: u16 = EXT | 0x50;
pub const DEL: u16 = EXT | 0x53;
/// Windows key: a latch like Ctrl (Win + next key); tapped again while lit it presses Win alone (Start menu).
pub const LWIN: u16 = EXT | 0x5B;
pub const F1: u16 = 0x3B;
/// Not a real key: toggles "send to VRChat chatbox" and is never sent to the OS.
pub const CHAT: u16 = 0xF100;
/// Not a real key: switches to the next installed input language; its face shows the current one.
pub const LANG: u16 = 0xF101;

pub struct KeyDef {
    pub sc: u16,
    /// Width in key units.
    pub w: f32,
    /// US fallback label; the app replaces it with the active OS layout's label.
    pub label: &'static str,
}

const fn k(sc: u16, label: &'static str) -> KeyDef {
    KeyDef { sc, w: 1.0, label }
}
const fn kw(sc: u16, w: f32, label: &'static str) -> KeyDef {
    KeyDef { sc, w, label }
}

pub const ROW_UNITS: f32 = 15.0;

#[rustfmt::skip]
pub static ROWS: [&[KeyDef]; 6] = [
    &[kw(ESC, 1.5, "Esc"), k(F1, "F1"), k(0x3C, "F2"), k(0x3D, "F3"), k(0x3E, "F4"), k(0x3F, "F5"), k(0x40, "F6"),
      k(0x41, "F7"), k(0x42, "F8"), k(0x43, "F9"), k(0x44, "F10"), k(0x57, "F11"), k(0x58, "F12"), kw(DEL, 1.5, "Del")],
    &[k(0x29, "`"), k(0x02, "1"), k(0x03, "2"), k(0x04, "3"), k(0x05, "4"), k(0x06, "5"), k(0x07, "6"),
      k(0x08, "7"), k(0x09, "8"), k(0x0A, "9"), k(0x0B, "0"), k(0x0C, "-"), k(0x0D, "="), kw(BACKSPACE, 2.0, "Bksp")],
    &[kw(TAB, 1.5, "Tab"), k(0x10, "q"), k(0x11, "w"), k(0x12, "e"), k(0x13, "r"), k(0x14, "t"), k(0x15, "y"),
      k(0x16, "u"), k(0x17, "i"), k(0x18, "o"), k(0x19, "p"), k(0x1A, "["), k(0x1B, "]"), kw(0x2B, 1.5, "\\")],
    &[kw(CAPS, 1.75, "Caps"), k(0x1E, "a"), k(0x1F, "s"), k(0x20, "d"), k(0x21, "f"), k(0x22, "g"), k(0x23, "h"),
      k(0x24, "j"), k(0x25, "k"), k(0x26, "l"), k(0x27, ";"), k(0x28, "'"), kw(ENTER, 2.25, "Enter")],
    &[kw(LSHIFT, 1.25, "Shift"), k(0x56, "<"), k(0x2C, "z"), k(0x2D, "x"), k(0x2E, "c"), k(0x2F, "v"), k(0x30, "b"),
      k(0x31, "n"), k(0x32, "m"), k(0x33, ","), k(0x34, "."), k(0x35, "/"), kw(RSHIFT, 2.75, "Shift")],
    &[kw(LCTRL, 1.25, "Ctrl"), kw(LWIN, 1.25, "Win"), kw(LALT, 1.25, "Alt"), kw(SPACE, 5.25, " "), k(CHAT, "Chat"),
      k(LANG, "Lang"), k(LEFT, "←"), k(UP, "↑"), k(DOWN, "↓"), k(RIGHT, "→")],
];

pub fn key_at(uv: [f32; 2]) -> Option<&'static KeyDef> {
    if !(0.0..1.0).contains(&uv[0]) || !(0.0..1.0).contains(&uv[1]) {
        return None;
    }
    let row = ROWS[(uv[1] * ROWS.len() as f32) as usize];
    let x = uv[0] * ROW_UNITS;
    let mut edge = 0.0;
    row.iter().find(|key| {
        edge += key.w;
        x < edge
    })
}

/// Center uv and half-extent uv of a key.
pub fn key_rect(sc: u16) -> Option<([f32; 2], [f32; 2])> {
    let rows = ROWS.len() as f32;
    for (r, row) in ROWS.iter().enumerate() {
        let mut left = 0.0;
        for key in row.iter() {
            if key.sc == sc {
                let center = [(left + key.w * 0.5) / ROW_UNITS, (r as f32 + 0.5) / rows];
                return Some((center, [key.w * 0.5 / ROW_UNITS, 0.5 / rows]));
            }
            left += key.w;
        }
    }
    None
}

pub fn key_for_label(label: &str) -> Option<&'static KeyDef> {
    ROWS.iter().flat_map(|r| r.iter()).find(|k| k.label == label)
}

const MODS: [u16; 4] = [LSHIFT, LCTRL, LALT, LWIN];
/// Win tapped while lit: a plain Win press (not a latch toggle).
const PLAIN: u8 = 0x80;

fn modifier_bit(sc: u16) -> Option<u8> {
    match sc {
        LSHIFT | RSHIFT => Some(1),
        LCTRL => Some(2),
        LALT => Some(4),
        LWIN => Some(8),
        _ => None,
    }
}

pub fn repeats(sc: u16) -> bool {
    modifier_bit(sc).is_none() && sc != CAPS && sc != CHAT && sc != LANG
}

/// Shift/Ctrl/Alt are one-shot latches: tap to arm, the next key consumes them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Keyboard {
    latched: u8,
}

impl Keyboard {
    /// Bit 1 = Shift, 2 = Ctrl, 4 = Alt, 8 = Win.
    pub fn latched(&self) -> u8 {
        self.latched
    }

    /// Emits key-down events; returns the modifier mask to pass to `release`.
    pub fn press(&mut self, sc: u16, out: &mut Vec<Event>) -> u8 {
        if sc == CHAT {
            out.push(Event::ChatToggle);
            return 0;
        }
        if sc == LANG {
            out.push(Event::NextLanguage);
            return 0;
        }
        if let Some(bit) = modifier_bit(sc) {
            if !(sc == LWIN && self.latched & bit != 0) {
                self.latched ^= bit;
                return 0;
            }
            // Win, Win: open Start (Win pressed on its own, with whatever else is latched).
            self.latched &= !bit;
            let mods = std::mem::take(&mut self.latched);
            for (i, m) in MODS.iter().enumerate() {
                if mods & (1 << i) != 0 {
                    out.push(Event::Key { scancode: *m, down: true });
                }
            }
            out.push(Event::Key { scancode: sc, down: true });
            return mods | PLAIN;
        }
        let mods = std::mem::take(&mut self.latched);
        for (i, m) in MODS.iter().enumerate() {
            if mods & (1 << i) != 0 {
                out.push(Event::Key { scancode: *m, down: true });
            }
        }
        out.push(Event::Key { scancode: sc, down: true });
        mods
    }

    pub fn release(&mut self, sc: u16, mods: u8, out: &mut Vec<Event>) {
        if sc == CHAT || sc == LANG || (modifier_bit(sc).is_some() && mods & PLAIN == 0) {
            return;
        }
        out.push(Event::Key { scancode: sc, down: false });
        for (i, m) in MODS.iter().enumerate().rev() {
            if mods & (1 << i) != 0 {
                out.push(Event::Key { scancode: *m, down: false });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_fill_the_board() {
        for row in ROWS.iter() {
            let units: f32 = row.iter().map(|k| k.w).sum();
            assert!((units - ROW_UNITS).abs() < 1e-4);
        }
    }

    #[test]
    fn win_latches_and_win_win_opens_start() {
        let mut kb = Keyboard::default();
        let mut out = Vec::new();
        assert_eq!(kb.press(LWIN, &mut out), 0);
        assert_eq!(kb.latched(), 8);
        let mods = kb.press(0x20, &mut out); // Win + D
        kb.release(0x20, mods, &mut out);
        let keys: Vec<(u16, bool)> = out
            .iter()
            .filter_map(|e| if let Event::Key { scancode, down } = *e { Some((scancode, down)) } else { None })
            .collect();
        assert_eq!(keys, vec![(LWIN, true), (0x20, true), (0x20, false), (LWIN, false)]);
        out.clear();
        kb.press(LWIN, &mut out);
        let mods = kb.press(LWIN, &mut out);
        kb.release(LWIN, mods, &mut out);
        assert_eq!(kb.latched(), 0);
        assert_eq!(out, vec![Event::Key { scancode: LWIN, down: true }, Event::Key { scancode: LWIN, down: false }]);
    }

    #[test]
    fn every_key_center_maps_back() {
        for key in ROWS.iter().flat_map(|r| r.iter()) {
            let (c, _) = key_rect(key.sc).expect("key exists");
            assert_eq!(key_at(c).map(|k| k.sc), Some(key.sc));
        }
    }
}
