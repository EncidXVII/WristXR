//! Gate 4c (first run < 60 s) and menu actions, driven through the real Slint menu with scripted pointers.
use crate::gates::{GateResult, HEAD, Rig, key_sc, press_key, us_char};
use crate::session::{MonitorInfo, Page, Session, TEST_WORD};
use dvr_core::engine::Event;
use dvr_core::panel::{Anchor, Hand, MENU_ID, PixelRect, Source};
use dvr_core::state::State;
use dvr_ui::{MENU_SIZE, Menu, Strings, slot_center, tab_center};

const FIRST_RUN_BUDGET_MS: u64 = 60_000;

struct Ctx {
    menu: Menu,
    session: Session,
    rig: Rig,
    strings: Strings,
    screens: Vec<MonitorInfo>,
}

impl Ctx {
    /// Feeds the rig's events to the session and menu, like the run loop does.
    fn pump(&mut self) {
        let events = self.rig.take();
        self.session.pointer(&events, &self.menu);
        for e in &events {
            match *e {
                Event::Key { scancode, down } => {
                    self.session.key(scancode, down, us_char);
                }
                Event::MenuToggle => self.session.toggle_menu(&mut self.rig.engine.panels, HEAD),
                _ => {}
            }
        }
        let poses = [self.rig.hands[0].pose, self.rig.hands[1].pose];
        for click in self.menu.take_inputs() {
            self.session.input(click, &mut self.rig.engine.panels, HEAD, &poses, &self.screens);
        }
        let model = self.session.model(&self.rig.engine.panels, &self.screens, &self.strings, (0.0, 0.0));
        self.menu.set(&model);
    }

    /// Points the right hand at menu pixel `at` and clicks with the trigger.
    fn click(&mut self, at: (f32, f32)) -> GateResult {
        self.rig.aim(Hand::Right, MENU_ID, [at.0 / MENU_SIZE.0 as f32, at.1 / MENU_SIZE.1 as f32])?;
        self.rig.frames(2);
        self.rig.tap(Hand::Right);
        self.rig.frames(1);
        self.pump();
        Ok(())
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
        session,
        rig: Rig::new(panels),
        strings: Strings::english()?,
        screens: vec![screen(0, 2560, 1440, true), screen(2560, 1920, 1080, false)],
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
    if !c.session.state.onboarded || c.session.menu_open(&c.rig.engine.panels) {
        return Err("Done must finish onboarding and close the menu".into());
    }
    if c.rig.now >= FIRST_RUN_BUDGET_MS {
        return Err(format!("first run took {} ms of scripted time", c.rig.now));
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
    // Select the screen by clicking its right edge (beside the menu), then lock it from the Panels tab.
    // The left laser stays on the keyboard: hovering must not steal the selection.
    let screen_id = c.panel(Source::Monitor(0)).ok_or("screen missing")?.id;
    c.rig.aim(Hand::Right, screen_id, [0.95, 0.5])?;
    c.rig.frames(1);
    c.rig.tap(Hand::Right);
    c.rig.hold_ms(500);
    c.pump();
    if c.session.selected != Some(screen_id) {
        return Err(format!("clicking must select the screen, selected {:?}", c.session.selected));
    }
    c.click(slot_center(2))?;
    if !c.panel(Source::Monitor(0)).is_some_and(|p| p.locked) {
        return Err("Lock did not lock the selected screen".into());
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
    if !c.session.state.media_to_chatbox {
        return Err("Keyboard tab: now-playing toggle failed".into());
    }
    if c.session.state.typing_sounds == sounds {
        return Err("Keyboard tab: typing sounds toggle failed".into());
    }
    c.session.persist(&c.rig.engine.panels);
    if c.session.state.panels.iter().any(|p| p.id == MENU_ID) {
        return Err("menu must never be persisted".into());
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

    /// Gate 7: every menu view's content (titles, labels, toggle states). Text-level snapshots stay portable
    /// across machines, unlike pixels rendered with each system's fonts.
    #[test]
    fn menu_views_match_snapshots() {
        let strings = Strings::english().expect("strings");
        let screens = [
            MonitorInfo { rect: PixelRect { x: 0, y: 0, w: 2560, h: 1440 }, primary: true },
            MonitorInfo { rect: PixelRect { x: 2560, y: 0, w: 1920, h: 1080 }, primary: false },
        ];
        let mut panels = Vec::new();
        let mut session = Session::new(State::default(), &mut panels, HEAD);
        panels.push(dvr_core::panel::Panel::in_front_of(9, Source::Monitor(0), HEAD, 0.5625));
        session.selected = Some(9);
        let pages = [
            ("first_hand", Page::FirstHand),
            ("first_monitor", Page::FirstMonitor),
            ("first_keyboard", Page::FirstKeyboard),
            ("tab_panels", Page::Tab(0)),
            ("tab_keyboard", Page::Tab(1)),
            ("tab_wrist", Page::Tab(2)),
            ("tab_settings", Page::Tab(3)),
        ];
        for (name, page) in pages {
            session.page = page;
            insta::assert_debug_snapshot!(name, session.model(&panels, &screens, &strings, (0.05, 1.2)));
        }
    }
}
