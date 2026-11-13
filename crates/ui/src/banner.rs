//! Notification banner, low in your view: the app's tile, the app and when, the title, and up to three lines of
//! the message. The card stands on the texture's bottom edge, so a shorter message keeps that edge where it is
//! and the card only grows upward. The app moves and fades the overlay itself; this view only draws the card.
use crate::{Canvas, ensure_platform, menu::Slot, rgb};
use slint::ComponentHandle;

slint::slint! {
    import "../fonts/Barlow-Regular.ttf";
    import "../fonts/Barlow-SemiBold.ttf";

    // Graphite, as the watch and menu: card #1D1D20 with its #2A2A2E edge, ink #F5F5F7 / #A8A8AF / #88888F.
    export component BannerView inherits Window {
        default-font-family: "Barlow";
        in property <string> tile;
        in property <color> tile-color;
        in property <string> app;
        in property <string> when;
        in property <string> heading;
        in property <string> body;
        width: 880px;
        height: 232px;
        background: transparent;
        card := Rectangle {
            property <length> text-h: min(message.preferred-height, 96px);
            x: 0px; width: parent.width;
            height: (root.body == "" ? 0px : self.text-h + 4px) + 112px;
            y: parent.height - self.height;
            border-radius: 32px; background: #1D1D20; border-width: 2px; border-color: #2A2A2E;
            Rectangle {
                x: 24px; y: 24px; width: 64px; height: 64px; border-radius: 18px; background: root.tile-color;
                Text { text: root.tile; font-size: 32px; font-weight: 600; color: #FFFFFF; }
            }
            Text {
                x: 112px; y: 18px; width: parent.width - 112px - 120px; height: 32px; text: root.app;
                font-size: 24px; color: #88888F; vertical-alignment: center; overflow: elide;
            }
            Text {
                x: parent.width - 120px; y: 18px; width: 96px; height: 32px; text: root.when;
                font-size: 24px; color: #88888F; horizontal-alignment: right; vertical-alignment: center;
            }
            Text {
                x: 112px; y: 52px; width: parent.width - 136px; height: 40px; text: root.heading;
                font-size: 28px; font-weight: 600; color: #F5F5F7; vertical-alignment: center; overflow: elide;
            }
            message := Text {
                x: 112px; y: 94px; width: parent.width - 136px; height: parent.text-h; text: root.body;
                font-size: 24px; color: #A8A8AF; wrap: word-wrap; overflow: elide;
            }
        }
    }
}

pub const BANNER_SIZE: (u32, u32) = (880, 232);

pub struct Banner {
    view: BannerView,
    canvas: Canvas,
}

impl Banner {
    pub fn new() -> Result<Self, String> {
        ensure_platform();
        let view = BannerView::new().map_err(|e| e.to_string())?;
        let canvas = Canvas::take(BANNER_SIZE)?;
        view.show().map_err(|e| e.to_string())?;
        Ok(Self { view, canvas })
    }

    /// Shows this message: `app` names the sender's app (its tile comes from the name), `when` is e.g. "now".
    pub fn set(&self, app: &str, title: &str, body: &str, when: &str) {
        let tile = Slot::default().tile(if app.is_empty() { title } else { app });
        let v = &self.view;
        v.set_tile(tile.tile.as_str().into());
        v.set_tile_color(rgb(tile.tile_color));
        v.set_app(app.into());
        v.set_when(when.into());
        v.set_heading(title.into());
        v.set_body(body.into());
    }

    /// BGRA rows when something changed since the last call.
    pub fn render(&mut self) -> Option<&[u8]> {
        self.canvas.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_banner_draws_its_card_on_the_bottom_edge() {
        let mut b = Banner::new().expect("banner");
        b.set("Discord", "Alex", "pull in 2 min", "now");
        let px = b.render().expect("first frame").to_vec();
        let alpha = |x: u32, y: u32| px[((y * BANNER_SIZE.0 + x) * 4 + 3) as usize];
        assert!(alpha(440, BANNER_SIZE.1 - 10) > 0, "card at the bottom");
        assert_eq!(alpha(440, 4), 0, "a short message leaves the top clear");
        assert!(b.render().is_none(), "no change, no render");
    }
}
