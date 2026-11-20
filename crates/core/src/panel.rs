// Copyright (C) 2026 EncidXVII
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::keyboard;
use crate::math::{Pose, Quat, Vec3};
use serde::{Deserialize, Serialize};

pub const DEFAULT_WIDTH_M: f32 = 1.2;
pub const DEFAULT_DISTANCE_M: f32 = 1.2;
/// Panels spawn slightly below eye line.
pub const DEFAULT_DROP_M: f32 = 0.15;
pub const KEYBOARD_WIDTH_M: f32 = 0.6;
pub const WRIST_WIDTH_M: f32 = 0.14;
/// Distance from the watch's anchor point to its top edge.
const WRIST_TOP_M: f32 = 0.049;
pub const MIN_WIDTH_M: f32 = 0.1;
pub const MAX_WIDTH_M: f32 = 10.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hand {
    Left = 0,
    #[default]
    Right = 1,
}

impl Hand {
    pub const BOTH: [Hand; 2] = [Hand::Left, Hand::Right];
    pub fn other(self) -> Self {
        match self {
            Hand::Left => Hand::Right,
            Hand::Right => Hand::Left,
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Monitor(u32),
    Window(u64),
    Keyboard,
    Wrist,
    /// The in-VR menu; never persisted.
    Menu,
    /// Per-panel toolbar shown under the panel being pointed at; never persisted.
    Toolbar,
    /// Notification banner low in your view; never persisted.
    Banner,
}

/// Fixed id of the menu panel (outside the range persisted layouts use).
pub const MENU_ID: u32 = u32::MAX - 1;
pub const MENU_WIDTH_M: f32 = 0.5;
/// Fixed id of the toolbar panel.
pub const TOOLBAR_ID: u32 = u32::MAX - 2;
/// Fixed id of the notification banner (above the toolbar's, so it is never persisted).
pub const BANNER_ID: u32 = u32::MAX;
/// The banner's width; its texture keeps the toolbar's 0.5 mm a pixel.
pub const BANNER_WIDTH_M: f32 = 0.44;
/// How far ahead of the eyes the banner rides, and how far below where you look (radians).
const BANNER_DISTANCE_M: f32 = 0.9;
const BANNER_DROP_RAD: f32 = 0.35;
pub const TOOLBAR_WIDTH_M: f32 = 0.63;
/// Gap between a panel's bottom edge and its toolbar.
const TOOLBAR_GAP_M: f32 = 0.02;
/// A panel dimmed this far is hidden and ignores the laser.
pub const DIM_HIDDEN: f32 = 0.99;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor {
    World,
    Hand(Hand),
    /// Follows the headset (a HUD).
    Head,
}

/// Poses panels can be anchored to: left hand, right hand, head.
pub type Anchors = [Pose; 3];
/// Index of the head in `Anchors`.
pub const HEAD: usize = 2;
/// Screens and windows start to fade out when the head is this close to them...
pub const NEAR_FADE_M: f32 = 0.3;
/// ...and are gone this close (the headset's near clip would cut them a little nearer).
pub const NEAR_GONE_M: f32 = 0.08;

/// A flat quad. `pose` is relative to `anchor`; local +Z faces the viewer, +X right, +Y up.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    pub id: u32,
    pub source: Source,
    pub anchor: Anchor,
    pub pose: Pose,
    pub width_m: f32,
    /// height / width
    pub aspect: f32,
    pub curvature: f32,
    pub opacity: f32,
    pub locked: bool,
    pub visible: bool,
    /// Free angle (toolbar): held, it keeps the turn you give it with the controller instead of turning to face you,
    /// level.
    #[serde(default)]
    pub free_turn: bool,
    /// Runtime fade (0 shown .. 1 hidden): hide-all and the wrist's look-to-show. Never persisted.
    #[serde(skip)]
    pub dim: f32,
}

/// Desktop rectangle in physical pixels (virtual-desktop coordinates).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl PixelRect {
    pub fn pixel_at(self, uv: [f32; 2]) -> (i32, i32) {
        let axis = |t: f32, len: u32| ((t * len as f32) as i32).clamp(0, len as i32 - 1);
        (self.x + axis(uv[0], self.w), self.y + axis(uv[1], self.h))
    }
}

impl Panel {
    pub fn new(id: u32, source: Source, pose: Pose, width_m: f32, aspect: f32) -> Self {
        Self {
            id,
            source,
            anchor: Anchor::World,
            pose,
            width_m,
            aspect,
            curvature: 0.0,
            opacity: 1.0,
            locked: false,
            visible: true,
            free_turn: false,
            dim: 0.0,
        }
    }

    /// Default spawn: 1.2 m wide, 1 m ahead, slightly below eye line, facing the user upright.
    pub fn in_front_of(id: u32, source: Source, head: Pose, aspect: f32) -> Self {
        let f = flat_forward(head);
        let pos = head.pos + f * DEFAULT_DISTANCE_M + Vec3::new(0.0, -DEFAULT_DROP_M, 0.0);
        Self::new(id, source, Pose::new(pos, facing(f)), DEFAULT_WIDTH_M, aspect)
    }

    /// Puts the panel in the room `yaw` radians to the right of where the head looks, `distance` away, slightly
    /// below eye line, facing the user upright (layout reset arranges screens in an arc this way).
    pub fn place_at(&mut self, head: Pose, yaw: f32, distance: f32) {
        let f = Quat::from_axis_angle(Vec3::Y, -yaw).rotate(flat_forward(head));
        self.anchor = Anchor::World;
        self.pose = Pose::new(head.pos + f * distance + Vec3::new(0.0, -DEFAULT_DROP_M, 0.0), facing(f));
    }

    /// Default keyboard: below the main panel, tilted up toward the user.
    pub fn keyboard_below(id: u32, head: Pose) -> Self {
        let f = flat_forward(head);
        let pos = head.pos + f * 0.66 + Vec3::new(0.0, -0.5, 0.0);
        let rot = facing(f) * Quat::from_axis_angle(Vec3::X, -0.6);
        let aspect = keyboard::ROWS.len() as f32 / keyboard::ROW_UNITS;
        Self::new(id, Source::Keyboard, Pose::new(pos, rot), KEYBOARD_WIDTH_M, aspect)
    }

    /// Menu: closer and smaller than a monitor panel, slightly below eye line, facing the user.
    pub fn menu_in_front(head: Pose, aspect: f32) -> Self {
        let f = flat_forward(head);
        let pos = head.pos + f * 0.72 + Vec3::new(0.0, -0.1, 0.0);
        Self::new(MENU_ID, Source::Menu, Pose::new(pos, facing(f)), MENU_WIDTH_M, aspect)
    }

    /// Watch on the given hand: behind the pointer tip, on the back of the hand and facing out of it, where a real
    /// watch sits. The controller's top (+Y, under the thumb) is a quarter turn around the forearm from there: a
    /// watch on top faced your chest when you turned the wrist to read it. Like a real watch, text runs along the
    /// forearm and its top points away from you when you raise the wrist across your chest, palm down.
    pub fn on_wrist(id: u32, hand: Hand, aspect: f32) -> Self {
        use std::f32::consts::FRAC_PI_2;
        let turn = if hand == Hand::Left { FRAC_PI_2 } else { -FRAC_PI_2 };
        let face_up = Quat::from_axis_angle(Vec3::X, -FRAC_PI_2);
        let along_arm = Quat::from_axis_angle(Vec3::Z, turn);
        // From the top of the controller round to the back of the hand (left: -X, right: +X).
        let to_back = Quat::from_axis_angle(Vec3::Z, turn);
        let rot = to_back * face_up * along_arm;
        // The top edge stays put whatever the height: the watch grows downward (toward the elbow) when it opens.
        let drop = WRIST_WIDTH_M * aspect * 0.5 - WRIST_TOP_M;
        let pose = Pose::new(to_back.rotate(Vec3::new(0.0, 0.04, 0.16)) + rot.rotate(Vec3::new(0.0, -drop, 0.0)), rot);
        let mut p = Self::new(id, Source::Wrist, pose, WRIST_WIDTH_M, aspect);
        p.anchor = Anchor::Hand(hand);
        p
    }

    /// The notification banner: on the headset, 0.9 m ahead and about 20 degrees below where you look, tilted to
    /// face the eyes. `shown` (0..1) is how far it has come in: on the way it rises a few centimetres and grows from
    /// 96 % as it fades in, and goes back down the same way.
    pub fn banner(aspect: f32, shown: f32) -> Self {
        let rise = (1.0 - shown) * 0.03;
        let (s, c) = BANNER_DROP_RAD.sin_cos();
        let pos = Vec3::new(0.0, -BANNER_DISTANCE_M * s - rise, -BANNER_DISTANCE_M * c);
        let rot = Quat::from_axis_angle(Vec3::X, -BANNER_DROP_RAD);
        let mut p =
            Self::new(BANNER_ID, Source::Banner, Pose::new(pos, rot), BANNER_WIDTH_M * (0.96 + 0.04 * shown), aspect);
        p.anchor = Anchor::Head;
        p.locked = true;
        p.dim = 1.0 - shown;
        p.visible = shown > 0.01;
        p
    }

    /// World pose for a toolbar of `aspect` centred under this panel (whose world pose is `world`), a little in
    /// front so it never hides behind the panel.
    /// The keyboard's toolbar sits above it instead: below would be in your lap.
    pub fn toolbar_pose(&self, world: Pose, aspect: f32) -> Pose {
        let offset = self.height_m() * 0.5 + TOOLBAR_GAP_M + TOOLBAR_WIDTH_M * aspect * 0.5;
        let y = if self.source == Source::Keyboard { offset } else { -offset };
        Pose::new(world.transform_point(Vec3::new(0.0, y, 0.005)), world.rot)
    }

    /// Hand the panel is attached to (left when it is in the world).
    pub fn anchor_hand(&self) -> Hand {
        match self.anchor {
            Anchor::Hand(h) => h,
            Anchor::World | Anchor::Head => Hand::Left,
        }
    }

    /// Shown and not faded out: drawn and hit by the laser.
    pub fn shown(&self) -> bool {
        self.visible && self.dim < DIM_HIDDEN
    }

    pub fn height_m(&self) -> f32 {
        self.width_m * self.aspect
    }

    pub fn world_pose(&self, anchors: &Anchors) -> Pose {
        match self.anchor {
            Anchor::World => self.pose,
            Anchor::Hand(h) => anchors[h.index()] * self.pose,
            Anchor::Head => anchors[HEAD] * self.pose,
        }
    }

    pub fn set_world_pose(&mut self, world: Pose, anchors: &Anchors) {
        self.pose = match self.anchor {
            Anchor::World => world,
            Anchor::Hand(h) => anchors[h.index()].inverse() * world,
            Anchor::Head => anchors[HEAD].inverse() * world,
        };
    }

    /// Point on the panel surface (panel-local) for a uv in [0,1]² (v down).
    pub fn local_point(&self, uv: [f32; 2]) -> Vec3 {
        Vec3::new((uv[0] - 0.5) * self.width_m, (0.5 - uv[1]) * self.height_m(), 0.0)
    }

    /// How much of the panel shows with the eyes at `eye` (0..1): screens, windows and their toolbar fade out as
    /// the head comes within `NEAR_FADE_M` of them, and are gone at `NEAR_GONE_M`, instead of being sliced off
    /// by the headset's near clip as you lean into them.
    pub fn near_fade(&self, world: Pose, eye: Vec3) -> f32 {
        // A head-locked panel keeps its distance from the eyes: it is never leaned into.
        if !matches!(self.source, Source::Monitor(_) | Source::Window(_) | Source::Toolbar)
            || self.anchor == Anchor::Head
        {
            return 1.0;
        }
        let local = world.inverse().transform_point(eye);
        let (hw, hh) = (self.width_m * 0.5, self.height_m() * 0.5);
        let nearest = Vec3::new(local.x.clamp(-hw, hw), local.y.clamp(-hh, hh), 0.0);
        let t = ((local - nearest).length() - NEAR_GONE_M) / (NEAR_FADE_M - NEAR_GONE_M);
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Ray along `ray.forward()`; returns distance and uv.
    pub fn hit(&self, world: Pose, ray: Pose) -> Option<(f32, [f32; 2])> {
        let (t, uv) = self.plane_hit(world, ray)?;
        let inside = (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]);
        inside.then_some((t, uv))
    }

    /// The ray runs through the gap between this panel and its toolbar (`bar_aspect`: the toolbar's height / width),
    /// or over either: the toolbar must stay while the laser travels from one to the other. Gives how far along the
    /// ray that is.
    pub fn toolbar_bridge(&self, world: Pose, ray: Pose, bar_aspect: f32) -> Option<f32> {
        let (t, [u, v]) = self.plane_hit(world, ray)?;
        let half = TOOLBAR_WIDTH_M.max(self.width_m) * 0.5 / self.width_m + 0.03;
        let reach = (TOOLBAR_GAP_M + TOOLBAR_WIDTH_M * bar_aspect) / self.height_m() + 0.02;
        let along = if self.source == Source::Keyboard { -reach..=1.0 } else { 0.0..=1.0 + reach };
        ((u - 0.5).abs() <= half && along.contains(&v)).then_some(t)
    }

    /// Where the ray meets the panel's plane: distance along the ray and UV (outside 0..1 off the panel).
    fn plane_hit(&self, world: Pose, ray: Pose) -> Option<(f32, [f32; 2])> {
        let inv = world.inverse();
        let o = inv.transform_point(ray.pos);
        let d = inv.rot.rotate(ray.forward());
        if d.z.abs() < 1e-6 {
            return None;
        }
        let t = -o.z / d.z;
        if t <= 0.0 {
            return None;
        }
        let p = o + d * t;
        Some((t, [p.x / self.width_m + 0.5, 0.5 - p.y / self.height_m()]))
    }
}

/// Whether the watch shows: its face turned toward the eyes and the eyes on it. Wide to show (a glance from the
/// corner of the eye is enough), wider still to stay, and shown for at least `GLANCE_HOLD_S`, so hand tremor and
/// head bob at the edge never make it blink.
#[derive(Clone, Copy, Debug, Default)]
pub struct Glance {
    shown: bool,
    held_s: f32,
}

/// Shows when the wrist is turned to within about 50 degrees of the eyes, the face within about 66 degrees of them
/// and the watch within 52 degrees of where the head looks; stays until those pass about 61, 81 and 70 degrees.
/// The wrist's turn is its roll around the forearm alone; the face and look angles are what they always were, so
/// where the arm and head are counts as before and only the wrist has to turn less far.
const GLANCE_SHOW: Gaze = Gaze { roll: 0.64, facing: 0.4, looking: 0.62 };
const GLANCE_KEEP: Gaze = Gaze { roll: 0.48, facing: 0.15, looking: 0.34 };
const GLANCE_HOLD_S: f32 = 0.3;

/// Cosines, 1 = dead on: the wrist's roll toward the eyes, the face toward the eyes, the watch toward the middle of
/// the view.
#[derive(Clone, Copy, Debug)]
struct Gaze {
    roll: f32,
    facing: f32,
    looking: f32,
}

impl Gaze {
    fn passes(self, at_least: Self) -> bool {
        self.roll > at_least.roll && self.facing > at_least.facing && self.looking > at_least.looking
    }
}

impl Glance {
    /// Steps by `dt` seconds; true while the watch should show.
    pub fn update(&mut self, head: Pose, watch: Pose, dt: f32) -> bool {
        let g = gaze(head, watch);
        self.held_s += dt;
        let want = g.passes(if self.shown { GLANCE_KEEP } else { GLANCE_SHOW });
        if want != self.shown && (want || self.held_s >= GLANCE_HOLD_S) {
            self.shown = want;
            self.held_s = 0.0;
        }
        self.shown
    }
}

/// The roll is measured around the forearm (the watch's text runs along it), so only rolling the wrist changes it.
/// Looking straight down the forearm, the face angle stands in.
fn gaze(head: Pose, watch: Pose) -> Gaze {
    let to_eye = head.pos - watch.pos;
    let d = to_eye.length();
    if d < 1e-4 {
        return Gaze { roll: 1.0, facing: 1.0, looking: 1.0 };
    }
    let to_eye = to_eye * (1.0 / d);
    let arm = watch.rot.rotate(Vec3::X);
    let across = to_eye - arm * to_eye.dot(arm);
    let toward = if across.length() > 0.2 { across.normalized() } else { to_eye };
    let normal = watch.rot.rotate(Vec3::Z);
    Gaze { roll: normal.dot(toward), facing: normal.dot(to_eye), looking: head.forward().dot(to_eye * -1.0) }
}

/// The head frame turned only about the vertical: where "in front of the user" is.
pub fn yaw_frame(head: Pose) -> Pose {
    Pose::new(head.pos, facing(flat_forward(head)))
}

fn flat_forward(head: Pose) -> Vec3 {
    let f = head.forward();
    let f = Vec3::new(f.x, 0.0, f.z);
    if f.length() < 1e-3 { Vec3::FORWARD } else { f.normalized() }
}

fn facing(forward: Vec3) -> Quat {
    Quat::from_arc(Vec3::Z, -forward)
}

/// Turn for a panel at `at` that faces the eyes at `eye` and stays level: turned about the vertical toward
/// them, then tilted up or down toward them, never rolled. Held panels take this, so a twist of the wrist never
/// tips a screen over.
pub fn face_eyes(at: Vec3, eye: Vec3) -> Quat {
    level_facing(at - eye)
}

/// Level turn for a panel seen along `away` (from the eyes toward the panel).
pub fn level_facing(away: Vec3) -> Quat {
    if away.length() < 1e-4 {
        return Quat::IDENTITY;
    }
    let d = away.normalized();
    let flat = Vec3::new(d.x, 0.0, d.z);
    let flat = if flat.length() < 1e-3 { Vec3::FORWARD } else { flat.normalized() };
    facing(flat) * Quat::from_axis_angle(Vec3::X, d.y.clamp(-1.0, 1.0).asin())
}

/// Widest a panel held in a hand gets (a tablet, not a wall).
pub const IN_HAND_WIDTH_M: f32 = 0.5;
/// A held panel let go this close to the other hand stays on that hand.
pub const ATTACH_REACH_M: f32 = 0.25;

impl Panel {
    /// Puts the panel in `hand` like a tablet held up: its bottom edge a little above the controller, facing the
    /// eyes, no wider than `IN_HAND_WIDTH_M`. From then on it moves with that hand.
    pub fn hold_in(&mut self, hand: Hand, anchors: &Anchors) {
        self.width_m = self.width_m.min(IN_HAND_WIDTH_M);
        let at = anchors[hand.index()].pos + Vec3::new(0.0, 0.06 + self.height_m() * 0.5, 0.0);
        self.anchor = Anchor::Hand(hand);
        self.set_world_pose(Pose::new(at, face_eyes(at, anchors[HEAD].pos)), anchors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_turn_of_the_wrist_decides_the_glance() {
        let eye = Pose::new(Vec3::new(0.0, 1.6, 0.0), Quat::from_axis_angle(Vec3::X, -0.9));
        let at = Vec3::new(0.0, 1.2, -0.3);
        // Forearm along X, face toward the eyes.
        let base = Quat::from_arc(Vec3::Z, (eye.pos - at).normalized());
        let shows = |rot: Quat| {
            let mut g = Glance::default();
            (0..30).fold(false, |_, _| g.update(eye, Pose::new(at, rot), 0.016))
        };
        assert!(shows(base), "facing");
        assert!(shows(base * Quat::from_axis_angle(Vec3::X, 0.7)), "wrist turned 40 degrees");
        assert!(!shows(base * Quat::from_axis_angle(Vec3::X, 0.96)), "wrist turned 55 degrees");
        // The rest is as it always was: the face within 66 degrees of the eyes, whatever the wrist.
        assert!(shows(base * Quat::from_axis_angle(Vec3::Y, 1.05)), "forearm tipped 60 degrees, wrist not turned");
        assert!(!shows(base * Quat::from_axis_angle(Vec3::Y, 1.22)), "forearm tipped 70 degrees");
    }

    #[test]
    fn a_held_panel_faces_the_eyes_level() {
        let eye = Vec3::new(0.0, 1.6, 0.0);
        for at in [Vec3::new(0.0, 1.6, -1.0), Vec3::new(1.0, 2.4, -1.0), Vec3::new(-0.5, 0.9, -0.6)] {
            let rot = face_eyes(at, eye);
            let normal = rot.rotate(Vec3::Z);
            assert!(normal.dot((eye - at).normalized()) > 0.9999, "faces the eyes from {at:?}");
            assert!(rot.rotate(Vec3::X).y.abs() < 1e-4, "never rolled at {at:?}");
        }
    }

    #[test]
    fn put_in_a_hand_it_sits_above_the_controller() {
        let hand = Pose::new(Vec3::new(-0.2, 1.1, -0.4), Quat::from_axis_angle(Vec3::Z, 1.0));
        let anchors = [hand, Pose::IDENTITY, Pose::new(Vec3::new(0.0, 1.6, 0.0), Quat::IDENTITY)];
        let mut p = Panel::in_front_of(1, Source::Monitor(0), anchors[HEAD], 16.0 / 9.0);
        p.hold_in(Hand::Left, &anchors);
        assert_eq!(p.anchor, Anchor::Hand(Hand::Left));
        assert!(p.width_m <= IN_HAND_WIDTH_M);
        let w = p.world_pose(&anchors);
        assert!(w.pos.y - p.height_m() * 0.5 > hand.pos.y, "above the hand");
        assert!((w.pos.x - hand.pos.x).abs() < 1e-4);
    }

    /// Left forearm raised across the chest, palm down: the hand points to the user's right (+X), the user looks
    /// down at it from above. Text must run along the arm (+X) with its top away from the user (-Z), the face up
    /// out of the back of the hand.
    #[test]
    fn watch_reads_like_a_watch_on_both_hands() {
        use std::f32::consts::FRAC_PI_2;
        for (hand, along, roll) in [(Hand::Left, Vec3::X, -FRAC_PI_2), (Hand::Right, Vec3::X * -1.0, FRAC_PI_2)] {
            // Controller as gripped: -Z (laser) along the arm toward the fingers, its top (+Y, under the thumb)
            // toward the user; the back of the hand faces up.
            let rot = Quat::from_arc(Vec3::FORWARD, along) * Quat::from_axis_angle(Vec3::Z, roll);
            assert!(rot.rotate(Vec3::Y).dot(Vec3::Z) > 0.99, "{hand:?}: thumb toward the user");
            let ctrl = Pose::new(Vec3::new(0.0, 1.2, -0.3), rot);
            let w = Panel::on_wrist(1, hand, 0.5);
            let world = ctrl * w.pose;
            let text_dir = world.rot.rotate(Vec3::X);
            let text_up = world.rot.rotate(Vec3::Y);
            let normal = world.rot.rotate(Vec3::Z);
            let reading = if hand == Hand::Left { along } else { along * -1.0 };
            assert!(text_dir.dot(reading) > 0.99, "{hand:?}: text runs {text_dir:?}");
            assert!(text_up.dot(Vec3::new(0.0, 0.0, -1.0)) > 0.99, "{hand:?}: top points {text_up:?}");
            assert!(normal.dot(Vec3::Y) > 0.99, "{hand:?}: faces {normal:?}");
        }
    }

    #[test]
    fn watch_top_edge_stays_put_when_it_grows() {
        let top = |aspect: f32| {
            let w = Panel::on_wrist(1, Hand::Left, aspect);
            w.pose.transform_point(Vec3::new(0.0, w.height_m() * 0.5, 0.0))
        };
        assert!((top(0.7) - top(1.36)).length() < 1e-5);
    }

    #[test]
    fn watch_shows_only_when_looked_at_and_never_blinks() {
        let watch = Pose::new(Vec3::new(0.0, 1.2, -0.3), Quat::from_axis_angle(Vec3::X, -std::f32::consts::FRAC_PI_2));
        let eye = Vec3::new(0.0, 1.6, 0.0);
        let at = |target: Vec3| Pose::new(eye, Quat::from_arc(Vec3::FORWARD, (target - eye).normalized()));
        let ahead = at(Vec3::new(0.0, 1.6, -2.0));
        let mut g = Glance::default();
        assert!(!g.update(ahead, watch, 0.016), "looking ahead");
        assert!(g.update(at(watch.pos), watch, 0.016), "looking at the face");
        // Looking 50 degrees past it (outside "show", inside "keep") does not hide it.
        let side = at(watch.pos + Vec3::new(0.0, 0.0, -0.45));
        assert!(g.update(side, watch, 1.0), "still within keep");
        // Looking away (up, well past "keep") hides it, but not before the hold.
        let away = at(Vec3::new(0.0, 2.6, -1.0));
        let mut quick = Glance::default();
        quick.update(at(watch.pos), watch, 0.016);
        assert!(quick.update(away, watch, 0.1), "held");
        assert!(!quick.update(away, watch, 0.3), "then hidden");
        let turned = Pose::new(watch.pos, Quat::from_axis_angle(Vec3::X, std::f32::consts::FRAC_PI_2));
        assert!(!Glance::default().update(at(watch.pos), turned, 0.016), "face turned away");
    }

    #[test]
    fn toolbar_stays_while_the_laser_crosses_the_gap() {
        let screen = Panel::new(5, Source::Monitor(0), Pose::IDENTITY, 1.0, 0.5);
        let bar = 84.0 / 1260.0;
        let ray = |y: f32| Pose::new(Vec3::new(0.0, y, 1.0), Quat::default());
        assert!(screen.toolbar_bridge(Pose::IDENTITY, ray(-0.26), bar).is_some(), "just below the screen, in the gap");
        assert!(screen.toolbar_bridge(Pose::IDENTITY, ray(-0.29), bar).is_some(), "on the toolbar");
        assert!(screen.toolbar_bridge(Pose::IDENTITY, ray(-0.5), bar).is_none(), "well below");
        assert!(screen.toolbar_bridge(Pose::IDENTITY, ray(0.3), bar).is_none(), "above");
    }

    #[test]
    fn a_screen_fades_as_the_head_leans_into_it() {
        let screen = Panel::new(5, Source::Monitor(0), Pose::IDENTITY, 1.0, 0.5);
        let at = |z: f32| screen.near_fade(Pose::IDENTITY, Vec3::new(0.1, 0.0, z));
        assert_eq!(at(0.6), 1.0);
        assert_eq!(at(NEAR_FADE_M), 1.0);
        assert!(at(0.2) > 0.2 && at(0.2) < 0.8, "half way: {}", at(0.2));
        assert!(at(0.15) < at(0.2), "fades on the way in");
        assert_eq!(at(NEAR_GONE_M), 0.0);
        // Beside it (past its edge) counts the distance to the edge.
        assert_eq!(screen.near_fade(Pose::IDENTITY, Vec3::new(1.0, 0.0, 0.05)), 1.0);
        let keys = Panel::new(6, Source::Keyboard, Pose::IDENTITY, 1.0, 0.5);
        assert_eq!(keys.near_fade(Pose::IDENTITY, Vec3::new(0.0, 0.0, 0.05)), 1.0, "the keyboard stays");
        let hud = Panel { anchor: Anchor::Head, ..screen };
        assert_eq!(hud.near_fade(Pose::IDENTITY, Vec3::new(0.0, 0.0, 0.2)), 1.0, "a head-locked panel stays");
    }

    #[test]
    fn the_banner_sits_low_in_view_and_faces_the_eyes() {
        let b = Panel::banner(0.26, 1.0);
        assert_eq!(b.anchor, Anchor::Head);
        let to_eye = (Vec3::new(0.0, 0.0, 0.0) - b.pose.pos).normalized();
        assert!(b.pose.rot.rotate(Vec3::Z).dot(to_eye) > 0.9999, "faces the eyes");
        let below = (-b.pose.pos.y / -b.pose.pos.z).atan();
        assert!((below - 0.35).abs() < 1e-3, "about 20 degrees down");
        let coming = Panel::banner(0.26, 0.0);
        assert!(!coming.visible && coming.pose.pos.y < b.pose.pos.y && coming.width_m < b.width_m);
    }

    #[test]
    fn yaw_frame_turns_only_about_the_vertical() {
        let head = Pose::new(Vec3::new(1.0, 1.6, 0.0), Quat::from_axis_angle(Vec3::X, -0.5));
        let f = yaw_frame(head);
        assert!((f.forward() - Vec3::FORWARD).length() < 1e-4);
        assert!((f.pos - head.pos).length() < 1e-6);
    }
}
