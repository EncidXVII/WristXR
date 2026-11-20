// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keys: WristXR's set-1 scancodes to Linux evdev key codes, and what a key types on a US layout.
use wxr_core::keyboard::EXT;

/// The evdev key code for a set-1 scancode. The main block is the same in both; extended keys differ.
pub fn evdev(scancode: u16) -> Option<u16> {
    let code = scancode & 0xFF;
    if scancode & EXT == 0 {
        return (1..=0x58).contains(&code).then_some(code);
    }
    Some(match code {
        0x1C => 96,  // keypad Enter
        0x1D => 97,  // right Ctrl
        0x35 => 98,  // keypad /
        0x38 => 100, // right Alt
        0x47 => 102, // Home
        0x48 => 103, // Up
        0x49 => 104, // Page Up
        0x4B => 105, // Left
        0x4D => 106, // Right
        0x4F => 107, // End
        0x50 => 108, // Down
        0x51 => 109, // Page Down
        0x52 => 110, // Insert
        0x53 => 111, // Delete
        0x5B => 125, // left Super
        0x5C => 126, // right Super
        0x5D => 127, // Menu
        _ => return None,
    })
}

/// US layout: (unshifted, shifted) for the keys that type a character.
const US: &[(u16, char, char)] = &[
    (0x02, '1', '!'),
    (0x03, '2', '@'),
    (0x04, '3', '#'),
    (0x05, '4', '$'),
    (0x06, '5', '%'),
    (0x07, '6', '^'),
    (0x08, '7', '&'),
    (0x09, '8', '*'),
    (0x0A, '9', '('),
    (0x0B, '0', ')'),
    (0x0C, '-', '_'),
    (0x0D, '=', '+'),
    (0x10, 'q', 'Q'),
    (0x11, 'w', 'W'),
    (0x12, 'e', 'E'),
    (0x13, 'r', 'R'),
    (0x14, 't', 'T'),
    (0x15, 'y', 'Y'),
    (0x16, 'u', 'U'),
    (0x17, 'i', 'I'),
    (0x18, 'o', 'O'),
    (0x19, 'p', 'P'),
    (0x1A, '[', '{'),
    (0x1B, ']', '}'),
    (0x1E, 'a', 'A'),
    (0x1F, 's', 'S'),
    (0x20, 'd', 'D'),
    (0x21, 'f', 'F'),
    (0x22, 'g', 'G'),
    (0x23, 'h', 'H'),
    (0x24, 'j', 'J'),
    (0x25, 'k', 'K'),
    (0x26, 'l', 'L'),
    (0x27, ';', ':'),
    (0x28, '\'', '"'),
    (0x29, '`', '~'),
    (0x2B, '\\', '|'),
    (0x2C, 'z', 'Z'),
    (0x2D, 'x', 'X'),
    (0x2E, 'c', 'C'),
    (0x2F, 'v', 'V'),
    (0x30, 'b', 'B'),
    (0x31, 'n', 'N'),
    (0x32, 'm', 'M'),
    (0x33, ',', '<'),
    (0x34, '.', '>'),
    (0x35, '/', '?'),
    (0x39, ' ', ' '),
    (0x56, '<', '>'),
];

/// What the key types with Shift and Caps Lock as given, on a US layout. The keys a panel sends are key codes, so
/// apps get the user's real layout; this is only for the key labels and the VRChat chat line.
pub fn us_char(scancode: u16, shift: bool, caps: bool) -> Option<char> {
    let &(_, plain, shifted) = US.iter().find(|(sc, _, _)| *sc == scancode)?;
    let upper = shift ^ (caps && plain.is_ascii_alphabetic());
    Some(if upper { shifted } else { plain })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxr_core::keyboard::{DEL, ESC, LWIN, SPACE, UP};

    #[test]
    fn scancodes_map_to_evdev() {
        assert_eq!(evdev(ESC), Some(1));
        assert_eq!(evdev(SPACE), Some(57));
        assert_eq!(evdev(UP), Some(103));
        assert_eq!(evdev(DEL), Some(111));
        assert_eq!(evdev(LWIN), Some(125));
        assert_eq!(evdev(0xF100), None);
    }

    #[test]
    fn us_layout_types_with_shift_and_caps() {
        assert_eq!(us_char(0x1E, false, false), Some('a'));
        assert_eq!(us_char(0x1E, true, false), Some('A'));
        assert_eq!(us_char(0x1E, false, true), Some('A'));
        assert_eq!(us_char(0x1E, true, true), Some('a'));
        assert_eq!(us_char(0x02, false, true), Some('1'));
        assert_eq!(us_char(0x02, true, false), Some('!'));
    }
}
