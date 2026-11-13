//! Gate 4c (first run < 60 s), menu, toolbar and hide-all, driven through the real Slint views with scripted
//! pointers. Controls are found by their label, as a user would.
use crate::gates::{GateResult, HEAD, Rig, key_sc, press_key, us_char};
use crate::session::{
    Effect, MonitorInfo, Page, Pin, PointerInput, Session, TAB_INBOX, TAB_KEYS, TAB_PANELS, TAB_SETUP, TAB_WATCH,
    TEST_WORD, Tool,
};
use dvr_core::engine::Event;
use dvr_core::panel::{Anchor, DIM_HIDDEN, Hand, MENU_ID, PixelRect, Source, TOOLBAR_ID};
use dvr_core::state::{NotifyFrom, State};
use dvr_ui::{
    MENU_SIZE, Menu, MenuModel, SlotKind, Strings, TOOLBAR_SIZE, Toolbar, ToolbarModel, close_center, tab_center,
};

const FIRST_RUN_BUDGET_MS: u64 = 60_000;

struct Ctx {
    menu: Menu,
    toolbar: Toolbar,
    session: Session,
    rig: Rig,
    strings: Strings,
    screens: Vec<MonitorInfo>,
    effects: Vec<Effect>,
    /// What the menu and toolbar show now.
    shown: MenuModel,
    bar: ToolbarModel,
}

impl Ctx {
    /// Feeds the rig's events to the session, menu and toolbar, like the run loop does.
    fn pump(&mut self) {
        let events = self.rig.take();
        let now = self.rig.now;
        let seen = PointerInput { events: &events, now, rays: [None; 2], anchors: Default::default() };
        self.session.pointer(seen, &self.rig.engine.panels, &self.menu, &self.toolbar);
        for e in &events {
            match *e {
                Event::Key { scancode, down } => {
                    self.session.key(scancode, down, us_char);
                }
                Event::HideToggle => self.session.toggle_hidden(&mut self.rig.engine.panels, HEAD),
                _ => {}
            }
        }
        let poses = [self.rig.hands[0].pose, self.rig.hands[1].pose, HEAD];
        for click in self.menu.take_inputs() {
            let fx = self.session.input(click, &mut self.rig.engine.panels, HEAD, &poses, &self.screens);
            self.effects.extend(fx);
        }
        for click in self.toolbar.take_inputs() {
            let fx = self.session.toolbar_input(click, &mut self.rig.engine.panels, &poses, HEAD);
            self.effects.extend(fx);
        }
        self.shown = self.session.model(&self.rig.engine.panels, &self.screens, &self.strings, (0.0, 0.0));
        self.menu.set(&self.shown);
        // Fades settle (the run loop steps them every frame).
        self.session.update_dims(&mut self.rig.engine.panels, 1.0, 1.0);
        self.session.place_toolbar(&mut self.rig.engine.panels, &poses);
        self.bar = self.session.toolbar_model(&self.rig.engine.panels, &self.strings);
        self.toolbar.set(&self.bar);
    }

    /// Points the right hand at menu pixel `at` and clicks with the trigger.
    fn click(&mut self, at: (f32, f32)) -> GateResult {
        self.click_on(MENU_ID, [at.0 / MENU_SIZE.0 as f32, at.1 / MENU_SIZE.1 as f32])
    }

    /// Clicks the `nth` (0-based) slot labelled with string `id`.
    fn press_nth(&mut self, id: &str, nth: usize) -> GateResult {
        let label = self.strings.get(id, &[]);
        self.press_label(&label, nth)
    }

    fn press(&mut self, id: &str) -> GateResult {
        self.press_nth(id, 0)
    }

    fn press_label(&mut self, label: &str, nth: usize) -> GateResult {
        let i = (0..self.shown.slots.len())
            .filter(|&i| self.shown.slots[i].label == label)
            .nth(nth)
            .ok_or_else(|| format!("no slot {label:?} on {:?}: {:?}", self.session.page, self.shown.texts()))?;
        self.press_slot(i)
    }

    fn press_slot(&mut self, i: usize) -> GateResult {
        let at = self.shown.center(i).ok_or("no such slot")?;
        self.click(at)
    }

    fn slot_on(&self, id: &str) -> Option<bool> {
        let label = self.strings.get(id, &[]);
        self.shown.find(&label).map(|i| self.shown.slots[i].on)
    }

    /// Clicks toolbar control `tool`.
    fn tool(&mut self, tool: Tool) -> GateResult {
        let i = self.session.tool_index(tool).ok_or_else(|| format!("no {tool:?} on the toolbar"))?;
        let (x, y) = self.bar.center(i).ok_or("toolbar control missing")?;
        self.click_on(TOOLBAR_ID, [x / TOOLBAR_SIZE.0 as f32, y / TOOLBAR_SIZE.1 as f32])
    }

    /// Sets the opacity slider to `value` (0..1).
    fn slide(&mut self, value: f32) -> GateResult {
        let i = self.session.tool_index(Tool::Opacity).ok_or("no opacity slider")?;
        let (x, y) = self.bar.slider_at(i, value).ok_or("slider missing")?;
        self.click_on(TOOLBAR_ID, [x / TOOLBAR_SIZE.0 as f32, y / TOOLBAR_SIZE.1 as f32])
    }

    fn click_on(&mut self, panel: u32, uv: [f32; 2]) -> GateResult {
        self.rig.aim(Hand::Right, panel, uv)?;
        self.rig.frames(2);
        self.rig.tap(Hand::Right);
        self.rig.frames(1);
        self.pump();
        Ok(())
    }

    /// Points at a panel until its toolbar shows.
    fn point_at(&mut self, id: u32, uv: [f32; 2]) -> GateResult {
        self.rig.aim(Hand::Right, id, uv)?;
        self.rig.frames(2);
        self.pump();
        if self.session.toolbar_target() != Some(id) {
            return Err(format!("pointing at panel {id} must show its toolbar"));
        }
        Ok(())
    }

    /// Double-taps the left menu button (hide / show everything, as in XSOverlay).
    fn double_tap_menu(&mut self) {
        for _ in 0..2 {
            self.rig.hand(Hand::Left).menu = true;
            self.rig.frames(1);
            self.rig.hand(Hand::Left).menu = false;
            self.rig.frames(2);
        }
        self.pump();
    }

    fn panel(&self, source: Source) -> Option<dvr_core::panel::Panel> {
        self.rig.engine.panels.iter().find(|p| p.source == source).copied()
    }

    fn open_menu(&mut self) {
        self.session.toggle_menu(&mut self.rig.engine.panels, HEAD);
        self.pump();
    }
}

pub fn first_run_and_menu() -> GateResult {
    let screen = |x, w, h, primary| MonitorInfo { rect: PixelRect { x, y: 0, w, h }, primary };
    let mut panels = Vec::new();
    let mut state = State::default();
    let imp = crate::import::xsoverlay(crate::gates::XSOVERLAY_FIXTURE).ok_or("fixture")?;
    crate::import::apply(&mut state, &imp);
    let mut session = Session::new(state, &mut panels, HEAD);
    session.imported = true;
    let mut c = Ctx {
        menu: Menu::new()?,
        toolbar: Toolbar::new()?,
        session,
        rig: Rig::new(panels),
        strings: Strings::english()?,
        screens: vec![screen(0, 2560, 1440, true), screen(2560, 1920, 1080, false)],
        effects: Vec::new(),
        shown: MenuModel::default(),
        bar: ToolbarModel::default(),
    };
    c.pump();
    if !(c.session.menu_open(&c.rig.engine.panels) && c.session.page == Page::FirstHand) {
        return Err("first run must open the menu on the hand question".into());
    }
    if !c.shown.texts().iter().any(|t| t.contains("XSOverlay")) {
        return Err("first run must say settings were imported".into());
    }
    c.press("first-hand-right")?;
    if c.session.state.dominant != Hand::Right || c.session.page != Page::FirstMonitor {
        return Err(format!("hand step: dominant {:?}, page {:?}", c.session.state.dominant, c.session.page));
    }
    if c.panel(Source::Wrist).map(|w| w.anchor) != Some(Anchor::Hand(Hand::Left)) {
        return Err("wrist must be on the non-dominant hand".into());
    }
    c.press_slot(0)?;
    if c.panel(Source::Monitor(0)).is_none() || c.session.page != Page::FirstKeyboard {
        return Err(format!("screen step: page {:?}", c.session.page));
    }
    if !c.panel(Source::Monitor(0)).is_some_and(|p| p.curvature > 0.0 && (p.opacity - 0.85).abs() < 1e-6) {
        return Err("new screen must use the imported curve/opacity defaults".into());
    }
    for (i, ch) in TEST_WORD.chars().enumerate() {
        let hand = if i % 2 == 0 { Hand::Left } else { Hand::Right };
        press_key(&mut c.rig, hand, key_sc(&ch.to_string())?)?;
        c.pump();
    }
    if c.slot_on("first-done") != Some(true) {
        return Err("Done must light up once the word is typed".into());
    }
    c.press("first-done")?;
    if c.session.page != Page::FirstTips {
        return Err(format!("Done must show the gesture tips, page {:?}", c.session.page));
    }
    c.press("first-got-it")?;
    if !c.session.state.onboarded || c.session.menu_open(&c.rig.engine.panels) {
        return Err("Got it must finish onboarding and close the menu".into());
    }
    if c.panel(Source::Keyboard).is_some_and(|k| k.visible) {
        return Err("the keyboard must be put away after first run (wrist or menu brings it back)".into());
    }
    if c.rig.now >= FIRST_RUN_BUDGET_MS {
        return Err(format!("first run took {} ms of scripted time", c.rig.now));
    }

    // Toolbar: pointing at the screen shows it, named; Lock, +, opacity, pin and reset work from there.
    let screen_id = c.panel(Source::Monitor(0)).ok_or("screen missing")?.id;
    c.point_at(screen_id, [0.5, 0.9])?;
    if c.bar.tools.first().map(|t| t.label.as_str()) != Some("Screen 1") {
        return Err(format!("the toolbar must start with the panel's name: {:?}", c.bar.tools.first()));
    }
    let width = c.panel(Source::Monitor(0)).ok_or("screen missing")?.width_m;
    let home = c.panel(Source::Monitor(0)).ok_or("screen missing")?.pose;
    c.tool(Tool::Lock)?;
    // + and - step through round sizes and ease there: 1.2 m, + twice, - once: 1.5 m.
    c.tool(Tool::Bigger)?;
    c.tool(Tool::Bigger)?;
    c.tool(Tool::Smaller)?;
    let mid = c.panel(Source::Monitor(0)).ok_or("screen missing")?.width_m;
    if (mid - width).abs() > 1e-4 {
        return Err(format!("a size change must ease, not jump: {width} -> {mid} at once"));
    }
    for _ in 0..60 {
        c.session.ease_sizes(&mut c.rig.engine.panels, None, 0.011);
    }
    c.slide(0.5)?;
    let p = c.panel(Source::Monitor(0)).ok_or("screen missing")?;
    if !p.locked || (p.width_m - 1.5).abs() > 1e-4 || (p.opacity - 0.6).abs() > 0.03 {
        return Err(format!("toolbar Lock / + / opacity: {} {} -> {} {}", p.locked, width, p.width_m, p.opacity));
    }
    // Hand: into the hand that is not pointing, above it and facing you.
    c.tool(Tool::Pin(Pin::Hand))?;
    let off = c.session.state.dominant.other();
    let anchors = [c.rig.hands[0].pose, c.rig.hands[1].pose, crate::gates::HEAD];
    let p = c.panel(Source::Monitor(0)).ok_or("screen missing")?;
    let held = p.world_pose(&anchors).pos - anchors[off.index()].pos;
    if p.anchor != Anchor::Hand(off) || held.length() > 0.5 || held.y <= 0.0 {
        return Err(format!("toolbar Hand must put the screen in the free hand: {:?} at {held:?}", p.anchor));
    }
    c.point_at(screen_id, [0.5, 0.9])?;
    c.tool(Tool::Pin(Pin::Head))?;
    if c.panel(Source::Monitor(0)).map(|p| p.anchor) != Some(Anchor::Head) {
        return Err("toolbar Head must pin the screen to the head".into());
    }
    c.tool(Tool::Pin(Pin::Room))?;
    // Reset puts it back at its default place and size.
    if let Some(m) = c.rig.engine.panels.iter_mut().find(|p| p.source == Source::Monitor(0)) {
        m.pose.pos = m.pose.pos + dvr_core::math::Vec3::new(0.4, 0.3, 0.0);
    }
    c.point_at(screen_id, [0.5, 0.9])?;
    c.tool(Tool::Reset)?;
    let p = c.panel(Source::Monitor(0)).ok_or("screen missing")?;
    if (p.pose.pos - home.pos).length() > 1e-3 || (p.width_m - width).abs() > 1e-4 || p.anchor != Anchor::World {
        return Err(format!("toolbar Reset must restore place and size: {:?} vs {:?}", p.pose.pos, home.pos));
    }

    // Double-tap the left menu button: screens and keyboard fade out and ignore the laser; again: all back,
    // keyboard included (as in XSOverlay).
    c.double_tap_menu();
    if !c.panel(Source::Monitor(0)).is_some_and(|p| p.dim >= DIM_HIDDEN && !p.shown()) {
        return Err("double-tapping left menu must hide the screens".into());
    }
    if c.panel(Source::Wrist).is_some_and(|w| w.dim > 0.0) {
        return Err("hide all must keep the watch".into());
    }
    c.double_tap_menu();
    if !c.panel(Source::Monitor(0)).is_some_and(|p| p.shown()) || !c.panel(Source::Keyboard).is_some_and(|k| k.shown())
    {
        return Err("double-tapping again must show the screens and the keyboard".into());
    }

    // The menu opens from the watch (or the tray); here directly. The Screens switch hides everything, keeps
    // the menu and shows its state.
    c.open_menu();
    c.press("menu-screens")?;
    if !c.session.hidden || !c.session.menu_open(&c.rig.engine.panels) || c.slot_on("menu-screens") != Some(false) {
        return Err("the Screens switch must hide, keep the menu and show its state".into());
    }
    c.press("menu-screens")?;
    if c.session.hidden {
        return Err("the Screens switch must show again".into());
    }

    // Add window: the list opens, picking a window asks the app to show it; Back returns.
    c.press("menu-add-window")?;
    if c.session.page != Page::Windows(0) || !c.effects.contains(&Effect::ListWindows) {
        return Err(format!("Add window must open the window list, page {:?}", c.session.page));
    }
    c.session.windows = vec![(77, "Notes".into()), (78, "Browser".into())];
    c.pump();
    c.press_label("Browser", 0)?;
    if !c.effects.contains(&Effect::AddWindow(78)) || c.session.menu_open(&c.rig.engine.panels) {
        return Err("picking a window must add it and close the menu so it is visible".into());
    }
    if c.session.window_names.get(&78).map(String::as_str) != Some("Browser") {
        return Err("an added window must keep its name for its toolbar".into());
    }
    c.open_menu();
    c.press("menu-add-window")?;
    c.press("menu-back")?;
    if c.session.page != Page::Tab(TAB_PANELS) {
        return Err("Back must return to Panels".into());
    }

    // Add screen 2, then close it from "Open now" (its close button).
    c.press_label("Screen 2", 0)?;
    let second = c.panel(Source::Monitor(1)).ok_or("Screen 2 must be added")?.id;
    c.open_menu();
    let close = (0..c.shown.slots.len()).filter(|&i| c.shown.slots[i].kind == SlotKind::IconButton).nth(1);
    c.press_slot(close.ok_or("Open now must list both screens with close buttons")?)?;
    if c.panel(Source::Monitor(1)).is_some() || !c.effects.contains(&Effect::Removed(second)) {
        return Err("closing from Open now must remove that screen".into());
    }

    // Inbox: newest first with unread dots and a count on the rail; leaving marks them read; Clear all.
    c.session.notified("Discord", "Alex", "are you on?");
    c.session.notified("Steam", "Steam", "Download finished");
    c.pump();
    if c.shown.tabs.get(TAB_INBOX).map(|t| t.badge) != Some(2) {
        return Err("the Inbox tab must count unread messages".into());
    }
    c.click(tab_center(TAB_INBOX))?;
    let first = c.shown.slots.iter().find(|s| s.kind == SlotKind::Message);
    if c.session.page != Page::Tab(TAB_INBOX) || !first.is_some_and(|m| m.label == "Steam" && m.on) {
        return Err(format!("the Inbox must list newest first, unread: {first:?}"));
    }
    c.press("notify-all")?;
    if !c.effects.contains(&Effect::NotifyFrom(NotifyFrom::AllApps)) {
        return Err("All apps must switch where notifications come from".into());
    }
    c.click(tab_center(TAB_PANELS))?;
    if c.session.unread() != 0 {
        return Err("leaving the Inbox must mark its messages read".into());
    }
    c.click(tab_center(TAB_INBOX))?;
    c.press("menu-clear")?;
    if !c.session.history.is_empty() {
        return Err("Clear all must empty the Inbox".into());
    }

    // Setup: laser hand moves the watch to the other hand; the watch hand is its own choice.
    c.click(tab_center(TAB_SETUP))?;
    c.press_nth("hand-left", 0)?;
    if c.session.state.dominant != Hand::Left
        || c.panel(Source::Wrist).map(|w| w.anchor) != Some(Anchor::Hand(Hand::Right))
    {
        return Err("Setup: laser hand / watch swap failed".into());
    }
    c.press_nth("hand-left", 1)?;
    if c.panel(Source::Wrist).map(|w| w.anchor) != Some(Anchor::Hand(Hand::Left)) {
        return Err("Setup: Watch on Left failed".into());
    }
    c.click(tab_center(TAB_KEYS))?;
    let sounds = c.session.state.typing_sounds;
    c.press("menu-language")?;
    c.press("menu-sounds")?;
    c.press("menu-now-playing")?;
    if !c.effects.contains(&Effect::NextLanguage) {
        return Err("Keys: language must switch language".into());
    }
    if !c.session.state.media_to_chatbox || c.session.state.typing_sounds == sounds {
        return Err("Keys: share songs / key sounds switches failed".into());
    }
    c.click(tab_center(TAB_WATCH))?;
    c.press("menu-wrist-fade")?;
    if c.session.state.wrist_fade {
        return Err("Watch: only-when-looked-at switch failed".into());
    }
    // Quit is destructive: the first tap only arms it.
    c.click(tab_center(TAB_SETUP))?;
    c.press("menu-quit")?;
    if c.effects.contains(&Effect::Quit) {
        return Err("Quit must ask for a second tap".into());
    }
    c.press("menu-confirm")?;
    if !c.effects.contains(&Effect::Quit) {
        return Err("second tap on Quit must quit".into());
    }

    // The rail's Close button; then the keyboard's toolbar puts it away and a screen's removes it.
    c.click(close_center())?;
    if c.session.menu_open(&c.rig.engine.panels) {
        return Err("Close must close the menu".into());
    }
    let kb = c.panel(Source::Keyboard).ok_or("keyboard missing")?.id;
    c.point_at(kb, [0.5, 0.5])?;
    c.tool(Tool::Close)?;
    if c.panel(Source::Keyboard).is_some_and(|k| k.visible) {
        return Err("keyboard toolbar Put away must put the keyboard away".into());
    }
    c.point_at(screen_id, [0.5, 0.9])?;
    c.tool(Tool::Close)?;
    if c.panel(Source::Monitor(0)).is_some() || !c.effects.contains(&Effect::Removed(screen_id)) {
        return Err("toolbar close must remove the screen".into());
    }
    c.session.persist(&c.rig.engine.panels, None);
    if c.session.state.panels.iter().any(|p| p.id == MENU_ID || p.id == TOOLBAR_ID) {
        return Err("menu and toolbar must never be persisted".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_and_menu_gate() {
        assert_eq!(super::first_run_and_menu(), Ok(()));
    }

    fn setup() -> (Session, Vec<dvr_core::panel::Panel>, [MonitorInfo; 2]) {
        let screens = [
            MonitorInfo { rect: PixelRect { x: 0, y: 0, w: 2560, h: 1440 }, primary: true },
            MonitorInfo { rect: PixelRect { x: 2560, y: 0, w: 1920, h: 1080 }, primary: false },
        ];
        let mut panels = Vec::new();
        let mut session = Session::new(State::default(), &mut panels, HEAD);
        panels.push(dvr_core::panel::Panel::in_front_of(9, Source::Monitor(0), HEAD, 0.5625));
        let window = dvr_core::panel::Panel::in_front_of(10, Source::Window(1), HEAD, 0.75);
        panels.push(window);
        session.window_names.insert(1, "Vesktop".into());
        session.language = "EN".into();
        session.languages = vec!["EN".into(), "UK".into()];
        session.windows = vec![(1, "Vesktop".into()), (2, "Steam".into()), (3, "notes.txt – Notepad".into())];
        session.notified("Vesktop", "Mira · #vr-night", "bring the new avatar");
        session.notified("Vesktop", "Alex", "are you on? joining the club world");
        (session, panels, screens)
    }

    #[test]
    fn theme_and_watch_layout_are_picked_in_the_menu() {
        use dvr_core::state::{Accent, WatchLayout};
        let strings = Strings::english().expect("strings");
        let (mut session, mut panels, screens) = setup();
        let click = |session: &mut Session, panels: &mut Vec<_>, label: &str| {
            let m = session.model(panels, &screens, &strings, (0.0, 0.0));
            let i = m.find(label).unwrap_or_else(|| panic!("{label} not on {}", m.title));
            session.input(dvr_ui::MenuInput::Slot(i), panels, HEAD, &Default::default(), &screens)
        };
        session.page = Page::Tab(TAB_SETUP);
        click(&mut session, &mut panels, "Theme");
        assert_eq!(session.page, Page::Theme);
        let fx = click(&mut session, &mut panels, "Aqua");
        assert_eq!((session.state.accent, fx), (Accent::Aqua, vec![Effect::Accent]));
        click(&mut session, &mut panels, "Back");
        assert_eq!(session.page, Page::Tab(TAB_SETUP));
        session.page = Page::Tab(TAB_WATCH);
        click(&mut session, &mut panels, "Layout");
        click(&mut session, &mut panels, "Performance");
        assert_eq!((session.page, session.state.wrist_layout), (Page::WatchLayouts, WatchLayout::Performance));
        let m = session.model(&panels, &screens, &strings, (0.0, 0.0));
        assert_eq!(m.tab, TAB_WATCH, "a sub-page keeps its tab lit");
    }

    /// One line per control: what a user reads and its state, without pixel positions.
    fn outline(m: &MenuModel) -> String {
        let mut out = format!("{} | {}\n", m.title, m.hint);
        for t in &m.tabs {
            out += &format!("tab {} ({})\n", t.label, t.badge);
        }
        for l in &m.labels {
            out += &format!("  {}\n", l.text.replace('\n', " / "));
        }
        for s in &m.slots {
            let state = match (s.on, s.enabled) {
                (true, true) => " [on]",
                (true, false) => " [on, off-limits]",
                (false, false) => " [disabled]",
                (false, true) => "",
            };
            out += &format!("{:?} {:?} {:?} {:?}{state}\n", s.kind, s.label, s.detail, s.value);
        }
        out
    }

    /// Gate 7: every menu view's content (titles, labels, switch states). Text-level snapshots stay portable
    /// across machines, unlike pixels rendered with each system's fonts.
    #[test]
    fn menu_views_match_snapshots() {
        let strings = Strings::english().expect("strings");
        let (mut session, panels, screens) = setup();
        let pages: [(&str, Page); 13] = [
            ("first_hand", Page::FirstHand),
            ("first_monitor", Page::FirstMonitor),
            ("first_keyboard", Page::FirstKeyboard),
            ("first_tips", Page::FirstTips),
            ("tab_panels", Page::Tab(TAB_PANELS)),
            ("tab_keys", Page::Tab(TAB_KEYS)),
            ("tab_watch", Page::Tab(TAB_WATCH)),
            ("tab_inbox", Page::Tab(TAB_INBOX)),
            ("tab_setup", Page::Tab(TAB_SETUP)),
            ("windows", Page::Windows(0)),
            ("watch_layouts", Page::WatchLayouts),
            ("theme", Page::Theme),
            ("controls", Page::Controls),
        ];
        for (name, page) in pages {
            session.page = page;
            let m = session.model(&panels, &screens, &strings, (0.05, 1.2));
            for (i, a) in m.slots.iter().enumerate() {
                let r = a.rect;
                assert!(
                    r.x >= 0.0 && r.x + r.w <= 800.0 && r.y >= 0.0 && r.y + r.h <= 600.0,
                    "{name}: slot {i} off page"
                );
            }
            insta::assert_snapshot!(name, outline(&m));
        }
    }

    /// Writes the README screenshots from the real views:
    /// `cargo test -p dvr-app --release screenshots -- --ignored`.
    #[test]
    #[ignore = "writes docs/img; run on demand"]
    fn screenshots() {
        use dvr_ui::{KeyFace, Keyboard, Wrist, WristInfo, WristLoad, WristMedia};
        fn save(name: &str, size: (u32, u32), bgra: &[u8], height: u32) {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/img");
            std::fs::create_dir_all(&dir).expect("img dir");
            let rows = (height.min(size.1) * size.0 * 4) as usize;
            let rgba: Vec<u8> = bgra[..rows].as_chunks::<4>().0.iter().flat_map(|&[b, g, r, a]| [r, g, b, a]).collect();
            let file = std::fs::File::create(dir.join(format!("{name}.png"))).expect("png file");
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), size.0, height.min(size.1));
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().and_then(|mut w| w.write_image_data(&rgba)).expect("png write");
        }
        let strings = Strings::english().expect("strings");
        let (mut session, panels, screens) = setup();
        let mut menu = Menu::new().expect("menu");
        for (name, page) in [
            ("menu-panels", Page::Tab(TAB_PANELS)),
            ("menu-inbox", Page::Tab(TAB_INBOX)),
            ("menu-settings", Page::Tab(TAB_SETUP)),
            ("menu-windows", Page::Windows(0)),
            ("menu-watch-layout", Page::WatchLayouts),
            ("menu-theme", Page::Theme),
            ("menu-controls", Page::Controls),
            ("first-run", Page::FirstHand),
        ] {
            session.page = page;
            menu.set(&session.model(&panels, &screens, &strings, (0.012, 1.4)));
            let px = menu.render().expect("menu render").to_vec();
            save(name, MENU_SIZE, &px, MENU_SIZE.1);
        }
        let mut toolbar = Toolbar::new().expect("toolbar");
        for (name, id) in [("toolbar", 10), ("toolbar-keyboard", 1)] {
            let id =
                if id == 1 { panels.iter().find(|p| p.source == Source::Keyboard).map_or(1, |p| p.id) } else { id };
            let hover = [Event::Hover { hand: Hand::Right, panel: id, uv: [0.5, 0.5], cursor: true }];
            session.pointer(
                PointerInput { events: &hover, now: 0, rays: [None; 2], anchors: Default::default() },
                &panels,
                &menu,
                &toolbar,
            );
            toolbar.set(&session.toolbar_model(&panels, &strings));
            let px = toolbar.render().expect("toolbar render").to_vec();
            save(name, TOOLBAR_SIZE, &px, TOOLBAR_SIZE.1);
        }
        let mut wrist = Wrist::new().expect("wrist");
        let song = WristMedia {
            title: "Night Drive".into(),
            artist: "Synthwave Radio".into(),
            app: "Spotify".into(),
            playing: true,
            position: 72.0,
            duration: 220.0,
            shuffle: false,
        };
        let mut info = WristInfo {
            hour: 21,
            minute: 37,
            weekday: 1,
            day: 9,
            month: 11,
            controllers: [Some(0.8), Some(0.65)],
            load: Some(WristLoad {
                game_fps: Some(90.0),
                target_fps: Some(90.0),
                cpu: Some(0.23),
                gpu: Some(0.67),
                own_gpu_ms: 0.012,
            }),
            volume: Some((0.62, false)),
            unread: 3,
            keyboard: true,
            ..WristInfo::default()
        };
        type Change<'a> = Box<dyn Fn(&mut WristInfo) + 'a>;
        let shots: [(&str, Change, u32); 5] = [
            ("wrist-idle", Box::new(|_| {}), 152),
            ("wrist", Box::new(|i| i.media = Some(song.clone())), 220),
            (
                "wrist-message",
                Box::new(|i| {
                    i.media = Some(song.clone());
                    i.toast = Some(("Vesktop".into(), "Alex".into(), "are you on? joining the club".into()));
                }),
                220,
            ),
            ("wrist-low", Box::new(|i| i.controllers = [Some(0.8), Some(0.12)]), 220),
            (
                "wrist-expanded",
                Box::new(|i| {
                    i.media = Some(song.clone());
                    i.expanded = true;
                    i.add.screen = Some(2);
                }),
                840,
            ),
        ];
        for (name, change, height) in shots {
            let mut shot = info.clone();
            change(&mut shot);
            wrist.set(&shot);
            let px = wrist.render().expect("wrist render").to_vec();
            save(name, dvr_ui::WRIST_SIZE, &px, height);
        }
        // Every closed layout with a song playing, and the open watch's Add list.
        for layout in dvr_ui::WatchLayout::ALL {
            let shot = WristInfo { layout, media: Some(song.clone()), lowest_tracker: Some(0.9), ..info.clone() };
            wrist.set(&shot);
            let px = wrist.render().expect("wrist render").to_vec();
            let name = format!("wrist-layout-{}", strings.get(layout.ids().0, &[]).to_lowercase().replace(' ', "-"));
            save(&name, dvr_ui::WRIST_SIZE, &px, 240);
            // And with nothing playing, the way the watch looks most of the time.
            wrist.set(&WristInfo { media: None, ..shot });
            let px = wrist.render().expect("wrist render").to_vec();
            save(&format!("{name}-quiet"), dvr_ui::WRIST_SIZE, &px, 240);
        }
        wrist.set(&WristInfo { expanded: true, ..info.clone() });
        let px = wrist.render().expect("wrist render").to_vec();
        save("wrist-expanded-quiet", dvr_ui::WRIST_SIZE, &px, 840);
        let windows = ["Steam", "notes.txt - Notepad", "Vesktop"]
            .iter()
            .enumerate()
            .map(|(i, t)| dvr_ui::WristWindow { id: i as u64, title: (*t).into(), shown: i == 2 })
            .collect();
        let add = dvr_ui::WristAdd { adding: true, screen: Some(2), windows };
        wrist.set(&WristInfo { expanded: true, add, ..info.clone() });
        let px = wrist.render().expect("wrist render").to_vec();
        save("wrist-add", dvr_ui::WRIST_SIZE, &px, 520);
        info.expanded = false;
        let mut kb = Keyboard::new().expect("keyboard");
        let faces: Vec<KeyFace> = dvr_core::keyboard::ROWS
            .iter()
            .flat_map(|r| r.iter())
            .map(|k| KeyFace {
                label: match k.sc {
                    dvr_core::keyboard::LANG => "EN".to_owned(),
                    _ if k.label.chars().count() == 1 => k.label.to_uppercase(),
                    _ => k.label.to_owned(),
                },
                lit: k.label == "Shift",
                hover: k.label == "h",
            })
            .collect();
        kb.set(&faces);
        let px = kb.render().expect("keyboard render").to_vec();
        save("keyboard", dvr_ui::KEYBOARD_SIZE, &px, dvr_ui::KEYBOARD_SIZE.1);
    }
}
