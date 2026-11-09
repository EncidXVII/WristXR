//! In-VR menu: a side rail of pages and grouped lists (rows, switches, segmented choices). The app builds each
//! page with [`Layout`], which also decides where every control sits, so tests can click them; this view only
//! draws the model and reports which tab or slot was clicked.
use crate::{Canvas, ensure_platform};
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

slint::slint! {
    // Night Lime (soft): bg #0C0B0A, rail #15130F, surface #1F1C18, line/hover #2A2622, ink #F4F1EC,
    // secondary #AAA49B, tertiary #8A847B, accent #C6FF1A, danger #FF5A3C. Display face: Bahnschrift.
    export struct TabView { label: string, icon: string, badge: int }
    export struct GroupView { x: int, y: int, w: int, h: int, inset: bool }
    // tone: 0 secondary, 1 ink, 2 tertiary
    export struct LabelView { x: int, y: int, w: int, h: int, text: string, tone: int, wrap: bool }
    // kind: see SlotKind
    export struct SlotView {
        x: int, y: int, w: int, h: int, kind: int,
        label: string, detail: string, value: string, tile: string, tile-color: color, icon: string,
        on: bool, enabled: bool, divider: bool,
    }

    export component MenuView inherits Window {
        in property <[TabView]> tabs;
        in property <int> tab;
        in property <string> heading;
        in property <int> heading-x;
        in property <string> hint;
        in property <[GroupView]> groups;
        in property <[LabelView]> labels;
        in property <[SlotView]> slots;
        in property <string> close-icon;
        // Off: switches flip at once (Windows "Animation effects" off, tests).
        in property <bool> motion;
        callback tab-clicked(int);
        callback close-clicked;
        callback slot-clicked(int);
        width: 800px;
        height: 600px;
        background: #0C0B0A;

        if root.tabs.length > 0 : Rectangle { x: 0px; y: 0px; width: 120px; height: 600px; background: #15130F; }
        for t[i] in root.tabs : Rectangle {
            x: 10px;
            y: (12 + i * 92) * 1px;
            width: 100px;
            height: 88px;
            border-radius: 16px;
            background: i != root.tab && tab-touch.pressed ? #2A2622 : i != root.tab && tab-touch.has-hover ? #1F1C18 : transparent;
            Rectangle {
                x: 22px; y: 6px; width: 56px; height: 40px; border-radius: 20px;
                background: i == root.tab ? #C6FF1A : transparent;
                Path {
                    width: 28px; height: 28px; viewbox-width: 24; viewbox-height: 24;
                    commands: t.icon; fill: transparent; stroke-width: 2.2px;
                    stroke: i == root.tab ? #0C0B0A : #AAA49B;
                }
            }
            if t.badge > 0 : Rectangle {
                x: 64px; y: 0px; width: 30px; height: 30px; border-radius: 15px; background: #F4F1EC;
                Text { text: t.badge; font-size: 24px; font-weight: 700; color: #0C0B0A; }
            }
            Text {
                y: 50px; width: parent.width; height: 32px; horizontal-alignment: center; vertical-alignment: center;
                text: t.label; font-size: 24px; font-weight: i == root.tab ? 700 : 400;
                color: i == root.tab ? #F4F1EC : #AAA49B;
            }
            tab-touch := TouchArea { clicked => { root.tab-clicked(i); } }
        }
        if root.tabs.length > 0 : Rectangle {
            x: 10px; y: 524px; width: 100px; height: 64px; border-radius: 16px;
            background: close-touch.pressed ? #36312B : close-touch.has-hover ? #2A2622 : #1F1C18;
            Path {
                width: 26px; height: 26px; viewbox-width: 24; viewbox-height: 24;
                commands: root.close-icon; fill: transparent; stroke: #F4F1EC; stroke-width: 2.4px;
            }
            close-touch := TouchArea { clicked => { root.close-clicked(); } }
        }

        Text {
            x: root.heading-x * 1px; y: 18px; height: 52px; width: 776px - self.x;
            text: root.heading; font-family: "Bahnschrift"; font-size: 34px; font-weight: 700; color: #F4F1EC;
            vertical-alignment: center; overflow: elide;
        }
        Text {
            x: 776px - self.width; y: 18px; height: 52px; width: 400px;
            text: root.hint; font-size: 24px; color: #8A847B;
            horizontal-alignment: right; vertical-alignment: center; overflow: elide;
        }

        for g in root.groups : Rectangle {
            x: g.x * 1px; y: g.y * 1px; width: g.w * 1px; height: g.h * 1px;
            border-radius: g.inset ? 12px : 16px;
            background: g.inset ? #0C0B0A : #1F1C18;
        }
        for l in root.labels : Text {
            x: l.x * 1px; y: l.y * 1px; width: l.w * 1px; height: l.h * 1px;
            text: l.text; font-size: 24px; wrap: l.wrap ? word-wrap : no-wrap; overflow: elide;
            vertical-alignment: l.wrap ? top : center;
            color: l.tone == 1 ? #F4F1EC : l.tone == 2 ? #8A847B : #AAA49B;
        }

        for s[i] in root.slots : Rectangle {
            x: s.x * 1px; y: s.y * 1px; width: s.w * 1px; height: s.h * 1px;
            visible: s.kind != 10;
            opacity: s.enabled || s.kind == 4 || s.kind == 7 ? 1.0 : 0.4;
            border-radius: s.kind == 2 ? 9px : s.kind == 5 ? 12px : s.kind == 8 ? 14px : 16px;
            border-width: s.kind == 3 && !s.on ? 2px : 0px;
            border-color: #FF5A3C;
            background: s.kind == 2 && s.on ? #F4F1EC
                : s.kind == 3 && s.on ? #FF5A3C
                : s.kind == 6 && s.on ? #C6FF1A
                // Pressed answers on trigger-down, before the click lands on release.
                : touch.pressed && s.enabled ? (s.kind == 3 ? #45201A : #36312B)
                : touch.has-hover && s.enabled ? #2A2622
                : s.kind == 6 || s.kind == 7 || s.kind == 8 ? #1F1C18
                : transparent;
            if s.divider : Rectangle { x: 16px; y: 0px; width: parent.width - 32px; height: 1px; background: #2A2622; }
            // Open-now dot (lime when shown).
            if s.kind == 4 : Rectangle {
                x: 16px; y: (parent.height - 12px) / 2; width: 12px; height: 12px; border-radius: 6px;
                background: s.on ? #C6FF1A : #8A847B;
            }
            // Unread dot.
            if s.kind == 9 && s.on : Rectangle {
                x: 6px; y: 18px; width: 10px; height: 10px; border-radius: 5px; background: #C6FF1A;
            }
            if s.tile != "" : Rectangle {
                x: 20px; y: s.kind == 9 ? 14px : (parent.height - 40px) / 2;
                width: 40px; height: 40px; border-radius: 11px; background: s.tile-color;
                Text { text: s.tile; font-size: 24px; font-weight: 700; color: #FFFFFF; }
            }
            // Row text: label (and detail on a second line for messages), value on the right.
            if s.kind == 0 || s.kind == 1 || s.kind == 4 || s.kind == 7 : Text {
                x: s.tile != "" ? 72px : s.kind == 4 ? 40px : 16px;
                y: 0px; height: parent.height; width: parent.width - self.x - (s.kind == 1 ? 90px : 92px);
                text: s.label; font-size: s.kind == 4 || s.kind == 7 ? 24px : 26px; color: #F4F1EC;
                vertical-alignment: center; overflow: elide;
            }
            if s.kind == 9 : Text {
                x: 72px; y: 6px; height: 30px; width: parent.width - 168px;
                text: s.label; font-size: 24px; font-weight: 700; color: s.on ? #F4F1EC : #AAA49B;
                vertical-alignment: center; overflow: elide;
            }
            if s.kind == 9 : Text {
                x: 72px; y: 34px; height: 30px; width: parent.width - 88px;
                text: s.detail; font-size: 24px; color: #AAA49B; vertical-alignment: center; overflow: elide;
            }
            if s.kind == 0 || s.kind == 7 || s.kind == 9 : Text {
                x: parent.width - self.width - 16px; y: 0px; height: s.kind == 9 ? 42px : parent.height;
                text: s.value; font-size: s.value == "+" ? 30px : 24px;
                color: s.kind == 9 ? #8A847B : s.on ? #C6FF1A : #AAA49B; vertical-alignment: center;
            }
            if s.kind == 1 : Rectangle {
                x: parent.width - 74px; y: (parent.height - 34px) / 2; width: 58px; height: 34px; border-radius: 17px;
                background: s.on ? #C6FF1A : #2A2622;
                animate background { duration: root.motion ? 160ms : 0ms; easing: ease-out; }
                Rectangle {
                    x: s.on ? 28px : 4px; y: 4px; width: 26px; height: 26px; border-radius: 13px;
                    background: s.on ? #0C0B0A : #8A847B;
                    animate x, background { duration: root.motion ? 160ms : 0ms; easing: cubic-bezier(0.23, 1, 0.32, 1); }
                }
            }
            if s.kind == 2 || s.kind == 3 || s.kind == 6 : Text {
                x: 8px; width: parent.width - 16px; horizontal-alignment: center;
                text: s.label; font-size: s.kind == 6 ? 26px : 24px; overflow: elide;
                font-weight: s.on || s.kind == 3 ? 700 : 400;
                color: s.on ? #0C0B0A : s.kind == 3 ? #FF5A3C : s.kind == 2 ? #AAA49B : #F4F1EC;
            }
            if s.kind == 5 || s.kind == 8 : Path {
                width: 22px; height: 22px; viewbox-width: 24; viewbox-height: 24;
                commands: s.icon; fill: transparent; stroke-width: 2.6px;
                stroke: s.kind == 5 ? #AAA49B : #F4F1EC;
            }
            touch := TouchArea { enabled: s.enabled; clicked => { root.slot-clicked(i); } }
        }
    }
}

pub const MENU_SIZE: (u32, u32) = (800, 600);

/// Center of rail tab `i` in menu pixels (matches the Slint layout above).
pub fn tab_center(i: usize) -> (f32, f32) {
    (60.0, 12.0 + i as f32 * 92.0 + 44.0)
}

/// Center of the rail's Close button (shown with the tabs, not during first run).
pub fn close_center() -> (f32, f32) {
    (60.0, 556.0)
}

/// How a slot looks and what `on` means for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlotKind {
    /// List row: label, value on the right (accent when `on`).
    #[default]
    Action = 0,
    /// List row with a switch.
    Toggle = 1,
    /// One option of a segmented control; `on` = chosen.
    Segment = 2,
    /// Outlined destructive button; `on` = armed (tap again to confirm).
    Danger = 3,
    /// Read-only row with a dot (lime when `on`).
    Item = 4,
    /// Small icon button inside a row (e.g. close).
    IconButton = 5,
    /// Standalone button; `on` = lit (accent).
    Primary = 6,
    /// Standalone card with a tile, label and value.
    Entry = 7,
    /// Standalone square icon button (back).
    Back = 8,
    /// Two-line message row: tile, bold label, detail, time; `on` = unread.
    Message = 9,
    /// Placeholder (nothing drawn, not clickable).
    Hidden = 10,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub kind: SlotKind,
    pub label: String,
    /// Second line (messages).
    pub detail: String,
    /// Right-aligned text ("7 ›", "+", "Shown", "2 m").
    pub value: String,
    /// Letter tile and its colour (0xRRGGBB); empty = none.
    pub tile: String,
    pub tile_color: u32,
    pub icon: &'static str,
    pub on: bool,
    pub enabled: bool,
    pub rect: Rect,
    /// Hairline above (rows after the first in a group).
    pub divider: bool,
}

impl Slot {
    pub fn new(kind: SlotKind, label: impl Into<String>) -> Self {
        Self { kind, label: label.into(), enabled: true, ..Self::default() }
    }
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
    /// Letter tile for `name` with a colour picked from the name.
    pub fn tile(mut self, name: &str) -> Self {
        const COLORS: [u32; 6] = [0x5865F2, 0x1B6FD1, 0xB85000, 0x2E7D4F, 0x8E44AD, 0xB03A2E];
        let letter = name.chars().find(|c| c.is_alphanumeric()).unwrap_or('?');
        self.tile = letter.to_uppercase().collect();
        let hash = name.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(usize::from(b)));
        self.tile_color = COLORS[hash % COLORS.len()];
        self
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = icon;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Group {
    pub rect: Rect,
    /// Dark inset (a segmented control inside a group).
    pub inset: bool,
}

/// Text placed by the layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub rect: Rect,
    pub text: String,
    pub tone: Tone,
    pub wrap: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Secondary = 0,
    Ink = 1,
    Tertiary = 2,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tab {
    pub label: String,
    pub icon: &'static str,
    /// Count shown on the tab (unread), 0 = none.
    pub badge: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuModel {
    /// Empty during first run (no rail).
    pub tabs: Vec<Tab>,
    pub tab: usize,
    pub title: String,
    pub title_x: f32,
    /// Short note right of the title.
    pub hint: String,
    pub groups: Vec<Group>,
    pub labels: Vec<Label>,
    pub slots: Vec<Slot>,
}

impl MenuModel {
    /// Center of slot `i` in menu pixels.
    pub fn center(&self, i: usize) -> Option<(f32, f32)> {
        self.slots.get(i).map(|s| s.rect.center())
    }

    /// Index of the first clickable slot with this label.
    pub fn find(&self, label: &str) -> Option<usize> {
        self.slots.iter().position(|s| s.label == label && s.kind != SlotKind::Hidden)
    }

    /// Every text on the page (title, hint, labels, slots), for tests.
    pub fn texts(&self) -> Vec<&str> {
        let mut v = vec![self.title.as_str(), self.hint.as_str()];
        v.extend(self.labels.iter().map(|l| l.text.as_str()));
        v.extend(self.slots.iter().flat_map(|s| [s.label.as_str(), s.detail.as_str(), s.value.as_str()]));
        v
    }
}

/// Column of the two-column page grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Col {
    Left,
    Right,
    Full,
}

const TOP: f32 = 84.0;
const BOTTOM: f32 = 580.0;
const COL_GAP: f32 = 16.0;
const LABEL_H: f32 = 30.0;
pub const ROW_H: f32 = 62.0;
const GROUP_GAP: f32 = 14.0;

/// Places a page's controls top-down in one or two columns; slots get their rects as they are added.
pub struct Layout {
    model: MenuModel,
    x: f32,
    w: f32,
    y: [f32; 2],
}

impl Layout {
    /// Page with the rail (content right of it).
    pub fn rail(title: impl Into<String>) -> Self {
        let x = 144.0;
        Self::start(title.into(), x, 776.0 - x)
    }

    /// First-run page: full width, no rail.
    pub fn plain(title: impl Into<String>) -> Self {
        Self::start(title.into(), 32.0, 736.0)
    }

    fn start(title: String, x: f32, w: f32) -> Self {
        Self { model: MenuModel { title, title_x: x, ..MenuModel::default() }, x, w, y: [TOP; 2] }
    }

    pub fn hint(&mut self, hint: impl Into<String>) {
        self.model.hint = hint.into();
    }

    /// A back button left of the title.
    pub fn back(&mut self, back: Slot) -> usize {
        let i = self.place(Slot { kind: SlotKind::Back, ..back }, Rect { x: self.x, y: 18.0, w: 52.0, h: 52.0 });
        self.model.title_x = self.x + 66.0;
        i
    }

    fn span(&self, col: Col) -> (f32, f32, std::ops::Range<usize>) {
        let half = (self.w - COL_GAP) / 2.0;
        match col {
            Col::Left => (self.x, half, 0..1),
            Col::Right => (self.x + half + COL_GAP, half, 1..2),
            Col::Full => (self.x, self.w, 0..2),
        }
    }

    fn top(&self, col: Col) -> f32 {
        let (_, _, cols) = self.span(col);
        cols.map(|c| self.y[c]).fold(0.0, f32::max)
    }

    fn advance(&mut self, col: Col, to: f32) {
        let (_, _, cols) = self.span(col);
        for c in cols {
            self.y[c] = to;
        }
    }

    fn place(&mut self, mut slot: Slot, rect: Rect) -> usize {
        slot.rect = rect;
        self.model.slots.push(slot);
        self.model.slots.len() - 1
    }

    fn text(&mut self, rect: Rect, text: impl Into<String>, tone: Tone, wrap: bool) {
        self.model.labels.push(Label { rect, text: text.into(), tone, wrap });
    }

    /// Small heading above a group.
    pub fn label(&mut self, col: Col, text: impl Into<String>) {
        let (x, w, _) = self.span(col);
        let y = self.top(col);
        self.text(Rect { x: x + 4.0, y, w: w - 8.0, h: LABEL_H }, text, Tone::Secondary, false);
        self.advance(col, y + LABEL_H + 8.0);
    }

    /// Wrapped text of height `h`.
    pub fn paragraph(&mut self, col: Col, text: impl Into<String>, tone: Tone, h: f32) {
        let (x, w, _) = self.span(col);
        let y = self.top(col);
        self.text(Rect { x, y, w, h }, text, tone, true);
        self.advance(col, y + h + GROUP_GAP);
    }

    /// Rows of height `h` in one rounded group; returns their slot indices.
    pub fn rows(&mut self, col: Col, rows: Vec<Slot>, h: f32) -> Vec<usize> {
        let (x, w, _) = self.span(col);
        let y = self.top(col);
        let n = rows.len() as f32;
        self.model.groups.push(Group { rect: Rect { x, y, w, h: h * n }, inset: false });
        let ids = rows
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let divider = i > 0;
                self.place(Slot { divider, ..r }, Rect { x, y: y + i as f32 * h, w, h })
            })
            .collect();
        self.advance(col, y + h * n + GROUP_GAP);
        ids
    }

    /// Rows of height `h`, each with an optional small button at its right end (e.g. close).
    pub fn rows_with(&mut self, col: Col, rows: Vec<(Slot, Option<Slot>)>, h: f32) -> Vec<Option<usize>> {
        let (rows, buttons): (Vec<Slot>, Vec<Option<Slot>>) = rows.into_iter().unzip();
        let ids = self.rows(col, rows, h);
        ids.into_iter()
            .zip(buttons)
            .map(|(row, button)| {
                let r = self.model.slots[row].rect;
                let size = 44.0;
                let rect = Rect { x: r.x + r.w - size - 8.0, y: r.y + (h - size) / 2.0, w: size, h: size };
                button.map(|b| self.place(Slot { kind: SlotKind::IconButton, ..b }, rect))
            })
            .collect()
    }

    /// A segmented control on its own (options share the width).
    pub fn segments(&mut self, col: Col, options: Vec<Slot>) -> Vec<usize> {
        let (x, w, _) = self.span(col);
        let y = self.top(col);
        self.model.groups.push(Group { rect: Rect { x, y, w, h: 56.0 }, inset: false });
        let ids = self.segment_row(x, y, w, options);
        self.advance(col, y + 56.0 + GROUP_GAP);
        ids
    }

    fn segment_row(&mut self, x: f32, y: f32, w: f32, options: Vec<Slot>) -> Vec<usize> {
        let each = (w - 8.0) / options.len().max(1) as f32;
        options
            .into_iter()
            .enumerate()
            .map(|(i, o)| {
                let rect = Rect { x: x + 4.0 + i as f32 * each, y: y + 4.0, w: each, h: 48.0 };
                self.place(Slot { kind: SlotKind::Segment, ..o }, rect)
            })
            .collect()
    }

    /// A group of captioned segmented controls ("Laser hand: Left | Right").
    pub fn choices(&mut self, col: Col, items: Vec<(String, Vec<Slot>)>) -> Vec<Vec<usize>> {
        let (x, w, _) = self.span(col);
        let top = self.top(col);
        let group = self.model.groups.len();
        self.model.groups.push(Group { rect: Rect { x, y: top, w, h: 0.0 }, inset: false });
        let mut y = top + 12.0;
        let mut ids = Vec::new();
        for (caption, options) in items {
            self.text(Rect { x: x + 16.0, y, w: w - 32.0, h: LABEL_H }, caption, Tone::Ink, false);
            y += LABEL_H + 8.0;
            self.model.groups.push(Group { rect: Rect { x: x + 16.0, y, w: w - 32.0, h: 56.0 }, inset: true });
            ids.push(self.segment_row(x + 16.0, y, w - 32.0, options));
            y += 56.0 + 10.0;
        }
        self.model.groups[group].rect.h = y + 2.0 - top;
        self.advance(col, y + 2.0 + GROUP_GAP);
        ids
    }

    /// Standalone cards (`per_row` across, height `h`).
    pub fn cards(&mut self, col: Col, cards: Vec<Slot>, per_row: usize, h: f32) -> Vec<usize> {
        let (x, w, _) = self.span(col);
        let top = self.top(col);
        let gap = 10.0;
        let per_row = per_row.max(1);
        let each = (w - gap * (per_row - 1) as f32) / per_row as f32;
        let n = cards.len();
        let ids = cards
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let rect = Rect {
                    x: x + (i % per_row) as f32 * (each + gap),
                    y: top + (i / per_row) as f32 * (h + gap),
                    w: each,
                    h,
                };
                self.place(c, rect)
            })
            .collect();
        let rows = n.div_ceil(per_row) as f32;
        self.advance(col, top + rows * (h + gap) - gap + GROUP_GAP);
        ids
    }

    /// Bottom bar: a note on the left and, optionally, a button on the right.
    pub fn footer(&mut self, note: impl Into<String>, button: Option<Slot>) -> Option<usize> {
        let h = 64.0;
        let y = BOTTOM - h;
        let bw = if button.is_some() { 220.0 } else { 0.0 };
        self.text(Rect { x: self.x + 4.0, y, w: self.w - bw - 20.0, h }, note, Tone::Tertiary, false);
        button.map(|b| self.place(b, Rect { x: self.x + self.w - bw, y, w: bw, h }))
    }

    pub fn finish(self) -> MenuModel {
        self.model
    }
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
    /// Kept across updates of the same page, so a flipped switch slides instead of being redrawn from scratch.
    slots: Rc<VecModel<SlotView>>,
}

fn px(v: f32) -> i32 {
    v.round() as i32
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
        view.set_close_icon(crate::icons::CLOSE.into());
        view.set_motion(crate::motion());
        let slots = Rc::new(VecModel::default());
        view.set_slots(ModelRc::from(slots.clone()));
        Ok(Self { view, canvas, inputs, shown: MenuModel::default(), slots })
    }

    /// Updates the view; a no-op when nothing changed.
    pub fn set(&mut self, m: &MenuModel) {
        if *m == self.shown {
            return;
        }
        let tabs: Vec<TabView> = m
            .tabs
            .iter()
            .map(|t| TabView { label: t.label.as_str().into(), icon: t.icon.into(), badge: t.badge as i32 })
            .collect();
        self.view.set_tabs(ModelRc::new(VecModel::from(tabs)));
        self.view.set_tab(m.tab as i32);
        self.view.set_heading(SharedString::from(m.title.to_uppercase()));
        self.view.set_heading_x(px(m.title_x));
        self.view.set_hint(m.hint.as_str().into());
        let groups: Vec<GroupView> = m
            .groups
            .iter()
            .map(|g| GroupView { x: px(g.rect.x), y: px(g.rect.y), w: px(g.rect.w), h: px(g.rect.h), inset: g.inset })
            .collect();
        self.view.set_groups(ModelRc::new(VecModel::from(groups)));
        let labels: Vec<LabelView> = m
            .labels
            .iter()
            .map(|l| LabelView {
                x: px(l.rect.x),
                y: px(l.rect.y),
                w: px(l.rect.w),
                h: px(l.rect.h),
                text: l.text.as_str().into(),
                tone: l.tone as i32,
                wrap: l.wrap,
            })
            .collect();
        self.view.set_labels(ModelRc::new(VecModel::from(labels)));
        let slots: Vec<SlotView> = m
            .slots
            .iter()
            .map(|s| SlotView {
                x: px(s.rect.x),
                y: px(s.rect.y),
                w: px(s.rect.w),
                h: px(s.rect.h),
                kind: s.kind as i32,
                label: s.label.as_str().into(),
                detail: s.detail.as_str().into(),
                value: s.value.as_str().into(),
                tile: s.tile.as_str().into(),
                tile_color: Color::from_rgb_u8(
                    (s.tile_color >> 16) as u8,
                    (s.tile_color >> 8) as u8,
                    s.tile_color as u8,
                ),
                icon: s.icon.into(),
                on: s.on,
                enabled: s.enabled && !matches!(s.kind, SlotKind::Hidden | SlotKind::Item),
                divider: s.divider,
            })
            .collect();
        let same_page = m.tab == self.shown.tab && m.title == self.shown.title && slots.len() == self.slots.row_count();
        if same_page {
            for (i, s) in slots.into_iter().enumerate() {
                if self.slots.row_data(i).as_ref() != Some(&s) {
                    self.slots.set_row_data(i, s);
                }
            }
        } else {
            self.slots.set_vec(slots);
        }
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

    fn page() -> MenuModel {
        let row = |l: &str| Slot::new(SlotKind::Action, l);
        let mut l = Layout::rail("Title");
        l.label(Col::Left, "Add");
        l.rows(Col::Left, vec![row("1"), row("2").enabled(false)], ROW_H);
        l.label(Col::Right, "Show");
        l.rows(Col::Right, vec![Slot::new(SlotKind::Toggle, "3"), Slot::new(SlotKind::Item, "4")], ROW_H);
        l.choices(Col::Left, vec![("Hand".into(), vec![row("5"), row("6")])]);
        l.footer("note", Some(Slot::new(SlotKind::Danger, "7")));
        let mut m = l.finish();
        m.tabs = (0..5).map(|i| Tab { label: i.to_string(), icon: crate::icons::BELL, badge: i }).collect();
        m
    }

    #[test]
    fn clicks_map_to_tabs_and_slots_and_hover_never_clicks() {
        let mut menu = Menu::new().expect("menu");
        let m = page();
        menu.set(&m);
        assert!(menu.render().is_some());
        for i in 0..m.slots.len() {
            menu.pointer_move(uv(m.center(i).expect("slot")));
        }
        assert!(menu.take_inputs().is_empty(), "hover must never click");
        click(&menu, tab_center(4));
        for i in 0..m.slots.len() {
            click(&menu, m.center(i).expect("slot"));
        }
        click(&menu, close_center());
        // Slot 1 is disabled and 3 is a read-only item.
        let slots = [0, 2, 4, 5, 6].map(MenuInput::Slot);
        let mut want = vec![MenuInput::Tab(4)];
        want.extend(slots);
        want.push(MenuInput::Close);
        assert_eq!(menu.take_inputs(), want);
    }

    #[test]
    fn layout_keeps_controls_inside_the_page_and_apart() {
        let m = page();
        for (i, a) in m.slots.iter().enumerate() {
            let r = a.rect;
            assert!(r.x >= 120.0 && r.x + r.w <= 800.0 && r.y >= 0.0 && r.y + r.h <= 600.0, "slot {i} off page");
            for b in &m.slots[i + 1..] {
                let o = b.rect;
                let apart = r.x + r.w <= o.x || o.x + o.w <= r.x || r.y + r.h <= o.y || o.y + o.h <= r.y;
                assert!(apart, "slots {} and {} overlap", a.label, b.label);
            }
        }
    }
}
