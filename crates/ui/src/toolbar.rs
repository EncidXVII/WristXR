//! Toolbar shown under the panel the laser points at: the panel's name, then grouped controls (lock and curve,
//! size, opacity slider, Room / Hand / Head, reset, close). The app builds it with [`ToolbarLayout`], which places
//! every control; this view only draws them and reports clicks and slider positions.
use crate::menu::Rect;
use crate::{Canvas, ensure_platform};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

slint::slint! {
    // kind: see ToolKind. Tokens as in the menu.
    export struct ToolView { x: int, y: int, w: int, h: int, kind: int, label: string, icon: string, on: bool, value: float }
    export struct BoxView { x: int, w: int }

    export component ToolbarView inherits Window {
        in property <BoxView> bar;
        in property <[BoxView]> groups;
        in property <[ToolView]> tools;
        callback clicked(int);
        callback slid(int, float);
        width: 1180px;
        height: 84px;
        background: transparent;
        Rectangle {
            x: root.bar.x * 1px; y: 0px; width: root.bar.w * 1px; height: 84px; border-radius: 20px;
            background: #0C0B0A;
        }
        for g in root.groups : Rectangle {
            x: g.x * 1px; y: 8px; width: g.w * 1px; height: 68px; border-radius: 14px; background: #1F1C18;
        }
        for t[i] in root.tools : Rectangle {
            x: t.x * 1px; y: t.y * 1px; width: t.w * 1px; height: t.h * 1px;
            border-radius: t.kind == 6 || t.kind == 7 || t.kind == 8 ? 14px : 10px;
            background: t.kind == 1 && t.on ? #C6FF1A
                : t.kind == 5 && t.on ? #F4F1EC
                : t.kind == 7 ? (touch.pressed ? #5A2A20 : touch.has-hover ? #45201A : #2B1712)
                : touch.pressed && t.kind != 0 && t.kind != 3 && t.kind != 4 ? #36312B
                : touch.has-hover && t.kind != 0 && t.kind != 3 && t.kind != 4 ? #2A2622
                : t.kind == 6 || t.kind == 8 ? #1F1C18
                : transparent;
            if t.kind == 0 : Path {
                x: 12px; width: 28px; height: 28px; viewbox-width: 24; viewbox-height: 24;
                commands: t.icon; fill: transparent; stroke: #AAA49B; stroke-width: 2.2px;
            }
            if t.kind == 0 : Text {
                x: 46px; width: parent.width - 52px; text: t.label; font-size: 24px; color: #AAA49B; overflow: elide;
            }
            if t.kind == 1 || t.kind == 6 || t.kind == 7 : Path {
                width: 30px; height: 30px; viewbox-width: 24; viewbox-height: 24;
                commands: t.icon; fill: transparent; stroke-width: 2.2px;
                stroke: t.kind == 7 ? #FF5A3C : t.kind == 1 && t.on ? #0C0B0A : #F4F1EC;
            }
            if t.kind == 2 : Text { text: t.label; font-size: 30px; color: #F4F1EC; }
            if t.kind == 3 : Text { text: t.label; font-size: 24px; color: #AAA49B; }
            if t.kind == 5 || t.kind == 8 : Text {
                text: t.label; font-size: 24px; font-weight: t.on ? 700 : 400;
                color: t.on ? #0C0B0A : t.kind == 8 ? #F4F1EC : #AAA49B;
            }
            if t.kind == 4 : Path {
                x: 14px; width: 28px; height: 28px; viewbox-width: 24; viewbox-height: 24;
                commands: t.icon; fill: transparent; stroke: #AAA49B; stroke-width: 2.2px;
            }
            if t.kind == 4 : Rectangle {
                x: 52px; y: (parent.height - 8px) / 2; width: parent.width - 68px; height: 8px; border-radius: 4px;
                background: #2A2622;
                Rectangle {
                    x: 0px; width: parent.width * clamp(t.value, 0, 1); height: 8px; border-radius: 4px;
                    background: #C6FF1A;
                }
                Rectangle {
                    x: parent.width * clamp(t.value, 0, 1) - 10px; y: -6px; width: 20px; height: 20px;
                    border-radius: 10px; background: #F4F1EC;
                }
            }
            touch := TouchArea {
                enabled: t.kind != 0 && t.kind != 3;
                clicked => {
                    if t.kind == 4 {
                        root.slid(i, (self.mouse-x - 52px) / (self.width - 68px));
                    } else {
                        root.clicked(i);
                    }
                }
                moved => {
                    if t.kind == 4 && self.pressed {
                        root.slid(i, (self.mouse-x - 52px) / (self.width - 68px));
                    }
                }
            }
        }
    }
}

pub const TOOLBAR_SIZE: (u32, u32) = (1180, 84);

/// How a toolbar control looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolKind {
    /// The panel's icon and name (read-only).
    #[default]
    Chip = 0,
    /// Icon button in a group; `on` = lit.
    Toggle = 1,
    /// Text button in a group (− / +).
    Step = 2,
    /// Read-only text in a group (size).
    Readout = 3,
    /// Icon and a slider (`value` 0..1).
    Slider = 4,
    /// One option of a segmented group; `on` = chosen.
    Segment = 5,
    /// Standalone icon button.
    Icon = 6,
    /// Standalone destructive icon button.
    Danger = 7,
    /// Standalone text button.
    Text = 8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolButton {
    pub kind: ToolKind,
    pub label: String,
    pub icon: &'static str,
    pub on: bool,
    pub value: f32,
    pub rect: Rect,
}

impl ToolButton {
    pub fn new(kind: ToolKind, label: impl Into<String>, icon: &'static str) -> Self {
        Self { kind, label: label.into(), icon, ..Self::default() }
    }
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
    pub fn value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolbarModel {
    /// Background bar (x, width).
    pub bar: (f32, f32),
    pub groups: Vec<(f32, f32)>,
    pub tools: Vec<ToolButton>,
}

impl ToolbarModel {
    pub fn center(&self, i: usize) -> Option<(f32, f32)> {
        self.tools.get(i).map(|t| t.rect.center())
    }

    /// Toolbar pixel of `value` (0..1) along slider `i`.
    pub fn slider_at(&self, i: usize, value: f32) -> Option<(f32, f32)> {
        self.tools.get(i).map(|t| (t.rect.x + 52.0 + (t.rect.w - 68.0) * value, t.rect.center().1))
    }
}

const PAD: f32 = 8.0;
const ITEM: f32 = 60.0;
const BIG: f32 = 68.0;

/// Packs controls left to right, then centres the bar in the toolbar.
#[derive(Default)]
pub struct ToolbarLayout {
    model: ToolbarModel,
    x: f32,
}

impl ToolbarLayout {
    pub fn new() -> Self {
        Self { model: ToolbarModel::default(), x: PAD }
    }

    fn push(&mut self, mut t: ToolButton, x: f32, y: f32, w: f32, h: f32) -> usize {
        t.rect = Rect { x, y, w, h };
        self.model.tools.push(t);
        self.model.tools.len() - 1
    }

    /// The panel's name (fixed width; long names are cut).
    pub fn chip(&mut self, label: impl Into<String>, icon: &'static str) -> usize {
        let i = self.push(ToolButton::new(ToolKind::Chip, label, icon), self.x, PAD, 200.0, BIG);
        self.x += 200.0 + PAD;
        i
    }

    /// Controls sharing one rounded group; widths default to a square button.
    pub fn group(&mut self, items: Vec<(ToolButton, Option<f32>)>) -> Vec<usize> {
        let start = self.x;
        let mut x = start + 4.0;
        let ids = items
            .into_iter()
            .map(|(t, w)| {
                let w = w.unwrap_or(match t.kind {
                    ToolKind::Slider => 170.0,
                    ToolKind::Readout => 80.0,
                    _ => ITEM,
                });
                let (y, h) = if t.kind == ToolKind::Slider { (PAD, BIG) } else { (PAD + 4.0, ITEM) };
                let i = self.push(t, x, y, w, h);
                x += w;
                i
            })
            .collect();
        x += 4.0;
        self.model.groups.push((start, x - start));
        self.x = x + PAD;
        ids
    }

    /// A standalone button (square for icons).
    pub fn button(&mut self, t: ToolButton, w: Option<f32>) -> usize {
        let w = w.unwrap_or(BIG);
        let i = self.push(t, self.x, PAD, w, BIG);
        self.x += w + PAD;
        i
    }

    pub fn finish(mut self) -> ToolbarModel {
        let width = self.x;
        let offset = ((TOOLBAR_SIZE.0 as f32 - width) / 2.0).max(0.0).round();
        for t in &mut self.model.tools {
            t.rect.x += offset;
        }
        for g in &mut self.model.groups {
            g.0 += offset;
        }
        self.model.bar = (offset, width);
        self.model
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToolInput {
    Click(usize),
    /// Slider `i` set to a value in 0..1.
    Slide(usize, f32),
}

pub struct Toolbar {
    view: ToolbarView,
    canvas: Canvas,
    inputs: Rc<RefCell<Vec<ToolInput>>>,
    shown: ToolbarModel,
}

fn px(v: f32) -> i32 {
    v.round() as i32
}

impl Toolbar {
    pub fn new() -> Result<Self, String> {
        ensure_platform();
        let view = ToolbarView::new().map_err(|e| e.to_string())?;
        let canvas = Canvas::take(TOOLBAR_SIZE)?;
        view.show().map_err(|e| e.to_string())?;
        let inputs = Rc::new(RefCell::new(Vec::new()));
        let q = inputs.clone();
        view.on_clicked(move |i| q.borrow_mut().push(ToolInput::Click(i.max(0) as usize)));
        let q = inputs.clone();
        view.on_slid(move |i, v| q.borrow_mut().push(ToolInput::Slide(i.max(0) as usize, v.clamp(0.0, 1.0))));
        Ok(Self { view, canvas, inputs, shown: ToolbarModel::default() })
    }

    /// Updates the bar; a no-op when nothing changed.
    pub fn set(&mut self, m: &ToolbarModel) {
        if *m == self.shown {
            return;
        }
        self.view.set_bar(BoxView { x: px(m.bar.0), w: px(m.bar.1) });
        let groups: Vec<BoxView> = m.groups.iter().map(|g| BoxView { x: px(g.0), w: px(g.1) }).collect();
        self.view.set_groups(ModelRc::new(VecModel::from(groups)));
        let tools: Vec<ToolView> = m
            .tools
            .iter()
            .map(|t| ToolView {
                x: px(t.rect.x),
                y: px(t.rect.y),
                w: px(t.rect.w),
                h: px(t.rect.h),
                kind: t.kind as i32,
                label: t.label.as_str().into(),
                icon: t.icon.into(),
                on: t.on,
                value: t.value,
            })
            .collect();
        self.view.set_tools(ModelRc::new(VecModel::from(tools)));
        self.shown = m.clone();
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

    /// Clicks and slider moves since the last call.
    pub fn take_inputs(&self) -> Vec<ToolInput> {
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
    use crate::icons;

    #[test]
    fn controls_report_their_index_and_the_slider_its_value() {
        let mut bar = Toolbar::new().expect("toolbar");
        let mut l = ToolbarLayout::new();
        l.chip("Vesktop", icons::WINDOW);
        l.group(vec![
            (ToolButton::new(ToolKind::Toggle, "", icons::LOCK).on(true), None),
            (ToolButton::new(ToolKind::Toggle, "", icons::CURVE), None),
        ]);
        l.group(vec![(ToolButton::new(ToolKind::Slider, "", icons::OPACITY).value(0.5), None)]);
        l.button(ToolButton::new(ToolKind::Danger, "", icons::CLOSE), None);
        let m = l.finish();
        bar.set(&m);
        assert!(bar.render().is_some());
        let uv = |(x, y): (f32, f32)| [x / TOOLBAR_SIZE.0 as f32, y / TOOLBAR_SIZE.1 as f32];
        let click = |bar: &Toolbar, at| {
            bar.pointer_move(uv(at));
            bar.pointer_button(uv(at), true);
            bar.pointer_button(uv(at), false);
        };
        for i in [4, 0, 2, 1] {
            click(&bar, m.center(i).expect("tool"));
        }
        click(&bar, m.slider_at(3, 0.25).expect("slider"));
        let got = bar.take_inputs();
        assert_eq!(got[..3], [ToolInput::Click(4), ToolInput::Click(2), ToolInput::Click(1)], "chip is read-only");
        let ToolInput::Slide(3, v) = got[got.len() - 1] else { panic!("no slide: {got:?}") };
        assert!((v - 0.25).abs() < 0.02, "slider value {v}");
        assert!(m.bar.0 > 0.0 && m.bar.0 + m.bar.1 <= TOOLBAR_SIZE.0 as f32, "bar is centred inside");
    }
}
