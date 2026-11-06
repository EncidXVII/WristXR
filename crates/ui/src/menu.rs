//! In-VR menu: tabs, a title, and a fixed 2x3 grid of large buttons. Content and wording come from the app;
//! this view only draws and reports which tab/slot was clicked. Geometry is fixed so tests can click it.
use crate::{Canvas, ensure_platform};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

slint::slint! {
    export struct SlotView { label: string, on: bool, enabled: bool }

    export component MenuView inherits Window {
        in property <[string]> tabs;
        in property <int> tab;
        in property <string> heading;
        in property <string> subtitle;
        in property <[SlotView]> slots;
        callback tab-clicked(int);
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
        Text { x: 32px; y: 104px; text: root.heading; font-size: 40px; font-weight: 700; color: #F4F1EC; }
        Text { x: 32px; y: 160px; width: 736px; text: root.subtitle; font-size: 28px; color: #AAA49B; overflow: elide; }
        for slot[i] in root.slots : Rectangle {
            x: 32px + mod(i, 2) * 376px;
            y: 216px + floor(i / 2) * 128px;
            width: 360px;
            height: 112px;
            opacity: slot.enabled ? 1.0 : 0.4;
            background: slot.on ? #C6FF1A : touch.has-hover && slot.enabled ? #2A2622 : #1F1C18;
            Text { text: slot.label; font-size: 30px; color: slot.on ? #0C0B0A : #F4F1EC; }
            touch := TouchArea { enabled: slot.enabled; clicked => { root.slot-clicked(i); } }
        }
    }
}

pub const MENU_SIZE: (u32, u32) = (800, 600);
pub const SLOTS: usize = 6;

/// Center of tab `i` in menu pixels (matches the Slint layout above).
pub fn tab_center(i: usize) -> (f32, f32) {
    (i as f32 * 200.0 + 100.0, 44.0)
}

/// Center of button slot `i` (row-major, 2 columns).
pub fn slot_center(i: usize) -> (f32, f32) {
    (32.0 + (i % 2) as f32 * 376.0 + 180.0, 216.0 + (i / 2) as f32 * 128.0 + 56.0)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub label: String,
    /// Drawn lit (accent): an active toggle or the recommended choice.
    pub on: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuModel {
    /// Empty during first run (no tabs).
    pub tabs: Vec<String>,
    pub tab: usize,
    pub title: String,
    pub subtitle: String,
    pub slots: Vec<Slot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuInput {
    Tab(usize),
    Slot(usize),
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
        let slots: Vec<SlotView> = m
            .slots
            .iter()
            .take(SLOTS)
            .map(|s| SlotView { label: s.label.as_str().into(), on: s.on, enabled: s.enabled })
            .collect();
        self.view.set_slots(ModelRc::new(VecModel::from(slots)));
        self.shown = m.clone();
    }

    fn dispatch(&self, event: WindowEvent) {
        // An event the window cannot take (e.g. not shown yet) is simply dropped.
        let _ = self.canvas.window.dispatch_event_with_result(event);
    }

    fn position(uv: [f32; 2]) -> LogicalPosition {
        LogicalPosition::new(uv[0] * MENU_SIZE.0 as f32, uv[1] * MENU_SIZE.1 as f32)
    }

    /// Laser moved over the menu (uv in 0..1).
    pub fn pointer_move(&self, uv: [f32; 2]) {
        self.dispatch(WindowEvent::PointerMoved { position: Self::position(uv) });
    }

    pub fn pointer_button(&self, uv: [f32; 2], down: bool) {
        let position = Self::position(uv);
        let button = PointerEventButton::Left;
        self.dispatch(if down {
            WindowEvent::PointerPressed { position, button }
        } else {
            WindowEvent::PointerReleased { position, button }
        });
    }

    pub fn pointer_leave(&self) {
        self.dispatch(WindowEvent::PointerExited);
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
        let slot = |label: &str, enabled| Slot { label: label.into(), on: false, enabled };
        menu.set(&MenuModel {
            tabs: vec!["A".into(), "B".into(), "C".into(), "D".into()],
            tab: 0,
            title: "Title".into(),
            subtitle: String::new(),
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
