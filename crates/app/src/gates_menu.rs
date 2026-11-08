//! Gate 4c (first run < 60 s), menu, toolbar and hide-all, driven through the real Slint views with scripted
//! pointers.
use crate::gates::{GateResult, HEAD, Rig, key_sc, press_key, us_char};
use crate::session::{Effect, MonitorInfo, Page, Session, TEST_WORD};
use dvr_core::engine::{Event, HIDE_HOLD_MS};
use dvr_core::panel::{Anchor, DIM_HIDDEN, Hand, MENU_ID, PixelRect, Source, TOOLBAR_ID};
use dvr_core::state::State;
use dvr_ui::{MENU_SIZE, Menu, Strings, TOOLBAR_SIZE, Toolbar, slot_center, tab_center, toolbar_button_center};

const FIRST_RUN_BUDGET_MS: u64 = 60_000;

struct Ctx {
    menu: Menu,
    toolbar: Toolbar,
    session: Session,
    rig: Rig,
    strings: Strings,
    screens: Vec<MonitorInfo>,
    effects: Vec<Effect>,
}

impl Ctx {
    /// Feeds the rig's events to the session, menu and toolbar, like the run loop does.
    fn pump(&mut self) {
        let events = self.rig.take();
        let now = self.rig.now;
        self.session.pointer(&events, &self.rig.engine.panels, &self.menu, &self.toolbar, now);
        for e in &events {
            match *e {
                Event::Key { scancode, down } => {
                    self.session.key(scancode, down, us_char);
                }
                Event::MenuToggle => self.session.toggle_menu(&mut self.rig.engine.panels, HEAD),
                Event::HideToggle => self.session.toggle_hidden(&mut self.rig.engine.panels),
                _ => {}
            }
        }
        let poses = [self.rig.hands[0].pose, self.rig.hands[1].pose, HEAD];
        for click in self.menu.take_inputs() {
            let fx = self.session.input(click, &mut self.rig.engine.panels, HEAD, &poses, &self.screens);
            self.effects.extend(fx);
        }
        for click in self.toolbar.take_inputs() {
            let fx = self.session.toolbar_input(click, &mut self.rig.engine.panels, &poses);
            self.effects.extend(fx);
        }
        let model = self.session.model(&self.rig.engine.panels, &self.screens, &self.strings, (0.0, 0.0));
        self.menu.set(&model);
        self.toolbar.set(&self.session.toolbar_model(&self.rig.engine.panels, &self.strings));
        // Fades settle (the run loop steps them every frame).
        self.session.update_dims(&mut self.rig.engine.panels, 1.0, 1.0);
        self.session.place_toolbar(&mut self.rig.engine.panels, &poses);
    }

    /// Points the right hand at menu pixel `at` and clicks with the trigger.
    fn click(&mut self, at: (f32, f32)) -> GateResult {
        self.click_on(MENU_ID, [at.0 / MENU_SIZE.0 as f32, at.1 / MENU_SIZE.1 as f32])
    }

    /// Clicks toolbar button `i` of `n`.
    fn tool(&mut self, i: usize, n: usize) -> GateResult {
        let (x, y) = toolbar_button_center(i, n);
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

    /// Holds the left menu button long enough to hide or show everything.
    fn hold_menu(&mut self) {
        self.rig.hand(Hand::Left).menu = true;
        self.rig.hold_ms(HIDE_HOLD_MS + 50);
        self.rig.hand(Hand::Left).menu = false;
        self.rig.frames(1);
        self.pump();
    }

    fn panel(&self, source: Source) -> Option<dvr_core::panel::Panel> {
        self.rig.engine.panels.iter().find(|p| p.source == source).copied()
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
    };
    c.pump();
    if !(c.session.menu_open(&c.rig.engine.panels) && c.session.page == Page::FirstHand) {
        return Err("first run must open the menu on the hand question".into());
    }

    if !c.session.model(&c.rig.engine.panels, &c.screens, &c.strings, (0.0, 0.0)).subtitle.contains("XSOverlay") {
        return Err("first run must say settings were imported".into());
    }
    c.click(slot_center(1))?;
    if c.session.state.dominant != Hand::Right || c.session.page != Page::FirstMonitor {
        return Err(format!("hand step: dominant {:?}, page {:?}", c.session.state.dominant, c.session.page));
    }
    if c.panel(Source::Wrist).map(|w| w.anchor) != Some(Anchor::Hand(Hand::Left)) {
        return Err("wrist must be on the non-dominant hand".into());
    }
    c.click(slot_center(0))?;
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
    c.click(slot_center(0))?;
    if c.session.page != Page::FirstTips {
        return Err(format!("Done must show the gesture tips, page {:?}", c.session.page));
    }
    c.click(slot_center(4))?;
    if !c.session.state.onboarded || c.session.menu_open(&c.rig.engine.panels) {
        return Err("Got it must finish onboarding and close the menu".into());
    }
    if c.panel(Source::Keyboard).is_some_and(|k| k.visible) {
        return Err("the keyboard must be put away after first run (wrist or menu brings it back)".into());
    }
    if c.rig.now >= FIRST_RUN_BUDGET_MS {
        return Err(format!("first run took {} ms of scripted time", c.rig.now));
    }

    // Toolbar: pointing at the screen shows it; Lock, + and close work from there.
    let screen_id = c.panel(Source::Monitor(0)).ok_or("screen missing")?.id;
    c.rig.aim(Hand::Right, screen_id, [0.5, 0.9])?;
    c.rig.frames(2);
    c.pump();
    if c.session.toolbar_target() != Some(screen_id) {
        return Err("pointing at a screen must show its toolbar".into());
    }
    let width = c.panel(Source::Monitor(0)).ok_or("screen missing")?.width_m;
    c.tool(0, 7)?;
    c.tool(3, 7)?;
    let p = c.panel(Source::Monitor(0)).ok_or("screen missing")?;
    if !p.locked || p.width_m <= width {
        return Err(format!("toolbar Lock / + failed: locked {}, width {} -> {}", p.locked, width, p.width_m));
    }

    // Hold the left menu button: everything fades out and ignores the laser; hold again: back.
    c.hold_menu();
    if !c.panel(Source::Monitor(0)).is_some_and(|p| p.dim >= DIM_HIDDEN && !p.shown()) {
        return Err("holding left menu must hide the screens".into());
    }
    if c.panel(Source::Wrist).is_some_and(|w| w.dim > 0.0) {
        return Err("hide all must keep the watch".into());
    }
    c.hold_menu();
    if !c.panel(Source::Monitor(0)).is_some_and(|p| p.shown()) {
        return Err("holding left menu again must show the screens".into());
    }

    // Reopen with a double-tap on the left menu button.
    for _ in 0..2 {
        c.rig.hand(Hand::Left).menu = true;
        c.rig.frames(1);
        c.rig.hand(Hand::Left).menu = false;
        c.rig.frames(2);
    }
    c.pump();
    if !c.session.menu_open(&c.rig.engine.panels) {
        return Err("double-tap must open the menu".into());
    }

    // Add window: the list opens, picking a window asks the app to show it.
    c.click(slot_center(1))?;
    if c.session.page != Page::Windows(0) || !c.effects.contains(&Effect::ListWindows) {
        return Err(format!("Add window must open the window list, page {:?}", c.session.page));
    }
    c.session.windows = vec![(77, "Notes".into()), (78, "Browser".into())];
    c.pump();
    c.click(slot_center(1))?;
    if !c.effects.contains(&Effect::AddWindow(78)) {
        return Err("picking a window must add it".into());
    }
    c.click(slot_center(5))?;
    if c.session.page != Page::Tab(0) {
        return Err("Back must return to the Panels tab".into());
    }

    // Notification history: newest first, Clear empties it.
    c.session.notified("Discord", "hey");
    c.session.notified("Steam", "Download finished");
    c.pump();
    c.click(slot_center(5))?;
    let model = c.session.model(&c.rig.engine.panels, &c.screens, &c.strings, (0.0, 0.0));
    if c.session.page != Page::Notifications || !model.slots[0].label.starts_with("Steam") {
        return Err(format!("history must list newest first, page {:?}", c.session.page));
    }
    c.click(slot_center(4))?;
    c.click(slot_center(5))?;
    if !c.session.history.is_empty() || c.session.page != Page::Tab(0) {
        return Err("Clear must empty the history and Back return".into());
    }

    c.click(tab_center(3))?;
    c.click(slot_center(0))?;
    if c.session.state.dominant != Hand::Left
        || c.panel(Source::Wrist).map(|w| w.anchor) != Some(Anchor::Hand(Hand::Right))
    {
        return Err("Settings: pointing hand / wrist swap failed".into());
    }
    c.click(tab_center(1))?;
    let sounds = c.session.state.typing_sounds;
    c.click(slot_center(2))?;
    c.click(slot_center(4))?;
    c.click(slot_center(5))?;
    if !c.effects.contains(&Effect::NextLanguage) {
        return Err("Keyboard tab: language button must switch language".into());
    }
    if !c.session.state.media_to_chatbox {
        return Err("Keyboard tab: now-playing toggle failed".into());
    }
    if c.session.state.typing_sounds == sounds {
        return Err("Keyboard tab: typing sounds toggle failed".into());
    }
    c.click(tab_center(2))?;
    c.click(slot_center(2))?;
    if c.session.state.wrist_fade {
        return Err("Wrist tab: look-to-show toggle failed".into());
    }
    // Quit is destructive: the first tap only arms it.
    c.click(tab_center(3))?;
    c.click(slot_center(5))?;
    if c.effects.contains(&Effect::Quit) {
        return Err("Quit must ask for a second tap".into());
    }
    c.click(slot_center(5))?;
    if !c.effects.contains(&Effect::Quit) {
        return Err("second tap on Quit must quit".into());
    }

    // Toolbar close removes the screen (the menu is closed so it can't block the laser).
    c.session.toggle_menu(&mut c.rig.engine.panels, HEAD);
    c.rig.aim(Hand::Right, screen_id, [0.5, 0.9])?;
    c.rig.frames(2);
    c.pump();
    c.tool(6, 7)?;
    if c.panel(Source::Monitor(0)).is_some() || !c.effects.contains(&Effect::Removed(screen_id)) {
        return Err("toolbar close must remove the screen".into());
    }
    c.session.persist(&c.rig.engine.panels);
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
        session.language = "EN".into();
        session.windows = vec![(1, "Discord".into()), (2, "Firefox".into()), (3, "Notepad".into())];
        session.notified("Discord", "Alex: are you on?");
        (session, panels, screens)
    }

    /// Gate 7: every menu view's content (titles, labels, toggle states). Text-level snapshots stay portable
    /// across machines, unlike pixels rendered with each system's fonts.
    #[test]
    fn menu_views_match_snapshots() {
        let strings = Strings::english().expect("strings");
        let (mut session, panels, screens) = setup();
        let pages = [
            ("first_hand", Page::FirstHand),
            ("first_monitor", Page::FirstMonitor),
            ("first_keyboard", Page::FirstKeyboard),
            ("first_tips", Page::FirstTips),
            ("tab_panels", Page::Tab(0)),
            ("tab_keyboard", Page::Tab(1)),
            ("tab_wrist", Page::Tab(2)),
            ("tab_settings", Page::Tab(3)),
            ("windows", Page::Windows(0)),
            ("notifications", Page::Notifications),
        ];
        for (name, page) in pages {
            session.page = page;
            insta::assert_debug_snapshot!(name, session.model(&panels, &screens, &strings, (0.05, 1.2)));
        }
    }

    /// Writes the README screenshots from the real views:
    /// `cargo test -p dvr-app --release screenshots -- --ignored`.
    #[test]
    #[ignore = "writes docs/img; run on demand"]
    fn screenshots() {
        use dvr_ui::{KeyFace, Keyboard, Wrist, WristInfo, WristLoad};
        fn save(name: &str, size: (u32, u32), bgra: &[u8]) {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/img");
            std::fs::create_dir_all(&dir).expect("img dir");
            let rgba: Vec<u8> = bgra.as_chunks::<4>().0.iter().flat_map(|&[b, g, r, a]| [r, g, b, a]).collect();
            let file = std::fs::File::create(dir.join(format!("{name}.png"))).expect("png file");
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), size.0, size.1);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().and_then(|mut w| w.write_image_data(&rgba)).expect("png write");
        }
        let strings = Strings::english().expect("strings");
        let (mut session, panels, screens) = setup();
        let mut menu = Menu::new().expect("menu");
        for (name, page) in
            [("menu-panels", Page::Tab(0)), ("menu-settings", Page::Tab(3)), ("first-run", Page::FirstHand)]
        {
            session.page = page;
            menu.set(&session.model(&panels, &screens, &strings, (0.012, 1.4)));
            let px = menu.render().expect("menu render").to_vec();
            save(name, MENU_SIZE, &px);
        }
        let mut toolbar = Toolbar::new().expect("toolbar");
        let hover = [Event::Hover { hand: Hand::Right, panel: 9, uv: [0.5, 0.5], cursor: true }];
        session.pointer(&hover, &panels, &menu, &toolbar, 0);
        toolbar.set(&session.toolbar_model(&panels, &strings));
        let px = toolbar.render().expect("toolbar render").to_vec();
        save("toolbar", TOOLBAR_SIZE, &px);
        let mut wrist = Wrist::new().expect("wrist");
        let mut info = WristInfo {
            hour: 21,
            minute: 37,
            controllers: [Some(0.8), Some(0.65)],
            lowest_tracker: None,
            expanded: false,
            load: Some(WristLoad { game_fps: Some(90.0), cpu: Some(0.23), gpu: Some(0.67), own_gpu_ms: 0.012 }),
            toast: None,
            media: Some(("Night Drive".into(), "Synthwave Radio".into(), true)),
        };
        wrist.set(&info);
        let px = wrist.render().expect("wrist render").to_vec();
        save("wrist", dvr_ui::WRIST_SIZE, &px);
        info.expanded = true;
        wrist.set(&info);
        let px = wrist.render().expect("wrist render").to_vec();
        save("wrist-expanded", dvr_ui::WRIST_SIZE, &px);
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
        save("keyboard", dvr_ui::KEYBOARD_SIZE, &px);
    }
}
