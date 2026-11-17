//! VR UI: Slint components rendered offscreen with the software renderer, only when something changed.
//! Strings come from Fluent (`locales/`), English shipped.

mod banner;
mod i18n;
pub mod icons;
mod keyboard;
mod menu;
mod toolbar;

pub use banner::{BANNER_SIZE, Banner};
pub use i18n::Strings;
pub use keyboard::{KEYBOARD_SIZE, KeyFace, Keyboard};
pub use menu::{
    Col, Group, Label, Layout, MENU_SIZE, Menu, MenuInput, MenuModel, ROW_H, Rect, Slot, SlotKind, Tab, Tone,
    close_center, tab_center,
};
pub use toolbar::{TOOLBAR_SIZE, ToolButton, ToolInput, ToolKind, Toolbar, ToolbarLayout, ToolbarModel};

use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{Color, ComponentHandle, LogicalPosition, ModelRc, PhysicalSize, PlatformError, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

slint::slint! {
    import "../fonts/Barlow-Regular.ttf";
    import "../fonts/Barlow-SemiBold.ttf";
    import "../fonts/Anton-Regular.ttf";
    // Accent from Setup > Theme (lime by default).
    export global Theme { in property <color> accent: #C6FF1A; }
    // Night Lime (graphite), as in the menu. The watch grows only downward: nothing moves under the laser when it
    // opens. Everything outside the card is transparent.
    // Motion: the card grows from the time and shrinks back along the same path (critically damped, no bounce);
    // what it reveals fades in with it. Presses answer on trigger-down. `motion` off (Windows "Animation
    // effects" off) makes every change instant.
    export struct RingView { arc: string, label: string, low: bool, dim: bool, tag: string }
    export struct AddRowView { label: string, tile: string, tile-color: color, value: string, on: bool, screen: bool, enabled: bool }

    component Round inherits Rectangle {
        in property <string> icon;
        in property <bool> filled;
        in property <brush> tint: #F5F5F7;
        in property <brush> fill-color: transparent;
        // Pressed colour; filled buttons (#2A2A2E) need a lighter step than bare icons to show the press.
        in property <brush> press-color: #36363B;
        in property <bool> lit;
        callback clicked;
        border-radius: self.width / 2;
        background: touch.pressed && !root.lit ? root.press-color : touch.has-hover && !root.lit ? #2A2A2E : root.fill-color;
        opacity: touch.pressed && root.lit ? 0.8 : 1.0;
        Path {
            width: 26px; height: 26px; viewbox-width: 24; viewbox-height: 24;
            commands: root.icon; stroke: root.tint; stroke-width: 2.2px;
            fill: root.filled ? root.tint : transparent;
        }
        touch := TouchArea { clicked => { root.clicked(); } }
    }

    component Row inherits Rectangle {
        in property <string> label;
        in property <string> value;
        in property <bool> switch;
        in property <bool> on;
        in property <bool> divider;
        in property <bool> motion;
        callback clicked;
        height: 62px;
        border-radius: 20px;
        background: touch.pressed ? #36363B : touch.has-hover ? #2A2A2E : transparent;
        if root.divider : Rectangle { x: 18px; y: 0px; width: parent.width - 36px; height: 1px; background: #2A2A2E; }
        Text {
            x: 18px; width: parent.width - 120px; text: root.label; font-size: 26px; color: #F5F5F7;
            vertical-alignment: center; overflow: elide;
        }
        if !root.switch : Text {
            x: parent.width - self.width - 18px; text: root.value; font-size: 24px; color: #A8A8AF;
        }
        if root.switch : Rectangle {
            x: parent.width - 76px; y: 14px; width: 58px; height: 34px; border-radius: 17px;
            background: root.on ? Theme.accent : #2A2A2E;
            animate background { duration: root.motion ? 160ms : 0ms; easing: ease-out; }
            Rectangle {
                x: root.on ? 28px : 4px; y: 4px; width: 26px; height: 26px; border-radius: 13px;
                background: root.on ? #0B0B0C : #88888F;
                animate x, background { duration: root.motion ? 160ms : 0ms; easing: cubic-bezier(0.23, 1, 0.32, 1); }
            }
        }
        touch := TouchArea { clicked => { root.clicked(); } }
    }

    component Pill inherits Rectangle {
        in property <string> label;
        callback clicked;
        height: 44px;
        border-radius: 22px;
        background: touch.pressed ? #444449 : touch.has-hover ? #38383D : #2A2A2E;
        Path {
            x: 12px; y: 11px; width: 22px; height: 22px; viewbox-width: 24; viewbox-height: 24;
            commands: "M12 5v14 M5 12h14"; stroke: #F5F5F7; stroke-width: 2.2px; fill: transparent;
        }
        Text {
            x: 38px; width: parent.width - 50px; height: parent.height; text: root.label; font-size: 24px;
            font-weight: 600; color: #F5F5F7; vertical-alignment: center; overflow: elide;
        }
        touch := TouchArea { clicked => { root.clicked(); } }
    }

    // One line of the on-watch Add list: a screen (icon tile) or a window (letter tile).
    component AddItem inherits Rectangle {
        in property <AddRowView> item;
        in property <bool> divider;
        callback clicked;
        height: 62px;
        border-radius: 20px;
        background: !root.item.enabled ? transparent : touch.pressed ? #36363B : touch.has-hover ? #2A2A2E : transparent;
        if root.divider : Rectangle { x: 18px; y: 0px; width: parent.width - 36px; height: 1px; background: #2A2A2E; }
        Rectangle {
            x: 18px; y: 11px; width: 40px; height: 40px; border-radius: 11px;
            background: root.item.screen ? #2A2A2E : root.item.tile-color;
            if root.item.screen : Path {
                width: 24px; height: 24px; viewbox-width: 24; viewbox-height: 24;
                commands: "M3 5h18v11H3z M8 20h8 M12 16v4"; stroke: #F5F5F7; stroke-width: 2.2px; fill: transparent;
            }
            if !root.item.screen : Text { text: root.item.tile; font-size: 24px; font-weight: 700; color: #FFFFFF; }
        }
        Text {
            x: 72px; width: parent.width - 180px; height: parent.height; text: root.item.label; font-size: 26px;
            color: #F5F5F7; vertical-alignment: center; overflow: elide;
        }
        Text {
            x: parent.width - self.width - 18px; height: parent.height; text: root.item.value;
            font-size: root.item.value == "+" ? 30px : 24px; color: root.item.on ? Theme.accent : #A8A8AF;
            vertical-alignment: center;
        }
        touch := TouchArea { enabled: root.item.enabled; clicked => { root.clicked(); } }
    }

    // Load meter for the Performance strip: label, bar, percent.
    component Meter inherits Rectangle {
        in property <string> label;
        in property <float> level;
        in property <string> value;
        in property <bool> high;
        height: 68px;
        Text { x: 0px; height: parent.height; text: root.label; font-size: 24px; color: #A8A8AF; vertical-alignment: center; }
        Rectangle {
            x: 52px; y: 30px; width: parent.width - 52px - 64px; height: 8px; border-radius: 4px; background: #2A2A2E;
            Rectangle {
                x: 0px; width: parent.width * clamp(root.level, 0, 1); height: 8px; border-radius: 4px;
                background: root.high ? #FF5A3C : Theme.accent;
            }
        }
        Text {
            x: parent.width - 58px; width: 58px; height: parent.height; text: root.value; font-size: 24px;
            color: root.high ? #FF8A6E : #F5F5F7; horizontal-alignment: right; vertical-alignment: center;
        }
    }

    // Now playing: four bars standing on one line. While the song plays and the watch is seen they rise and fall
    // on their own, each on two slow waves of its own (no two in step, nothing faster than about 2.3 a second),
    // like a music app's playing mark; otherwise they hold their resting shape.
    component Wave inherits Rectangle {
        in property <float> t;
        in property <bool> live;
        property <[float]> rest: [0.58, 1.0, 0.42, 0.75];
        property <[float]> fa: [1.275, 1.725, 0.975, 2.175];
        property <[float]> fb: [2.325, 0.825, 1.95, 1.425];
        property <[float]> pa: [0.0, 1.9, 3.7, 5.1];
        property <[float]> pb: [2.4, 0.7, 4.4, 1.3];
        width: 29px; height: 24px;
        for i in 4 : Rectangle {
            property <float> level: root.live
                ? clamp(0.55 + 0.27 * sin((root.t * root.fa[i] * 6.2832 + root.pa[i]) * 1rad)
                    + 0.18 * sin((root.t * root.fb[i] * 6.2832 + root.pb[i]) * 1rad), 0.2, 1.0)
                : root.rest[i];
            x: i * 8px; width: 5px; height: root.height * self.level; y: root.height - self.height;
            border-radius: 2.5px; background: Theme.accent;
        }
    }

    export component WristView inherits Window {
        default-font-family: "Barlow";
        in property <string> time;
        in property <string> date;
        // Date beside the time (Clock and No batteries layouts): weekday over day and month.
        in property <string> weekday;
        in property <string> day-month;
        in property <[RingView]> rings;
        in property <string> ring-track;
        in property <bool> expanded;
        in property <int> card-height;
        // WatchLayout: 0 Standard, 1 Clock, 2 No batteries, 3 Batteries, 4 Music, 5 Performance, 6 Compact, 7 Game,
        // 8 Player.
        in property <int> layout;
        // 0 none, 1 music, 2 message, 3 low battery, 4 performance
        in property <int> strip;
        in property <string> strip-title;
        in property <string> strip-text;
        in property <string> strip-tile;
        in property <color> strip-tile-color;
        in property <string> strip-time;
        in property <string> fps-text;
        in property <bool> fps-low;
        in property <float> cpu;
        in property <float> gpu;
        in property <string> cpu-text;
        in property <string> gpu-text;
        in property <string> cpu-label;
        in property <string> gpu-label;
        in property <string> fps-label;
        in property <bool> playing;
        // Seconds on the playing mark's clock; it moves only while `beating` (playing, seen, motion on).
        in property <float> beat;
        in property <bool> beating;
        in property <bool> has-media;
        in property <string> media-title;
        in property <string> media-sub;
        in property <string> position;
        in property <string> duration;
        in property <float> progress;
        in property <bool> can-seek;
        in property <bool> shuffle;
        in property <bool> has-volume;
        in property <float> volume;
        in property <bool> muted;
        in property <string> volume-text;
        // Top of what opens below the top row (the player, else the list).
        in property <int> body-y;
        in property <int> list-y;
        in property <string> keyboard-label;
        in property <string> screens-label;
        in property <string> inbox-label;
        in property <string> inbox-value;
        in property <string> menu-label;
        in property <string> open-value;
        in property <string> add-label;
        in property <string> add-window-label;
        // Empty when every screen is shown.
        in property <string> add-screen-label;
        in property <bool> keyboard-on;
        in property <bool> screens-on;
        // The Add list replaces the player and the switches.
        in property <bool> adding;
        in property <[AddRowView]> add-rows;
        in property <string> footer;
        in property <bool> motion;
        callback action(int);
        callback seek(float);
        callback set-volume(float);
        callback add-picked(int);
        width: 560px;
        height: 840px;
        background: transparent;

        property <bool> compact: root.layout == 6;
        property <bool> big: root.layout == 4 || root.layout == 8;
        property <bool> tagged: root.layout == 3;
        property <bool> date-right: root.layout == 1 || root.layout == 2 || root.layout == 7 || root.layout == 8;
        // 0 closed .. 1 open; content shown while it is above 0, so closing fades it out instead of cutting it.
        property <float> open: root.expanded ? 1 : 0;
        animate open { duration: root.motion ? 200ms : 0ms; easing: ease-out; }
        // The date shows under the time only when closed and nothing is on the strip.
        property <float> date-t: root.strip == 0 && !root.expanded ? 1 : 0;
        animate date-t { duration: root.motion ? 150ms : 0ms; easing: ease-out; }
        property <length> card-h: root.card-height * 1px;
        animate card-h { duration: root.motion ? 280ms : 0ms; easing: cubic-bezier(0.23, 1, 0.32, 1); }
        // Compact bar: where its content starts, after the time and the battery numbers.
        property <length> bar-x: (168 + root.rings.length * 58 + (root.rings.length > 0 ? 18 : 0)) * 1px;
        // The glance strip leaves three times faster than the card arrives, so the two never read as one.
        // Clickable as soon as the watch closes, even while the media card is still fading over it.
        property <bool> strip-shown: (!root.expanded || root.open < 0.34) && root.strip > 0 && !root.compact;

        Rectangle {
            x: 0px; y: 0px; width: 560px; height: root.card-h; border-radius: 28px; background: #0B0B0C; clip: true;
            Text {
                // Never moves: not when the watch opens, not when a song starts or stops (the card grows instead).
                x: root.compact ? 22px : 26px;
                y: root.compact ? 0px : 22px;
                height: root.compact ? 84px : 88px;
                text: root.time; font-family: "Anton"; font-size: root.compact ? 60px : 88px; color: #F5F5F7;
                vertical-alignment: center;
            }
            if root.date-t > 0 && root.date != "" && !root.compact && !root.date-right && !root.tagged : Text {
                x: 28px; y: 108px; height: 30px; text: root.date; font-size: 24px; color: #A8A8AF; opacity: root.date-t;
            }
            if root.date-right && !root.compact && root.open < 1 : Rectangle {
                x: 234px; y: 24px; width: 300px; height: 84px; opacity: 1 - root.open;
                Text {
                    x: 0px; y: 0px; width: parent.width; height: 30px; text: root.weekday; font-size: 24px;
                    font-weight: 600; color: #A8A8AF; horizontal-alignment: right; letter-spacing: 1.5px;
                }
                Text {
                    x: 0px; y: 28px; width: parent.width; height: 56px; text: root.day-month; font-family: "Anton";
                    font-size: 46px; color: #F5F5F7; horizontal-alignment: right; vertical-alignment: center;
                }
            }
            for r[i] in root.rings : Rectangle {
                x: root.compact ? (168 + i * 58) * 1px : (534 - (root.rings.length - i) * 92 + 12) * 1px;
                y: root.compact ? 16px : root.tagged ? 10px : 26px;
                width: root.compact ? 52px : 80px; height: self.width;
                opacity: root.date-right ? root.open : 1;
                Path {
                    width: parent.width; height: parent.height; viewbox-width: 80; viewbox-height: 80;
                    commands: root.ring-track; stroke: #2A2A2E; stroke-width: root.compact ? 5px : 8px; fill: transparent;
                }
                Path {
                    width: parent.width; height: parent.height; viewbox-width: 80; viewbox-height: 80;
                    commands: r.arc; stroke: r.low ? #FF5A3C : Theme.accent; stroke-width: root.compact ? 5px : 8px;
                    fill: transparent;
                }
                Text { text: r.label; font-size: 24px; font-weight: 700; color: r.low ? #FF5A3C : r.dim ? #88888F : #F5F5F7; }
                if r.tag != "" : Text {
                    x: 0px; y: 82px; width: parent.width; height: 28px; text: r.tag; font-size: 24px; color: #88888F;
                    horizontal-alignment: center;
                }
            }

            // Compact: the bar's right part carries the song, a message, a warning or the date.
            if root.compact && root.rings.length > 0 : Rectangle {
                x: root.bar-x - 13px; y: 22px; width: 1px; height: 40px; background: #2A2A2E;
            }
            if root.compact && root.strip == 0 : Text {
                x: root.bar-x; y: 0px; height: 84px; width: 534px - self.x; text: root.date; font-size: 24px;
                color: #A8A8AF; vertical-alignment: center; overflow: elide;
            }
            if root.compact && root.strip == 1 : Rectangle {
                x: root.bar-x; y: 0px; width: 548px - self.x; height: 84px;
                if root.playing : Wave { x: 0px; y: 30px; t: root.beat; live: root.beating; }
                Text {
                    x: root.playing ? 40px : 0px; width: parent.width - self.x - 64px; height: parent.height;
                    text: root.strip-title; font-size: 24px; color: #F5F5F7; vertical-alignment: center; overflow: elide;
                }
                Round {
                    x: parent.width - 52px; y: 16px; width: 52px; height: 52px;
                    icon: root.playing ? "M7 5h3v14H7z M14 5h3v14h-3z" : "M8 5v14l11-7z"; filled: true;
                    tint: #0B0B0C; fill-color: #F5F5F7; lit: true;
                    clicked => { root.action(5); }
                }
            }
            if root.compact && root.strip == 2 : Rectangle {
                x: root.bar-x; y: 0px; width: 534px - self.x; height: 84px;
                Rectangle {
                    x: 0px; y: 22px; width: 40px; height: 40px; border-radius: 11px; background: root.strip-tile-color;
                    Text { text: root.strip-tile; font-size: 24px; font-weight: 700; color: #FFFFFF; }
                }
                HorizontalLayout {
                    x: 52px; width: parent.width - 52px; height: parent.height; spacing: 8px;
                    Text { text: root.strip-title; font-size: 24px; font-weight: 700; color: #F5F5F7; vertical-alignment: center; overflow: elide; max-width: 160px; }
                    Text { text: root.strip-text; font-size: 24px; color: #A8A8AF; vertical-alignment: center; overflow: elide; horizontal-stretch: 1; }
                }
            }
            if root.compact && root.strip == 3 : Text {
                x: root.bar-x; y: 0px; height: 84px; width: 534px - self.x; text: root.strip-title; font-size: 24px;
                color: #FF8A6E; vertical-alignment: center; overflow: elide;
            }
            if root.compact && root.strip == 4 : Text {
                x: root.bar-x; y: 0px; height: 84px; width: 534px - self.x; text: root.fps-text + " " + root.fps-label;
                font-size: 24px; color: root.fps-low ? #FF8A6E : #F5F5F7; vertical-alignment: center; overflow: elide;
            }

            // Glance strip: one line under the time (music, a message, a low battery or the game's load).
            if root.strip-shown && !root.big : Rectangle {
                x: 12px; y: 132px; width: 536px; height: 68px; border-radius: 18px; opacity: max(0, 1 - root.open * 3);
                background: root.strip == 3 ? #2B1712 : #1D1D20;
                if root.strip == 1 && root.playing : Wave { x: 18px; y: 26px; t: root.beat; live: root.beating; }
                if root.strip == 2 && root.strip-tile != "" : Rectangle {
                    x: 18px; y: 14px; width: 40px; height: 40px; border-radius: 11px; background: root.strip-tile-color;
                    Text { text: root.strip-tile; font-size: 24px; font-weight: 700; color: #FFFFFF; }
                }
                if root.strip == 3 : Path {
                    x: 18px; y: 21px; width: 26px; height: 26px; viewbox-width: 24; viewbox-height: 24;
                    commands: "M12 3l10 18H2z M12 10v5 M12 18v1"; stroke: #FF8A6E; stroke-width: 2.2px; fill: transparent;
                }
                if root.strip != 4 : HorizontalLayout {
                    y: 0px; height: parent.height;
                    x: root.strip == 1 && !root.playing ? 18px : root.strip == 2 ? 72px : 60px;
                    width: parent.width - self.x - (root.strip == 1 ? 76px : root.strip == 2 ? 84px : 18px);
                    spacing: 8px;
                    Text {
                        text: root.strip-title; font-size: 24px; font-weight: root.strip == 2 ? 700 : 400;
                        color: root.strip == 3 ? #FF8A6E : #F5F5F7; vertical-alignment: center; overflow: elide;
                        max-width: root.strip == 3 ? 10000px : 260px;
                    }
                    if root.strip-text != "" : Text {
                        text: root.strip-text; font-size: 24px; color: #A8A8AF; vertical-alignment: center;
                        overflow: elide; horizontal-stretch: 1;
                    }
                }
                if root.strip == 2 : Text {
                    x: parent.width - self.width - 18px; text: root.strip-time; font-size: 24px; color: #88888F;
                }
                if root.strip == 1 : Round {
                    x: 476px; y: 8px; width: 52px; height: 52px;
                    icon: root.playing ? "M7 5h3v14H7z M14 5h3v14h-3z" : "M8 5v14l11-7z"; filled: true;
                    tint: #0B0B0C; fill-color: #F5F5F7; lit: true;
                    clicked => { root.action(5); }
                }
                // Performance: fps as a number, then CPU and GPU as bars.
                if root.strip == 4 : HorizontalLayout {
                    x: 18px; width: 118px; height: parent.height; spacing: 6px; alignment: start;
                    Text {
                        text: root.fps-text; font-family: "Anton"; font-size: 40px; color: root.fps-low ? #FF5A3C : #F5F5F7;
                        vertical-alignment: center;
                    }
                    Text { text: root.fps-label; font-size: 24px; color: #A8A8AF; vertical-alignment: center; }
                }
                if root.strip == 4 : Rectangle { x: 140px; y: 16px; width: 1px; height: 36px; background: #2A2A2E; }
                if root.strip == 4 : Meter {
                    x: 156px; y: 0px; width: 172px; label: root.cpu-label; level: root.cpu; value: root.cpu-text; high: root.cpu > 0.9;
                }
                if root.strip == 4 : Meter {
                    x: 346px; y: 0px; width: 172px; label: root.gpu-label; level: root.gpu; value: root.gpu-text; high: root.gpu > 0.9;
                }
            }

            // Music layout: a two-line card under the time, with back, play and next.
            if root.strip-shown && root.big : Rectangle {
                x: 12px; y: 132px; width: 536px; height: 96px; border-radius: 20px; opacity: max(0, 1 - root.open * 3);
                background: root.strip == 3 ? #2B1712 : #1D1D20; clip: true;
                if root.strip == 1 : Rectangle {
                    x: 14px; y: 16px; width: 64px; height: 64px; border-radius: 14px; background: #36363B;
                    Path {
                        width: 28px; height: 28px; viewbox-width: 24; viewbox-height: 24;
                        commands: "M9 18V5l11-2v13 M9 18a3 3 0 1 1-6 0a3 3 0 1 1 6 0z M20 16a3 3 0 1 1-6 0a3 3 0 1 1 6 0z";
                        stroke: #F5F5F7; stroke-width: 2.2px; fill: transparent;
                    }
                }
                if root.strip == 2 : Rectangle {
                    x: 18px; y: 20px; width: 56px; height: 56px; border-radius: 14px; background: root.strip-tile-color;
                    Text { text: root.strip-tile; font-size: 26px; font-weight: 700; color: #FFFFFF; }
                }
                if root.strip == 3 : Path {
                    x: 22px; y: 35px; width: 26px; height: 26px; viewbox-width: 24; viewbox-height: 24;
                    commands: "M12 3l10 18H2z M12 10v5 M12 18v1"; stroke: #FF8A6E; stroke-width: 2.2px; fill: transparent;
                }
                if root.strip != 3 : Text {
                    x: root.strip == 1 ? 92px : 88px; y: 14px; height: 34px;
                    width: (root.strip == 1 ? 338px : 520px) - self.x - (root.strip == 2 ? 70px : 0px);
                    text: root.strip-title; font-size: 26px; font-weight: 700; color: #F5F5F7; overflow: elide;
                    vertical-alignment: center;
                }
                if root.strip != 3 : Text {
                    x: root.strip == 1 ? 92px : 88px; y: 48px; height: 32px; width: (root.strip == 1 ? 338px : 520px) - self.x;
                    text: root.strip-text; font-size: 24px; color: #A8A8AF; overflow: elide; vertical-alignment: center;
                }
                if root.strip == 3 : Text {
                    x: 60px; width: parent.width - 78px; height: parent.height; text: root.strip-title; font-size: 24px;
                    color: #FF8A6E; vertical-alignment: center; overflow: elide;
                }
                if root.strip == 2 : Text {
                    x: parent.width - self.width - 18px; y: 14px; height: 34px; text: root.strip-time; font-size: 24px;
                    color: #88888F; vertical-alignment: center;
                }
                if root.strip == 1 : Round {
                    x: 344px; y: 22px; width: 52px; height: 52px; icon: "M6 5h2v14H6z M19 5v14l-10-7z"; filled: true;
                    tint: #A8A8AF; clicked => { root.action(4); }
                }
                if root.strip == 1 : Round {
                    x: 404px; y: 18px; width: 60px; height: 60px;
                    icon: root.playing ? "M7 5h3v14H7z M14 5h3v14h-3z" : "M8 5v14l11-7z"; filled: true;
                    tint: #0B0B0C; fill-color: #F5F5F7; lit: true; clicked => { root.action(5); }
                }
                if root.strip == 1 : Round {
                    x: 472px; y: 22px; width: 52px; height: 52px; icon: "M16 5h2v14h-2z M5 5v14l10-7z"; filled: true;
                    tint: #A8A8AF; clicked => { root.action(6); }
                }
                if root.strip == 1 && root.can-seek : Rectangle {
                    x: 0px; y: 92px; width: parent.width * clamp(root.progress, 0, 1); height: 4px; background: Theme.accent;
                }
            }

            // Pointed at: player, volume and quick switches below the top row.
            if root.open > 0 && root.has-media && !root.adding : Rectangle {
                x: 12px; y: root.body-y * 1px; width: 536px; height: 306px; border-radius: 20px; background: #1D1D20; opacity: root.open;
                Rectangle {
                    x: 18px; y: 16px; width: 72px; height: 72px; border-radius: 14px; background: #36363B;
                    Path {
                        width: 30px; height: 30px; viewbox-width: 24; viewbox-height: 24;
                        commands: "M9 18V5l11-2v13 M9 18a3 3 0 1 1-6 0a3 3 0 1 1 6 0z M20 16a3 3 0 1 1-6 0a3 3 0 1 1 6 0z";
                        stroke: #F5F5F7; stroke-width: 2.2px; fill: transparent;
                    }
                }
                Text {
                    x: 104px; y: 18px; width: 414px; height: 34px; text: root.media-title; font-size: 26px; font-weight: 700;
                    color: #F5F5F7; overflow: elide; vertical-alignment: center;
                }
                Text {
                    x: 104px; y: 52px; width: 414px; height: 32px; text: root.media-sub; font-size: 24px; color: #A8A8AF;
                    overflow: elide; vertical-alignment: center;
                }
                Text { x: 18px; y: 102px; width: 64px; height: 28px; text: root.position; font-size: 24px; color: #A8A8AF; }
                Text {
                    x: 454px; y: 102px; width: 64px; height: 28px; text: root.duration; font-size: 24px; color: #A8A8AF;
                    horizontal-alignment: right;
                }
                Rectangle {
                    x: 94px; y: 112px; width: 348px; height: 8px; border-radius: 4px; background: #2A2A2E;
                    Rectangle { x: 0px; width: parent.width * clamp(root.progress, 0, 1); height: 8px; border-radius: 4px; background: #F5F5F7; }
                    if root.can-seek : Rectangle {
                        x: parent.width * clamp(root.progress, 0, 1) - 10px; y: -6px; width: 20px; height: 20px;
                        border-radius: 10px; background: #F5F5F7;
                    }
                }
                TouchArea {
                    x: 88px; y: 96px; width: 360px; height: 40px; enabled: root.can-seek;
                    clicked => { root.seek((self.mouse-x - 6px) / 348px); }
                }
                Round {
                    x: 56px; y: 158px; width: 52px; height: 52px; icon: "M3 7h4l10 10h4 M3 17h4l3-3 M14 10l3-3h4 M18 4l3 3-3 3 M18 14l3 3-3 3";
                    tint: root.shuffle ? Theme.accent : #A8A8AF; clicked => { root.action(7); }
                }
                Round {
                    x: 136px; y: 152px; width: 64px; height: 64px; icon: "M6 5h2v14H6z M19 5v14l-10-7z"; filled: true; press-color: #444449;
                    fill-color: #2A2A2E; clicked => { root.action(4); }
                }
                Round {
                    x: 228px; y: 144px; width: 80px; height: 80px;
                    icon: root.playing ? "M7 5h3v14H7z M14 5h3v14h-3z" : "M8 5v14l11-7z"; filled: true;
                    tint: #0B0B0C; fill-color: Theme.accent; lit: true; clicked => { root.action(5); }
                }
                Round {
                    x: 336px; y: 152px; width: 64px; height: 64px; icon: "M16 5h2v14h-2z M5 5v14l10-7z"; filled: true; press-color: #444449;
                    fill-color: #2A2A2E; clicked => { root.action(6); }
                }
                Round {
                    x: 428px; y: 158px; width: 52px; height: 52px; icon: "M4 12l16-8-6 16-3-7z M11 13l9-9";
                    tint: #A8A8AF; clicked => { root.action(8); }
                }
                Rectangle {
                    x: 18px; y: 238px; width: 52px; height: 52px; border-radius: 14px;
                    opacity: root.has-volume ? 1.0 : 0.4;
                    background: mute-touch.pressed ? #444449 : mute-touch.has-hover ? #38383D : #2A2A2E;
                    Path {
                        width: 26px; height: 26px; viewbox-width: 24; viewbox-height: 24; fill: transparent;
                        commands: root.muted ? "M4 9h4l5-4v14l-5-4H4z M16 9l6 6 M22 9l-6 6" : "M4 9h4l5-4v14l-5-4H4z M16 9a4 4 0 0 1 0 6 M19 6a8 8 0 0 1 0 12";
                        stroke: root.muted ? #FF5A3C : #F5F5F7; stroke-width: 2.2px;
                    }
                    mute-touch := TouchArea { enabled: root.has-volume; clicked => { root.action(9); } }
                }
                Rectangle {
                    x: 82px; y: 258px; width: 348px; height: 12px; border-radius: 6px; background: #2A2A2E;
                    opacity: root.has-volume ? 1.0 : 0.4;
                    Rectangle {
                        x: 0px; width: parent.width * clamp(root.volume, 0, 1); height: 12px; border-radius: 6px;
                        background: root.muted ? #88888F : Theme.accent;
                    }
                    Rectangle {
                        x: parent.width * clamp(root.volume, 0, 1) - 14px; y: -8px; width: 28px; height: 28px;
                        border-radius: 14px; background: #F5F5F7;
                    }
                }
                TouchArea {
                    x: 76px; y: 238px; width: 360px; height: 52px; enabled: root.has-volume;
                    clicked => { root.set-volume((self.mouse-x - 6px) / 348px); }
                    moved => { if self.pressed { root.set-volume((self.mouse-x - 6px) / 348px); } }
                }
                Text {
                    x: 446px; y: 238px; width: 72px; height: 52px; text: root.volume-text; font-size: 24px; color: #F5F5F7;
                    horizontal-alignment: right; vertical-alignment: center;
                }
            }
            if root.open > 0 && !root.adding : Rectangle {
                x: 12px; y: root.list-y * 1px; width: 536px; height: 310px; border-radius: 20px; background: #1D1D20;
                opacity: root.open;
                // One switch for everything (screens, windows, keyboard), then the keyboard alone.
                Row { x: 0px; y: 0px; width: parent.width; label: root.screens-label; switch: true; on: root.screens-on; motion: root.motion; clicked => { root.action(2); } }
                Row { x: 0px; y: 62px; width: parent.width; label: root.keyboard-label; switch: true; on: root.keyboard-on; motion: root.motion; divider: true; clicked => { root.action(1); } }
                // Add: a window (opens the list here) or the next screen, without opening the menu.
                Rectangle {
                    x: 0px; y: 124px; width: parent.width; height: 62px;
                    Rectangle { x: 18px; y: 0px; width: parent.width - 36px; height: 1px; background: #2A2A2E; }
                    Text { x: 18px; height: parent.height; text: root.add-label; font-size: 26px; color: #F5F5F7; vertical-alignment: center; }
                    if root.add-screen-label != "" : Pill {
                        x: parent.width - 18px - 170px; y: 9px; width: 170px; label: root.add-screen-label;
                        clicked => { root.action(11); }
                    }
                    Pill {
                        x: parent.width - 18px - (root.add-screen-label != "" ? 170px + 10px : 0px) - 150px; y: 9px; width: 150px;
                        label: root.add-window-label; clicked => { root.action(10); }
                    }
                }
                Row { x: 0px; y: 186px; width: parent.width; label: root.inbox-label; value: root.inbox-value; divider: true; clicked => { root.action(3); } }
                Row { x: 0px; y: 248px; width: parent.width; label: root.menu-label; value: root.open-value; divider: true; clicked => { root.action(0); } }
            }
            if root.open > 0 && root.adding : Rectangle {
                x: 12px; y: root.body-y * 1px; width: 536px; height: (62 + root.add-rows.length * 62) * 1px;
                border-radius: 20px; background: #1D1D20; opacity: root.open;
                Rectangle {
                    x: 0px; y: 0px; width: parent.width; height: 62px; border-radius: 20px;
                    background: back-touch.pressed ? #36363B : back-touch.has-hover ? #2A2A2E : transparent;
                    Rectangle {
                        x: 14px; y: 9px; width: 44px; height: 44px; border-radius: 22px; background: #2A2A2E;
                        Path {
                            width: 24px; height: 24px; viewbox-width: 24; viewbox-height: 24;
                            commands: "M15 6l-6 6 6 6"; stroke: #F5F5F7; stroke-width: 2.2px; fill: transparent;
                        }
                    }
                    Text {
                        x: 72px; height: parent.height; text: root.add-label; font-family: "Anton"; font-size: 30px;
                        color: #F5F5F7; vertical-alignment: center;
                    }
                    back-touch := TouchArea { clicked => { root.action(12); } }
                }
                for item[i] in root.add-rows : AddItem {
                    x: 0px; y: (62 + i * 62) * 1px; width: parent.width; item: item; divider: true;
                    clicked => { root.add-picked(i); }
                }
            }
            if root.open > 0 && root.footer != "" : Text {
                x: 26px; y: (root.card-height - 42) * 1px; width: 508px; height: 30px; opacity: root.open;
                text: root.footer; font-size: 24px; color: #88888F; overflow: elide;
            }
        }
    }
}

/// Battery level (0..1) below which the watch warns.
pub const LOW_BATTERY: f32 = 0.2;

pub const WRIST_SIZE: (u32, u32) = (560, 840);
/// Height of the time row; the closed watch is this tall without a strip.
const HEADER_H: f32 = 132.0;
/// The Compact layout's one bar.
const COMPACT_H: f32 = 84.0;
const STRIP_END: f32 = 212.0;
/// Closed with nothing on the strip, in the layouts that put the date under the time: a line for it, then the
/// card's bottom margin below.
const DATE_END: f32 = 152.0;
/// The Music layout's two-line card.
const BIG_END: f32 = 240.0;
/// Player card plus the gap under it.
const MEDIA_H: f32 = 316.0;
const LIST_H: f32 = 310.0;
const ROW: f32 = 62.0;
/// Windows listed on the watch's Add list; the rest are one press away in the menu.
pub const WRIST_ADD_WINDOWS: usize = 4;
/// Share of the headset's rate under which the Performance layout shows fps in red (a bar turns red above 90 %).
const LOW_FPS: f32 = 0.9;

pub use dvr_core::state::WatchLayout;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WristInput {
    Menu,
    Keyboard,
    /// Show overlay switch: hide or show screens, windows and keyboard together.
    HideAll,
    Inbox,
    Previous,
    PlayPause,
    Next,
    Shuffle,
    /// Send the song to the VRChat chatbox.
    Share,
    Mute,
    /// Seek to this fraction of the song.
    Seek(f32),
    /// Windows volume, 0..1.
    Volume(f32),
    /// Add > Window: list the windows on the watch.
    OpenAdd,
    /// Back from that list.
    AddBack,
    /// Add the next screen not shown yet.
    AddScreen,
    /// Show this window (handle) in front of you.
    AddWindow(u64),
    /// More windows than the watch lists: open the menu's Add window page.
    AddMore,
}

thread_local! {
    static MOTION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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

/// Turns UI motion on for views created after this call (off by default, so tests and snapshots see the
/// final frame at once). The app follows Windows' "Animation effects" setting.
pub fn set_motion(on: bool) {
    MOTION.with(|m| m.set(on));
}

pub(crate) fn motion() -> bool {
    MOTION.with(std::cell::Cell::get)
}

/// Installs the offscreen platform (once per process; must be the UI thread).
pub(crate) fn ensure_platform() {
    // Err means it is already installed, which is fine.
    let _ = slint::platform::set_platform(Box::new(OffscreenPlatform { start: Instant::now() }));
}

/// Values shown on the watch; formatting and wording live here. `None` fields are not shown.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WristInfo {
    pub hour: u16,
    pub minute: u16,
    /// 0 = Sunday.
    pub weekday: u16,
    pub day: u16,
    /// 1..=12.
    pub month: u16,
    /// What the closed watch shows.
    pub layout: WatchLayout,
    /// 0..1 per hand, left then right.
    pub controllers: [Option<f32>; 2],
    /// The controller is connected: its ring shows even when it reports no level (an empty ring, dim label), so the
    /// watch never has a blank where the battery should be. Nothing connected and no level hides the ring.
    pub controllers_on: [bool; 2],
    /// Charging: never shown as low.
    pub charging: [bool; 2],
    pub lowest_tracker: Option<f32>,
    /// Player, volume and quick switches are shown (the other hand's laser is on the watch).
    pub expanded: bool,
    /// Screens and windows are hidden (the Screens switch is off).
    pub hidden: bool,
    /// The keyboard is out.
    pub keyboard: bool,
    /// fps / load / overlay cost line; `None` when switched off.
    pub load: Option<WristLoad>,
    /// New message: app, title, body.
    pub toast: Option<(String, String, String)>,
    pub media: Option<WristMedia>,
    /// Windows volume (0..1) and mute.
    pub volume: Option<(f32, bool)>,
    /// Unread messages in the Inbox.
    pub unread: usize,
    pub add: WristAdd,
}

/// The open watch's Add row and list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WristAdd {
    /// The Add list is open (it replaces the player and the switches).
    pub adding: bool,
    /// Number (1-based) of the next screen not shown yet; `None` when all are.
    pub screen: Option<u32>,
    /// Windows you can add, front to back (at most `WRIST_ADD_WINDOWS` are listed).
    pub windows: Vec<WristWindow>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WristWindow {
    pub id: u64,
    pub title: String,
    /// Already shown as a panel.
    pub shown: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WristMedia {
    pub title: String,
    pub artist: String,
    /// Player name, e.g. "Spotify"; may be empty.
    pub app: String,
    pub playing: bool,
    /// Seconds; `duration` 0 = unknown (a stream).
    pub position: f32,
    pub duration: f32,
    pub shuffle: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WristLoad {
    pub game_fps: Option<f32>,
    /// Headset refresh rate: fps under 90 % of it shows red on the Performance layout.
    pub target_fps: Option<f32>,
    pub cpu: Option<f32>,
    pub gpu: Option<f32>,
    pub own_gpu_ms: f32,
}

/// Where things were drawn by the last `set`, for hit tests.
#[derive(Clone, Debug, Default)]
struct Shown {
    layout: WatchLayout,
    expanded: bool,
    media: bool,
    strip: i32,
    body_y: f32,
    list_y: f32,
    adding: bool,
    screen: bool,
    /// What each Add list row does.
    targets: Vec<WristInput>,
}

pub struct Wrist {
    view: WristView,
    canvas: Canvas,
    strings: Strings,
    inputs: Rc<RefCell<Vec<WristInput>>>,
    targets: Rc<RefCell<Vec<WristInput>>>,
    /// Bottom of the card (pixels), where the laser stops counting as "on the watch".
    card: f32,
    shown: Shown,
}

fn percent(v: Option<f32>) -> String {
    v.map_or_else(|| "–".to_owned(), |v| format!("{:.0}%", v * 100.0))
}

fn clock(s: f32) -> String {
    let s = s.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// SVG arc for a battery ring (radius 34 on an 80 grid), clockwise from the top.
fn ring_arc(level: f32) -> String {
    let f = level.clamp(0.0, 1.0);
    if f >= 0.999 {
        return RING_TRACK.to_owned();
    }
    let a = f * std::f32::consts::TAU;
    let (x, y) = (40.0 + 34.0 * a.sin(), 40.0 - 34.0 * a.cos());
    format!("M40 6A34 34 0 {} 1 {x:.2} {y:.2}", u8::from(f > 0.5))
}

const RING_TRACK: &str = "M40 6a34 34 0 1 1 0 68a34 34 0 1 1 0-68";

pub(crate) fn rgb(c: u32) -> Color {
    Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

impl Wrist {
    pub fn new() -> Result<Self, String> {
        ensure_platform();
        let view = WristView::new().map_err(|e| e.to_string())?;
        let canvas = Canvas::take(WRIST_SIZE)?;
        view.show().map_err(|e| e.to_string())?;
        let inputs = Rc::new(RefCell::new(Vec::new()));
        let q = inputs.clone();
        view.on_action(move |i| {
            use WristInput::*;
            let all = [
                Menu, Keyboard, HideAll, Inbox, Previous, PlayPause, Next, Shuffle, Share, Mute, OpenAdd, AddScreen,
                AddBack,
            ];
            q.borrow_mut().extend(all.get(i.max(0) as usize).copied());
        });
        let q = inputs.clone();
        view.on_seek(move |f| q.borrow_mut().push(WristInput::Seek(f.clamp(0.0, 1.0))));
        let q = inputs.clone();
        view.on_set_volume(move |f| q.borrow_mut().push(WristInput::Volume(f.clamp(0.0, 1.0))));
        let targets: Rc<RefCell<Vec<WristInput>>> = Rc::default();
        let (q, t) = (inputs.clone(), targets.clone());
        view.on_add_picked(move |i| {
            let picked = t.borrow().get(i.max(0) as usize).copied();
            q.borrow_mut().extend(picked);
        });
        let strings = Strings::english()?;
        let t = |id: &str| strings.get(id, &[]).into();
        view.set_keyboard_label(t("wrist-keyboard"));
        view.set_screens_label(t("wrist-screens"));
        view.set_inbox_label(t("wrist-inbox"));
        view.set_menu_label(t("wrist-menu"));
        view.set_open_value(t("wrist-open"));
        view.set_add_label(t("wrist-add"));
        view.set_add_window_label(t("wrist-add-window"));
        view.set_fps_label(t("wrist-fps"));
        view.set_cpu_label(t("wrist-cpu"));
        view.set_gpu_label(t("wrist-gpu"));
        view.set_ring_track(RING_TRACK.into());
        view.set_motion(motion());
        Ok(Self { view, canvas, strings, inputs, targets, card: HEADER_H, shown: Shown::default() })
    }

    /// Accent colour (0xRRGGBB), from Setup > Theme.
    pub fn set_accent(&self, rgb: u32) {
        self.view.global::<Theme>().set_accent(crate::rgb(rgb));
    }

    /// Steps the playing mark to `t` seconds (`None`: it rests). Call while the watch shows; it redraws only the
    /// frames the bars move.
    pub fn beat(&mut self, t: Option<f32>) {
        self.view.set_beating(t.is_some());
        if let Some(t) = t {
            self.view.set_beat(t);
        }
    }

    pub fn set(&mut self, w: &WristInfo) {
        use WatchLayout as L;
        let s = &self.strings;
        let v = &self.view;
        let layout = w.layout;
        let compact = layout == L::Compact;
        let big = layout.shows_player();
        v.set_layout(layout.view_index());
        v.set_time(format!("{:02}:{:02}", w.hour, w.minute).into());

        // Battery rings: L / R (numbers when open), plus the lowest tracker. Clock and No batteries show them only
        // open; Batteries and Compact always with numbers.
        // (level, connected, charging, short label, warning name)
        let devices = [
            (w.controllers[0], w.controllers_on[0], w.charging[0], "L", "wrist-left-controller"),
            (w.controllers[1], w.controllers_on[1], w.charging[1], "R", "wrist-right-controller"),
            (w.lowest_tracker, false, false, "T", "wrist-tracker"),
        ];
        let show_rings = w.expanded || !layout.date_beside();
        let numbers = w.expanded || matches!(layout, L::Batteries | L::Compact);
        let tag = |short: &str| if layout == L::Batteries { short.into() } else { Default::default() };
        let rings: Vec<RingView> = devices
            .iter()
            // The compact bar has room for the two controllers.
            .take(if compact { 2 } else { 3 })
            .filter(|_| show_rings)
            .filter_map(|&(level, on, charging, short, _)| match level {
                Some(level) => {
                    let label = if numbers { format!("{:.0}", level * 100.0) } else { short.to_owned() };
                    let low = level < LOW_BATTERY && !charging;
                    Some(RingView {
                        arc: ring_arc(level).into(),
                        label: label.into(),
                        low,
                        dim: false,
                        tag: tag(short),
                    })
                }
                // Connected, level unknown: the empty track and a dim label.
                None if on => Some(RingView {
                    arc: "".into(),
                    label: if numbers { "–".into() } else { short.into() },
                    low: false,
                    dim: true,
                    tag: tag(short),
                }),
                None => None,
            })
            .collect();
        v.set_rings(ModelRc::new(VecModel::from(rings)));
        let low = devices
            .iter()
            .filter_map(|&(level, _, charging, _, name)| {
                level.filter(|l| *l < LOW_BATTERY && !charging).map(|l| (l, name))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));

        // One line under the time: a new message, else a low battery, else what the layout is about.
        let artist = |m: &WristMedia, dot: bool| match (m.artist.is_empty(), dot) {
            (true, _) => String::new(),
            (false, true) => format!("· {}", m.artist),
            (false, false) => m.artist.clone(),
        };
        let (strip, title, text) = match (&w.toast, low, &w.media) {
            (Some((_, title, body)), _, _) => (2, title.clone(), body.clone()),
            (None, Some((level, device)), _) => {
                let args = [("device", s.get(device, &[])), ("level", percent(Some(level)))];
                (3, s.get("wrist-low", &args), String::new())
            }
            _ if matches!(layout, L::Clock | L::Batteries) => (0, String::new(), String::new()),
            _ if layout.shows_load() => match &w.load {
                Some(_) => (4, String::new(), String::new()),
                None => (0, String::new(), String::new()),
            },
            (None, None, Some(m)) => (1, m.title.clone(), artist(m, !big)),
            _ => (0, String::new(), String::new()),
        };
        v.set_strip(strip);
        v.set_strip_title(title.into());
        v.set_strip_text(text.into());
        let tile = w.toast.as_ref().map(|(app, _, _)| Slot::default().tile(app));
        v.set_strip_tile(tile.as_ref().map_or_else(Default::default, |t| t.tile.as_str().into()));
        v.set_strip_tile_color(rgb(tile.map_or(0, |t| t.tile_color)));
        v.set_strip_time(s.get("wrist-now", &[]).into());
        let load = w.load.clone().unwrap_or_default();
        v.set_fps_text(load.game_fps.map_or_else(|| "–".to_owned(), |f| format!("{f:.0}")).into());
        v.set_fps_low(matches!((load.game_fps, load.target_fps), (Some(f), Some(t)) if f < LOW_FPS * t));
        v.set_cpu(load.cpu.unwrap_or(0.0));
        v.set_gpu(load.gpu.unwrap_or(0.0));
        v.set_cpu_text(percent(load.cpu).into());
        v.set_gpu_text(percent(load.gpu).into());
        let weekday = s.get(&format!("day-{}", w.weekday % 7), &[]);
        let month = s.get(&format!("month-{}", w.month.clamp(1, 12)), &[]);
        let day_month = s.get("wrist-day-month", &[("day", w.day.to_string()), ("month", month.clone())]);
        let date = s.get("wrist-date", &[("weekday", weekday.clone()), ("day", w.day.to_string()), ("month", month)]);
        v.set_date(date.into());
        v.set_weekday(weekday.to_uppercase().into());
        v.set_day_month(day_month.into());

        // Open: player, volume, quick switches, load; or the Add list in their place.
        v.set_expanded(w.expanded);
        let media = w.media.clone().unwrap_or_default();
        v.set_has_media(w.media.is_some());
        v.set_playing(media.playing);
        v.set_media_title(media.title.as_str().into());
        let sub = match (media.artist.is_empty(), media.app.is_empty()) {
            (false, false) => s.get("wrist-media-sub", &[("artist", media.artist.clone()), ("app", media.app.clone())]),
            (true, _) => media.app.clone(),
            (false, true) => media.artist.clone(),
        };
        v.set_media_sub(sub.into());
        let can_seek = media.duration > 0.0;
        v.set_can_seek(can_seek);
        v.set_position(if can_seek { clock(media.position).into() } else { Default::default() });
        v.set_duration(if can_seek { clock(media.duration).into() } else { Default::default() });
        v.set_progress(if can_seek { media.position / media.duration } else { 0.0 });
        v.set_shuffle(media.shuffle);
        let (volume, muted) = w.volume.unwrap_or((0.0, false));
        v.set_has_volume(w.volume.is_some());
        v.set_volume(volume);
        v.set_muted(muted);
        v.set_volume_text(if w.volume.is_some() { percent(Some(volume)).into() } else { Default::default() });
        v.set_keyboard_on(w.keyboard);
        v.set_screens_on(!w.hidden);
        let inbox = if w.unread > 0 {
            s.get("wrist-inbox-new", &[("n", w.unread.to_string())])
        } else {
            s.get("wrist-open", &[])
        };
        v.set_inbox_value(inbox.into());

        let adding = w.expanded && w.add.adding;
        let screen_label = w.add.screen.map(|n| s.get("wrist-add-screen", &[("n", n.to_string())]));
        v.set_add_screen_label(screen_label.clone().unwrap_or_default().into());
        let mut rows = Vec::new();
        let mut targets = Vec::new();
        if let Some(label) = screen_label {
            rows.push(AddRowView {
                label: label.into(),
                tile: Default::default(),
                tile_color: rgb(0),
                value: "+".into(),
                on: true,
                screen: true,
                enabled: true,
            });
            targets.push(WristInput::AddScreen);
        }
        for win in w.add.windows.iter().take(WRIST_ADD_WINDOWS) {
            let t = Slot::default().tile(&win.title);
            rows.push(AddRowView {
                label: win.title.as_str().into(),
                tile: t.tile.as_str().into(),
                tile_color: rgb(t.tile_color),
                value: if win.shown { s.get("wrist-add-shown", &[]).into() } else { "+".into() },
                on: win.shown,
                screen: false,
                enabled: !win.shown,
            });
            targets.push(WristInput::AddWindow(win.id));
        }
        if w.add.windows.len() > WRIST_ADD_WINDOWS {
            rows.push(AddRowView {
                label: s.get("wrist-add-more", &[]).into(),
                tile: "…".into(),
                tile_color: rgb(0x2A2A2E),
                value: "›".into(),
                on: false,
                screen: false,
                enabled: true,
            });
            targets.push(WristInput::AddMore);
        }
        let add_rows = rows.len();
        v.set_add_rows(ModelRc::new(VecModel::from(rows)));
        v.set_adding(adding);

        let footer = if adding {
            s.get("wrist-add-hint", &[])
        } else {
            w.load.as_ref().map_or_else(String::new, |l| {
                let fps = l.game_fps.map_or_else(|| s.get("wrist-no-game", &[]), |f| format!("{f:.0}"));
                let args = [
                    ("fps", fps),
                    ("cpu", percent(l.cpu)),
                    ("gpu", percent(l.gpu)),
                    ("own", format!("{:.2}", l.own_gpu_ms)),
                ];
                s.get("wrist-load", &args)
            })
        };
        v.set_footer(footer.as_str().into());
        let body_y = if compact { COMPACT_H + 12.0 } else { HEADER_H };
        let list_y = if w.media.is_some() { body_y + MEDIA_H } else { body_y };
        v.set_body_y(body_y as i32);
        v.set_list_y(list_y as i32);
        let below = if footer.is_empty() { 12.0 } else { 60.0 };
        self.card = if adding {
            body_y + ROW * (1 + add_rows) as f32 + below
        } else if w.expanded {
            list_y + LIST_H + below
        } else if compact {
            COMPACT_H
        } else if strip > 0 {
            if big { BIG_END } else { STRIP_END }
        } else if matches!(layout, WatchLayout::Standard | WatchLayout::Music) {
            DATE_END
        } else {
            HEADER_H
        };
        v.set_card_height(self.card as i32);
        *self.targets.borrow_mut() = targets.clone();
        self.shown = Shown {
            layout,
            expanded: w.expanded,
            media: w.media.is_some(),
            strip,
            body_y,
            list_y,
            adding,
            screen: w.add.screen.is_some(),
            targets,
        };
    }

    /// The laser at `uv` holds the watch open: on the card when open, on the top row when closed (so the glance
    /// strip's buttons work without opening it).
    pub fn opens_at(&self, uv: [f32; 2], expanded: bool) -> bool {
        let y = uv[1] * WRIST_SIZE.1 as f32;
        let top = if self.shown.layout == WatchLayout::Compact { COMPACT_H } else { HEADER_H };
        y < if expanded { self.card } else { top }
    }

    /// Where a control sits (wrist pixels) as last drawn; `None` when it is not drawn.
    pub fn center(&self, input: WristInput) -> Option<(f32, f32)> {
        use WristInput::*;
        let g = &self.shown;
        let row = |k: f32| (g.expanded && !g.adding).then_some((280.0, g.list_y + k * ROW + ROW / 2.0));
        let card = |x: f32, y: f32| (g.expanded && g.media && !g.adding).then_some((12.0 + x, g.body_y + y));
        let listed = |i: usize| (280.0, g.body_y + ROW * (1 + i) as f32 + ROW / 2.0);
        let pick = |want: WristInput| g.adding.then(|| g.targets.iter().position(|t| *t == want).map(listed)).flatten();
        let add_y = g.list_y + 2.0 * ROW + ROW / 2.0;
        let closed_song = !g.expanded && g.strip == 1;
        match input {
            PlayPause if closed_song => Some(match g.layout {
                WatchLayout::Compact => (522.0, COMPACT_H / 2.0),
                l if l.shows_player() => (12.0 + 434.0, HEADER_H + 48.0),
                _ => (514.0, 166.0),
            }),
            Previous if closed_song && g.layout.shows_player() => Some((12.0 + 370.0, HEADER_H + 48.0)),
            Next if closed_song && g.layout.shows_player() => Some((12.0 + 498.0, HEADER_H + 48.0)),
            _ if !g.expanded => None,
            AddBack => g.adding.then_some((280.0, g.body_y + ROW / 2.0)),
            AddScreen if g.adding => pick(AddScreen),
            AddWindow(_) | AddMore => pick(input),
            _ if g.adding => None,
            HideAll => row(0.0),
            Keyboard => row(1.0),
            OpenAdd => Some((if g.screen { 12.0 + 188.0 + 75.0 } else { 12.0 + 368.0 + 75.0 }, add_y)),
            AddScreen => g.screen.then_some((12.0 + 348.0 + 85.0, add_y)),
            Inbox => row(3.0),
            Menu => row(4.0),
            Shuffle => card(82.0, 184.0),
            Previous => card(168.0, 184.0),
            PlayPause => card(268.0, 184.0),
            Next => card(368.0, 184.0),
            Share => card(454.0, 184.0),
            Mute => card(44.0, 264.0),
            Seek(f) => card(94.0 + 348.0 * f, 116.0),
            Volume(f) => card(82.0 + 348.0 * f, 264.0),
        }
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

    /// Buttons pressed since the last call.
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

    fn song() -> WristMedia {
        WristMedia {
            title: "Song".into(),
            artist: "Band".into(),
            app: "Spotify".into(),
            playing: true,
            position: 72.0,
            duration: 220.0,
            shuffle: false,
        }
    }

    #[test]
    fn wrist_renders_on_change_only() {
        let mut wrist = Wrist::new().expect("wrist");
        wrist.set(&WristInfo { hour: 9, minute: 5, weekday: 1, day: 9, month: 11, ..WristInfo::default() });
        let first = wrist.render().expect("first frame").to_vec();
        assert_eq!(first.len(), (WRIST_SIZE.0 * WRIST_SIZE.1 * 4) as usize);
        // Card token #0B0B0C inside the card (BGRA); compact: transparent below the card.
        let at = |x: usize, y: usize| &first[(y * WRIST_SIZE.0 as usize + x) * 4..][..4];
        assert_eq!(at(280, 4), &[0x0C, 0x0B, 0x0B, 0xFF]);
        assert_eq!(at(280, 300)[3], 0, "compact watch must be transparent below the card");
        // Some text pixels are light.
        assert!(first.as_chunks::<4>().0.iter().any(|p| p[0] > 0xC0 && p[1] > 0xC0 && p[2] > 0xC0));
        assert!(wrist.render().is_none(), "no change, no render");
        wrist.set(&WristInfo { hour: 9, minute: 6, ..WristInfo::default() });
        assert!(wrist.render().is_some(), "changed time must redraw");
    }

    #[test]
    fn glance_pause_works_closed_and_controls_only_when_open() {
        use WristInput::*;
        let mut wrist = Wrist::new().expect("wrist");
        let uv = |(x, y): (f32, f32)| [x / WRIST_SIZE.0 as f32, y / WRIST_SIZE.1 as f32];
        let press = |wrist: &mut Wrist, input, expanded| {
            let _ = expanded;
            let at = uv(wrist.center(input).expect("drawn"));
            wrist.pointer_move(at);
            wrist.pointer_button(at, true);
            wrist.pointer_button(at, false);
        };
        let open = [Keyboard, HideAll, OpenAdd, Inbox, Menu, Shuffle, Previous, PlayPause, Next, Share, Mute];
        wrist.set(&WristInfo { media: Some(song()), volume: Some((0.5, false)), ..WristInfo::default() });
        assert!(wrist.render().is_some());
        press(&mut wrist, PlayPause, false);
        assert_eq!(wrist.take_inputs(), vec![PlayPause], "the glance strip's pause works closed");
        assert!(!wrist.opens_at(uv((514.0, 166.0)), false), "pointing at the strip must not open the watch");
        assert!(wrist.opens_at(uv((100.0, 60.0)), false), "pointing at the time opens it");
        wrist.set(&WristInfo {
            media: Some(song()),
            volume: Some((0.5, false)),
            expanded: true,
            ..WristInfo::default()
        });
        assert!(wrist.render().is_some());
        for i in open {
            press(&mut wrist, i, true);
        }
        press(&mut wrist, Seek(0.5), true);
        press(&mut wrist, Volume(0.25), true);
        let got = wrist.take_inputs();
        assert_eq!(got[..open.len()], open);
        let near = |a: &WristInput, want: f32| match a {
            Seek(f) | Volume(f) => (f - want).abs() < 0.02,
            _ => false,
        };
        assert!(near(&got[open.len()], 0.5) && matches!(got[open.len()], Seek(_)), "seek: {got:?}");
        assert!(near(&got[open.len() + 1], 0.25) && matches!(got[open.len() + 1], Volume(_)), "volume: {got:?}");
        assert!(wrist.opens_at(uv((280.0, 700.0)), true), "the open card holds the watch open");
    }

    #[test]
    fn watch_grows_from_the_time_and_settles_when_motion_is_on() {
        // Renders until nothing changes; returns the last frame drawn.
        fn settle(wrist: &mut Wrist) -> Vec<u8> {
            let (start, mut last) = (Instant::now(), Vec::new());
            while start.elapsed() < Duration::from_millis(800) {
                if let Some(px) = wrist.render() {
                    last = px.to_vec();
                }
                std::thread::sleep(Duration::from_millis(8));
            }
            last
        }
        let alpha = |px: &[u8], y: usize| px[(y * WRIST_SIZE.0 as usize + 280) * 4 + 3];
        set_motion(true);
        let mut wrist = Wrist::new().expect("wrist");
        set_motion(false);
        let closed = WristInfo { media: Some(song()), volume: Some((0.5, false)), ..WristInfo::default() };
        wrist.set(&closed);
        settle(&mut wrist);
        assert!(wrist.render().is_none(), "a settled watch must not keep redrawing");
        wrist.set(&WristInfo { expanded: true, ..closed.clone() });
        let first = wrist.render().expect("opening redraws").to_vec();
        assert_eq!(alpha(&first, 690), 0, "the card grows toward the bottom, it does not jump there");
        assert_eq!(alpha(&settle(&mut wrist), 690), 0xFF, "the open card reaches the bottom");
        wrist.set(&closed);
        // The closed card ends at 212 px; mid-way the card still covers 300.
        assert_eq!(alpha(wrist.render().expect("closing redraws"), 300), 0xFF, "closing shrinks back, no cut");
        assert_eq!(alpha(&settle(&mut wrist), 690), 0, "the closed card is short again");
    }

    #[test]
    fn the_time_stays_put_when_the_watch_opens() {
        // Topmost row of light (clock) pixels left of the rings.
        fn clock_top(px: &[u8]) -> Option<usize> {
            (0..132).find(|&y| (26..300).any(|x| px[(y * WRIST_SIZE.0 as usize + x) * 4 + 1] > 0xC0))
        }
        let mut wrist = Wrist::new().expect("wrist");
        let closed = WristInfo { hour: 21, minute: 37, weekday: 1, day: 9, month: 11, ..WristInfo::default() };
        wrist.set(&closed);
        let shut = clock_top(wrist.render().expect("closed"));
        wrist.set(&WristInfo { expanded: true, ..closed });
        let open = clock_top(wrist.render().expect("open"));
        assert!(shut.is_some() && shut == open, "the clock moved: {shut:?} -> {open:?}");
    }

    #[test]
    fn glance_pause_works_right_after_the_watch_closes() {
        set_motion(true);
        let mut wrist = Wrist::new().expect("wrist");
        set_motion(false);
        let closed = WristInfo { media: Some(song()), ..WristInfo::default() };
        wrist.set(&WristInfo { expanded: true, ..closed.clone() });
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(400) {
            wrist.render();
            std::thread::sleep(Duration::from_millis(5));
        }
        wrist.set(&closed);
        wrist.render();
        let (x, y) = wrist.center(WristInput::PlayPause).expect("drawn");
        let uv = [x / WRIST_SIZE.0 as f32, y / WRIST_SIZE.1 as f32];
        wrist.pointer_move(uv);
        wrist.pointer_button(uv, true);
        wrist.pointer_button(uv, false);
        assert_eq!(wrist.take_inputs(), vec![WristInput::PlayPause], "pause must work while the card still fades");
    }

    #[test]
    fn every_connected_controller_gets_a_ring() {
        use slint::Model;
        let mut wrist = Wrist::new().expect("wrist");
        wrist.set(&WristInfo {
            controllers: [Some(0.1), None],
            controllers_on: [true, true],
            charging: [true, false],
            ..WristInfo::default()
        });
        let rings: Vec<RingView> = wrist.view.get_rings().iter().collect();
        assert_eq!(rings.len(), 2, "both hands, the right one without a level");
        assert!(!rings[0].low, "charging is never low");
        assert!(rings[1].dim && rings[1].arc.is_empty() && rings[1].label == "R");
        wrist.set(&WristInfo::default());
        assert_eq!(wrist.view.get_rings().row_count(), 0, "nothing connected, nothing shown");
    }

    fn press(wrist: &mut Wrist, input: WristInput) {
        let (x, y) = wrist.center(input).unwrap_or_else(|| panic!("{input:?} not drawn"));
        let uv = [x / WRIST_SIZE.0 as f32, y / WRIST_SIZE.1 as f32];
        wrist.pointer_move(uv);
        wrist.pointer_button(uv, true);
        wrist.pointer_button(uv, false);
    }

    fn base(layout: WatchLayout) -> WristInfo {
        WristInfo {
            hour: 21,
            minute: 37,
            weekday: 2,
            day: 11,
            month: 11,
            layout,
            controllers: [Some(0.8), Some(0.65)],
            lowest_tracker: Some(0.9),
            media: Some(song()),
            load: Some(WristLoad {
                game_fps: Some(70.0),
                target_fps: Some(90.0),
                cpu: Some(0.23),
                gpu: Some(0.95),
                own_gpu_ms: 0.01,
            }),
            ..WristInfo::default()
        }
    }

    #[test]
    fn each_layout_shows_its_own_glance() {
        use WatchLayout as L;
        use slint::Model;
        let mut wrist = Wrist::new().expect("wrist");
        // (layout, rings closed, strip with a song playing, closed card height)
        let want = [
            (L::Standard, 3, 1, STRIP_END),
            (L::Clock, 0, 0, HEADER_H),
            (L::NoBatteries, 0, 1, STRIP_END),
            (L::Batteries, 3, 0, HEADER_H),
            (L::Music, 3, 1, BIG_END),
            (L::Performance, 3, 4, STRIP_END),
            (L::Compact, 2, 1, COMPACT_H),
            (L::Game, 0, 4, STRIP_END),
            (L::Player, 0, 1, BIG_END),
        ];
        for (layout, rings, strip, height) in want {
            wrist.set(&base(layout));
            assert!(wrist.render().is_some(), "{layout:?} draws");
            assert_eq!(wrist.view.get_rings().row_count(), rings, "{layout:?} rings");
            assert_eq!(wrist.view.get_strip(), strip, "{layout:?} strip");
            assert_eq!(wrist.view.get_card_height(), height as i32, "{layout:?} height");
            // A message comes through in every layout.
            let toast = Some(("Vesktop".into(), "Alex".into(), "on in 5?".into()));
            wrist.set(&WristInfo { toast, ..base(layout) });
            assert_eq!(wrist.view.get_strip(), 2, "{layout:?} shows messages");
            // Opening shows every battery with its number.
            wrist.set(&WristInfo { expanded: true, ..base(layout) });
            let rings: Vec<RingView> = wrist.view.get_rings().iter().collect();
            assert!(rings.len() >= 2 && rings[0].label == "80", "{layout:?} open rings: {rings:?}");
        }
        // Nothing playing: the date gets its own line under the time, which stays where it was.
        for (layout, height) in [(L::Standard, DATE_END), (L::Music, DATE_END), (L::Clock, HEADER_H)] {
            wrist.set(&WristInfo { media: None, ..base(layout) });
            assert_eq!(wrist.view.get_strip(), 0, "{layout:?} quiet strip");
            assert_eq!(wrist.view.get_card_height(), height as i32, "{layout:?} quiet height");
        }
        wrist.set(&base(L::Batteries));
        let tags: Vec<String> = wrist.view.get_rings().iter().map(|r| format!("{} {}", r.label, r.tag)).collect();
        assert_eq!(tags, ["80 L", "65 R", "90 T"]);
        wrist.set(&base(L::Performance));
        assert!(wrist.view.get_fps_low(), "70 fps on a 90 Hz headset is low");
        // A low battery still warns where batteries are hidden.
        wrist.set(&WristInfo { controllers: [Some(0.1), Some(0.65)], ..base(L::Clock) });
        assert_eq!(wrist.view.get_strip(), 3);
    }

    #[test]
    fn glance_buttons_work_in_music_and_compact() {
        use WristInput::*;
        let mut wrist = Wrist::new().expect("wrist");
        wrist.set(&base(WatchLayout::Music));
        wrist.render();
        for input in [Previous, PlayPause, Next] {
            press(&mut wrist, input);
        }
        assert_eq!(wrist.take_inputs(), vec![Previous, PlayPause, Next]);
        wrist.set(&base(WatchLayout::Compact));
        wrist.render();
        press(&mut wrist, PlayPause);
        assert_eq!(wrist.take_inputs(), vec![PlayPause]);
        let uv = |y: f32| [0.1, y / WRIST_SIZE.1 as f32];
        assert!(wrist.opens_at(uv(60.0), false) && !wrist.opens_at(uv(100.0), false), "the bar is the time row");
    }

    #[test]
    fn the_top_row_stays_put_when_opening_in_every_layout() {
        fn clock_top(px: &[u8]) -> Option<usize> {
            (0..132).find(|&y| (22..150).any(|x| px[(y * WRIST_SIZE.0 as usize + x) * 4 + 1] > 0xC0))
        }
        let mut wrist = Wrist::new().expect("wrist");
        for layout in WatchLayout::ALL {
            let closed = WristInfo { media: None, ..base(layout) };
            wrist.set(&closed);
            let shut = clock_top(wrist.render().expect("closed"));
            wrist.set(&WristInfo { expanded: true, ..closed });
            let open = clock_top(wrist.render().expect("open"));
            assert!(shut.is_some() && shut == open, "{layout:?}: the clock moved {shut:?} -> {open:?}");
        }
    }

    #[test]
    fn add_works_from_the_open_watch() {
        use WristInput::*;
        let mut wrist = Wrist::new().expect("wrist");
        let windows: Vec<WristWindow> =
            (1..=6).map(|i| WristWindow { id: i, title: format!("Window {i}"), shown: i == 1 }).collect();
        let open = WristInfo {
            expanded: true,
            add: WristAdd { screen: Some(2), ..WristAdd::default() },
            ..base(WatchLayout::Standard)
        };
        wrist.set(&open);
        wrist.render();
        press(&mut wrist, OpenAdd);
        press(&mut wrist, AddScreen);
        assert_eq!(wrist.take_inputs(), vec![OpenAdd, AddScreen], "Add row: Window and Screen 2");
        let adding = WristInfo { add: WristAdd { adding: true, screen: Some(2), windows }, ..open.clone() };
        wrist.set(&adding);
        wrist.render();
        assert!(wrist.center(Menu).is_none(), "the list replaces the switches");
        assert!(wrist.center(AddWindow(5)).is_none(), "the watch lists four windows");
        for input in [AddScreen, AddWindow(2), AddWindow(1), AddMore, AddBack] {
            press(&mut wrist, input);
        }
        assert_eq!(
            wrist.take_inputs(),
            vec![AddScreen, AddWindow(2), AddMore, AddBack],
            "a shown window is not added again"
        );
        // Back, then all screens shown: only Window is offered.
        wrist.set(&WristInfo { add: WristAdd::default(), ..open.clone() });
        assert!(wrist.center(AddScreen).is_none() && wrist.center(OpenAdd).is_some());
    }

    #[test]
    fn rings_cover_the_battery_level() {
        assert_eq!(ring_arc(1.0), RING_TRACK);
        assert!(ring_arc(0.25).starts_with("M40 6A34 34 0 0 1 74.00 40.00"), "{}", ring_arc(0.25));
        assert!(ring_arc(0.75).contains(" 0 1 1 6.00 40.00"), "{}", ring_arc(0.75));
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
            (0xF5F5F7, 0x0B0B0C, "ink on bg"),
            (0xA8A8AF, 0x0B0B0C, "secondary on bg"),
            (0x88888F, 0x0B0B0C, "tertiary on bg (hints, footers)"),
            (0xF5F5F7, 0x141416, "ink on rail"),
            (0xA8A8AF, 0x141416, "secondary on rail"),
            (0xA8A8AF, 0x1D1D20, "secondary on group / hovered tab"),
            (0x88888F, 0x1D1D20, "tertiary on group (message time)"),
            (0xF5F5F7, 0x1D1D20, "ink on group"),
            (0xF5F5F7, 0x2A2A2E, "ink on hovered row"),
            (0xA8A8AF, 0x2A2A2E, "secondary on hovered segment"),
            (0xC6FF1A, 0x1D1D20, "accent value on group"),
            (0xC6FF1A, 0x2A2A2E, "accent value on hovered row"),
            (0x0B0B0C, 0xC6FF1A, "bg on lit button / tab / key"),
            (0x0B0B0C, 0xF5F5F7, "bg on chosen segment"),
            (0x0B0B0C, 0xFF5A3C, "bg on armed danger button"),
            (0xFF5A3C, 0x0B0B0C, "danger on bg (outlined Quit)"),
            (0xFF5A3C, 0x1D1D20, "danger on key"),
            (0xFF8A6E, 0x2B1712, "low battery strip"),
            (0xF5F5F7, 0x36363B, "ink on pressed row / button"),
            (0xF5F5F7, 0x444449, "ink on pressed Previous / Next"),
            (0xA8A8AF, 0x36363B, "secondary on pressed segment"),
            (0xC6FF1A, 0x36363B, "accent value on pressed row"),
            (0xFF5A3C, 0x45201A, "danger on pressed Quit"),
        ];
        // Letter tiles: 24 px bold is large text (3:1).
        for tile in [0x5865F2, 0x1B6FD1, 0xB85000, 0x2E7D4F, 0x8E44AD, 0xB03A2E] {
            assert!(contrast(0xFFFFFF, tile) >= 3.0, "white on tile {tile:06X}");
        }
        for (fg, bg, what) in pairs {
            let c = contrast(fg, bg);
            assert!(c >= 4.5, "{what}: contrast {c:.2} < 4.5");
        }
        // Every theme accent works where lime does: as text on groups, rows and presses, and under dark text.
        for a in dvr_core::state::Accent::ALL.map(dvr_core::state::Accent::rgb) {
            for bg in [0x0B0B0C, 0x1D1D20, 0x2A2A2E, 0x36363B] {
                assert!(contrast(a, bg) >= 4.5, "accent {a:06X} on {bg:06X}");
            }
            assert!(contrast(0x0B0B0C, a) >= 7.0, "dark text on accent {a:06X}");
        }
    }

    #[test]
    fn no_text_below_24px() {
        for src in
            [include_str!("lib.rs"), include_str!("menu.rs"), include_str!("keyboard.rs"), include_str!("toolbar.rs")]
        {
            // Every `<n>px` inside each font-size expression (conditional sizes included).
            let sizes: Vec<u32> = src
                .split("font-size: ")
                .skip(1)
                .filter_map(|p| p.split(';').next())
                .flat_map(|expr| expr.split("px").filter_map(|t| t.rsplit(' ').next()?.parse().ok()))
                .collect();
            assert!(!sizes.is_empty(), "no font sizes found");
            assert!(sizes.iter().all(|&px| px >= 24), "font sizes below 24px: {sizes:?}");
        }
    }
}
