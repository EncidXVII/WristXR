//! VR UI: Slint components rendered offscreen with the software renderer, only when something changed.
//! Strings come from Fluent (`locales/`), English shipped.

mod i18n;
mod menu;

pub use i18n::Strings;
pub use menu::{MENU_SIZE, Menu, MenuInput, MenuModel, SLOTS, Slot, slot_center, tab_center};

use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, PlatformError};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

slint::slint! {
    // design-tokens.json "Night Lime (warm)": 2 weights, 1 accent, warm grays. Fixed geometry (tests press buttons).
    component WristButton inherits Rectangle {
        in property <string> label;
        callback clicked;
        width: 176px;
        height: 72px;
        background: touch.has-hover ? #2A2622 : #1F1C18;
        Text { text: root.label; font-size: 28px; color: #F4F1EC; }
        touch := TouchArea { clicked => { root.clicked(); } }
    }

    export component WristView inherits Window {
        in property <string> time;
        in property <string> game;
        in property <string> battery;
        in property <string> load;
        in property <string> cost;
        in property <string> toast-title;
        in property <string> toast-body;
        in property <string> media;
        in property <string> prev-label;
        in property <string> play-label;
        in property <string> next-label;
        callback media-clicked(int);
        width: 600px;
        height: 480px;
        background: #0C0B0A;
        Text { x: 24px; y: 8px; text: root.time; font-size: 96px; font-weight: 700; color: #F4F1EC; }
        if root.toast-title != "" : VerticalLayout {
            x: 24px; y: 128px; width: 552px; height: 160px; spacing: 4px;
            Text { text: root.toast-title; font-size: 32px; font-weight: 700; color: #C6FF1A; overflow: elide; }
            Text { text: root.toast-body; font-size: 28px; color: #F4F1EC; wrap: word-wrap; overflow: elide; }
        }
        if root.toast-title == "" : VerticalLayout {
            x: 24px; y: 128px; width: 552px; height: 160px; spacing: 4px;
            Text { text: root.game; font-size: 32px; color: #F4F1EC; }
            Text { text: root.battery; font-size: 28px; color: #AAA49B; }
            Text { text: root.load; font-size: 28px; color: #AAA49B; }
        }
        if root.media != "" : Text {
            x: 24px; y: 296px; width: 552px; text: root.media; font-size: 26px; color: #F4F1EC; overflow: elide;
        }
        if root.media != "" : WristButton { x: 24px; y: 336px; label: root.prev-label; clicked => { root.media-clicked(0); } }
        if root.media != "" : WristButton { x: 212px; y: 336px; label: root.play-label; clicked => { root.media-clicked(1); } }
        if root.media != "" : WristButton { x: 400px; y: 336px; label: root.next-label; clicked => { root.media-clicked(2); } }
        Text { x: 24px; y: 432px; text: root.cost; font-size: 24px; color: #C6FF1A; }
    }
}

pub const WRIST_SIZE: (u32, u32) = (600, 480);

/// Center of wrist media button `i` (0 previous, 1 play/pause, 2 next), in wrist pixels.
pub fn wrist_button_center(i: usize) -> (f32, f32) {
    (24.0 + i as f32 * 188.0 + 88.0, 336.0 + 36.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WristInput {
    Previous,
    PlayPause,
    Next,
}

thread_local! {
    static CREATED: RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(None) };
}

struct OffscreenPlatform {
    start: Instant,
}

impl Platform for OffscreenPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        CREATED.with(|c| *c.borrow_mut() = Some(window.clone()));
        Ok(window)
    }
    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}

/// A component's window plus its BGRA pixels.
pub(crate) struct Canvas {
    pub(crate) window: Rc<MinimalSoftwareWindow>,
    pixels: Vec<PremultipliedRgbaColor>,
    bgra: Vec<u8>,
    size: (u32, u32),
}

impl Canvas {
    pub(crate) fn take(size: (u32, u32)) -> Result<Self, String> {
        let window = CREATED.with(|c| c.borrow_mut().take()).ok_or("no window was created")?;
        window.set_size(PhysicalSize::new(size.0, size.1));
        let n = (size.0 * size.1) as usize;
        Ok(Self { window, pixels: vec![PremultipliedRgbaColor::default(); n], bgra: vec![0; n * 4], size })
    }

    /// Renders when the component asked for a redraw; returns BGRA rows.
    pub(crate) fn render(&mut self) -> Option<&[u8]> {
        slint::platform::update_timers_and_animations();
        let mut drew = false;
        self.window.draw_if_needed(|r| {
            r.render(&mut self.pixels, self.size.0 as usize);
            drew = true;
        });
        if !drew {
            return None;
        }
        for (px, out) in self.pixels.iter().zip(self.bgra.as_chunks_mut::<4>().0) {
            out.copy_from_slice(&[px.blue, px.green, px.red, px.alpha]);
        }
        Some(&self.bgra)
    }

    fn dispatch(&self, event: WindowEvent) {
        // An event the window cannot take (e.g. not shown yet) is simply dropped.
        let _ = self.window.dispatch_event_with_result(event);
    }

    fn position(&self, uv: [f32; 2]) -> LogicalPosition {
        LogicalPosition::new(uv[0] * self.size.0 as f32, uv[1] * self.size.1 as f32)
    }

    /// Laser moved over the panel (uv in 0..1).
    pub(crate) fn pointer_move(&self, uv: [f32; 2]) {
        self.dispatch(WindowEvent::PointerMoved { position: self.position(uv) });
    }

    pub(crate) fn pointer_button(&self, uv: [f32; 2], down: bool) {
        let (position, button) = (self.position(uv), PointerEventButton::Left);
        self.dispatch(if down {
            WindowEvent::PointerPressed { position, button }
        } else {
            WindowEvent::PointerReleased { position, button }
        });
    }

    pub(crate) fn pointer_leave(&self) {
        self.dispatch(WindowEvent::PointerExited);
    }
}

/// Installs the offscreen platform (once per process; must be the UI thread).
pub(crate) fn ensure_platform() {
    // Err means it is already installed, which is fine.
    let _ = slint::platform::set_platform(Box::new(OffscreenPlatform { start: Instant::now() }));
}

/// Values shown on the wrist; formatting and wording live here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WristInfo {
    pub hour: u16,
    pub minute: u16,
    pub game_fps: Option<f32>,
    /// 0..1 per hand, left then right.
    pub controllers: [Option<f32>; 2],
    pub lowest_tracker: Option<f32>,
    pub cpu: Option<f32>,
    pub ram: Option<f32>,
    pub gpu: Option<f32>,
    pub own_gpu_ms: f32,
    pub own_cpu_percent: f32,
    pub toast: Option<(String, String)>,
    /// Now playing: title, artist, playing.
    pub media: Option<(String, String, bool)>,
}

pub struct Wrist {
    view: WristView,
    canvas: Canvas,
    strings: Strings,
    inputs: Rc<RefCell<Vec<WristInput>>>,
}

fn percent(v: Option<f32>) -> String {
    v.map_or_else(|| "–".to_owned(), |v| format!("{:.0}%", v * 100.0))
}

impl Wrist {
    pub fn new() -> Result<Self, String> {
        ensure_platform();
        let view = WristView::new().map_err(|e| e.to_string())?;
        let canvas = Canvas::take(WRIST_SIZE)?;
        view.show().map_err(|e| e.to_string())?;
        let inputs = Rc::new(RefCell::new(Vec::new()));
        let q = inputs.clone();
        view.on_media_clicked(move |i| {
            let input = match i {
                0 => WristInput::Previous,
                1 => WristInput::PlayPause,
                _ => WristInput::Next,
            };
            q.borrow_mut().push(input);
        });
        let strings = Strings::english()?;
        view.set_prev_label(strings.get("wrist-prev", &[]).into());
        view.set_next_label(strings.get("wrist-next", &[]).into());
        Ok(Self { view, canvas, strings, inputs })
    }

    pub fn set(&self, w: &WristInfo) {
        let s = &self.strings;
        self.view.set_time(format!("{:02}:{:02}", w.hour, w.minute).into());
        self.view.set_game(
            match w.game_fps {
                Some(fps) => s.get("wrist-game", &[("fps", format!("{fps:.0}"))]),
                None => s.get("wrist-no-game", &[]),
            }
            .into(),
        );
        let (left, right) = (percent(w.controllers[0]), percent(w.controllers[1]));
        self.view.set_battery(
            match w.lowest_tracker {
                Some(t) => {
                    s.get("wrist-battery-trackers", &[("left", left), ("right", right), ("trackers", percent(Some(t)))])
                }
                None => s.get("wrist-battery", &[("left", left), ("right", right)]),
            }
            .into(),
        );
        self.view.set_load(
            s.get("wrist-load", &[("cpu", percent(w.cpu)), ("ram", percent(w.ram)), ("gpu", percent(w.gpu))]).into(),
        );
        self.view.set_cost(
            s.get(
                "wrist-cost",
                &[("gpu", format!("{:.2}", w.own_gpu_ms)), ("cpu", format!("{:.1}", w.own_cpu_percent))],
            )
            .into(),
        );
        let (title, body) = w.toast.clone().unwrap_or_default();
        self.view.set_toast_title(title.into());
        self.view.set_toast_body(body.into());
        let (media, playing) = match &w.media {
            Some((title, artist, playing)) if artist.is_empty() => (title.clone(), *playing),
            Some((title, artist, playing)) => {
                (s.get("wrist-media", &[("title", title.clone()), ("artist", artist.clone())]), *playing)
            }
            None => (String::new(), false),
        };
        self.view.set_media(media.into());
        self.view.set_play_label(s.get(if playing { "wrist-pause" } else { "wrist-play" }, &[]).into());
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

    /// Media buttons pressed since the last call.
    pub fn take_inputs(&self) -> Vec<WristInput> {
        std::mem::take(&mut *self.inputs.borrow_mut())
    }

    /// BGRA pixels (`WRIST_SIZE`) when the view changed since the last call.
    pub fn render(&mut self) -> Option<&[u8]> {
        self.canvas.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrist_renders_on_change_only() {
        let mut wrist = Wrist::new().expect("wrist");
        wrist.set(&WristInfo { hour: 9, minute: 5, game_fps: Some(90.0), ..WristInfo::default() });
        let first = wrist.render().expect("first frame").to_vec();
        assert_eq!(first.len(), (WRIST_SIZE.0 * WRIST_SIZE.1 * 4) as usize);
        // Background token #0C0B0A at the corner (BGRA).
        assert_eq!(&first[..4], &[0x0A, 0x0B, 0x0C, 0xFF]);
        // Some text pixels are light.
        assert!(first.as_chunks::<4>().0.iter().any(|p| p[0] > 0xC0 && p[1] > 0xC0 && p[2] > 0xC0));
        assert!(wrist.render().is_none(), "no change, no render");
        wrist.set(&WristInfo { hour: 9, minute: 6, game_fps: Some(90.0), ..WristInfo::default() });
        assert!(wrist.render().is_some(), "changed time must redraw");
    }

    #[test]
    fn media_buttons_report_clicks() {
        let mut wrist = Wrist::new().expect("wrist");
        let media = Some(("Song".to_owned(), "Band".to_owned(), true));
        wrist.set(&WristInfo { media, ..WristInfo::default() });
        assert!(wrist.render().is_some());
        let uv = |(x, y): (f32, f32)| [x / WRIST_SIZE.0 as f32, y / WRIST_SIZE.1 as f32];
        for i in [2, 1, 0] {
            let at = uv(wrist_button_center(i));
            wrist.pointer_move(at);
            wrist.pointer_button(at, true);
            wrist.pointer_button(at, false);
        }
        assert_eq!(wrist.take_inputs(), vec![WristInput::Next, WristInput::PlayPause, WristInput::Previous]);
    }
}

/// Gate 7: WCAG AA contrast for every text/background pair the views use, and no text under 24 px in VR.
#[cfg(test)]
mod a11y {
    fn luminance(hex: u32) -> f64 {
        let lin = |c: u32| {
            let c = f64::from(c) / 255.0;
            if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(hex >> 16 & 0xFF) + 0.7152 * lin(hex >> 8 & 0xFF) + 0.0722 * lin(hex & 0xFF)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_colors_meet_wcag_aa() {
        // (text, background, where)
        let pairs = [
            (0xF4F1EC, 0x0C0B0A, "ink on bg"),
            (0xAAA49B, 0x0C0B0A, "secondary on bg"),
            (0xC6FF1A, 0x0C0B0A, "accent on bg (cost, toast title)"),
            (0xF4F1EC, 0x1F1C18, "ink on button"),
            (0xF4F1EC, 0x2A2622, "ink on hovered button"),
            (0xAAA49B, 0x1F1C18, "secondary on key / tab"),
            (0xAAA49B, 0x15130F, "secondary on hovered tab"),
            (0xC6FF1A, 0x1F1C18, "accent on active tab"),
            (0x0C0B0A, 0xC6FF1A, "bg on lit button / key"),
        ];
        for (fg, bg, what) in pairs {
            let c = contrast(fg, bg);
            assert!(c >= 4.5, "{what}: contrast {c:.2} < 4.5");
        }
    }

    #[test]
    fn no_text_below_24px() {
        for src in [include_str!("lib.rs"), include_str!("menu.rs")] {
            let sizes: Vec<u32> =
                src.split("font-size: ").skip(1).filter_map(|p| p.split("px").next()?.trim().parse().ok()).collect();
            assert!(!sizes.is_empty(), "no font sizes found");
            assert!(sizes.iter().all(|&px| px >= 24), "font sizes below 24px: {sizes:?}");
        }
    }
}
