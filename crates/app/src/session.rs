//! Menu, toolbar, hide-all and first-run logic, independent of SteamVR and the GPU so gates can drive it.
//! Panels live in the engine's list; the menu (`MENU_ID`) and toolbar (`TOOLBAR_ID`) are among them and are never
//! persisted.
use dvr_core::chat::ChatLine;
use dvr_core::engine::{Event, MouseButton};
use dvr_core::math::Pose;
use dvr_core::panel::{
    Anchor, Anchors, Hand, MAX_WIDTH_M, MENU_ID, MIN_WIDTH_M, Panel, PixelRect, Source, TOOLBAR_ID, TOOLBAR_WIDTH_M,
};
use dvr_core::state::{NotifyFrom, State};
use dvr_ui::{
    MENU_SIZE, Menu, MenuInput, MenuModel, Slot, SlotKind, Strings, TOOLBAR_SIZE, ToolButton, Toolbar, WRIST_SIZE,
};

const OPACITY_STEPS: [f32; 4] = [1.0, 0.85, 0.7, 0.5];
const CURVATURE: f32 = 0.15;
const TABS: usize = 4;
/// Size buttons scale a panel by this factor.
const SIZE_STEP: f32 = 1.25;
/// The toolbar stays this long after the laser leaves its panel (time to move down onto it).
const TOOLBAR_LINGER_MS: u64 = 1500;
/// Fades (hide-all, the watch) take about this long.
const FADE_S: f32 = 0.15;
/// Windows listed per page of "Add window".
const WINDOWS_PER_PAGE: usize = 4;
/// Notifications kept for the history page (newest first).
pub const HISTORY_LEN: usize = 3;
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
    /// "Add window" list, page n.
    Windows(usize),
    /// Recent notifications.
    Notifications,
}

/// What the platform layer must do after a menu, toolbar or wrist action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Added(u32),
    Removed(u32),
    RedrawKeyboard,
    Quit,
    /// Refresh `Session::windows` (the "Add window" list opened).
    ListWindows,
    /// Show this window as a panel in front of the user.
    AddWindow(u64),
    NextLanguage,
    AutoLaunch(bool),
    /// Where app notifications come from changed (start or stop listening).
    NotifyFrom(NotifyFrom),
}

/// Toolbar buttons, in the order shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Lock,
    Curve,
    Smaller,
    Bigger,
    Opacity,
    Attach,
    Close,
}

const SCREEN_TOOLS: &[Tool] =
    &[Tool::Lock, Tool::Curve, Tool::Smaller, Tool::Bigger, Tool::Opacity, Tool::Attach, Tool::Close];
const KEYBOARD_TOOLS: &[Tool] = &[Tool::Lock, Tool::Smaller, Tool::Bigger, Tool::Opacity, Tool::Close];

fn tools(source: Source) -> &'static [Tool] {
    match source {
        Source::Monitor(_) | Source::Window(_) => SCREEN_TOOLS,
        Source::Keyboard => KEYBOARD_TOOLS,
        Source::Wrist | Source::Menu | Source::Toolbar => &[],
    }
}

pub struct Session {
    pub state: State,
    pub page: Page,
    test: ChatLine,
    menu_hovered: bool,
    /// Settings came from another overlay (shown on the first-run page).
    pub imported: bool,
    /// False when another app holds the notification port (shown on the Wrist tab).
    pub notifications_ok: bool,
    /// Danger button waiting for its confirming second tap.
    armed: Option<(Page, usize)>,
    /// Screens, windows and keyboard are faded out (hold left menu, or Hide all).
    pub hidden: bool,
    /// Windows the user can add, front to back (filled by the app on `Effect::ListWindows`).
    pub windows: Vec<(u64, String)>,
    /// Current input language, e.g. "EN" (kept by the app).
    pub language: String,
    /// SteamVR starts the overlay with itself (kept by the app).
    pub auto_launch: bool,
    /// Windows refuses to share app notifications (kept by the app).
    pub notify_blocked: bool,
    /// Recent notifications (title, body), newest first.
    pub history: std::collections::VecDeque<(String, String)>,
    /// Panel whose toolbar is shown, and when the laser was last on it or its toolbar.
    toolbar: Option<u32>,
    toolbar_seen: u64,
    toolbar_hovered: bool,
}

pub fn next_id(panels: &[Panel]) -> u32 {
    panels.iter().filter(|p| p.id < TOOLBAR_ID).map(|p| p.id + 1).max().unwrap_or(1)
}

fn aspect(m: &MonitorInfo) -> f32 {
    m.rect.h as f32 / m.rect.w as f32
}

fn find(panels: &mut [Panel], source: Source) -> Option<&mut Panel> {
    panels.iter_mut().find(|p| p.source == source)
}

fn hand_name(s: &Strings, hand: Hand) -> String {
    s.get(if hand == Hand::Left { "hand-left" } else { "hand-right" }, &[])
}

impl Session {
    /// Ensures keyboard, wrist, menu and toolbar panels exist; opens the first-run flow when not onboarded.
    pub fn new(state: State, panels: &mut Vec<Panel>, head: Pose) -> Self {
        if !panels.iter().any(|p| p.source == Source::Keyboard) {
            panels.push(Panel::keyboard_below(next_id(panels), head));
        }
        if !panels.iter().any(|p| p.source == Source::Wrist) {
            let aspect = WRIST_SIZE.1 as f32 / WRIST_SIZE.0 as f32;
            panels.push(Panel::on_wrist(next_id(panels), state.dominant.other(), aspect));
        }
        // A wrist saved by an older build has the old size and orientation.
        if let Some(w) = find(panels, Source::Wrist) {
            *w = Panel {
                visible: w.visible,
                ..Panel::on_wrist(w.id, w.anchor_hand(), WRIST_SIZE.1 as f32 / WRIST_SIZE.0 as f32)
            };
        }
        let mut menu = Panel::menu_in_front(head, MENU_SIZE.1 as f32 / MENU_SIZE.0 as f32);
        menu.visible = !state.onboarded;
        let mut toolbar = Panel::new(
            TOOLBAR_ID,
            Source::Toolbar,
            Pose::IDENTITY,
            TOOLBAR_WIDTH_M,
            TOOLBAR_SIZE.1 as f32 / TOOLBAR_SIZE.0 as f32,
        );
        toolbar.locked = true;
        toolbar.visible = false;
        panels.retain(|p| p.id != MENU_ID && p.id != TOOLBAR_ID);
        panels.push(menu);
        panels.push(toolbar);
        let page = if state.onboarded { Page::Tab(0) } else { Page::FirstHand };
        Self {
            state,
            page,
            test: ChatLine::default(),
            menu_hovered: false,
            imported: false,
            notifications_ok: true,
            armed: None,
            hidden: false,
            windows: Vec::new(),
            language: String::new(),
            auto_launch: false,
            notify_blocked: false,
            history: std::collections::VecDeque::new(),
            toolbar: None,
            toolbar_seen: 0,
            toolbar_hovered: false,
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
                if matches!(self.page, Page::Windows(_) | Page::Notifications) {
                    self.page = Page::Tab(0);
                }
            }
            m.visible = !open;
        }
        if !open {
            self.hidden = false;
        }
    }

    /// Hold left menu / Hide all: fade screens, windows and keyboard out or back in. Hiding closes the menu.
    pub fn toggle_hidden(&mut self, panels: &mut [Panel]) {
        self.hidden = !self.hidden;
        if self.hidden {
            self.toolbar = None;
            if let Some(m) = panels.iter_mut().find(|p| p.id == MENU_ID) {
                m.visible = false;
            }
        }
    }

    /// Steps the fades. `watch` is how visible the watch should be (0..1); `dt` seconds since the last call.
    pub fn update_dims(&self, panels: &mut [Panel], watch: f32, dt: f32) {
        let k = (dt / FADE_S).min(1.0);
        for p in panels.iter_mut() {
            let target = match p.source {
                Source::Monitor(_) | Source::Window(_) | Source::Keyboard => f32::from(u8::from(self.hidden)),
                Source::Wrist => 1.0 - watch,
                Source::Menu | Source::Toolbar => 0.0,
            };
            p.dim += (target - p.dim) * k;
            if (target - p.dim).abs() < 0.01 {
                p.dim = target;
            }
        }
    }

    /// Panels persisted in the layout (everything but the menu and toolbar).
    pub fn persist(&mut self, panels: &[Panel]) {
        self.state.panels = panels.iter().filter(|p| p.id < TOOLBAR_ID).copied().collect();
    }

    /// Routes laser events on the menu and toolbar to their views and decides which panel shows the toolbar.
    /// Returns true when the menu saw pointer activity.
    pub fn pointer(&mut self, events: &[Event], panels: &[Panel], menu: &Menu, toolbar: &Toolbar, now: u64) -> bool {
        let mut over_menu = false;
        let mut over_toolbar = false;
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
                Event::Hover { panel: TOOLBAR_ID, uv, .. } => {
                    over_toolbar = true;
                    toolbar.pointer_move(uv);
                }
                Event::Mouse { panel: TOOLBAR_ID, uv, button: MouseButton::Left, down } => {
                    over_toolbar = true;
                    toolbar.pointer_button(uv, down);
                }
                Event::Hover { panel, .. } | Event::Mouse { panel, .. } | Event::Moved { panel }
                    if panels.iter().any(|p| p.id == panel && !tools(p.source).is_empty()) =>
                {
                    self.toolbar = Some(panel);
                    self.toolbar_seen = now;
                }
                _ => {}
            }
        }
        if over_toolbar {
            self.toolbar_seen = now;
        }
        if self.toolbar.is_some() && now.saturating_sub(self.toolbar_seen) > TOOLBAR_LINGER_MS {
            self.toolbar = None;
        }
        if self.menu_hovered && !over_menu {
            menu.pointer_leave();
        }
        if self.toolbar_hovered && !over_toolbar {
            toolbar.pointer_leave();
        }
        self.menu_hovered = over_menu;
        self.toolbar_hovered = over_toolbar;
        over_menu
    }

    /// Remembers a notification for the history page.
    pub fn notified(&mut self, title: &str, body: &str) {
        self.history.push_front((title.to_owned(), body.to_owned()));
        self.history.truncate(HISTORY_LEN);
    }

    /// Panel the toolbar belongs to, if it is showing.
    pub fn toolbar_target(&self) -> Option<u32> {
        self.toolbar
    }

    /// Puts the toolbar under its panel (or hides it). Call every frame before stepping the engine.
    pub fn place_toolbar(&mut self, panels: &mut [Panel], hands: &Anchors) {
        let target = self.toolbar.and_then(|id| panels.iter().find(|p| p.id == id && p.shown()).copied());
        if target.is_none() {
            self.toolbar = None;
        }
        if let Some(bar) = panels.iter_mut().find(|p| p.id == TOOLBAR_ID) {
            bar.visible = target.is_some();
            if let Some(t) = target {
                bar.pose = t.toolbar_pose(t.world_pose(hands), bar.aspect);
            }
        }
    }

    /// Buttons for the panel under the toolbar.
    pub fn toolbar_model(&self, panels: &[Panel], s: &Strings) -> Vec<ToolButton> {
        let Some(p) = self.toolbar.and_then(|id| panels.iter().find(|p| p.id == id)) else { return Vec::new() };
        let t = |id: &str| s.get(id, &[]);
        tools(p.source)
            .iter()
            .map(|tool| {
                let (label, on) = match tool {
                    Tool::Lock => (t("tool-lock"), p.locked),
                    Tool::Curve => (t("tool-curve"), p.curvature > 0.0),
                    Tool::Smaller => ("−".to_owned(), false),
                    Tool::Bigger => ("+".to_owned(), false),
                    Tool::Opacity => (format!("{:.0}%", p.opacity * 100.0), false),
                    Tool::Attach => match p.anchor {
                        Anchor::World => (t("tool-world"), false),
                        Anchor::Hand(_) => (t("tool-hand"), true),
                        Anchor::Head => (t("tool-head"), true),
                    },
                    Tool::Close => (t("tool-close"), false),
                };
                ToolButton { label, on, danger: *tool == Tool::Close }
            })
            .collect()
    }

    /// Applies a toolbar click (button index).
    pub fn toolbar_input(&mut self, button: usize, panels: &mut Vec<Panel>, hands: &Anchors) -> Vec<Effect> {
        let mut fx = Vec::new();
        let Some(i) = self.toolbar.and_then(|id| panels.iter().position(|p| p.id == id)) else { return fx };
        let Some(&tool) = tools(panels[i].source).get(button) else { return fx };
        let p = &mut panels[i];
        match tool {
            Tool::Lock => p.locked = !p.locked,
            Tool::Curve => p.curvature = if p.curvature > 0.0 { 0.0 } else { CURVATURE },
            Tool::Smaller => p.width_m = (p.width_m / SIZE_STEP).max(MIN_WIDTH_M),
            Tool::Bigger => p.width_m = (p.width_m * SIZE_STEP).min(MAX_WIDTH_M),
            Tool::Opacity => {
                let now = OPACITY_STEPS.iter().position(|o| (*o - p.opacity).abs() < 0.01).unwrap_or(0);
                p.opacity = OPACITY_STEPS[(now + 1) % OPACITY_STEPS.len()];
            }
            // World -> hand -> head (HUD) -> world; the panel stays where it is when it changes anchor.
            Tool::Attach => {
                let world = p.world_pose(hands);
                p.anchor = match p.anchor {
                    Anchor::World => Anchor::Hand(self.state.dominant.other()),
                    Anchor::Hand(_) => Anchor::Head,
                    Anchor::Head => Anchor::World,
                };
                p.set_world_pose(world, hands);
            }
            Tool::Close if p.source == Source::Keyboard => {
                p.visible = false;
                self.toolbar = None;
            }
            Tool::Close => {
                let id = panels.remove(i).id;
                self.toolbar = None;
                fx.push(Effect::Removed(id));
            }
        }
        fx
    }

    /// During the keyboard test, typed keys go to the test line instead of the OS. Returns true when consumed.
    pub fn key(&mut self, sc: u16, down: bool, to_char: impl Fn(u16, bool, bool) -> Option<char>) -> bool {
        if self.page != Page::FirstKeyboard {
            return false;
        }
        self.test.key(sc, down, to_char);
        true
    }

    fn unshown_monitor(panels: &[Panel], monitors: &[MonitorInfo]) -> Option<u32> {
        (0..monitors.len() as u32).find(|i| !panels.iter().any(|p| p.source == Source::Monitor(*i)))
    }

    /// What the menu shows now. `cost` is (GPU ms, CPU %) of this overlay.
    pub fn model(&self, panels: &[Panel], monitors: &[MonitorInfo], s: &Strings, cost: (f32, f32)) -> MenuModel {
        let t = |id: &str| s.get(id, &[]);
        let mk = |label: String, kind: SlotKind, on: bool, enabled: bool| Slot { label, on, enabled, kind };
        let action = |label: String, enabled: bool| mk(label, SlotKind::Action, false, enabled);
        let toggle = |label: String, on: bool| mk(label, SlotKind::Toggle, on, true);
        let choice = |label: String, on: bool| mk(label, SlotKind::Choice, on, true);
        let hidden = || mk(String::new(), SlotKind::Hidden, false, false);
        // A danger button shows "tap again" once armed.
        let danger = |id: &str, slot: usize| {
            let armed = self.armed == Some((self.page, slot));
            mk(if armed { t("menu-confirm") } else { t(id) }, SlotKind::Danger, armed, true)
        };
        let tabs = ["menu-tab-panels", "menu-tab-keyboard", "menu-tab-wrist", "menu-tab-settings"].map(t).to_vec();
        let visible = |src: Source| panels.iter().any(|p| p.source == src && p.visible);
        let wrist_hand = panels.iter().find(|p| p.source == Source::Wrist).map_or(Hand::Left, Panel::anchor_hand);
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
                let add = match next {
                    Some(i) => s.get("menu-add-monitor", &[("n", (i + 1).to_string())]),
                    None => t("menu-add-monitor-none"),
                };
                (
                    0,
                    t("menu-panels-title"),
                    t("menu-panels-hint"),
                    vec![
                        action(add, next.is_some()),
                        action(t("menu-add-window"), true),
                        action(t("menu-recenter"), true),
                        action(t("menu-hide-all"), true),
                        toggle(t("menu-show-keyboard"), visible(Source::Keyboard)),
                        action(s.get("menu-notifications", &[("n", self.history.len().to_string())]), true),
                    ],
                )
            }
            Page::Notifications => {
                let mut slots: Vec<Slot> = (0..HISTORY_LEN)
                    .map(|i| match self.history.get(i) {
                        Some((title, body)) if body.is_empty() => action(title.clone(), true),
                        Some((title, body)) => action(format!("{title}: {body}"), true),
                        None => hidden(),
                    })
                    .collect();
                let from = match self.state.notify_from {
                    NotifyFrom::Discord => t("notify-discord"),
                    NotifyFrom::AllApps => t("notify-all"),
                    NotifyFrom::Off => t("notify-off"),
                };
                slots.push(action(s.get("menu-notify-from", &[("from", from)]), true));
                slots.push(action(t("menu-clear"), !self.history.is_empty()));
                slots.push(action(t("menu-back"), true));
                let hint = if self.notify_blocked && self.state.notify_from != NotifyFrom::Off {
                    t("menu-notify-blocked")
                } else if self.history.is_empty() {
                    t("menu-notifications-none")
                } else {
                    t("menu-notifications-hint")
                };
                (0, t("menu-notifications-title"), hint, slots)
            }
            Page::Windows(page) => {
                let start = page * WINDOWS_PER_PAGE;
                let mut slots: Vec<Slot> = (start..start + WINDOWS_PER_PAGE)
                    .map(|i| match self.windows.get(i) {
                        Some((h, title)) => {
                            let shown = panels.iter().any(|p| p.source == Source::Window(*h));
                            mk(title.clone(), SlotKind::Choice, shown, !shown)
                        }
                        None => hidden(),
                    })
                    .collect();
                slots.push(action(t("menu-more"), self.windows.len() > start + WINDOWS_PER_PAGE));
                slots.push(action(t("menu-back"), true));
                let hint = if self.windows.is_empty() { t("menu-windows-none") } else { t("menu-windows-hint") };
                (0, t("menu-windows-title"), hint, slots)
            }
            Page::Tab(1) => (
                1,
                t("menu-keyboard-title"),
                t("menu-keyboard-hint"),
                vec![
                    toggle(t("menu-show-keyboard"), visible(Source::Keyboard)),
                    action(t("menu-keyboard-here"), true),
                    action(s.get("menu-language", &[("lang", self.language.clone())]), true),
                    toggle(t("menu-chat"), self.state.chatbox),
                    toggle(t("menu-sounds"), self.state.typing_sounds),
                    toggle(t("menu-now-playing"), self.state.media_to_chatbox),
                ],
            ),
            Page::Tab(2) => (
                2,
                t("menu-wrist-title"),
                t(if self.notifications_ok { "menu-wrist-hint" } else { "menu-notifications-blocked" }),
                vec![
                    toggle(t("menu-show-wrist"), visible(Source::Wrist)),
                    action(s.get("menu-wrist-on", &[("hand", hand_name(s, wrist_hand))]), true),
                    toggle(t("menu-wrist-fade"), self.state.wrist_fade),
                    toggle(t("menu-wrist-battery"), self.state.wrist_battery),
                    toggle(t("menu-wrist-load"), self.state.wrist_load),
                    toggle(t("menu-wrist-media"), self.state.wrist_media),
                ],
            ),
            Page::Tab(_) => {
                let cost = [("gpu", format!("{:.2}", cost.0)), ("cpu", format!("{:.1}", cost.1))];
                (
                    3,
                    t("menu-settings-title"),
                    s.get("menu-cost", &cost),
                    vec![
                        action(s.get("menu-point", &[("hand", hand_name(s, self.state.dominant))]), true),
                        toggle(t("menu-autostart"), self.auto_launch),
                        toggle(t("menu-haptics"), self.state.haptics),
                        toggle(t("menu-curved-default"), self.state.default_curved),
                        toggle(t("menu-update-check"), self.state.update_check),
                        danger("menu-quit", 5),
                    ],
                )
            }
        };
        let first_run =
            matches!(self.page, Page::FirstHand | Page::FirstMonitor | Page::FirstKeyboard | Page::FirstTips);
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

    fn close(panels: &mut [Panel]) {
        if let Some(m) = panels.iter_mut().find(|p| p.id == MENU_ID) {
            m.visible = false;
        }
    }

    fn toggle_keyboard(panels: &mut [Panel], head: Pose) {
        if let Some(k) = find(panels, Source::Keyboard) {
            k.visible = !k.visible;
            // Shown again: in front of the user, wherever they turned since.
            if k.visible {
                k.pose = Panel::keyboard_below(k.id, head).pose;
                k.anchor = Anchor::World;
            }
        }
    }

    /// Wrist "Keyboard" button.
    pub fn wrist_keyboard(&mut self, panels: &mut [Panel], head: Pose) {
        self.hidden = false;
        Self::toggle_keyboard(panels, head);
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
        for p in panels.iter_mut().filter(|p| p.anchor == Anchor::World && p.id < TOOLBAR_ID) {
            p.pose = delta * p.pose;
        }
    }

    /// Applies a menu click. `hands` are the current controller poses.
    pub fn input(
        &mut self,
        input: MenuInput,
        panels: &mut Vec<Panel>,
        head: Pose,
        hands: &Anchors,
        monitors: &[MonitorInfo],
    ) -> Vec<Effect> {
        let _ = hands;
        let mut fx = Vec::new();
        let slot = match input {
            MenuInput::Tab(i) => {
                self.armed = None;
                if matches!(self.page, Page::Tab(_) | Page::Windows(_) | Page::Notifications) && i < TABS {
                    self.page = Page::Tab(i);
                }
                return fx;
            }
            MenuInput::Slot(i) => i,
            MenuInput::Close => {
                self.armed = None;
                if let Some(m) = panels.iter_mut().find(|p| p.id == MENU_ID) {
                    m.visible = false;
                }
                return fx;
            }
        };
        // Destructive buttons need a second tap on the same button; any other click disarms.
        let destructive = matches!((self.page, slot), (Page::Tab(3), 5));
        if destructive && self.armed != Some((self.page, slot)) {
            self.armed = Some((self.page, slot));
            return fx;
        }
        self.armed = None;
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
                    self.page = Page::FirstKeyboard;
                    fx.push(Effect::Added(id));
                }
            }
            (Page::FirstKeyboard, 0 | 1) => self.page = Page::FirstTips,
            (Page::FirstTips, 4) => {
                self.state.onboarded = true;
                self.page = Page::Tab(0);
                for p in panels.iter_mut().filter(|p| p.id == MENU_ID || p.source == Source::Keyboard) {
                    p.visible = false;
                }
            }
            // Adding a screen or window closes the menu so the new panel is not hidden behind it.
            (Page::Tab(0), 0) => {
                if let Some(i) = Self::unshown_monitor(panels, monitors) {
                    let id = next_id(panels);
                    panels.push(self.new_screen(id, i, head, &monitors[i as usize]));
                    fx.push(Effect::Added(id));
                    Self::close(panels);
                }
            }
            (Page::Tab(0), 1) => {
                self.page = Page::Windows(0);
                fx.push(Effect::ListWindows);
            }
            (Page::Tab(0), 2) => Self::recenter(panels, head),
            (Page::Tab(0), 3) => self.toggle_hidden(panels),
            (Page::Tab(0), 4) | (Page::Tab(1), 0) => Self::toggle_keyboard(panels, head),
            (Page::Windows(page), i) if i < WINDOWS_PER_PAGE => {
                if let Some(&(h, _)) = self.windows.get(page * WINDOWS_PER_PAGE + i)
                    && !panels.iter().any(|p| p.source == Source::Window(h))
                {
                    fx.push(Effect::AddWindow(h));
                    self.page = Page::Tab(0);
                    Self::close(panels);
                }
            }
            (Page::Windows(page), 4) => {
                if self.windows.len() > (page + 1) * WINDOWS_PER_PAGE {
                    self.page = Page::Windows(page + 1);
                }
            }
            (Page::Windows(_), _) => self.page = Page::Tab(0),
            (Page::Tab(0), 5) => self.page = Page::Notifications,
            (Page::Notifications, 3) => {
                self.state.notify_from = self.state.notify_from.next();
                fx.push(Effect::NotifyFrom(self.state.notify_from));
            }
            (Page::Notifications, 4) => self.history.clear(),
            (Page::Notifications, 5) => self.page = Page::Tab(0),
            (Page::Tab(1), 1) => {
                if let Some(k) = find(panels, Source::Keyboard) {
                    *k = Panel { id: k.id, ..Panel::keyboard_below(k.id, head) };
                }
            }
            (Page::Tab(1), 2) => fx.push(Effect::NextLanguage),
            (Page::Tab(1), 3) => {
                self.state.chatbox = !self.state.chatbox;
                fx.push(Effect::RedrawKeyboard);
            }
            (Page::Tab(1), 4) => self.state.typing_sounds = !self.state.typing_sounds,
            (Page::Tab(1), 5) => self.state.media_to_chatbox = !self.state.media_to_chatbox,
            (Page::Tab(2), 0) => {
                if let Some(w) = find(panels, Source::Wrist) {
                    w.visible = !w.visible;
                }
            }
            (Page::Tab(2), 1) => {
                let now = panels.iter().find(|p| p.source == Source::Wrist).map_or(Hand::Left, Panel::anchor_hand);
                self.wear_wrist(now.other(), panels);
            }
            (Page::Tab(2), 2) => self.state.wrist_fade = !self.state.wrist_fade,
            (Page::Tab(2), 3) => self.state.wrist_battery = !self.state.wrist_battery,
            (Page::Tab(2), 4) => self.state.wrist_load = !self.state.wrist_load,
            (Page::Tab(2), 5) => self.state.wrist_media = !self.state.wrist_media,
            (Page::Tab(3), 0) => self.set_dominant(self.state.dominant.other(), panels),
            (Page::Tab(3), 1) => {
                self.auto_launch = !self.auto_launch;
                fx.push(Effect::AutoLaunch(self.auto_launch));
            }
            (Page::Tab(3), 2) => self.state.haptics = !self.state.haptics,
            (Page::Tab(3), 3) => self.state.default_curved = !self.state.default_curved,
            (Page::Tab(3), 4) => self.state.update_check = !self.state.update_check,
            (Page::Tab(3), 5) => fx.push(Effect::Quit),
            _ => {}
        }
        fx
    }
}
