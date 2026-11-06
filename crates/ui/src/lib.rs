//! VR UI: Slint components rendered offscreen with the software renderer, only when something changed.
//! Strings come from Fluent (`locales/`), English shipped.

mod i18n;

pub use i18n::Strings;

use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, PhysicalSize, PlatformError};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

slint::slint! {
    // design-tokens.json "Night Lime (warm)": 2 weights, 1 accent, warm grays.
    export component WristView inherits Window {
        in property <string> time;
        in property <string> game;
        in property <string> battery;
        in property <string> load;
        in property <string> cost;
        in property <string> toast-title;
        in property <string> toast-body;
        width: 600px;
        height: 360px;
        background: #0C0B0A;
        VerticalLayout {
            padding: 24px;
            spacing: 8px;
            Text { text: root.time; font-size: 96px; font-weight: 700; color: #F4F1EC; }
            if root.toast-title != "" : VerticalLayout {
                spacing: 4px;
                Text { text: root.toast-title; font-size: 32px; font-weight: 700; color: #C6FF1A; overflow: elide; }
                Text { text: root.toast-body; font-size: 28px; color: #F4F1EC; wrap: word-wrap; overflow: elide; }
            }
            if root.toast-title == "" : VerticalLayout {
                spacing: 4px;
                Text { text: root.game; font-size: 32px; color: #F4F1EC; }
                Text { text: root.battery; font-size: 28px; color: #AAA49B; }
                Text { text: root.load; font-size: 28px; color: #AAA49B; }
            }
            Rectangle { }
            Text { text: root.cost; font-size: 24px; color: #C6FF1A; }
        }
    }
}

pub const WRIST_SIZE: (u32, u32) = (600, 360);

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
struct Canvas {
    window: Rc<MinimalSoftwareWindow>,
    pixels: Vec<PremultipliedRgbaColor>,
    bgra: Vec<u8>,
    size: (u32, u32),
}

impl Canvas {
    fn take(size: (u32, u32)) -> Result<Self, String> {
        let window = CREATED.with(|c| c.borrow_mut().take()).ok_or("no window was created")?;
        window.set_size(PhysicalSize::new(size.0, size.1));
        let n = (size.0 * size.1) as usize;
        Ok(Self { window, pixels: vec![PremultipliedRgbaColor::default(); n], bgra: vec![0; n * 4], size })
    }

    /// Renders when the component asked for a redraw; returns BGRA rows.
    fn render(&mut self) -> Option<&[u8]> {
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
}

/// Installs the offscreen platform (once per process; must be the UI thread).
fn ensure_platform() {
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
}

pub struct Wrist {
    view: WristView,
    canvas: Canvas,
    strings: Strings,
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
        Ok(Self { view, canvas, strings: Strings::english()? })
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
}
