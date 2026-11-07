//! Menu and first-run logic, independent of SteamVR and the GPU so gates can drive it.
//! Panels live in the engine's list; the menu is one of them (`MENU_ID`) and is never persisted.
use dvr_core::chat::ChatLine;
use dvr_core::engine::{Event, MouseButton};
use dvr_core::math::Pose;
use dvr_core::panel::{Anchor, Hand, MENU_ID, Panel, PixelRect, Source};
use dvr_core::state::State;
use dvr_ui::{MENU_SIZE, Menu, MenuInput, MenuModel, Slot, SlotKind, Strings, WRIST_SIZE};

const OPACITY_STEPS: [f32; 4] = [1.0, 0.85, 0.7, 0.5];
const CURVATURE: f32 = 0.15;
const TABS: usize = 4;
/// The keyboard test asks for this word.
pub const TEST_WORD: &str = "hi";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonitorInfo {
    pub rect: PixelRect,
    pub primary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    FirstHand,
    FirstMonitor,
    FirstKeyboard,
    /// Gesture tips; finishing it completes onboarding.
    FirstTips,
    /// Panels, Keyboard, Wrist, Settings.
    Tab(usize),
}

/// What the platform layer must do after a menu action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Added(u32),
    Removed(u32),
    RedrawKeyboard,
    Quit,
}

pub struct Session {
    pub state: State,
    pub page: Page,
    pub selected: Option<u32>,
    test: ChatLine,
    menu_hovered: bool,
    /// Settings came from another overlay (shown on the first-run page).
    pub imported: bool,
    /// False when another app holds the notification port (shown on the Wrist tab).
    pub notifications_ok: bool,
    /// Danger button waiting for its confirming second tap.
    armed: Option<(Page, usize)>,
}

pub fn next_id(panels: &[Panel]) -> u32 {
    panels.iter().filter(|p| p.id != MENU_ID).map(|p| p.id + 1).max().unwrap_or(1)
}

fn aspect(m: &MonitorInfo) -> f32 {
    m.rect.h as f32 / m.rect.w as f32
}

fn find(panels: &mut [Panel], source: Source) -> Option<&mut Panel> {
    panels.iter_mut().find(|p| p.source == source)
}

impl Session {
    /// Ensures keyboard, wrist and menu panels exist; opens the first-run flow when not onboarded.
    pub fn new(state: State, panels: &mut Vec<Panel>, head: Pose) -> Self {
        if !panels.iter().any(|p| p.source == Source::Keyboard) {
            panels.push(Panel::keyboard_below(next_id(panels), head));
        }
        if !panels.iter().any(|p| p.source == Source::Wrist) {
            let aspect = WRIST_SIZE.1 as f32 / WRIST_SIZE.0 as f32;
            panels.push(Panel::on_wrist(next_id(panels), state.dominant.other(), aspect));
        }
        let mut menu = Panel::menu_in_front(head, MENU_SIZE.1 as f32 / MENU_SIZE.0 as f32);
        menu.visible = !state.onboarded;
        panels.retain(|p| p.id != MENU_ID);
        panels.push(menu);
        let page = if state.onboarded { Page::Tab(0) } else { Page::FirstHand };
        Self {
            state,
            page,
            selected: None,
            test: ChatLine::default(),
            menu_hovered: false,
            imported: false,
            notifications_ok: true,
            armed: None,
        }
    }

    pub fn menu_open(&self, panels: &[Panel]) -> bool {
        panels.iter().any(|p| p.id == MENU_ID && p.visible)
    }

    /// Double-tap menu: open in front of the user, or close.
    pub fn toggle_menu(&mut self, panels: &mut [Panel], head: Pose) {
        let open = self.menu_open(panels);
        if let Some(m) = panels.iter_mut().find(|p| p.id == MENU_ID) {
            if !open {
                m.pose = Panel::menu_in_front(head, m.aspect).pose;
            }
            m.visible = !open;
        }
    }

    /// Panels persisted in the layout (everything but the menu).
    pub fn persist(&mut self, panels: &[Panel]) {
        self.state.panels = panels.iter().filter(|p| p.id != MENU_ID).copied().collect();
    }

    /// Routes laser events on the menu panel to the Slint view; records the selected panel. Returns true when
    /// the menu saw pointer activity.
    pub fn pointer(&mut self, events: &[Event], menu: &Menu) -> bool {
        let mut over_menu = false;
        for e in events {
            match *e {
                Event::Hover { panel: MENU_ID, uv, .. } => {
                    over_menu = true;
                    menu.pointer_move(uv);
                }
                Event::Mouse { panel: MENU_ID, uv, button: MouseButton::Left, down } => {
                    over_menu = true;
                    menu.pointer_button(uv, down);
                }
                // Selection follows deliberate actions only: an idle second laser must not steal it.
                Event::Mouse { panel, down: true, .. } | Event::Moved { panel } if panel != MENU_ID => {
                    self.selected = Some(panel);
                }
                _ => {}
            }
        }
        if self.menu_hovered && !over_menu {
            menu.pointer_leave();
        }
        self.menu_hovered = over_menu;
        over_menu
    }

    /// During the keyboard test, typed keys go to the test line instead of the OS. Returns true when consumed.
    pub fn key(&mut self, sc: u16, down: bool, to_char: impl Fn(u16, bool, bool) -> Option<char>) -> bool {
        if self.page != Page::FirstKeyboard {
            return false;
        }
        self.test.key(sc, down, to_char);
        true
    }

    fn panel_name(s: &Strings, p: &Panel) -> String {
        match p.source {
            Source::Monitor(i) => s.get("panel-monitor", &[("n", (i + 1).to_string())]),
            Source::Window(_) => s.get("panel-window", &[]),
            Source::Keyboard => s.get("panel-keyboard", &[]),
            Source::Wrist => s.get("panel-wrist", &[]),
            Source::Menu => String::new(),
        }
    }

    fn unshown_monitor(panels: &[Panel], monitors: &[MonitorInfo]) -> Option<u32> {
        (0..monitors.len() as u32).find(|i| !panels.iter().any(|p| p.source == Source::Monitor(*i)))
    }

    /// What the menu shows now. `cost` is (GPU ms, CPU %) of this overlay.
    pub fn model(&self, panels: &[Panel], monitors: &[MonitorInfo], s: &Strings, cost: (f32, f32)) -> MenuModel {
        let t = |id: &str| s.get(id, &[]);
        let mk = |label: String, kind: SlotKind, on: bool, enabled: bool| Slot { label, on, enabled, kind };
        let action = |label: String, enabled: bool| mk(label, SlotKind::Action, false, enabled);
        let toggle = |label: String, on: bool, enabled: bool| mk(label, SlotKind::Toggle, on, enabled);
        let choice = |label: String, on: bool| mk(label, SlotKind::Choice, on, true);
        let hidden = || mk(String::new(), SlotKind::Hidden, false, false);
        // A danger button shows "tap again" once armed.
        let danger = |id: &str, slot: usize, enabled: bool| {
            let armed = self.armed == Some((self.page, slot));
            mk(if armed { t("menu-confirm") } else { t(id) }, SlotKind::Danger, armed, enabled)
        };
        let selected = self.selected.and_then(|id| panels.iter().find(|p| p.id == id));
        let tabs = ["menu-tab-panels", "menu-tab-keyboard", "menu-tab-wrist", "menu-tab-settings"].map(t).to_vec();
        let visible = |src: Source| panels.iter().any(|p| p.source == src && p.visible);
        let wrist_hand = panels.iter().find(|p| p.source == Source::Wrist).map(|p| p.anchor);
        let mut body = String::new();
        let (tab, title, subtitle, slots) = match self.page {
            Page::FirstHand => (
                0,
                t("first-hand-title"),
                t(if self.imported { "first-hand-imported" } else { "first-hand-hint" }),
                vec![
                    choice(t("first-hand-left"), self.state.dominant == Hand::Left),
                    choice(t("first-hand-right"), self.state.dominant == Hand::Right),
                ],
            ),
            Page::FirstMonitor => (
                0,
                t("first-monitor-title"),
                t("first-monitor-hint"),
                monitors
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let args =
                            [("n", (i + 1).to_string()), ("w", m.rect.w.to_string()), ("h", m.rect.h.to_string())];
                        choice(s.get("first-monitor", &args), m.primary)
                    })
                    .collect(),
            ),
            Page::FirstKeyboard => {
                let typed = self.test.text().to_lowercase();
                (
                    0,
                    t("first-keyboard-title"),
                    s.get("first-keyboard-hint", &[("typed", self.test.text().to_owned())]),
                    vec![choice(t("first-done"), typed.contains(TEST_WORD)), action(t("first-skip"), true)],
                )
            }
            Page::FirstTips => {
                body = t("first-tips-body");
                (
                    0,
                    t("first-tips-title"),
                    t("first-tips-hint"),
                    vec![hidden(), hidden(), hidden(), hidden(), choice(t("first-got-it"), true)],
                )
            }
            Page::Tab(0) => {
                let next = Self::unshown_monitor(panels, monitors);
                let removable = selected.is_some_and(|p| matches!(p.source, Source::Monitor(_) | Source::Window(_)));
                let add = match next {
                    Some(i) => s.get("menu-add-monitor", &[("n", (i + 1).to_string())]),
                    None => t("menu-add-monitor-none"),
                };
                let title = match selected {
                    Some(p) => s.get("menu-selected", &[("name", Self::panel_name(s, p))]),
                    None => t("panel-none"),
                };
                (
                    0,
                    title,
                    t("menu-panels-hint"),
                    vec![
                        action(add, next.is_some()),
                        action(t("menu-recenter"), true),
                        toggle(t("menu-lock"), selected.is_some_and(|p| p.locked), selected.is_some()),
                        toggle(t("menu-curved"), selected.is_some_and(|p| p.curvature > 0.0), selected.is_some()),
                        toggle(t("menu-attach"), selected.is_some_and(|p| p.anchor != Anchor::World), removable),
                        danger("menu-remove", 5, removable),
                    ],
                )
            }
            Page::Tab(1) => (
                1,
                t("menu-keyboard-title"),
                t("menu-keyboard-hint"),
                vec![
                    toggle(t("menu-show-keyboard"), visible(Source::Keyboard), true),
                    toggle(t("menu-chat"), self.state.chatbox, true),
                    toggle(t("menu-sounds"), self.state.typing_sounds, true),
                    action(t("menu-keyboard-here"), true),
                    toggle(t("menu-now-playing"), self.state.media_to_chatbox, true),
                ],
            ),
            Page::Tab(2) => (
                2,
                t("menu-wrist-title"),
                t(if self.notifications_ok { "menu-wrist-hint" } else { "menu-notifications-blocked" }),
                vec![
                    toggle(t("menu-show-wrist"), visible(Source::Wrist), true),
                    choice(t("menu-wrist-left"), wrist_hand == Some(Anchor::Hand(Hand::Left))),
                    choice(t("menu-wrist-right"), wrist_hand == Some(Anchor::Hand(Hand::Right))),
                ],
            ),
            Page::Tab(_) => {
                let pct = selected.map_or(100.0, |p| p.opacity * 100.0);
                let cost = [("gpu", format!("{:.2}", cost.0)), ("cpu", format!("{:.1}", cost.1))];
                (
                    3,
                    t("menu-settings-title"),
                    s.get("menu-cost", &cost),
                    vec![
                        choice(t("menu-point-left"), self.state.dominant == Hand::Left),
                        choice(t("menu-point-right"), self.state.dominant == Hand::Right),
                        action(s.get("menu-opacity", &[("pct", format!("{pct:.0}"))]), selected.is_some()),
                        danger("menu-quit", 3, true),
                    ],
                )
            }
        };
        let first_run = !matches!(self.page, Page::Tab(_));
        MenuModel { tabs: if first_run { Vec::new() } else { tabs }, tab, title, subtitle, body, slots }
    }

    /// A new screen panel with the user's defaults (curvature, opacity).
    fn new_screen(&self, id: u32, monitor: u32, head: Pose, m: &MonitorInfo) -> Panel {
        let mut p = Panel::in_front_of(id, Source::Monitor(monitor), head, aspect(m));
        p.curvature = if self.state.default_curved { CURVATURE } else { 0.0 };
        p.opacity = self.state.default_opacity;
        p
    }

    fn set_dominant(&mut self, hand: Hand, panels: &mut [Panel]) {
        self.state.dominant = hand;
        self.wear_wrist(hand.other(), panels);
    }

    fn wear_wrist(&mut self, hand: Hand, panels: &mut [Panel]) {
        if let Some(w) = find(panels, Source::Wrist) {
            *w = Panel { id: w.id, visible: w.visible, ..Panel::on_wrist(w.id, hand, w.aspect) };
        }
    }

    /// Moves every world panel so the main screen sits in front of the user again.
    fn recenter(panels: &mut [Panel], head: Pose) {
        let main = panels
            .iter()
            .find(|p| matches!(p.source, Source::Monitor(_) | Source::Window(_)) && p.anchor == Anchor::World)
            .or_else(|| panels.iter().find(|p| p.source == Source::Keyboard));
        let Some(main) = main.copied() else { return };
        let target = Panel::in_front_of(0, main.source, head, main.aspect).pose;
        let delta = target * main.pose.inverse();
        for p in panels.iter_mut().filter(|p| p.anchor == Anchor::World && p.id != MENU_ID) {
            p.pose = delta * p.pose;
        }
    }

    /// Applies a menu click. `hands` are the current controller poses (for attach/detach).
    pub fn input(
        &mut self,
        input: MenuInput,
        panels: &mut Vec<Panel>,
        head: Pose,
        hands: &[Pose; 2],
        monitors: &[MonitorInfo],
    ) -> Vec<Effect> {
        let mut fx = Vec::new();
        let slot = match input {
            MenuInput::Tab(i) => {
                self.armed = None;
                if matches!(self.page, Page::Tab(_)) && i < TABS {
                    self.page = Page::Tab(i);
                }
                return fx;
            }
            MenuInput::Slot(i) => i,
        };
        // Destructive buttons need a second tap on the same button; any other click disarms.
        let destructive = matches!((self.page, slot), (Page::Tab(0), 5) | (Page::Tab(3), 3));
        if destructive && self.armed != Some((self.page, slot)) {
            self.armed = Some((self.page, slot));
            return fx;
        }
        self.armed = None;
        let selected = self.selected;
        let sel = |panels: &mut Vec<Panel>| -> Option<usize> { panels.iter().position(|p| Some(p.id) == selected) };
        match (self.page, slot) {
            (Page::FirstHand, 0 | 1) => {
                self.set_dominant(if slot == 0 { Hand::Left } else { Hand::Right }, panels);
                self.page = Page::FirstMonitor;
            }
            (Page::FirstMonitor, i) => {
                if let Some(m) = monitors.get(i) {
                    let id = next_id(panels);
                    panels.push(self.new_screen(id, i as u32, head, m));
                    if let Some(k) = find(panels, Source::Keyboard) {
                        k.pose = Panel::keyboard_below(k.id, head).pose;
                        k.visible = true;
                    }
                    self.selected = Some(id);
                    self.page = Page::FirstKeyboard;
                    fx.push(Effect::Added(id));
                }
            }
            (Page::FirstKeyboard, 0 | 1) => self.page = Page::FirstTips,
            (Page::FirstTips, 4) => {
                self.state.onboarded = true;
                self.page = Page::Tab(0);
                if let Some(m) = panels.iter_mut().find(|p| p.id == MENU_ID) {
                    m.visible = false;
                }
            }
            (Page::Tab(0), 0) => {
                if let Some(i) = Self::unshown_monitor(panels, monitors) {
                    let id = next_id(panels);
                    panels.push(self.new_screen(id, i, head, &monitors[i as usize]));
                    self.selected = Some(id);
                    fx.push(Effect::Added(id));
                }
            }
            (Page::Tab(0), 1) => Self::recenter(panels, head),
            (Page::Tab(0), 2) => {
                if let Some(i) = sel(panels) {
                    panels[i].locked = !panels[i].locked;
                }
            }
            (Page::Tab(0), 3) => {
                if let Some(i) = sel(panels) {
                    panels[i].curvature = if panels[i].curvature > 0.0 { 0.0 } else { CURVATURE };
                }
            }
            (Page::Tab(0), 4) => {
                if let Some(i) = sel(panels) {
                    let world = panels[i].world_pose(hands);
                    panels[i].anchor = match panels[i].anchor {
                        Anchor::World => Anchor::Hand(self.state.dominant.other()),
                        Anchor::Hand(_) => Anchor::World,
                    };
                    panels[i].set_world_pose(world, hands);
                }
            }
            (Page::Tab(0), 5) => {
                if let Some(i) =
                    sel(panels).filter(|&i| matches!(panels[i].source, Source::Monitor(_) | Source::Window(_)))
                {
                    let id = panels.remove(i).id;
                    self.selected = None;
                    fx.push(Effect::Removed(id));
                }
            }
            (Page::Tab(1), 0) => {
                if let Some(k) = find(panels, Source::Keyboard) {
                    k.visible = !k.visible;
                }
            }
            (Page::Tab(1), 1) => {
                self.state.chatbox = !self.state.chatbox;
                fx.push(Effect::RedrawKeyboard);
            }
            (Page::Tab(1), 2) => self.state.typing_sounds = !self.state.typing_sounds,
            (Page::Tab(1), 3) => {
                if let Some(k) = find(panels, Source::Keyboard) {
                    *k = Panel { id: k.id, ..Panel::keyboard_below(k.id, head) };
                }
            }
            (Page::Tab(1), 4) => self.state.media_to_chatbox = !self.state.media_to_chatbox,
            (Page::Tab(2), 0) => {
                if let Some(w) = find(panels, Source::Wrist) {
                    w.visible = !w.visible;
                }
            }
            (Page::Tab(2), 1 | 2) => self.wear_wrist(if slot == 1 { Hand::Left } else { Hand::Right }, panels),
            (Page::Tab(3), 0 | 1) => self.set_dominant(if slot == 0 { Hand::Left } else { Hand::Right }, panels),
            (Page::Tab(3), 2) => {
                if let Some(i) = sel(panels) {
                    let now = OPACITY_STEPS.iter().position(|o| (*o - panels[i].opacity).abs() < 0.01).unwrap_or(0);
                    panels[i].opacity = OPACITY_STEPS[(now + 1) % OPACITY_STEPS.len()];
                }
            }
            (Page::Tab(3), 3) => fx.push(Effect::Quit),
            _ => {}
        }
        fx
    }
}
