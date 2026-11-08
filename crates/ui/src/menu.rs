//! In-VR menu: tabs, a title, and a fixed 2x3 grid of large buttons. Content and wording come from the app;
//! this view only draws and reports which tab/slot was clicked. Geometry is fixed so tests can click it.
use crate::{Canvas, ensure_platform};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

slint::slint! {
    // kind: 0 action, 1 on/off setting, 2 choice (lit = chosen), 3 danger (on = armed, tap again), 4 hidden
    export struct SlotView { label: string, on: bool, enabled: bool, kind: int }

    export component MenuView inherits Window {
        in property <[string]> tabs;
        in property <int> tab;
        in property <string> heading;
        in property <string> subtitle;
        in property <[SlotView]> slots;
        in property <string> body;
        in property <string> on-text;
        in property <string> off-text;
        in property <string> close-text;
        callback tab-clicked(int);
        callback close-clicked;
        callback slot-clicked(int);
        width: 800px;
        height: 600px;
        background: #0C0B0A;

        for name[i] in root.tabs : Rectangle {
            x: i * 200px;
            y: 0px;
            width: 200px;
            height: 88px;
            background: i == root.tab ? #1F1C18 : tab-touch.has-hover ? #15130F : transparent;
            Text { text: name; font-size: 28px; font-weight: i == root.tab ? 700 : 400; color: i == root.tab ? #C6FF1A : #AAA49B; }
            tab-touch := TouchArea { clicked => { root.tab-clicked(i); } }
        }
        Text { x: 32px; y: 104px; width: 600px; text: root.heading; font-size: 40px; font-weight: 700; color: #F4F1EC; overflow: elide; }
        if root.tabs.length > 0 : Rectangle {
            x: 648px; y: 100px; width: 120px; height: 56px;
            background: close-touch.has-hover ? #2A2622 : #1F1C18;
            Text { text: root.close-text; font-size: 26px; color: #F4F1EC; }
            close-touch := TouchArea { clicked => { root.close-clicked(); } }
        }
        Text { x: 32px; y: 160px; width: 736px; text: root.subtitle; font-size: 28px; color: #AAA49B; overflow: elide; }
        if root.body != "" : Text {
            x: 32px; y: 216px; width: 736px; height: 240px;
            text: root.body; font-size: 28px; color: #F4F1EC; wrap: word-wrap;
        }
        for slot[i] in root.slots : Rectangle {
            x: 32px + mod(i, 2) * 376px;
            y: 216px + floor(i / 2) * 128px;
            width: 360px;
            height: 112px;
            visible: slot.kind != 4;
            opacity: slot.enabled ? 1.0 : 0.4;
            background: slot.kind == 2 && slot.on ? #C6FF1A
                : slot.kind == 3 && slot.on ? #FF5A3C
                : touch.has-hover && slot.enabled ? #2A2622 : #1F1C18;
            if slot.kind != 1 : Text {
                x: 24px;
                width: parent.width - 48px;
                text: slot.label;
                horizontal-alignment: center;
                overflow: elide;
                font-size: 30px;
                color: slot.on && (slot.kind == 2 || slot.kind == 3) ? #0C0B0A : slot.kind == 3 ? #FF5A3C : #F4F1EC;
            }
            if slot.kind == 1 : Text {
                x: 24px; width: parent.width - 120px; text: slot.label; font-size: 30px; color: #F4F1EC; overflow: elide;
            }
            if slot.kind == 1 : Text {
                x: parent.width - self.width - 24px;
                text: slot.on ? root.on-text : root.off-text;
                font-size: 28px;
                font-weight: slot.on ? 700 : 400;
                color: slot.on ? #C6FF1A : #AAA49B;
            }
            touch := TouchArea { enabled: slot.enabled && slot.kind != 4; clicked => { root.slot-clicked(i); } }
        }
    }
}

pub const MENU_SIZE: (u32, u32) = (800, 600);
pub const SLOTS: usize = 6;

/// Center of tab `i` in menu pixels (matches the Slint layout above).
pub fn tab_center(i: usize) -> (f32, f32) {
    (i as f32 * 200.0 + 100.0, 44.0)
}

/// Center of the Close button (shown with the tabs, not during first run).
pub fn close_center() -> (f32, f32) {
    (708.0, 128.0)
}

/// Center of button slot `i` (row-major, 2 columns).
pub fn slot_center(i: usize) -> (f32, f32) {
    (32.0 + (i % 2) as f32 * 376.0 + 180.0, 216.0 + (i / 2) as f32 * 128.0 + 56.0)
}

/// How a button looks and what `on` means for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlotKind {
    /// One-off action.
    #[default]
    Action,
    /// Setting with an On/Off readout.
    Toggle,
    /// One of several options; `on` = chosen (accent fill).
    Choice,
    /// Destructive; `on` = armed (tap again to confirm).
    Danger,
    /// Empty grid cell.
    Hidden,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub label: String,
    pub on: bool,
    pub enabled: bool,
    pub kind: SlotKind,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuModel {
    /// Empty during first run (no tabs).
    pub tabs: Vec<String>,
    pub tab: usize,
    pub title: String,
    pub subtitle: String,
    /// Multi-line text above the bottom row (tips); empty for button pages.
    pub body: String,
    pub slots: Vec<Slot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuInput {
    Tab(usize),
    Slot(usize),
    Close,
}

pub struct Menu {
    view: MenuView,
    canvas: Canvas,
    inputs: Rc<RefCell<Vec<MenuInput>>>,
    shown: MenuModel,
}

impl Menu {
    pub fn new() -> Result<Self, String> {
        ensure_platform();
        let view = MenuView::new().map_err(|e| e.to_string())?;
        let canvas = Canvas::take(MENU_SIZE)?;
        view.show().map_err(|e| e.to_string())?;
        let inputs = Rc::new(RefCell::new(Vec::new()));
        let q = inputs.clone();
        view.on_tab_clicked(move |i| q.borrow_mut().push(MenuInput::Tab(i.max(0) as usize)));
        let q = inputs.clone();
        view.on_slot_clicked(move |i| q.borrow_mut().push(MenuInput::Slot(i.max(0) as usize)));
        let q = inputs.clone();
        view.on_close_clicked(move || q.borrow_mut().push(MenuInput::Close));
        let strings = crate::Strings::english()?;
        view.set_on_text(strings.get("menu-on", &[]).into());
        view.set_off_text(strings.get("menu-off", &[]).into());
        view.set_close_text(strings.get("menu-close", &[]).into());
        Ok(Self { view, canvas, inputs, shown: MenuModel::default() })
    }

    /// Updates the view; a no-op when nothing changed.
    pub fn set(&mut self, m: &MenuModel) {
        if *m == self.shown {
            return;
        }
        let strings: Vec<SharedString> = m.tabs.iter().map(SharedString::from).collect();
        self.view.set_tabs(ModelRc::new(VecModel::from(strings)));
        self.view.set_tab(m.tab as i32);
        self.view.set_heading(m.title.as_str().into());
        self.view.set_subtitle(m.subtitle.as_str().into());
        self.view.set_body(m.body.as_str().into());
        let slots: Vec<SlotView> = m
            .slots
            .iter()
            .take(SLOTS)
            .map(|s| SlotView { label: s.label.as_str().into(), on: s.on, enabled: s.enabled, kind: s.kind as i32 })
            .collect();
        self.view.set_slots(ModelRc::new(VecModel::from(slots)));
        self.shown = m.clone();
    }

    /// Laser moved over the menu (uv in 0..1).
    pub fn pointer_move(&self, uv: [f32; 2]) {
        self.canvas.pointer_move(uv);
    }

    pub fn pointer_button(&self, uv: [f32; 2], down: bool) {
        self.canvas.pointer_button(uv, down);
    }

    pub fn pointer_leave(&self) {
        self.canvas.pointer_leave();
    }

    /// Clicks since the last call, in order.
    pub fn take_inputs(&self) -> Vec<MenuInput> {
        std::mem::take(&mut *self.inputs.borrow_mut())
    }

    /// BGRA pixels (`MENU_SIZE`) when the view changed since the last call.
    pub fn render(&mut self) -> Option<&[u8]> {
        self.canvas.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uv((x, y): (f32, f32)) -> [f32; 2] {
        [x / MENU_SIZE.0 as f32, y / MENU_SIZE.1 as f32]
    }

    fn click(m: &Menu, at: (f32, f32)) {
        m.pointer_move(uv(at));
        m.pointer_button(uv(at), true);
        m.pointer_button(uv(at), false);
    }

    #[test]
    fn clicks_map_to_tabs_and_slots_and_hover_never_clicks() {
        let mut menu = Menu::new().expect("menu");
        let slot = |label: &str, enabled| Slot { label: label.into(), on: false, enabled, kind: SlotKind::Action };
        menu.set(&MenuModel {
            tabs: vec!["A".into(), "B".into(), "C".into(), "D".into()],
            tab: 0,
            title: "Title".into(),
            subtitle: String::new(),
            body: String::new(),
            slots: vec![
                slot("1", true),
                slot("2", true),
                slot("3", true),
                slot("4", true),
                slot("5", false),
                slot("6", true),
            ],
        });
        assert!(menu.render().is_some());
        for i in 0..SLOTS {
            menu.pointer_move(uv(slot_center(i)));
        }
        assert!(menu.take_inputs().is_empty(), "hover must never click");
        click(&menu, tab_center(3));
        click(&menu, slot_center(5));
        click(&menu, slot_center(4));
        click(&menu, slot_center(0));
        assert_eq!(menu.take_inputs(), vec![MenuInput::Tab(3), MenuInput::Slot(5), MenuInput::Slot(0)]);
    }
}
