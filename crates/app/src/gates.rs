//! AUTO GATES that need no VR runtime. Shared by `cargo test` and `--selftest`.
use crate::bindings::{FILES, MANIFEST};
use dvr_core::chat::{ChatAction, ChatLine};
use dvr_core::engine::{Engine, Event, FrameInput, HandInput, MouseButton};
use dvr_core::keyboard::{self, LCTRL, LSHIFT};
use dvr_core::math::{Pose, Quat, Vec3};
use dvr_core::panel::{Anchor, Hand, Panel, PixelRect, Source};
use dvr_core::state::State;
use serde_json::Value;
use std::f32::consts::{FRAC_1_SQRT_2, FRAC_PI_2};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

pub type GateResult = Result<(), String>;
pub type Gate = (&'static str, fn() -> GateResult);

pub const GATES: &[Gate] = &[
    ("1 panels", panels),
    ("3 pointer", pointer),
    ("4 keyboard", keyboard_typing),
    ("4 osc chatbox", osc_chatbox),
    ("4 chat mode", chat_mode),
    ("4b controls", controls),
    ("5b grab + pop-out", grab),
    ("5c switch-over import", switch_over_import),
    ("6 notifications", notifications),
    ("C update check", update_check),
];

/// 60 characters, typed with alternating hands.
pub const TYPING_TEXT: &str = "pack my box with five dozen liquor jugs then type a bit more";

const CONTROLLERS: &[&str] =
    &["knuckles", "oculus_touch", "vive_controller", "holographic_controller", "frame_controller"];

macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {{
        let ok: bool = $cond;
        if !ok {
            return Err(format!($($msg)+));
        }
    }};
}

pub(crate) const HEAD: Pose = Pose::new(Vec3::new(0.0, 1.6, 0.0), Quat::IDENTITY);
const ORIGIN: [Vec3; 2] = [Vec3::new(-0.2, 1.3, -0.2), Vec3::new(0.2, 1.3, -0.2)];
const FRAME_MS: u64 = 11;

/// Scripted test channel: injects poses and buttons, records every event.
pub(crate) struct Rig {
    pub(crate) engine: Engine,
    pub(crate) now: u64,
    pub(crate) hands: [HandInput; 2],
    log: Vec<Event>,
    buf: Vec<Event>,
}

impl Rig {
    pub(crate) fn new(panels: Vec<Panel>) -> Self {
        let hand = |o: Vec3| HandInput { pose: Pose::aim(o, o + Vec3::FORWARD), tracked: true, ..HandInput::default() };
        Self {
            engine: Engine::new(panels),
            now: 0,
            hands: [hand(ORIGIN[0]), hand(ORIGIN[1])],
            log: Vec::new(),
            buf: Vec::with_capacity(64),
        }
    }
    pub(crate) fn hand(&mut self, h: Hand) -> &mut HandInput {
        &mut self.hands[h.index()]
    }
    fn panel(&self, id: u32) -> Result<Panel, String> {
        self.engine.panels.iter().find(|p| p.id == id).copied().ok_or_else(|| format!("no panel {id}"))
    }
    /// Point the hand's laser at a uv on a panel, keeping the hand's position.
    pub(crate) fn aim(&mut self, h: Hand, panel: u32, uv: [f32; 2]) -> GateResult {
        let poses = [self.hands[0].pose, self.hands[1].pose];
        let p = self.panel(panel)?;
        let target = p.world_pose(&poses).transform_point(p.local_point(uv));
        let from = self.hands[h.index()].pose.pos;
        self.hand(h).pose = Pose::aim(from, target);
        Ok(())
    }
    pub(crate) fn frames(&mut self, n: u64) {
        for _ in 0..n {
            self.now += FRAME_MS;
            self.buf.clear();
            self.engine.step(&FrameInput { now_ms: self.now, hands: self.hands }, &mut self.buf);
            self.log.extend_from_slice(&self.buf);
        }
    }
    pub(crate) fn hold_ms(&mut self, ms: u64) {
        self.frames(ms.div_ceil(FRAME_MS));
    }
    pub(crate) fn tap(&mut self, h: Hand) {
        self.hand(h).trigger = true;
        self.frames(1);
        self.hand(h).trigger = false;
        self.frames(1);
    }
    pub(crate) fn take(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.log)
    }
}

type MouseEv = (u32, [f32; 2], MouseButton, bool);

pub(crate) fn mouse(events: &[Event]) -> Vec<MouseEv> {
    events
        .iter()
        .filter_map(|e| match *e {
            Event::Mouse { panel, uv, button, down } => Some((panel, uv, button, down)),
            _ => None,
        })
        .collect()
}

pub(crate) fn keys(events: &[Event]) -> Vec<(u16, bool)> {
    events
        .iter()
        .filter_map(|e| match *e {
            Event::Key { scancode, down } => Some((scancode, down)),
            _ => None,
        })
        .collect()
}

fn close(a: [f32; 2], b: [f32; 2], tol: f32) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn panels() -> GateResult {
    let p = Panel::in_front_of(1, Source::Monitor(0), HEAD, 9.0 / 16.0);
    ensure!((p.width_m - 1.2).abs() < 1e-6, "default width {}", p.width_m);
    ensure!((p.pose.pos - Vec3::new(0.0, 1.45, -1.0)).length() < 1e-4, "default position {:?}", p.pose.pos);
    ensure!((p.pose.rot.rotate(Vec3::Z) - Vec3::Z).length() < 1e-4, "panel must face the user");

    let turned = Pose::new(HEAD.pos, Quat::from_axis_angle(Vec3::Y, FRAC_PI_2));
    let q = Panel::in_front_of(2, Source::Monitor(1), turned, 9.0 / 16.0);
    ensure!((q.pose.pos - Vec3::new(-1.0, 1.45, 0.0)).length() < 1e-4, "turned-head spawn {:?}", q.pose.pos);

    let kb = Panel::keyboard_below(3, HEAD);
    ensure!(kb.pose.pos.y < p.pose.pos.y - p.height_m() / 2.0, "keyboard must sit below the panel");

    let hand = Pose::new(Vec3::new(0.3, 1.0, -0.3), Quat::from_axis_angle(Vec3::Y, 0.5));
    let poses = [hand, Pose::IDENTITY];
    let mut w = p;
    w.anchor = Anchor::Hand(Hand::Left);
    let world = Pose::new(Vec3::new(0.0, 1.2, -0.5), Quat::IDENTITY);
    w.set_world_pose(world, &poses);
    ensure!((w.world_pose(&poses).pos - world.pos).length() < 1e-4, "hand anchor roundtrip");
    let moved = [Pose::new(hand.pos + Vec3::X, hand.rot), Pose::IDENTITY];
    ensure!((w.world_pose(&moved).pos - (world.pos + Vec3::X)).length() < 1e-4, "attached panel must follow the hand");

    let dir = std::env::temp_dir().join(format!("dvr-selftest-{}", std::process::id()));
    let path = dir.join("state.json");
    let state = State { panels: vec![p, q, kb, w], chatbox: true, ..State::default() };
    state.save(&path).map_err(err)?;
    let loaded = State::load(&path);
    std::fs::write(&path, b"{ not json").map_err(err)?;
    let corrupt = State::load(&path);
    // Best effort: a leftover temp dir is harmless.
    let _ = std::fs::remove_dir_all(&dir);
    ensure!(loaded == state, "state did not survive a restart");
    ensure!(corrupt == State::default(), "a corrupt state file must fall back to defaults");
    ensure!(state.next_id() == 4, "next id {}", state.next_id());
    Ok(())
}

fn pointer() -> GateResult {
    let at = |x: f32| Pose::new(Vec3::new(x, 1.5, -1.0), Quat::IDENTITY);
    let mut rig = Rig::new(vec![
        Panel::new(1, Source::Monitor(0), at(-0.65), 1.2, 9.0 / 16.0),
        Panel::new(2, Source::Monitor(1), at(0.65), 1.2, 9.0 / 16.0),
    ]);
    let r = Hand::Right;

    rig.aim(r, 1, [0.25, 0.75])?;
    rig.frames(30);
    let ev = rig.take();
    ensure!(mouse(&ev).is_empty(), "hover produced a click");
    ensure!(
        ev.iter()
            .any(|e| matches!(e, Event::Hover { panel: 1, uv, cursor: true, .. } if close(*uv, [0.25, 0.75], 1e-3))),
        "hover uv/cursor wrong"
    );
    // Input exclusivity: only a hand whose laser is on a panel is taken from the game.
    let up = rig.hands[0].pose.pos + Vec3::new(0.0, 2.0, 0.0);
    rig.hand(Hand::Left).pose = Pose::aim(rig.hands[0].pose.pos, up);
    rig.frames(1);
    ensure!(rig.engine.engaged(r) && !rig.engine.engaged(Hand::Left), "engaged: only the hand on a panel");
    rig.take();

    // 100 %, 150 %, 200 % scaled monitors, one left of the primary (negative origin).
    let rects = [
        PixelRect { x: 0, y: 0, w: 2560, h: 1440 },
        PixelRect { x: 2560, y: 0, w: 3840, h: 2160 },
        PixelRect { x: -5120, y: -200, w: 5120, h: 2880 },
    ];
    for (panel, uv) in [(1, [0.25, 0.75]), (2, [0.8125, 0.125]), (1, [0.6, 0.4])] {
        rig.aim(r, panel, uv)?;
        rig.frames(2);
        rig.take();
        rig.tap(r);
        let m = mouse(&rig.take());
        ensure!(
            m.len() == 2 && m[0].0 == panel && m[0].2 == MouseButton::Left && m[0].3 && !m[1].3,
            "click on panel {panel}: {m:?}"
        );
        for rect in rects {
            let (x, y) = rect.pixel_at(m[0].1);
            let (ex, ey) = rect.pixel_at(uv);
            ensure!((x - ex).abs() <= 1 && (y - ey).abs() <= 1, "pixel ({x},{y}) != ({ex},{ey}) on {rect:?}");
        }
        rig.hold_ms(500);
    }

    rig.aim(r, 1, [0.5, 0.5])?;
    rig.frames(1);
    rig.tap(r);
    rig.aim(r, 1, [0.505, 0.5])?;
    rig.frames(1);
    rig.tap(r);
    let m = mouse(&rig.take());
    ensure!(m.len() == 4 && m[2].1 == m[0].1, "double click must reuse the first point: {m:?}");

    rig.hold_ms(500);
    rig.hand(r).trigger = true;
    rig.hold_ms(700);
    rig.hand(r).trigger = false;
    rig.frames(1);
    let m = mouse(&rig.take());
    ensure!(m.len() == 2 && m.iter().all(|e| e.2 == MouseButton::Right), "long press: {m:?}");

    rig.aim(r, 1, [0.3, 0.3])?;
    rig.frames(1);
    rig.hand(r).trigger = true;
    rig.frames(1);
    for s in 1..=10 {
        let t = 0.3 + 0.03 * s as f32;
        rig.aim(r, 1, [t, t])?;
        rig.frames(1);
    }
    rig.hand(r).trigger = false;
    rig.frames(1);
    let m = mouse(&rig.take());
    ensure!(
        m.len() == 2 && m[0].3 && close(m[0].1, [0.3, 0.3], 1e-3) && !m[1].3 && close(m[1].1, [0.6, 0.6], 1e-3),
        "drag: {m:?}"
    );

    rig.aim(r, 2, [0.5, 0.5])?;
    rig.frames(1);
    rig.take();
    rig.hand(r).stick = [0.0, 1.0];
    rig.hold_ms(500);
    rig.hand(r).stick = [0.0; 2];
    let ev = rig.take();
    let notches: i32 = ev
        .iter()
        .filter_map(|e| match *e {
            Event::Scroll { panel: 2, notches, .. } => Some(notches),
            _ => None,
        })
        .sum();
    ensure!((5..=7).contains(&notches), "scroll notches {notches}");
    ensure!(mouse(&ev).is_empty(), "scroll must not click");

    let l = Hand::Left;
    for _ in 0..2 {
        rig.hand(l).menu = true;
        rig.frames(1);
        rig.hand(l).menu = false;
        rig.frames(3);
    }
    let toggles = rig.take().iter().filter(|e| matches!(e, Event::MenuToggle)).count();
    ensure!(toggles == 1, "menu double-tap toggles {toggles}");
    Ok(())
}

fn xorshift(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed as f32 / u32::MAX as f32 * 2.0 - 1.0
}

pub(crate) fn key_sc(label: &str) -> Result<u16, String> {
    keyboard::key_for_label(label).map(|k| k.sc).ok_or_else(|| format!("no key {label:?}"))
}

pub(crate) fn press_key(rig: &mut Rig, h: Hand, sc: u16) -> GateResult {
    let (center, _) = keyboard::key_rect(sc).ok_or_else(|| format!("no rect for {sc:#x}"))?;
    rig.aim(h, 1, center)?;
    rig.frames(1);
    rig.tap(h);
    Ok(())
}

fn keyboard_typing() -> GateResult {
    ensure!(TYPING_TEXT.chars().count() == 60, "typing text must be 60 characters");
    let mut rig = Rig::new(vec![Panel::keyboard_below(1, HEAD)]);
    let mut seed = 0x2545_F491;
    let mut expected = Vec::new();
    for (i, c) in TYPING_TEXT.chars().enumerate() {
        let sc = key_sc(&c.to_string())?;
        let (center, half) = keyboard::key_rect(sc).ok_or_else(|| format!("no rect for {c:?}"))?;
        // Aim anywhere within the inner 40 % of the key: realistic laser jitter.
        let uv = [center[0] + half[0] * 0.4 * xorshift(&mut seed), center[1] + half[1] * 0.4 * xorshift(&mut seed)];
        let h = if i % 2 == 0 { Hand::Left } else { Hand::Right };
        rig.aim(h, 1, uv)?;
        rig.frames(1);
        rig.tap(h);
        expected.push(sc);
    }
    let typed: Vec<u16> = keys(&rig.take()).into_iter().filter(|k| k.1).map(|k| k.0).collect();
    let missed = expected.iter().zip(&typed).filter(|(a, b)| a != b).count() + expected.len().abs_diff(typed.len());
    ensure!(missed == 0, "two-hand typing missed {missed} of {} keys", expected.len());

    for (m, k, msc) in [("Shift", "a", LSHIFT), ("Ctrl", "c", LCTRL), ("Ctrl", "v", LCTRL)] {
        let ksc = key_sc(k)?;
        press_key(&mut rig, Hand::Left, key_sc(m)?)?;
        press_key(&mut rig, Hand::Right, ksc)?;
        let got = keys(&rig.take());
        ensure!(got == [(msc, true), (ksc, true), (ksc, false), (msc, false)], "{m}+{k}: {got:?}");
        ensure!(rig.engine.keyboard().latched() == 0, "modifier latch must clear after use");
    }

    let x = key_sc("x")?;
    let (center, _) = keyboard::key_rect(x).ok_or("no rect for x")?;
    rig.aim(Hand::Right, 1, center)?;
    rig.frames(1);
    rig.hand(Hand::Right).trigger = true;
    rig.hold_ms(700);
    rig.hand(Hand::Right).trigger = false;
    rig.frames(1);
    let repeats = keys(&rig.take()).iter().filter(|k| **k == (x, true)).count();
    ensure!(repeats >= 5, "key repeat produced {repeats} presses in 700 ms");
    Ok(())
}

/// Mock VRChat: a local UDP socket plus a chatbox client pointed at it.
fn mock_vrchat() -> Result<(UdpSocket, dvr_osc::Chatbox), String> {
    let server = UdpSocket::bind("127.0.0.1:0").map_err(err)?;
    server.set_read_timeout(Some(Duration::from_secs(2))).map_err(err)?;
    let chat = dvr_osc::Chatbox::new(server.local_addr().map_err(err)?).map_err(err)?;
    Ok((server, chat))
}

/// Next `/chatbox/input` text the mock received.
fn received_chat(server: &UdpSocket) -> Result<String, String> {
    let mut buf = [0u8; 2048];
    let n = server.recv(&mut buf).map_err(err)?;
    let (_, packet) = rosc::decoder::decode_udp(&buf[..n]).map_err(|e| format!("{e:?}"))?;
    match packet {
        rosc::OscPacket::Message(m) if m.addr == "/chatbox/input" => match m.args.first() {
            Some(rosc::OscType::String(s)) => Ok(s.clone()),
            other => Err(format!("first arg {other:?}")),
        },
        other => Err(format!("unexpected packet {other:?}")),
    }
}

fn osc_chatbox() -> GateResult {
    let (server, chat) = mock_vrchat()?;
    let long = "x".repeat(200);
    for (text, want) in [("hello vr", 8), (long.as_str(), dvr_osc::CHATBOX_MAX_CHARS)] {
        chat.send(text).map_err(err)?;
        let s = received_chat(&server)?;
        ensure!(s.chars().count() == want && text.starts_with(s.as_str()), "chatbox text {s:?}");
    }
    Ok(())
}

/// US layout stand-in for the OS layout lookup.
pub(crate) fn us_char(sc: u16, shift: bool, caps: bool) -> Option<char> {
    let key = keyboard::ROWS.iter().flat_map(|r| r.iter()).find(|k| k.sc == sc && k.label.chars().count() == 1)?;
    let c = key.label.chars().next()?;
    Some(if shift ^ caps { c.to_ascii_uppercase() } else { c })
}

/// Chat key toggles chat mode without touching the OS; typed text reaches VRChat on Enter.
fn chat_mode() -> GateResult {
    let (server, chatbox) = mock_vrchat()?;
    let mut rig = Rig::new(vec![Panel::keyboard_below(1, HEAD)]);
    press_key(&mut rig, Hand::Right, keyboard::CHAT)?;
    let ev = rig.take();
    let toggles = ev.iter().filter(|e| matches!(e, Event::ChatToggle)).count();
    ensure!(toggles == 1 && keys(&ev).is_empty(), "Chat key: {toggles} toggles, keys {:?}", keys(&ev));
    for (i, label) in ["Shift", "h", "i", "Bksp", "Enter"].iter().enumerate() {
        let h = if i % 2 == 0 { Hand::Left } else { Hand::Right };
        press_key(&mut rig, h, key_sc(label)?)?;
    }
    let mut line = ChatLine::default();
    let mut sent = None;
    for (sc, down) in keys(&rig.take()) {
        if let ChatAction::Send(text) = line.key(sc, down, us_char) {
            chatbox.send(&text).map_err(err)?;
            sent = Some(text);
        }
    }
    ensure!(sent.as_deref() == Some("H"), "chat line sent {sent:?}");
    let got = received_chat(&server)?;
    ensure!(got == "H", "VRChat received {got:?}");
    Ok(())
}

fn collect_outputs<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                match (k.as_str(), x.as_str()) {
                    ("output", Some(s)) => out.push(s),
                    _ => collect_outputs(x, out),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_outputs(x, out)),
        _ => {}
    }
}

fn controls() -> GateResult {
    let manifest: Value = serde_json::from_str(MANIFEST).map_err(err)?;
    let actions: Vec<&str> = manifest["actions"]
        .as_array()
        .ok_or("manifest has no actions")?
        .iter()
        .filter_map(|a| a["name"].as_str())
        .collect();
    ensure!(actions.len() == 6, "expected 6 actions, got {}", actions.len());
    let defaults = manifest["default_bindings"].as_array().ok_or("manifest has no default_bindings")?;
    for ctype in CONTROLLERS {
        let url = defaults
            .iter()
            .find(|b| b["controller_type"] == *ctype)
            .and_then(|b| b["binding_url"].as_str())
            .ok_or_else(|| format!("no default binding for {ctype}"))?;
        let text = FILES.iter().find(|(n, _)| *n == url).map(|(_, t)| *t).ok_or_else(|| format!("missing {url}"))?;
        let binding: Value = serde_json::from_str(text).map_err(|e| format!("{url}: {e}"))?;
        ensure!(binding["controller_type"] == *ctype, "{url} has the wrong controller_type");
        let mut outputs = Vec::new();
        collect_outputs(&binding, &mut outputs);
        for a in &actions {
            ensure!(outputs.contains(a), "{ctype} does not bind {a}");
        }
    }
    Ok(())
}

fn grab() -> GateResult {
    let start = Pose::new(Vec3::new(0.0, 1.4, -1.0), Quat::IDENTITY);
    let mut rig = Rig::new(vec![Panel::new(1, Source::Monitor(0), start, 1.0, 0.5)]);
    let (l, r) = (Hand::Left, Hand::Right);

    rig.aim(r, 1, [0.5, 0.5])?;
    rig.hand(r).grip = true;
    rig.frames(1);
    ensure!(rig.engine.grabbed() == Some(1), "grip did not grab");
    let offset = Vec3::new(0.3, 0.1, 0.0);
    rig.hand(r).pose.pos = rig.hands[1].pose.pos + offset;
    rig.frames(1);
    let p = rig.panel(1)?;
    ensure!((p.pose.pos - (start.pos + offset)).length() < 1e-3, "one-hand move {:?}", p.pose.pos);
    ensure!(mouse(&rig.take()).is_empty(), "grip must never click");

    let before = (p.pose.pos - rig.hands[1].pose.pos).length();
    rig.hand(r).stick = [0.0, 1.0];
    rig.hold_ms(500);
    rig.hand(r).stick = [0.0; 2];
    let after = (rig.panel(1)?.pose.pos - rig.hands[1].pose.pos).length();
    ensure!((after - before - 0.75).abs() < 0.1, "stick push {before:.2} -> {after:.2} m");
    rig.hand(r).grip = false;
    rig.frames(1);
    ensure!(rig.engine.grabbed().is_none(), "grip release must drop the panel");

    rig.engine.panels[0].pose = start;
    rig.hand(l).pose.pos = Vec3::new(-0.2, 1.4, -0.2);
    rig.hand(r).pose.pos = Vec3::new(0.2, 1.4, -0.2);
    rig.aim(l, 1, [0.3, 0.5])?;
    rig.aim(r, 1, [0.7, 0.5])?;
    rig.hand(r).grip = true;
    rig.frames(1);
    rig.hand(l).grip = true;
    rig.frames(1);
    // Double the hand distance and roll 45°: width x2, panel X axis follows the hands.
    let v = Vec3::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0) * 0.8;
    let mid = Vec3::new(0.0, 1.4, -0.2);
    rig.hand(l).pose.pos = mid - v * 0.5;
    rig.hand(r).pose.pos = mid + v * 0.5;
    rig.frames(1);
    let p = rig.panel(1)?;
    ensure!((p.width_m - 2.0).abs() < 1e-3, "two-hand scale {}", p.width_m);
    ensure!((p.pose.rot.rotate(Vec3::X) - v.normalized()).length() < 1e-3, "two-hand rotation");
    ensure!((p.pose.pos - Vec3::new(0.0, 1.4, -1.8)).length() < 1e-3, "two-hand position {:?}", p.pose.pos);
    rig.hand(l).grip = false;
    rig.hand(r).grip = false;
    rig.frames(1);
    ensure!(rig.engine.grabbed().is_none(), "two-hand release");

    rig.engine.panels[0].locked = true;
    let locked = rig.engine.panels[0];
    rig.aim(r, 1, [0.5, 0.5])?;
    rig.hand(r).grip = true;
    rig.frames(1);
    rig.hand(r).pose.pos = rig.hands[1].pose.pos + Vec3::X;
    rig.frames(1);
    ensure!(rig.engine.grabbed().is_none() && rig.engine.panels[0] == locked, "locked panel moved");
    rig.hand(r).grip = false;
    rig.frames(1);

    // Pop-out: a quick yank toward the user (25 cm in ~55 ms) emits PopOut and leaves the monitor in place.
    rig.engine.panels[0] = Panel::new(1, Source::Monitor(0), start, 1.0, 0.5);
    rig.hand(r).pose.pos = Vec3::new(0.2, 1.4, -0.2);
    rig.aim(r, 1, [0.3, 0.4])?;
    rig.take();
    rig.hand(r).grip = true;
    rig.frames(1);
    let toward = -rig.hands[1].pose.forward();
    for _ in 0..5 {
        rig.hand(r).pose.pos = rig.hands[1].pose.pos + toward * 0.05;
        rig.frames(1);
    }
    rig.hand(r).grip = false;
    rig.frames(1);
    let ev = rig.take();
    let pop = ev.iter().find_map(|e| match *e {
        Event::PopOut { hand, panel, uv } => Some((hand, panel, uv)),
        _ => None,
    });
    ensure!(
        pop.is_some_and(|(h, p, uv)| h == r && p == 1 && close(uv, [0.3, 0.4], 1e-3)),
        "yank must pop out the window under the laser: {pop:?}"
    );
    ensure!((rig.panel(1)?.pose.pos - start.pos).length() < 1e-4, "monitor panel must stay where it was");

    // A slow pull is an ordinary move.
    rig.hand(r).pose.pos = Vec3::new(0.2, 1.4, -0.2);
    rig.aim(r, 1, [0.3, 0.4])?;
    rig.hand(r).grip = true;
    rig.frames(1);
    let toward = -rig.hands[1].pose.forward();
    for _ in 0..100 {
        rig.hand(r).pose.pos = rig.hands[1].pose.pos + toward * 0.0025;
        rig.frames(1);
    }
    rig.hand(r).grip = false;
    rig.frames(1);
    let ev = rig.take();
    ensure!(!ev.iter().any(|e| matches!(e, Event::PopOut { .. })), "slow pull must not pop out");
    ensure!((rig.panel(1)?.pose.pos - start.pos).length() > 0.2, "slow pull must move the panel");
    Ok(())
}

fn notifications() -> GateResult {
    let mut listener = dvr_notify_api::Listener::bind(0).map_err(err)?;
    let port = listener.port().map_err(err)?;
    let client = UdpSocket::bind("127.0.0.1:0").map_err(err)?;
    let msg = br#"{"messageType":1,"index":0,"timeout":4.0,"height":175.0,"opacity":1.0,"volume":0.7,"audioPath":"default","title":"Selftest","content":"Hello from a compatible app","useBase64Icon":false,"icon":"default","sourceApp":"selftest"}"#;
    client.send_to(b"garbage", ("127.0.0.1", port)).map_err(err)?;
    client.send_to(msg, ("127.0.0.1", port)).map_err(err)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(t) = listener.poll() {
            ensure!(
                t.title == "Selftest"
                    && t.content == "Hello from a compatible app"
                    && t.timeout_s == 4.0
                    && t.source_app == "selftest",
                "toast {t:?}"
            );
            return Ok(());
        }
        ensure!(Instant::now() < deadline, "no toast received");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Fixture written from the observed XSOverlay settings format (only the keys we import).
pub(crate) const XSOVERLAY_FIXTURE: &str = include_str!("../fixtures/xsoverlay_overlaySettings.json");

fn switch_over_import() -> GateResult {
    use crate::import::{Imported, apply, xsoverlay};
    let imp = xsoverlay(XSOVERLAY_FIXTURE).ok_or("fixture did not parse")?;
    let want =
        Imported { dominant: Some(Hand::Left), typing_sounds: Some(true), curved: Some(true), opacity: Some(0.85) };
    ensure!(imp == want, "imported {imp:?}");
    let mut state = State::default();
    apply(&mut state, &imp);
    ensure!(
        state.dominant == Hand::Left && state.typing_sounds && state.default_curved && state.default_opacity == 0.85,
        "applied {state:?}"
    );
    ensure!(xsoverlay("{ not json").is_none(), "garbage must not import");
    ensure!(xsoverlay("{}") == Some(Imported::default()), "missing keys import nothing");
    Ok(())
}

fn update_check() -> GateResult {
    use crate::update::{newer, parse};
    ensure!(newer("v1.2.10", "1.2.9") && !newer("1.2.9", "1.2.9") && !newer("v1.1.0", "1.2.0"), "version compare");
    let json = r#"{"tag_name":"v9.0.0","html_url":"https://github.com/o/r/releases/tag/v9.0.0","assets":[]}"#;
    let r = parse(json, "0.1.0").ok_or("newer release not offered")?;
    ensure!(r.version == "9.0.0" && r.url.starts_with("https://github.com/"), "release {r:?}");
    ensure!(parse(json, "9.0.0").is_none() && parse("not json", "0.1.0").is_none(), "no offer when current or bad");
    Ok(())
}
