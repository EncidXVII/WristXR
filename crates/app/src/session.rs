//! Menu, toolbar, hide-all and first-run logic, independent of SteamVR and the GPU so gates can drive it.
//! Panels live in the engine's list; the menu (`MENU_ID`) and toolbar (`TOOLBAR_ID`) are among them and are never
//! persisted.
use dvr_core::chat::ChatLine;
use dvr_core::engine::{Event, MouseButton};
use dvr_core::math::Pose;
use dvr_core::panel::{
    Anchor, Anchors, DEFAULT_DISTANCE_M, DEFAULT_WIDTH_M, Hand, MAX_WIDTH_M, MENU_ID, MIN_WIDTH_M, Panel, PixelRect,
    Source, TOOLBAR_ID, TOOLBAR_WIDTH_M, yaw_frame,
};
use dvr_core::state::{Accent, NotifyFrom, State, WatchLayout};
use dvr_ui::{
    Col, Layout, MENU_SIZE, Menu, MenuInput, MenuModel, ROW_H, Slot, SlotKind, Strings, TOOLBAR_SIZE, Tab, Tone,
    ToolButton, ToolInput, ToolKind, Toolbar, ToolbarLayout, ToolbarModel, WRIST_SIZE, icons,
};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

/// The opacity slider runs from this to fully opaque.
const MIN_OPACITY: f32 = 0.2;
const CURVATURE: f32 = 0.15;
/// Rail pages: Panels, Keys, Watch, Inbox, Setup.
pub const TABS: usize = 5;
pub const TAB_PANELS: usize = 0;
pub const TAB_KEYS: usize = 1;
pub const TAB_WATCH: usize = 2;
pub const TAB_INBOX: usize = 3;
pub const TAB_SETUP: usize = 4;
/// Sizes the toolbar's - and + step through (metres wide), so the readout always shows a round number.
const SIZES_M: [f32; 15] = [0.3, 0.4, 0.5, 0.6, 0.8, 1.0, 1.2, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0];
/// A size change eases over about this long instead of jumping.
const SIZE_EASE_S: f32 = 0.07;
/// The toolbar stays this long after the laser leaves its panel (time to move down onto it).
const TOOLBAR_LINGER_MS: u64 = 1500;
/// Fade lengths (linear, seconds). Hiding is quicker than showing: going away should never feel sluggish. The
/// watch answers a glance at once and leaves a little slower, so a glance that ends early does not cut it.
const SHOW_S: f32 = 0.2;
const HIDE_S: f32 = 0.1;
const WATCH_SHOW_S: f32 = 0.08;
const WATCH_HIDE_S: f32 = 0.18;
/// Windows listed per page of "Add window".
const WINDOWS_PER_PAGE: usize = 8;
/// Notifications kept for the Inbox (newest first).
pub const HISTORY_LEN: usize = 5;
/// Panels listed under "Open now" before "and n more".
const OPEN_NOW_ROWS: usize = 3;
/// Languages offered on the keyboard's toolbar.
const TOOLBAR_LANGUAGES: usize = 3;
/// An added window sits this much closer than the screens so it is never behind one.
pub const WINDOW_NEARER: f32 = 0.85;
/// Gap between screens when the layout is reset (radians of the arc around you).
const LAYOUT_GAP_RAD: f32 = 0.04;
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
    /// Rail page (`TAB_*`).
    Tab(usize),
    /// "Add window" list, page n.
    Windows(usize),
    /// Watch > Layout: what the closed watch shows.
    WatchLayouts,
    /// Setup > Theme: accent colours.
    Theme,
    /// Setup > Controls: what every button does.
    Controls,
}

impl Page {
    /// Rail tab this page belongs to.
    fn tab(self) -> usize {
        match self {
            Self::Tab(i) => i.min(TABS - 1),
            Self::WatchLayouts => TAB_WATCH,
            Self::Theme | Self::Controls => TAB_SETUP,
            _ => TAB_PANELS,
        }
    }

    /// A sub-page that opens from a rail page (closing the menu goes back to that page).
    fn sub(self) -> bool {
        matches!(self, Self::Windows(_) | Self::WatchLayouts | Self::Theme | Self::Controls)
    }
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
    /// Switch to installed input language `i`.
    Language(usize),
    AutoLaunch(bool),
    /// Where app notifications come from changed (start or stop listening).
    NotifyFrom(NotifyFrom),
    /// The accent changed: recolour every view.
    Accent,
    /// Open SteamVR's binding screen for DesktopVR.
    Rebind,
}

/// What a menu slot does (kept for the slots last drawn, so a click acts on what the user saw).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    None,
    FirstHand(Hand),
    FirstMonitor(usize),
    FirstDone,
    GotIt,
    AddScreen,
    OpenWindows,
    Recenter,
    ResetLayout,
    Screens,
    Keyboard,
    ClosePanel(u32),
    AddWindow(u64),
    MoreWindows,
    Back,
    NotifyFrom(NotifyFrom),
    ClearInbox,
    KeyboardHere,
    Language(usize),
    NextLanguage,
    Chat,
    Sounds,
    NowPlaying,
    WristBattery,
    WristMedia,
    WristLoad,
    WristFade,
    OpenLayouts,
    Layout(WatchLayout),
    OpenTheme,
    Accent(Accent),
    OpenControls,
    HoldRightClick,
    /// SteamVR's binding screen.
    Rebind,
    DragScroll,
    LaserHand(Hand),
    WatchHand(Hand),
    AutoStart,
    Haptics,
    Smoothing,
    Updates,
    Curved,
    Quit,
}

/// Toolbar controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Name chip / size readout (read-only).
    None,
    Lock,
    Curve,
    /// Held, it keeps the turn you give it instead of facing you.
    FreeTurn,
    Smaller,
    Bigger,
    Opacity,
    /// Room / hand / head.
    Pin(Pin),
    Language(usize),
    /// Back in front of you at its default place (and size, for screens and the keyboard).
    Reset,
    /// Screens and windows: remove. Keyboard: put away.
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pin {
    Room,
    Hand,
    Head,
}

fn has_toolbar(source: Source) -> bool {
    matches!(source, Source::Monitor(_) | Source::Window(_) | Source::Keyboard)
}

/// A notification kept for the Inbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub app: String,
    pub title: String,
    pub body: String,
    /// `Session` clock (ms) when it came in.
    pub at: u64,
    pub read: bool,
}

pub struct Session {
    pub state: State,
    pub page: Page,
    test: ChatLine,
    menu_hovered: bool,
    /// Settings came from another overlay (shown on the first-run page).
    pub imported: bool,
    /// False when another app holds the notification port (shown on the Watch page).
    pub notifications_ok: bool,
    /// Quit waits for its confirming second tap.
    armed: bool,
    /// Screens, windows and keyboard are faded out (double-tap left menu, or the Overlay switch).
    pub hidden: bool,
    /// Where the user was facing when everything was hidden: showing again brings the layout to where they face now.
    hidden_head: Option<Pose>,
    /// Windows the user can add, front to back (filled by the app on `Effect::ListWindows`).
    pub windows: Vec<(u64, String)>,
    /// Names of windows shown as panels (for their toolbar and "Open now").
    pub window_names: HashMap<u64, String>,
    /// Current input language, e.g. "EN", and the installed ones (kept by the app).
    pub language: String,
    pub languages: Vec<String>,
    /// SteamVR starts the overlay with itself (kept by the app).
    pub auto_launch: bool,
    /// The menu fades in when it opens (Windows "Animation effects" on; set by the app).
    pub motion: bool,
    /// Windows refuses to share app notifications (kept by the app).
    pub notify_blocked: bool,
    /// Recent notifications, newest first.
    pub history: VecDeque<Note>,
    /// Latest time seen (ms), for "2 m ago".
    clock: u64,
    /// Panel whose toolbar is shown, and when the laser was last on it or its toolbar.
    toolbar: Option<u32>,
    toolbar_seen: u64,
    /// When the toolbar last moved to its panel.
    toolbar_at: u64,
    /// The panel with a toolbar each hand's laser is on, and since when.
    pointing: [Option<(u32, u64)>; 2],
    toolbar_hovered: bool,
    /// A panel easing to the size the toolbar's - or + asked for.
    size_goal: Option<(u32, f32)>,
    /// What each slot / toolbar control last drawn does.
    acts: RefCell<Vec<Act>>,
    tools: RefCell<Vec<Tool>>,
}

/// What `Session::pointer` sees of one frame.
pub struct PointerInput<'a> {
    pub events: &'a [Event],
    pub now: u64,
    pub rays: [Option<Pose>; 2],
    pub anchors: Anchors,
}

pub fn next_id(panels: &[Panel]) -> u32 {
    panels.iter().filter(|p| p.id < TOOLBAR_ID).map(|p| p.id + 1).max().unwrap_or(1)
}

fn aspect(m: &MonitorInfo) -> f32 {
    m.rect.h as f32 / m.rect.w as f32
}

/// Turns and moves the room-placed screens, windows and keyboard, as arranged, from around the head at `then` to
/// around the head at `now` (turned only about the vertical).
fn carry(panels: &mut [Panel], then: Pose, now: Pose) {
    let carry = yaw_frame(now) * yaw_frame(then).inverse();
    for p in panels.iter_mut().filter(|p| {
        p.anchor == Anchor::World && matches!(p.source, Source::Monitor(_) | Source::Window(_) | Source::Keyboard)
    }) {
        p.pose = carry * p.pose;
    }
}

fn find(panels: &mut [Panel], source: Source) -> Option<&mut Panel> {
    panels.iter_mut().find(|p| p.source == source)
}

impl Session {
    /// Ensures keyboard, wrist, menu and toolbar panels exist; opens the first-run flow when not onboarded.
    pub fn new(state: State, panels: &mut Vec<Panel>, head: Pose) -> Self {
        if !panels.iter().any(|p| p.source == Source::Keyboard) {
            panels.push(Panel::keyboard_below(next_id(panels), head));
        }
        let wrist_aspect = WRIST_SIZE.1 as f32 / WRIST_SIZE.0 as f32;
        if !panels.iter().any(|p| p.source == Source::Wrist) {
            panels.push(Panel::on_wrist(next_id(panels), state.dominant.other(), wrist_aspect));
        }
        // A wrist saved by an older build has the old size and orientation; the watch is never hidden (it is the
        // way into the menu). Where the user moved it on the wrist stays.
        if let Some(w) = find(panels, Source::Wrist) {
            let mut fresh = Panel { visible: true, ..Panel::on_wrist(w.id, w.anchor_hand(), wrist_aspect) };
            if state.wrist_moved {
                fresh.pose.pos = w.pose.pos;
            }
            *w = fresh;
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
        let page = if state.onboarded { Page::Tab(TAB_PANELS) } else { Page::FirstHand };
        Self {
            state,
            page,
            test: ChatLine::default(),
            menu_hovered: false,
            imported: false,
            notifications_ok: true,
            armed: false,
            hidden: false,
            hidden_head: None,
            windows: Vec::new(),
            window_names: HashMap::new(),
            language: String::new(),
            languages: Vec::new(),
            auto_launch: false,
            motion: false,
            notify_blocked: false,
            history: VecDeque::new(),
            clock: 0,
            toolbar: None,
            toolbar_seen: 0,
            toolbar_at: 0,
            pointing: [None; 2],
            toolbar_hovered: false,
            size_goal: None,
            acts: RefCell::new(Vec::new()),
            tools: RefCell::new(Vec::new()),
        }
    }

    pub fn menu_open(&self, panels: &[Panel]) -> bool {
        panels.iter().any(|p| p.id == MENU_ID && p.visible)
    }

    /// Opens the menu on a page (e.g. the Inbox from the watch).
    pub fn open_menu_at(&mut self, page: Page, panels: &mut [Panel], head: Pose) {
        self.leave_page();
        self.page = page;
        if let Some(m) = panels.iter_mut().find(|p| p.id == MENU_ID) {
            if !m.visible {
                m.pose = Panel::menu_in_front(head, m.aspect).pose;
                m.dim = self.appear();
            }
            m.visible = true;
        }
    }

    /// Leaving the Inbox marks its messages read.
    fn leave_page(&mut self) {
        if self.page == Page::Tab(TAB_INBOX) {
            for n in &mut self.history {
                n.read = true;
            }
        }
    }

    /// Unread Inbox messages.
    pub fn unread(&self) -> usize {
        self.history.iter().filter(|n| !n.read).count()
    }

    /// Double-tap menu: open in front of the user, or close.
    pub fn toggle_menu(&mut self, panels: &mut [Panel], head: Pose) {
        let open = self.menu_open(panels);
        if let Some(m) = panels.iter_mut().find(|p| p.id == MENU_ID) {
            if !open {
                m.pose = Panel::menu_in_front(head, m.aspect).pose;
                m.dim = self.appear();
                if self.page.sub() {
                    self.page = Page::Tab(self.page.tab());
                }
            } else {
                self.leave_page();
            }
            m.visible = !open;
        }
    }

    /// Double-tap left menu / Hide all (as in XSOverlay): fade screens, windows and keyboard out, or bring them all
    /// back, the keyboard included. The menu and watch stay. Showing again turns and moves the whole layout, as it
    /// was arranged, to where the user now faces: after a playspace mover or a walk around the room everything is
    /// back in front with one double-tap.
    pub fn toggle_hidden(&mut self, panels: &mut [Panel], head: Pose) {
        self.hidden = !self.hidden;
        if self.hidden {
            self.toolbar = None;
            self.hidden_head = Some(head);
            return;
        }
        if let Some(then) = self.hidden_head.take() {
            carry(panels, then, head);
        }
        if let Some(k) = find(panels, Source::Keyboard)
            && !k.visible
        {
            k.visible = true;
            k.pose = Panel::keyboard_below(k.id, head).pose;
            k.anchor = Anchor::World;
        }
    }

    /// Reset layout: screens and windows in an arc in front of you (main screen in the middle, the others
    /// alternating right and left), screens at their default size, keyboard below.
    fn reset_layout(panels: &mut [Panel], head: Pose, monitors: &[MonitorInfo]) {
        let main = monitors.iter().position(|m| m.primary).unwrap_or(0) as u32;
        let mut order: Vec<usize> =
            (0..panels.len()).filter(|&i| matches!(panels[i].source, Source::Monitor(_) | Source::Window(_))).collect();
        order.sort_by_key(|&i| match panels[i].source {
            Source::Monitor(m) if m == main => (0, 0),
            Source::Monitor(m) => (1, m as u64),
            Source::Window(h) => (2, h),
            _ => (3, 0),
        });
        let (mut right, mut left) = (0.0f32, 0.0f32);
        for (n, &i) in order.iter().enumerate() {
            let p = &mut panels[i];
            let distance = if matches!(p.source, Source::Window(_)) {
                DEFAULT_DISTANCE_M * WINDOW_NEARER
            } else {
                DEFAULT_DISTANCE_M
            };
            if matches!(p.source, Source::Monitor(_)) {
                p.width_m = DEFAULT_WIDTH_M;
            }
            let half = (p.width_m * 0.5 / distance).atan();
            let yaw = if n == 0 {
                right = half;
                left = half;
                0.0
            } else if n % 2 == 1 {
                let y = right + LAYOUT_GAP_RAD + half;
                right = y + half;
                y
            } else {
                let y = left + LAYOUT_GAP_RAD + half;
                left = y + half;
                -y
            };
            p.place_at(head, yaw, distance);
        }
        if let Some(k) = find(panels, Source::Keyboard) {
            *k = Panel { id: k.id, visible: k.visible, opacity: k.opacity, ..Panel::keyboard_below(k.id, head) };
        }
    }

    /// Starting fade for a panel that appears in front of the user: a big surface fades in rather than flashing
    /// into view (instant without motion).
    fn appear(&self) -> f32 {
        if self.motion { 1.0 } else { 0.0 }
    }

    /// Steps the fades. `watch` is how visible the watch should be (0..1); `dt` seconds since the last call.
    pub fn update_dims(&self, panels: &mut [Panel], watch: f32, dt: f32) {
        for p in panels.iter_mut() {
            let target = match p.source {
                Source::Monitor(_) | Source::Window(_) | Source::Keyboard => f32::from(u8::from(self.hidden)),
                Source::Wrist => 1.0 - watch,
                Source::Menu | Source::Toolbar => 0.0,
            };
            let hiding = target > p.dim;
            let length = match (p.source == Source::Wrist, hiding) {
                (true, true) => WATCH_HIDE_S,
                (true, false) => WATCH_SHOW_S,
                (false, true) => HIDE_S,
                (false, false) => SHOW_S,
            };
            let step = if self.motion { dt / length } else { 1.0 };
            p.dim = if hiding { (p.dim + step).min(target) } else { (p.dim - step).max(target) };
        }
    }

    /// Panels persisted in the layout (everything but the menu and toolbar), with where the head is now (`None`
    /// while it is not tracked: the last known spot stays).
    pub fn persist(&mut self, panels: &[Panel], head: Option<Pose>) {
        self.state.panels = panels.iter().filter(|p| p.id < TOOLBAR_ID).copied().collect();
        if head.is_some() {
            self.state.layout_head = head;
        }
    }

    /// The first moment someone wears the headset after start: the saved layout comes to where they face, as it
    /// was arranged around them when it was saved. After a playspace mover (whose offset SteamVR drops on a
    /// restart) or a walk, the screens are in front again instead of somewhere across the room.
    pub fn arrive(&mut self, panels: &mut [Panel], head: Pose) {
        if let Some(then) = self.state.layout_head {
            carry(panels, then, head);
        }
        self.state.layout_head = Some(head);
    }

    /// Routes laser events on the menu and toolbar to their views and decides which panel shows the toolbar.
    /// `rays`: each hand's laser. Returns true when the menu saw pointer activity.
    pub fn pointer(&mut self, input: PointerInput, panels: &[Panel], menu: &Menu, toolbar: &Toolbar) -> bool {
        let PointerInput { events, now, rays, anchors } = input;
        self.clock = now;
        let mut over_menu = false;
        let mut over_toolbar = false;
        // A laser between the panel and its toolbar (the gap, or another panel seen through it) keeps the toolbar
        // where it is for that hand. A panel in front of the gap is not seen through it: the laser is on that panel.
        let bar_aspect = TOOLBAR_SIZE.1 as f32 / TOOLBAR_SIZE.0 as f32;
        let owner = self.toolbar.and_then(|id| panels.iter().find(|p| p.id == id));
        let bridged = rays.map(|ray| {
            ray.zip(owner).is_some_and(|(r, p)| {
                p.toolbar_bridge(p.world_pose(&anchors), r, bar_aspect).is_some_and(|t| {
                    !panels.iter().any(|q| {
                        q.id != p.id
                            && has_toolbar(q.source)
                            && q.shown()
                            && q.hit(q.world_pose(&anchors), r).is_some_and(|(near, _)| near < t)
                    })
                })
            })
        });
        if bridged.iter().any(|b| *b) {
            self.toolbar_seen = now;
        }
        let mut on = [None; 2];
        let mut claim = None;
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
                Event::Moved { panel } if panels.iter().any(|p| p.id == panel && p.source == Source::Wrist) => {
                    self.state.wrist_moved = true;
                }
                Event::Hover { hand, panel, .. } if panels.iter().any(|p| p.id == panel && has_toolbar(p.source)) => {
                    on[hand.index()] = Some(panel);
                }
                // A click or a move is meant for that panel: its toolbar comes at once.
                Event::Mouse { panel, .. } | Event::Moved { panel }
                    if panels.iter().any(|p| p.id == panel && has_toolbar(p.source)) =>
                {
                    claim = Some(panel);
                }
                _ => {}
            }
        }
        // Each hand's laser, the panel it is on and since when. The toolbar goes to the panel a laser came onto
        // last, so a laser resting on the keyboard never keeps a screen the other hand points at from its toolbar
        // (or the other way round), and two hands on two panels never make it flicker between them.
        for (i, panel) in on.into_iter().enumerate() {
            self.pointing[i] = match (panel, self.pointing[i]) {
                (Some(p), Some((q, since))) if p == q => Some((q, since)),
                (Some(p), _) => Some((p, now)),
                (None, _) => None,
            };
        }
        if claim.is_none() {
            claim = (0..2)
                .filter_map(|i| self.pointing[i].map(|(p, since)| (i, p, since)))
                .filter(|&(i, p, since)| {
                    Some(p) != self.toolbar && !bridged[i] && (self.toolbar.is_none() || since >= self.toolbar_at)
                })
                .max_by_key(|&(_, _, since)| since)
                .map(|(_, p, _)| p);
        }
        if let Some(p) = claim {
            if Some(p) != self.toolbar {
                self.toolbar_at = now;
            }
            self.toolbar = Some(p);
        }
        if self.toolbar.is_some_and(|t| claim == Some(t) || on.contains(&Some(t))) {
            self.toolbar_seen = now;
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

    /// Remembers a notification for the Inbox.
    pub fn notified(&mut self, app: &str, title: &str, body: &str) {
        let note =
            Note { app: app.to_owned(), title: title.to_owned(), body: body.to_owned(), at: self.clock, read: false };
        self.history.push_front(note);
        self.history.truncate(HISTORY_LEN);
    }

    /// Panel the toolbar belongs to, if it is showing.
    pub fn toolbar_target(&self) -> Option<u32> {
        self.toolbar
    }

    /// Index of `tool` in the toolbar last built.
    pub fn tool_index(&self, tool: Tool) -> Option<usize> {
        self.tools.borrow().iter().position(|t| *t == tool)
    }

    fn panel_name(&self, p: &Panel, s: &Strings) -> String {
        match p.source {
            Source::Monitor(i) => s.get("menu-screen", &[("n", (i + 1).to_string())]),
            Source::Window(h) => self.window_names.get(&h).cloned().unwrap_or_else(|| s.get("menu-add-window", &[])),
            _ => s.get("tool-keyboard", &[]),
        }
    }

    /// Eases a panel toward the size the toolbar asked for. A panel in hand is the hand's to size.
    pub fn ease_sizes(&mut self, panels: &mut [Panel], grabbed: Option<u32>, dt: f32) {
        let Some((id, goal)) = self.size_goal else { return };
        let p = panels.iter_mut().find(|p| p.id == id);
        match p {
            Some(p) if grabbed != Some(id) => {
                p.width_m += (goal - p.width_m) * (1.0 - (-dt / SIZE_EASE_S).exp());
                if (goal - p.width_m).abs() < 0.002 {
                    p.width_m = goal;
                    self.size_goal = None;
                }
            }
            _ => self.size_goal = None,
        }
    }

    /// Puts the toolbar under its panel (or hides it). Call every frame before stepping the engine (so the laser
    /// finds it) and again after (so it shows where the panel went this frame). It takes its panel's anchor, so a
    /// head-locked window's toolbar rides on the headset with it instead of trailing a frame behind.
    pub fn place_toolbar(&mut self, panels: &mut [Panel], hands: &Anchors) {
        let target = self.toolbar.and_then(|id| panels.iter().find(|p| p.id == id && p.shown()).copied());
        if target.is_none() {
            self.toolbar = None;
        }
        if let Some(bar) = panels.iter_mut().find(|p| p.id == TOOLBAR_ID) {
            bar.visible = target.is_some();
            if let Some(t) = target {
                bar.anchor = t.anchor;
                bar.set_world_pose(t.toolbar_pose(t.world_pose(hands), bar.aspect), hands);
            }
        }
    }

    /// Controls for the panel under the toolbar.
    pub fn toolbar_model(&self, panels: &[Panel], s: &Strings) -> ToolbarModel {
        let mut tools = Vec::new();
        let Some(p) = self.toolbar.and_then(|id| panels.iter().find(|p| p.id == id)) else {
            self.tools.replace(tools);
            return ToolbarModel::default();
        };
        let t = |id: &str| s.get(id, &[]);
        let b = |kind, label: String, icon| ToolButton::new(kind, label, icon);
        let keyboard = p.source == Source::Keyboard;
        let mut l = ToolbarLayout::new();
        let icon = match p.source {
            Source::Monitor(_) => icons::SCREEN,
            Source::Window(_) => icons::WINDOW,
            _ => icons::KEYS,
        };
        l.chip(self.panel_name(p, s), icon);
        tools.push(Tool::None);
        let mut lock = vec![(b(ToolKind::Toggle, String::new(), icons::LOCK).on(p.locked), None)];
        tools.push(Tool::Lock);
        if !keyboard {
            lock.push((b(ToolKind::Toggle, String::new(), icons::CURVE).on(p.curvature > 0.0), None));
            lock.push((b(ToolKind::Toggle, String::new(), icons::FREE_TURN).on(p.free_turn), None));
            tools.extend([Tool::Curve, Tool::FreeTurn]);
        }
        l.group(lock);
        let width = self.size_goal.filter(|g| g.0 == p.id).map_or(p.width_m, |g| g.1);
        let size = s.get("tool-size", &[("m", format!("{width:.1}"))]);
        l.group(vec![
            (b(ToolKind::Step, "−".into(), ""), None),
            (b(ToolKind::Readout, size, ""), None),
            (b(ToolKind::Step, "+".into(), ""), None),
        ]);
        tools.extend([Tool::Smaller, Tool::None, Tool::Bigger]);
        let opacity = ((p.opacity - MIN_OPACITY) / (1.0 - MIN_OPACITY)).clamp(0.0, 1.0);
        l.group(vec![(b(ToolKind::Slider, String::new(), icons::OPACITY).value(opacity), None)]);
        tools.push(Tool::Opacity);
        if keyboard {
            let langs: Vec<(ToolButton, Option<f32>)> = self
                .languages
                .iter()
                .take(TOOLBAR_LANGUAGES)
                .enumerate()
                .map(|(i, name)| {
                    tools.push(Tool::Language(i));
                    (b(ToolKind::Segment, name.clone(), "").on(*name == self.language), Some(80.0))
                })
                .collect();
            if !langs.is_empty() {
                l.group(langs);
            }
        } else {
            let pins = [(Pin::Room, "tool-room"), (Pin::Hand, "tool-hand"), (Pin::Head, "tool-head")];
            let now = match p.anchor {
                Anchor::World => Pin::Room,
                Anchor::Hand(_) => Pin::Hand,
                Anchor::Head => Pin::Head,
            };
            l.group(
                pins.iter()
                    .map(|(pin, id)| {
                        tools.push(Tool::Pin(*pin));
                        (b(ToolKind::Segment, t(id), "").on(*pin == now), Some(92.0))
                    })
                    .collect(),
            );
        }
        l.button(b(ToolKind::Icon, String::new(), icons::RESET), None);
        tools.push(Tool::Reset);
        if keyboard {
            l.button(b(ToolKind::Text, t("tool-put-away"), ""), Some(150.0));
        } else {
            l.button(b(ToolKind::Danger, String::new(), icons::CLOSE), None);
        }
        tools.push(Tool::Close);
        self.tools.replace(tools);
        l.finish()
    }

    /// Applies a toolbar click or slider move.
    pub fn toolbar_input(
        &mut self,
        input: ToolInput,
        panels: &mut Vec<Panel>,
        hands: &Anchors,
        head: Pose,
    ) -> Vec<Effect> {
        let mut fx = Vec::new();
        let Some(i) = self.toolbar.and_then(|id| panels.iter().position(|p| p.id == id)) else { return fx };
        let (index, value) = match input {
            ToolInput::Click(i) => (i, None),
            ToolInput::Slide(i, v) => (i, Some(v)),
        };
        let Some(tool) = self.tools.borrow().get(index).copied() else { return fx };
        let dominant = self.state.dominant;
        let p = &mut panels[i];
        match tool {
            Tool::None => {}
            Tool::Lock => p.locked = !p.locked,
            Tool::Curve => p.curvature = if p.curvature > 0.0 { 0.0 } else { CURVATURE },
            Tool::FreeTurn => p.free_turn = !p.free_turn,
            Tool::Smaller | Tool::Bigger => {
                // From the size already asked for, so quick taps keep stepping while it eases.
                let from = self.size_goal.filter(|g| g.0 == p.id).map_or(p.width_m, |g| g.1);
                self.size_goal = Some((p.id, size_step(from, tool == Tool::Bigger)));
            }
            Tool::Opacity => {
                if let Some(v) = value {
                    p.opacity = MIN_OPACITY + (1.0 - MIN_OPACITY) * v.clamp(0.0, 1.0);
                }
            }
            // Hand: into the hand that is not pointing, held up like a tablet (it moves with that hand). Room and
            // head: the panel stays where it is and changes what it follows.
            Tool::Pin(Pin::Hand) => p.hold_in(dominant.other(), hands),
            Tool::Pin(pin) => {
                let world = p.world_pose(hands);
                p.anchor = if pin == Pin::Head { Anchor::Head } else { Anchor::World };
                p.set_world_pose(world, hands);
            }
            Tool::Language(l) => fx.push(Effect::Language(l)),
            Tool::Reset if p.source == Source::Keyboard => {
                *p = Panel { id: p.id, visible: p.visible, opacity: p.opacity, ..Panel::keyboard_below(p.id, head) };
            }
            Tool::Reset => {
                let window = matches!(p.source, Source::Window(_));
                if !window {
                    p.width_m = DEFAULT_WIDTH_M;
                }
                p.place_at(head, 0.0, DEFAULT_DISTANCE_M * if window { WINDOW_NEARER } else { 1.0 });
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

    /// Number (1-based) of the next screen the watch's Add row offers.
    pub fn next_screen(panels: &[Panel], monitors: &[MonitorInfo]) -> Option<u32> {
        Self::unshown_monitor(panels, monitors).map(|i| i + 1)
    }

    /// Watch > Add > Screen: the next screen, in front of you.
    pub fn add_next_screen(&mut self, panels: &mut Vec<Panel>, head: Pose, monitors: &[MonitorInfo]) -> Vec<Effect> {
        self.act(Act::AddScreen, panels, head, monitors)
    }

    /// Watch > Add > a window from `windows`, in front of you.
    pub fn add_window(&mut self, h: u64, panels: &mut Vec<Panel>, head: Pose, monitors: &[MonitorInfo]) -> Vec<Effect> {
        self.act(Act::AddWindow(h), panels, head, monitors)
    }

    fn unshown_monitor(panels: &[Panel], monitors: &[MonitorInfo]) -> Option<u32> {
        (0..monitors.len() as u32).find(|i| !panels.iter().any(|p| p.source == Source::Monitor(*i)))
    }

    fn ago(&self, at: u64, s: &Strings) -> String {
        let min = self.clock.saturating_sub(at) / 60_000;
        match min {
            0 => s.get("ago-now", &[]),
            1..60 => s.get("ago-min", &[("n", min.to_string())]),
            _ => s.get("ago-hour", &[("n", (min / 60).to_string())]),
        }
    }

    /// What the menu shows now. `cost` is (GPU ms, CPU %) of this overlay.
    pub fn model(&self, panels: &[Panel], monitors: &[MonitorInfo], s: &Strings, cost: (f32, f32)) -> MenuModel {
        let t = |id: &str| s.get(id, &[]);
        let mut acts: Vec<Act> = Vec::new();
        // Each slot is built with what it does; the two lists stay in step.
        let add = |acts: &mut Vec<Act>, act: Act, slot: Slot| {
            acts.push(act);
            slot
        };
        let row = |label: String| Slot::new(SlotKind::Action, label);
        let toggle = |label: String, on: bool| Slot::new(SlotKind::Toggle, label).on(on);
        let seg = |label: String, on: bool| Slot::new(SlotKind::Segment, label).on(on);
        let primary = |label: String, on: bool| Slot::new(SlotKind::Primary, label).on(on);
        let visible = |src: Source| panels.iter().any(|p| p.source == src && p.visible);
        let wrist_hand = panels.iter().find(|p| p.source == Source::Wrist).map_or(Hand::Left, Panel::anchor_hand);
        let hands = |acts: &mut Vec<Act>, now: Hand, act: fn(Hand) -> Act| {
            [Hand::Left, Hand::Right]
                .map(|h| {
                    acts.push(act(h));
                    seg(t(if h == Hand::Left { "hand-left" } else { "hand-right" }), now == h)
                })
                .to_vec()
        };
        let mut model = match self.page {
            Page::FirstHand => {
                let mut l = Layout::plain(t("first-hand-title"));
                let hint = t(if self.imported { "first-hand-imported" } else { "first-hand-hint" });
                l.paragraph(Col::Full, hint, Tone::Secondary, 40.0);
                let d = self.state.dominant;
                let left = add(&mut acts, Act::FirstHand(Hand::Left), primary(t("first-hand-left"), d == Hand::Left));
                let right =
                    add(&mut acts, Act::FirstHand(Hand::Right), primary(t("first-hand-right"), d == Hand::Right));
                l.cards(Col::Full, vec![left, right], 2, 120.0);
                l.finish()
            }
            Page::FirstMonitor => {
                let mut l = Layout::plain(t("first-monitor-title"));
                l.paragraph(Col::Full, t("first-monitor-hint"), Tone::Secondary, 40.0);
                let cards = monitors
                    .iter()
                    .enumerate()
                    .take(4)
                    .map(|(i, m)| {
                        let args =
                            [("n", (i + 1).to_string()), ("w", m.rect.w.to_string()), ("h", m.rect.h.to_string())];
                        add(&mut acts, Act::FirstMonitor(i), primary(s.get("first-monitor", &args), m.primary))
                    })
                    .collect();
                l.cards(Col::Full, cards, 1, 72.0);
                l.finish()
            }
            Page::FirstKeyboard => {
                let mut l = Layout::plain(t("first-keyboard-title"));
                let hint = s.get("first-keyboard-hint", &[("typed", self.test.text().to_owned())]);
                l.paragraph(Col::Full, hint, Tone::Secondary, 40.0);
                let typed = self.test.text().to_lowercase().contains(TEST_WORD);
                let done = add(&mut acts, Act::FirstDone, primary(t("first-done"), typed));
                let skip = add(&mut acts, Act::FirstDone, primary(t("first-skip"), false));
                l.cards(Col::Full, vec![done, skip], 2, 96.0);
                l.finish()
            }
            Page::FirstTips => {
                let mut l = Layout::plain(t("first-tips-title"));
                l.hint(t("first-tips-hint"));
                l.paragraph(Col::Full, t("first-tips-body"), Tone::Ink, 300.0);
                l.footer("", Some(add(&mut acts, Act::GotIt, primary(t("first-got-it"), true))));
                l.finish()
            }
            Page::Tab(TAB_PANELS) => {
                let mut l = Layout::rail(t("menu-panels-title"));
                l.hint(t("menu-panels-hint"));
                l.label(Col::Left, t("menu-add"));
                let next = Self::unshown_monitor(panels, monitors);
                let screen = match next {
                    Some(i) => row(s.get("menu-add-screen", &[("n", (i + 1).to_string())])).value("+").on(true),
                    None => row(t("menu-add-screen-none")).enabled(false),
                };
                let rows = vec![
                    add(&mut acts, Act::OpenWindows, row(t("menu-add-window")).value("›")),
                    add(&mut acts, Act::AddScreen, screen),
                ];
                l.rows(Col::Left, rows, ROW_H);
                l.label(Col::Left, t("menu-arrange"));
                let rows = vec![
                    add(&mut acts, Act::Recenter, row(t("menu-recenter"))),
                    add(&mut acts, Act::ResetLayout, row(t("menu-reset-layout"))),
                ];
                l.rows(Col::Left, rows, ROW_H);
                l.label(Col::Right, t("menu-show"));
                let rows = vec![
                    add(&mut acts, Act::Screens, toggle(t("menu-screens"), !self.hidden)),
                    add(&mut acts, Act::Keyboard, toggle(t("menu-keyboard"), visible(Source::Keyboard))),
                ];
                l.rows(Col::Right, rows, ROW_H);
                l.label(Col::Right, t("menu-open-now"));
                let open: Vec<&Panel> =
                    panels.iter().filter(|p| matches!(p.source, Source::Monitor(_) | Source::Window(_))).collect();
                // Rows are placed first, then their close buttons.
                let (mut items, mut row_acts, mut button_acts) = (Vec::new(), Vec::new(), Vec::new());
                for p in open.iter().take(OPEN_NOW_ROWS) {
                    let item = Slot::new(SlotKind::Item, self.panel_name(p, s)).on(p.visible && !self.hidden);
                    items.push((item, Some(Slot::new(SlotKind::IconButton, "").icon(icons::CLOSE))));
                    row_acts.push(Act::None);
                    button_acts.push(Act::ClosePanel(p.id));
                }
                if open.len() > OPEN_NOW_ROWS {
                    let more = s.get("menu-open-more", &[("n", (open.len() - OPEN_NOW_ROWS).to_string())]);
                    items.push((Slot::new(SlotKind::Item, more), None));
                    row_acts.push(Act::None);
                }
                if open.is_empty() {
                    items.push((Slot::new(SlotKind::Item, t("menu-open-none")), None));
                    row_acts.push(Act::None);
                }
                acts.extend(row_acts);
                acts.extend(button_acts);
                l.rows_with(Col::Right, items, 56.0);
                l.footer(t("menu-double-tap"), None);
                l.finish()
            }
            Page::Windows(page) => {
                let mut l = Layout::rail(t("menu-windows-title"));
                l.back(add(&mut acts, Act::Back, Slot::new(SlotKind::Back, t("menu-back")).icon(icons::BACK)));
                l.hint(t("menu-windows-hint"));
                let start = page * WINDOWS_PER_PAGE;
                let cards: Vec<Slot> = self
                    .windows
                    .iter()
                    .skip(start)
                    .take(WINDOWS_PER_PAGE)
                    .map(|(h, title)| {
                        let shown = panels.iter().any(|p| p.source == Source::Window(*h));
                        let card = Slot::new(SlotKind::Entry, title.clone()).tile(title);
                        let card = if shown {
                            card.value(t("menu-window-shown")).on(true).enabled(false)
                        } else {
                            card.value("+")
                        };
                        add(&mut acts, Act::AddWindow(*h), card)
                    })
                    .collect();
                if cards.is_empty() {
                    l.paragraph(Col::Full, t("menu-windows-none"), Tone::Secondary, 40.0);
                }
                l.cards(Col::Full, cards, 2, 74.0);
                let more = (self.windows.len() > start + WINDOWS_PER_PAGE)
                    .then(|| add(&mut acts, Act::MoreWindows, primary(t("menu-more"), false)));
                l.footer(t("menu-yank"), more);
                l.finish()
            }
            Page::Tab(TAB_KEYS) => {
                let mut l = Layout::rail(t("menu-keys-title"));
                l.hint(t("menu-keys-hint"));
                l.label(Col::Left, t("menu-keyboard-group"));
                let rows = vec![
                    add(&mut acts, Act::Keyboard, toggle(t("menu-keyboard"), visible(Source::Keyboard))),
                    add(&mut acts, Act::KeyboardHere, row(t("menu-keyboard-here"))),
                ];
                l.rows(Col::Left, rows, ROW_H);
                l.label(Col::Left, t("menu-language"));
                if self.languages.len() > 1 {
                    let segs = self
                        .languages
                        .iter()
                        .take(TOOLBAR_LANGUAGES)
                        .enumerate()
                        .map(|(i, n)| add(&mut acts, Act::Language(i), seg(n.clone(), *n == self.language)))
                        .collect();
                    l.segments(Col::Left, segs);
                } else {
                    let r = row(t("menu-language")).value(format!("{} ›", self.language));
                    l.rows(Col::Left, vec![add(&mut acts, Act::NextLanguage, r)], ROW_H);
                }
                l.label(Col::Right, t("menu-typing"));
                let rows = vec![
                    add(&mut acts, Act::Chat, toggle(t("menu-chat"), self.state.chatbox)),
                    add(&mut acts, Act::Sounds, toggle(t("menu-sounds"), self.state.typing_sounds)),
                    add(&mut acts, Act::NowPlaying, toggle(t("menu-now-playing"), self.state.media_to_chatbox)),
                ];
                l.rows(Col::Right, rows, ROW_H);
                l.finish()
            }
            Page::Tab(TAB_WATCH) => {
                let mut l = Layout::rail(t("menu-watch-title"));
                l.hint(t(if self.notifications_ok { "menu-watch-hint" } else { "menu-notifications-blocked" }));
                l.label(Col::Left, t("menu-watch-glance"));
                let (name, _) = self.state.wrist_layout.ids();
                let layout = row(t("menu-watch-layout")).value(format!("{} ›", t(name)));
                l.rows(Col::Left, vec![add(&mut acts, Act::OpenLayouts, layout)], ROW_H);
                l.label(Col::Left, t("menu-watch-shows"));
                let rows = vec![
                    add(&mut acts, Act::WristBattery, toggle(t("menu-wrist-battery"), self.state.wrist_battery)),
                    add(&mut acts, Act::WristMedia, toggle(t("menu-wrist-media"), self.state.wrist_media)),
                    add(&mut acts, Act::WristLoad, toggle(t("menu-wrist-load"), self.state.wrist_load)),
                ];
                l.rows(Col::Left, rows, ROW_H);
                l.label(Col::Right, t("menu-watch-behaves"));
                let rows = vec![add(&mut acts, Act::WristFade, toggle(t("menu-wrist-fade"), self.state.wrist_fade))];
                l.rows(Col::Right, rows, ROW_H);
                l.finish()
            }
            Page::WatchLayouts => {
                let mut l = Layout::rail(t("menu-layout-title"));
                l.back(add(&mut acts, Act::Back, Slot::new(SlotKind::Back, t("menu-back")).icon(icons::BACK)));
                l.hint(t("menu-layout-hint"));
                let rows = WatchLayout::ALL
                    .iter()
                    .map(|&layout| {
                        let (name, hint) = layout.ids();
                        let choice =
                            Slot::new(SlotKind::Choice, t(name)).value(t(hint)).on(layout == self.state.wrist_layout);
                        add(&mut acts, Act::Layout(layout), choice)
                    })
                    .collect();
                l.rows(Col::Full, rows, 56.0);
                l.footer(t("layout-footer"), None);
                l.finish()
            }
            Page::Theme => {
                let mut l = Layout::rail(t("menu-theme"));
                l.back(add(&mut acts, Act::Back, Slot::new(SlotKind::Back, t("menu-back")).icon(icons::BACK)));
                l.hint(t("menu-theme-hint"));
                let cards = Accent::ALL
                    .iter()
                    .map(|&a| {
                        let card =
                            Slot::new(SlotKind::Swatch, t(a.name_id())).color(a.rgb()).on(a == self.state.accent);
                        add(&mut acts, Act::Accent(a), card)
                    })
                    .collect();
                l.cards(Col::Full, cards, 4, 74.0);
                l.footer(t("menu-theme-footer"), None);
                l.finish()
            }
            Page::Controls => {
                let mut l = Layout::rail(t("menu-controls"));
                l.back(add(&mut acts, Act::Back, Slot::new(SlotKind::Back, t("menu-back")).icon(icons::BACK)));
                l.hint(t("menu-controls-hint"));
                let control = |acts: &mut Vec<Act>, input: &str, does: &str| {
                    add(acts, Act::None, Slot::new(SlotKind::Control, t(input)).value(t(does)).enabled(false))
                };
                l.label(Col::Left, t("menu-controls-point"));
                let rows = vec![
                    control(&mut acts, "ctl-trigger", "ctl-click"),
                    control(&mut acts, "ctl-ax", "ctl-right-click"),
                    control(&mut acts, "ctl-stick", "ctl-scroll"),
                    control(&mut acts, "ctl-menu-twice", "ctl-hide"),
                ];
                l.rows(Col::Left, rows, ROW_H);
                l.label(Col::Left, t("menu-controls-also"));
                let hold = toggle(t("menu-hold-right"), self.state.hold_right_click);
                let drag = toggle(t("menu-drag-scroll"), self.state.drag_scroll);
                l.rows(
                    Col::Left,
                    vec![add(&mut acts, Act::HoldRightClick, hold), add(&mut acts, Act::DragScroll, drag)],
                    ROW_H,
                );
                l.label(Col::Right, t("menu-controls-hold"));
                let rows = vec![
                    control(&mut acts, "ctl-grip", "ctl-move"),
                    control(&mut acts, "ctl-stick-ud", "ctl-push"),
                    control(&mut acts, "ctl-stick-lr", "ctl-size"),
                    control(&mut acts, "ctl-both-grips", "ctl-size"),
                    control(&mut acts, "ctl-to-hand", "ctl-attach"),
                ];
                l.rows(Col::Right, rows, ROW_H);
                l.label(Col::Right, t("menu-controls-buttons"));
                let rebind = row(t("menu-rebind"));
                l.rows(Col::Right, vec![add(&mut acts, Act::Rebind, rebind)], ROW_H);
                l.finish()
            }
            Page::Tab(TAB_INBOX) => {
                let mut l = Layout::rail(t("menu-inbox-title"));
                let from = self.state.notify_from;
                let segs = [
                    (NotifyFrom::Discord, "notify-discord"),
                    (NotifyFrom::AllApps, "notify-all"),
                    (NotifyFrom::Off, "notify-off"),
                ]
                .map(|(f, id)| add(&mut acts, Act::NotifyFrom(f), seg(t(id), from == f)))
                .to_vec();
                l.segments(Col::Full, segs);
                let rows: Vec<Slot> = self
                    .history
                    .iter()
                    .map(|n| {
                        let m = Slot::new(SlotKind::Message, n.title.clone())
                            .detail(n.body.clone())
                            .value(self.ago(n.at, s))
                            .tile(if n.app.is_empty() { &n.title } else { &n.app })
                            .on(!n.read);
                        add(&mut acts, Act::None, m)
                    })
                    .collect();
                if rows.is_empty() {
                    l.paragraph(Col::Full, t("menu-inbox-empty"), Tone::Secondary, 40.0);
                } else {
                    l.rows(Col::Full, rows, 68.0);
                }
                let note = if self.notify_blocked && from != NotifyFrom::Off {
                    t("menu-notify-blocked")
                } else if from == NotifyFrom::Off {
                    t("menu-inbox-off")
                } else {
                    t("menu-inbox-hint")
                };
                let clear =
                    add(&mut acts, Act::ClearInbox, primary(t("menu-clear"), false).enabled(!self.history.is_empty()));
                l.footer(note, Some(clear));
                l.finish()
            }
            Page::Tab(_) => {
                let mut l = Layout::rail(t("menu-setup-title"));
                l.hint(s.get("menu-cost", &[("gpu", format!("{:.2}", cost.0)), ("cpu", format!("{:.1}", cost.1))]));
                l.label(Col::Left, t("menu-hands"));
                let laser = hands(&mut acts, self.state.dominant, Act::LaserHand);
                let watch = hands(&mut acts, wrist_hand, Act::WatchHand);
                l.choices(Col::Left, vec![(t("menu-laser-hand"), laser), (t("menu-watch-on"), watch)]);
                l.label(Col::Left, t("menu-feel"));
                let rows = vec![
                    add(&mut acts, Act::Haptics, toggle(t("menu-haptics"), self.state.haptics)),
                    add(&mut acts, Act::Smoothing, toggle(t("menu-smoothing"), self.state.smoothing)),
                    add(&mut acts, Act::OpenControls, row(t("menu-controls")).value("›")),
                ];
                l.rows(Col::Left, rows, ROW_H);
                l.label(Col::Right, t("menu-start"));
                let rows = vec![
                    add(&mut acts, Act::AutoStart, toggle(t("menu-autostart"), self.auto_launch)),
                    add(&mut acts, Act::Updates, toggle(t("menu-update-check"), self.state.update_check)),
                ];
                l.rows(Col::Right, rows, ROW_H);
                l.label(Col::Right, t("menu-new-screens"));
                let rows =
                    vec![add(&mut acts, Act::Curved, toggle(t("menu-curved-default"), self.state.default_curved))];
                l.rows(Col::Right, rows, ROW_H);
                l.label(Col::Right, t("menu-look"));
                let theme = row(t("menu-theme")).value(format!("{} ›", t(self.state.accent.name_id())));
                l.rows(Col::Right, vec![add(&mut acts, Act::OpenTheme, theme)], ROW_H);
                let quit = Slot::new(SlotKind::Danger, t(if self.armed { "menu-confirm" } else { "menu-quit" }))
                    .on(self.armed);
                l.footer("", Some(add(&mut acts, Act::Quit, quit)));
                l.finish()
            }
        };
        let first_run =
            matches!(self.page, Page::FirstHand | Page::FirstMonitor | Page::FirstKeyboard | Page::FirstTips);
        if !first_run {
            let tabs = [
                ("menu-tab-panels", icons::SCREEN, 0),
                ("menu-tab-keys", icons::KEYS, 0),
                ("menu-tab-watch", icons::WATCH, 0),
                ("menu-tab-inbox", icons::BELL, self.unread()),
                ("menu-tab-setup", icons::SLIDERS, 0),
            ];
            model.tabs = tabs.iter().map(|(id, icon, badge)| Tab { label: t(id), icon, badge: *badge }).collect();
            model.tab = self.page.tab();
        }
        debug_assert_eq!(acts.len(), model.slots.len(), "every slot needs its act");
        self.acts.replace(acts);
        model
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

    /// The watch at its default spot on `hand` (a hand change or Reset layout).
    fn wear_wrist(&mut self, hand: Hand, panels: &mut [Panel]) {
        self.state.wrist_moved = false;
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
        if self.hidden {
            self.hidden = false;
            if panels.iter().any(|p| p.source == Source::Keyboard && p.visible) {
                return;
            }
        }
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
        let fx = Vec::new();
        let slot = match input {
            MenuInput::Tab(i) => {
                self.armed = false;
                if (matches!(self.page, Page::Tab(_)) || self.page.sub()) && i < TABS {
                    self.leave_page();
                    self.page = Page::Tab(i);
                }
                return fx;
            }
            MenuInput::Slot(i) => i,
            MenuInput::Close => {
                self.armed = false;
                self.leave_page();
                Self::close(panels);
                return fx;
            }
        };
        let Some(act) = self.acts.borrow().get(slot).copied() else { return fx };
        // Quit needs a second tap; any other click disarms it.
        if act == Act::Quit && !self.armed {
            self.armed = true;
            return fx;
        }
        self.armed = false;
        self.act(act, panels, head, monitors)
    }

    fn act(&mut self, act: Act, panels: &mut Vec<Panel>, head: Pose, monitors: &[MonitorInfo]) -> Vec<Effect> {
        let mut fx = Vec::new();
        match act {
            Act::None => {}
            Act::FirstHand(hand) => {
                self.set_dominant(hand, panels);
                self.page = Page::FirstMonitor;
            }
            Act::FirstMonitor(i) => {
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
            Act::FirstDone => self.page = Page::FirstTips,
            Act::GotIt => {
                self.state.onboarded = true;
                self.page = Page::Tab(TAB_PANELS);
                for p in panels.iter_mut().filter(|p| p.id == MENU_ID || p.source == Source::Keyboard) {
                    p.visible = false;
                }
            }
            // Adding a screen or window closes the menu so the new panel is not hidden behind it.
            Act::AddScreen => {
                if let Some(i) = Self::unshown_monitor(panels, monitors) {
                    let id = next_id(panels);
                    panels.push(self.new_screen(id, i, head, &monitors[i as usize]));
                    fx.push(Effect::Added(id));
                    Self::close(panels);
                }
            }
            Act::OpenWindows => {
                self.page = Page::Windows(0);
                fx.push(Effect::ListWindows);
            }
            Act::Recenter => Self::recenter(panels, head),
            Act::ResetLayout => {
                Self::reset_layout(panels, head, monitors);
                let hand = panels.iter().find(|p| p.source == Source::Wrist).map_or(Hand::Left, Panel::anchor_hand);
                self.wear_wrist(hand, panels);
            }
            Act::Screens => self.toggle_hidden(panels, head),
            Act::Keyboard => Self::toggle_keyboard(panels, head),
            Act::ClosePanel(id) => {
                if let Some(i) = panels.iter().position(|p| p.id == id) {
                    panels.remove(i);
                    if self.toolbar == Some(id) {
                        self.toolbar = None;
                    }
                    fx.push(Effect::Removed(id));
                }
            }
            Act::AddWindow(h) => {
                if !panels.iter().any(|p| p.source == Source::Window(h)) {
                    if let Some((_, name)) = self.windows.iter().find(|(w, _)| *w == h) {
                        self.window_names.insert(h, name.clone());
                    }
                    fx.push(Effect::AddWindow(h));
                    self.page = Page::Tab(TAB_PANELS);
                    Self::close(panels);
                }
            }
            Act::MoreWindows => {
                if let Page::Windows(page) = self.page
                    && self.windows.len() > (page + 1) * WINDOWS_PER_PAGE
                {
                    self.page = Page::Windows(page + 1);
                }
            }
            Act::Back => self.page = Page::Tab(self.page.tab()),
            Act::NotifyFrom(from) => {
                if from != self.state.notify_from {
                    self.state.notify_from = from;
                    fx.push(Effect::NotifyFrom(from));
                }
            }
            Act::ClearInbox => self.history.clear(),
            Act::KeyboardHere => {
                if let Some(k) = find(panels, Source::Keyboard) {
                    *k = Panel { id: k.id, ..Panel::keyboard_below(k.id, head) };
                }
            }
            Act::Language(i) => fx.push(Effect::Language(i)),
            Act::NextLanguage => fx.push(Effect::NextLanguage),
            Act::Chat => {
                self.state.chatbox = !self.state.chatbox;
                fx.push(Effect::RedrawKeyboard);
            }
            Act::Sounds => self.state.typing_sounds = !self.state.typing_sounds,
            Act::NowPlaying => self.state.media_to_chatbox = !self.state.media_to_chatbox,
            Act::WristBattery => self.state.wrist_battery = !self.state.wrist_battery,
            Act::WristMedia => self.state.wrist_media = !self.state.wrist_media,
            Act::WristLoad => self.state.wrist_load = !self.state.wrist_load,
            Act::WristFade => self.state.wrist_fade = !self.state.wrist_fade,
            Act::OpenLayouts => self.page = Page::WatchLayouts,
            Act::Layout(layout) => self.state.wrist_layout = layout,
            Act::OpenTheme => self.page = Page::Theme,
            Act::OpenControls => self.page = Page::Controls,
            Act::HoldRightClick => self.state.hold_right_click = !self.state.hold_right_click,
            // The binding screen opens in the headset; the menu steps out of its way.
            Act::Rebind => {
                Self::close(panels);
                fx.push(Effect::Rebind);
            }
            Act::DragScroll => self.state.drag_scroll = !self.state.drag_scroll,
            Act::Accent(accent) => {
                if accent != self.state.accent {
                    self.state.accent = accent;
                    fx.push(Effect::Accent);
                }
            }
            Act::LaserHand(hand) => {
                if hand != self.state.dominant {
                    self.set_dominant(hand, panels);
                }
            }
            Act::WatchHand(hand) => self.wear_wrist(hand, panels),
            Act::AutoStart => {
                self.auto_launch = !self.auto_launch;
                fx.push(Effect::AutoLaunch(self.auto_launch));
            }
            Act::Haptics => self.state.haptics = !self.state.haptics,
            Act::Smoothing => self.state.smoothing = !self.state.smoothing,
            Act::Updates => self.state.update_check = !self.state.update_check,
            Act::Curved => self.state.default_curved = !self.state.default_curved,
            Act::Quit => fx.push(Effect::Quit),
        }
        fx
    }
}

/// The next round size up or down from `from` (metres), within the panel limits.
fn size_step(from: f32, up: bool) -> f32 {
    let next = if up {
        SIZES_M.iter().copied().find(|&w| w > from + 0.005).unwrap_or(SIZES_M[SIZES_M.len() - 1])
    } else {
        SIZES_M.iter().rev().copied().find(|&w| w < from - 0.005).unwrap_or(SIZES_M[0])
    };
    next.clamp(MIN_WIDTH_M, MAX_WIDTH_M)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moved_watch_keeps_its_spot_until_reset() {
        let head = Pose::new(dvr_core::math::Vec3::new(0.0, 1.6, 0.0), dvr_core::math::Quat::IDENTITY);
        let mut panels = Vec::new();
        let s = Session::new(State { onboarded: true, ..State::default() }, &mut panels, head);
        let w = panels.iter_mut().find(|p| p.source == Source::Wrist).expect("watch");
        let default = w.pose;
        w.pose.pos = w.pose.pos + dvr_core::math::Vec3::new(0.0, 0.0, 0.05);
        let moved = w.pose.pos;
        // Restart: the moved spot stays (the turn is the build's own).
        let state = State { wrist_moved: true, panels: panels.clone(), ..s.state };
        let mut again = state.panels.clone();
        let mut s = Session::new(state, &mut again, head);
        let w = again.iter().find(|p| p.source == Source::Wrist).expect("watch");
        assert_eq!(w.pose.pos, moved);
        assert_eq!(w.pose.rot, default.rot);
        // Reset layout puts it back.
        s.act(Act::ResetLayout, &mut again, head, &[]);
        let w = again.iter().find(|p| p.source == Source::Wrist).expect("watch");
        assert_eq!(w.pose.pos, default.pos);
        assert!(!s.state.wrist_moved);
    }

    #[test]
    fn size_steps_are_round_and_stop_at_the_ends() {
        assert_eq!(size_step(1.2, true), 1.5);
        assert_eq!(size_step(1.2, false), 1.0);
        // An odd size (two-hand pinch) goes to the next round one either way.
        assert_eq!(size_step(1.37, true), 1.5);
        assert_eq!(size_step(1.37, false), 1.2);
        assert_eq!(size_step(8.0, true), 8.0);
        assert_eq!(size_step(0.3, false), 0.3);
    }

    #[test]
    fn menu_fades_in_with_motion_and_appears_at_once_without() {
        for motion in [true, false] {
            let mut panels = Vec::new();
            let state = State { onboarded: true, ..State::default() };
            let mut s = Session::new(state, &mut panels, Pose::IDENTITY);
            s.motion = motion;
            let menu = |p: &[Panel]| *p.iter().find(|p| p.id == MENU_ID).expect("menu");
            assert!(!menu(&panels).visible);
            s.toggle_menu(&mut panels, Pose::IDENTITY);
            assert!(menu(&panels).visible);
            assert_eq!(menu(&panels).shown(), !motion, "motion {motion}: starts faded out only with motion");
            s.update_dims(&mut panels, 1.0, 0.03);
            assert!(menu(&panels).shown() && menu(&panels).dim < 1.0, "fading in after one frame");
            s.update_dims(&mut panels, 1.0, 1.0);
            assert_eq!(menu(&panels).dim, 0.0, "fully in");
        }
    }

    #[test]
    fn showing_again_brings_the_layout_to_where_you_face() {
        use dvr_core::math::{Quat, Vec3};
        let mut panels = Vec::new();
        let mut s = Session::new(State { onboarded: true, ..State::default() }, &mut panels, Pose::IDENTITY);
        let screen = Pose::new(Vec3::new(0.3, 1.5, -1.2), Quat::default());
        panels.push(Panel::new(50, Source::Monitor(0), screen, 1.0, 0.56));
        s.toggle_hidden(&mut panels, Pose::IDENTITY);
        // Turned a quarter left and moved (playspace mover): the screen comes back in front, as arranged.
        let head = Pose::new(Vec3::new(2.0, 0.0, 1.0), Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2));
        s.toggle_hidden(&mut panels, head);
        let p = panels.iter().find(|p| p.id == 50).expect("screen").pose;
        let seen = head.inverse().transform_point(p.pos);
        assert!((seen - screen.pos).length() < 1e-4, "screen at {seen:?} from the head");
        assert!(!s.hidden);
    }

    #[test]
    fn a_restart_brings_the_saved_layout_to_the_wearer() {
        use dvr_core::math::{Quat, Vec3};
        let mut panels = Vec::new();
        let mut s = Session::new(State { onboarded: true, ..State::default() }, &mut panels, Pose::IDENTITY);
        let screen = Pose::new(Vec3::new(0.3, 1.5, -1.2), Quat::default());
        panels.push(Panel::new(50, Source::Monitor(0), screen, 1.0, 0.56));
        s.persist(&panels, Some(Pose::IDENTITY));
        // SteamVR came back with the playspace mover's offset gone: the wearer is somewhere else in its space.
        let mut again = s.state.panels.clone();
        let mut s = Session::new(s.state, &mut again, Pose::IDENTITY);
        let head = Pose::new(Vec3::new(-1.5, 0.0, 2.0), Quat::from_axis_angle(Vec3::Y, 2.0));
        s.arrive(&mut again, head);
        let p = again.iter().find(|p| p.id == 50).expect("screen").pose;
        assert!((head.inverse().transform_point(p.pos) - screen.pos).length() < 1e-4, "in front as arranged");
        assert_eq!(s.state.layout_head, Some(head));
    }

    #[test]
    fn the_toolbar_goes_to_the_panel_a_laser_came_onto_last() {
        let mut panels = Vec::new();
        let mut s = Session::new(State { onboarded: true, ..State::default() }, &mut panels, Pose::IDENTITY);
        panels.push(Panel::new(50, Source::Monitor(0), Pose::IDENTITY, 1.0, 0.56));
        let keys = panels.iter().find(|p| p.source == Source::Keyboard).expect("keyboard").id;
        let (menu, bar) = (Menu::new().expect("menu"), Toolbar::new().expect("toolbar"));
        let frame = |s: &mut Session, now: u64, on: &[(Hand, u32)]| {
            let events: Vec<Event> =
                on.iter().map(|&(hand, panel)| Event::Hover { hand, panel, uv: [0.5, 0.5], cursor: true }).collect();
            let input = PointerInput { events: &events, now, rays: [None; 2], anchors: Default::default() };
            s.pointer(input, &panels, &menu, &bar);
            s.toolbar_target()
        };
        assert_eq!(frame(&mut s, 0, &[(Hand::Left, keys)]), Some(keys));
        // The left laser stays on the keyboard; the right one comes onto the screen: the screen's toolbar shows,
        // and stays while both rest where they are.
        for now in 10..20 {
            assert_eq!(frame(&mut s, now, &[(Hand::Left, keys), (Hand::Right, 50)]), Some(50), "frame {now}");
        }
        // The left laser leaves the keyboard and comes back: the keyboard's toolbar again.
        frame(&mut s, 20, &[(Hand::Right, 50)]);
        assert_eq!(frame(&mut s, 21, &[(Hand::Left, keys), (Hand::Right, 50)]), Some(keys));
        assert_eq!(frame(&mut s, 22, &[(Hand::Left, keys), (Hand::Right, 50)]), Some(keys));
    }
}
