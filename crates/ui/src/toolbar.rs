//! Toolbar shown under the panel the laser points at: one row of short buttons (lock, size, opacity, close...).
//! The app decides which buttons a panel gets; this view only draws them centred and reports clicks.
use crate::{Canvas, ensure_platform};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

slint::slint! {
    export struct ToolView { label: string, on: bool, danger: bool }

    export component ToolbarView inherits Window {
        in property <[ToolView]> buttons;
        callback clicked(int);
        width: 720px;
        height: 96px;
        background: transparent;
        for b[i] in root.buttons : Rectangle {
            x: (root.width - root.buttons.length * 104px + 8px) / 2 + i * 104px;
            y: 0px;
            width: 96px;
            height: 96px;
            background: b.on ? #C6FF1A : touch.has-hover ? #2A2622 : #1F1C18;
            Text {
                text: b.label;
                font-size: 26px;
                font-weight: b.on ? 700 : 400;
                color: b.on ? #0C0B0A : b.danger ? #FF5A3C : #F4F1EC;
            }
            touch := TouchArea { clicked => { root.clicked(i); } }
        }
    }
}

pub const TOOLBAR_SIZE: (u32, u32) = (720, 96);

/// Center of button `i` of `n`, in toolbar pixels (matches the layout above).
pub fn toolbar_button_center(i: usize, n: usize) -> (f32, f32) {
    let left = (TOOLBAR_SIZE.0 as f32 - n as f32 * 104.0 + 8.0) / 2.0;
    (left + i as f32 * 104.0 + 48.0, 48.0)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolButton {
    pub label: String,
    /// Setting is on (accent fill).
    pub on: bool,
    pub danger: bool,
}

pub struct Toolbar {
    view: ToolbarView,
    canvas: Canvas,
    inputs: Rc<RefCell<Vec<usize>>>,
    shown: Vec<ToolButton>,
}

impl Toolbar {
    pub fn new() -> Result<Self, String> {
        ensure_platform();
        let view = ToolbarView::new().map_err(|e| e.to_string())?;
        let canvas = Canvas::take(TOOLBAR_SIZE)?;
        view.show().map_err(|e| e.to_string())?;
        let inputs = Rc::new(RefCell::new(Vec::new()));
        let q = inputs.clone();
        view.on_clicked(move |i| q.borrow_mut().push(i.max(0) as usize));
        Ok(Self { view, canvas, inputs, shown: Vec::new() })
    }

    /// Updates the buttons; a no-op when nothing changed.
    pub fn set(&mut self, buttons: &[ToolButton]) {
        if buttons == self.shown {
            return;
        }
        let views: Vec<ToolView> =
            buttons.iter().map(|b| ToolView { label: b.label.as_str().into(), on: b.on, danger: b.danger }).collect();
        self.view.set_buttons(ModelRc::new(VecModel::from(views)));
        self.shown = buttons.to_vec();
    }

    pub fn pointer_move(&self, uv: [f32; 2]) {
        self.canvas.pointer_move(uv);
    }

    pub fn pointer_button(&self, uv: [f32; 2], down: bool) {
        self.canvas.pointer_button(uv, down);
    }

    pub fn pointer_leave(&self) {
        self.canvas.pointer_leave();
    }

    /// Indices of buttons clicked since the last call.
    pub fn take_inputs(&self) -> Vec<usize> {
        std::mem::take(&mut *self.inputs.borrow_mut())
    }

    /// BGRA pixels (`TOOLBAR_SIZE`) when the view changed since the last call.
    pub fn render(&mut self) -> Option<&[u8]> {
        self.canvas.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centred_buttons_report_their_index() {
        let mut bar = Toolbar::new().expect("toolbar");
        let b = |l: &str| ToolButton { label: l.into(), ..ToolButton::default() };
        bar.set(&[b("A"), b("B"), b("C"), b("D"), b("E")]);
        assert!(bar.render().is_some());
        for i in [4, 0, 2] {
            let (x, y) = toolbar_button_center(i, 5);
            let uv = [x / TOOLBAR_SIZE.0 as f32, y / TOOLBAR_SIZE.1 as f32];
            bar.pointer_move(uv);
            bar.pointer_button(uv, true);
            bar.pointer_button(uv, false);
        }
        assert_eq!(bar.take_inputs(), vec![4, 0, 2]);
    }
}
