//! Text line for "send to VRChat chatbox": built from the keyboard's key events.
//! Characters come from the OS layout (`to_char`), so UA/DE text works as typed.
use crate::keyboard::{BACKSPACE, CAPS, ENTER, LALT, LCTRL, LSHIFT, RSHIFT};

/// VRChat's chatbox limit.
pub const MAX_CHARS: usize = 144;

#[derive(Debug, PartialEq, Eq)]
pub enum ChatAction {
    None,
    Edited,
    Send(String),
}

#[derive(Debug, Default)]
pub struct ChatLine {
    text: String,
    shift: bool,
    ctrl: bool,
    alt: bool,
    caps: bool,
}

impl ChatLine {
    pub fn text(&self) -> &str {
        &self.text
    }

    /// `to_char(scancode, shift, caps)` maps a key to the character it types.
    pub fn key(&mut self, sc: u16, down: bool, to_char: impl Fn(u16, bool, bool) -> Option<char>) -> ChatAction {
        match sc {
            LSHIFT | RSHIFT => self.shift = down,
            LCTRL => self.ctrl = down,
            LALT => self.alt = down,
            _ if !down => {}
            CAPS => self.caps = !self.caps,
            ENTER => {
                if !self.text.is_empty() {
                    return ChatAction::Send(std::mem::take(&mut self.text));
                }
            }
            BACKSPACE => {
                if self.text.pop().is_some() {
                    return ChatAction::Edited;
                }
            }
            _ if self.ctrl || self.alt || self.text.chars().count() >= MAX_CHARS => {}
            _ => {
                if let Some(c) = to_char(sc, self.shift, self.caps).filter(|c| !c.is_control()) {
                    self.text.push(c);
                    return ChatAction::Edited;
                }
            }
        }
        ChatAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::key_for_label;

    fn us(sc: u16, shift: bool, caps: bool) -> Option<char> {
        let c = crate::keyboard::ROWS.iter().flat_map(|r| r.iter()).find(|k| k.sc == sc)?.label.chars().next()?;
        Some(if shift ^ caps { c.to_ascii_uppercase() } else { c })
    }

    fn tap(line: &mut ChatLine, label: &str) -> ChatAction {
        let sc = key_for_label(label).expect("key").sc;
        let a = line.key(sc, true, us);
        line.key(sc, false, us);
        a
    }

    #[test]
    fn types_edits_and_sends() {
        let mut line = ChatLine::default();
        line.key(LSHIFT, true, us);
        tap(&mut line, "h");
        line.key(LSHIFT, false, us);
        for l in ["i", " ", "x"] {
            tap(&mut line, l);
        }
        assert_eq!(tap(&mut line, "Bksp"), ChatAction::Edited);
        assert_eq!(line.text(), "Hi ");
        line.key(LCTRL, true, us);
        assert_eq!(tap(&mut line, "c"), ChatAction::None);
        line.key(LCTRL, false, us);
        assert_eq!(tap(&mut line, "Enter"), ChatAction::Send("Hi ".into()));
        assert_eq!(tap(&mut line, "Enter"), ChatAction::None);
        assert_eq!(tap(&mut line, "Bksp"), ChatAction::None);
        assert_eq!(line.text(), "");
    }
}
