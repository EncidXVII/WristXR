// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

//! VR keyboard view: same toolkit and fonts as the menu (lowercase, Cyrillic, even sizes). Input stays in the core
//! engine; this view only shows state: labels, lit keys (latched modifiers, held keys, Chat mode) and laser hover.
use crate::{Canvas, ensure_platform};
use slint::{ComponentHandle, ModelRc, VecModel};
use wxr_core::keyboard::{ROW_UNITS, ROWS};

slint::slint! {
    import "../fonts/Barlow-Regular.ttf";
    import "../fonts/Barlow-SemiBold.ttf";
    import "../fonts/Anton-Regular.ttf";
    // Accent from Setup > Theme (lime by default).
    export global Theme { in property <color> accent: #C6FF1A; }
    export struct KeyView { label: string, x: length, y: length, w: length, lit: bool, hover: bool, small: bool }

    export component KeyboardView inherits Window {
        default-font-family: "Barlow";
        in property <[KeyView]> keys;
        width: 1500px;
        height: 600px;
        // Rounded board and keys, like the menu and toolbar; the corners are see-through.
        background: transparent;
        Rectangle { x: 0px; y: 0px; width: 1500px; height: 600px; border-radius: 28px; background: #0B0B0C; }
        for key in root.keys : Rectangle {
            x: key.x + 6px;
            y: key.y + 6px;
            width: key.w - 12px;
            height: 88px;
            border-radius: 14px;
            background: key.lit ? Theme.accent : key.hover ? #2A2A2E : #1D1D20;
            border-width: key.hover ? 4px : 0px;
            border-color: Theme.accent;
            Text {
                text: key.label;
                font-size: key.small ? 28px : 44px;
                color: key.lit ? #0B0B0C : key.small ? #A8A8AF : #F5F5F7;
                overflow: elide;
            }
        }
    }
}

/// 100 px per key unit.
pub const KEYBOARD_SIZE: (u32, u32) = (1500, 600);
const UNIT: f32 = 100.0;

/// What one key shows, in `wxr_core::keyboard::ROWS` order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyFace {
    pub label: String,
    pub lit: bool,
    pub hover: bool,
}

pub struct Keyboard {
    view: KeyboardView,
    canvas: Canvas,
    shown: Vec<KeyFace>,
}

impl Keyboard {
    pub fn new() -> Result<Self, String> {
        ensure_platform();
        let view = KeyboardView::new().map_err(|e| e.to_string())?;
        let canvas = Canvas::take(KEYBOARD_SIZE)?;
        view.show().map_err(|e| e.to_string())?;
        Ok(Self { view, canvas, shown: Vec::new() })
    }

    /// Accent colour (0xRRGGBB), from Setup > Theme.
    pub fn set_accent(&self, rgb: u32) {
        self.view.global::<Theme>().set_accent(crate::rgb(rgb));
    }

    /// Updates the faces (one per key, ROWS order); a no-op when nothing changed.
    pub fn set(&mut self, faces: &[KeyFace]) {
        if faces == self.shown.as_slice() {
            return;
        }
        let mut views = Vec::with_capacity(faces.len());
        let mut faces_iter = faces.iter();
        for (r, row) in ROWS.iter().enumerate() {
            let mut left = 0.0;
            for key in row.iter() {
                let face = faces_iter.next().cloned().unwrap_or_default();
                views.push(KeyView {
                    small: face.label.chars().count() > 1 || key.w > 1.0,
                    label: face.label.into(),
                    x: left * UNIT,
                    y: r as f32 * UNIT,
                    w: key.w * UNIT,
                    lit: face.lit,
                    hover: face.hover,
                });
                left += key.w;
            }
        }
        debug_assert!((ROW_UNITS * UNIT - KEYBOARD_SIZE.0 as f32).abs() < 1.0);
        self.view.set_keys(ModelRc::new(VecModel::from(views)));
        self.shown = faces.to_vec();
    }

    /// BGRA pixels (`KEYBOARD_SIZE`) when the view changed since the last call.
    pub fn render(&mut self) -> Option<&[u8]> {
        self.canvas.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_only_on_change() {
        let mut kb = Keyboard::new().expect("keyboard");
        let mut faces: Vec<KeyFace> = ROWS
            .iter()
            .flat_map(|r| r.iter())
            .map(|k| KeyFace { label: k.label.into(), ..KeyFace::default() })
            .collect();
        kb.set(&faces);
        assert!(kb.render().is_some());
        kb.set(&faces);
        assert!(kb.render().is_none(), "same faces must not redraw");
        faces[0].hover = true;
        kb.set(&faces);
        assert!(kb.render().is_some(), "hover must redraw");
    }
}
