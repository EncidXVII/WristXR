use crate::keyboard;
use crate::math::{Pose, Quat, Vec3};
use serde::{Deserialize, Serialize};

pub const DEFAULT_WIDTH_M: f32 = 1.2;
pub const DEFAULT_DISTANCE_M: f32 = 1.0;
/// Panels spawn slightly below eye line.
pub const DEFAULT_DROP_M: f32 = 0.15;
pub const KEYBOARD_WIDTH_M: f32 = 0.6;
pub const WRIST_WIDTH_M: f32 = 0.14;
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
}

/// Fixed id of the menu panel (outside the range persisted layouts use).
pub const MENU_ID: u32 = u32::MAX - 1;
pub const MENU_WIDTH_M: f32 = 0.5;
/// Fixed id of the toolbar panel.
pub const TOOLBAR_ID: u32 = u32::MAX - 2;
pub const TOOLBAR_WIDTH_M: f32 = 0.36;
/// Gap between a panel's bottom edge and its toolbar.
const TOOLBAR_GAP_M: f32 = 0.02;
/// A panel dimmed this far is hidden and ignores the laser.
pub const DIM_HIDDEN: f32 = 0.99;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor {
    World,
    Hand(Hand),
}

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
            dim: 0.0,
        }
    }

    /// Default spawn: 1.2 m wide, 1 m ahead, slightly below eye line, facing the user upright.
    pub fn in_front_of(id: u32, source: Source, head: Pose, aspect: f32) -> Self {
        let f = flat_forward(head);
        let pos = head.pos + f * DEFAULT_DISTANCE_M + Vec3::new(0.0, -DEFAULT_DROP_M, 0.0);
        Self::new(id, source, Pose::new(pos, facing(f)), DEFAULT_WIDTH_M, aspect)
    }

    /// Default keyboard: below the main panel, tilted up toward the user.
    pub fn keyboard_below(id: u32, head: Pose) -> Self {
        let f = flat_forward(head);
        let pos = head.pos + f * 0.55 + Vec3::new(0.0, -0.5, 0.0);
        let rot = facing(f) * Quat::from_axis_angle(Vec3::X, -0.6);
        let aspect = keyboard::ROWS.len() as f32 / keyboard::ROW_UNITS;
        Self::new(id, Source::Keyboard, Pose::new(pos, rot), KEYBOARD_WIDTH_M, aspect)
    }

    /// Menu: closer and smaller than a monitor panel, slightly below eye line, facing the user.
    pub fn menu_in_front(head: Pose, aspect: f32) -> Self {
        let f = flat_forward(head);
        let pos = head.pos + f * 0.6 + Vec3::new(0.0, -0.1, 0.0);
        Self::new(MENU_ID, Source::Menu, Pose::new(pos, facing(f)), MENU_WIDTH_M, aspect)
    }

    /// Watch on the given hand: behind the pointer tip, facing up out of the controller. Like a real watch, text
    /// runs along the forearm and its top points away from you when you raise the wrist across your chest.
    pub fn on_wrist(id: u32, hand: Hand, aspect: f32) -> Self {
        use std::f32::consts::FRAC_PI_2;
        let face_up = Quat::from_axis_angle(Vec3::X, -FRAC_PI_2);
        let along_arm = Quat::from_axis_angle(Vec3::Z, if hand == Hand::Left { FRAC_PI_2 } else { -FRAC_PI_2 });
        let pose = Pose::new(Vec3::new(0.0, 0.04, 0.16), face_up * along_arm);
        let mut p = Self::new(id, Source::Wrist, pose, WRIST_WIDTH_M, aspect);
        p.anchor = Anchor::Hand(hand);
        p
    }

    /// World pose for a toolbar of `aspect` centred under this panel (whose world pose is `world`), a little in
    /// front so it never hides behind the panel.
    pub fn toolbar_pose(&self, world: Pose, aspect: f32) -> Pose {
        let drop = self.height_m() * 0.5 + TOOLBAR_GAP_M + TOOLBAR_WIDTH_M * aspect * 0.5;
        Pose::new(world.transform_point(Vec3::new(0.0, -drop, 0.005)), world.rot)
    }

    /// Hand the panel is attached to (left when it is in the world).
    pub fn anchor_hand(&self) -> Hand {
        match self.anchor {
            Anchor::Hand(h) => h,
            Anchor::World => Hand::Left,
        }
    }

    /// Shown and not faded out: drawn and hit by the laser.
    pub fn shown(&self) -> bool {
        self.visible && self.dim < DIM_HIDDEN
    }

    pub fn height_m(&self) -> f32 {
        self.width_m * self.aspect
    }

    pub fn world_pose(&self, hands: &[Pose; 2]) -> Pose {
        match self.anchor {
            Anchor::World => self.pose,
            Anchor::Hand(h) => hands[h.index()] * self.pose,
        }
    }

    pub fn set_world_pose(&mut self, world: Pose, hands: &[Pose; 2]) {
        self.pose = match self.anchor {
            Anchor::World => world,
            Anchor::Hand(h) => hands[h.index()].inverse() * world,
        };
    }

    /// Point on the panel surface (panel-local) for a uv in [0,1]² (v down).
    pub fn local_point(&self, uv: [f32; 2]) -> Vec3 {
        Vec3::new((uv[0] - 0.5) * self.width_m, (0.5 - uv[1]) * self.height_m(), 0.0)
    }

    /// Ray along `ray.forward()`; returns distance and uv.
    pub fn hit(&self, world: Pose, ray: Pose) -> Option<(f32, [f32; 2])> {
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
        let uv = [p.x / self.width_m + 0.5, 0.5 - p.y / self.height_m()];
        let inside = (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]);
        inside.then_some((t, uv))
    }
}

/// How visible the watch should be (0..1): its face turned toward the eyes and the head looking at it.
pub fn gaze_visibility(head: Pose, watch: Pose) -> f32 {
    let to_eye = head.pos - watch.pos;
    let d = to_eye.length();
    if d < 1e-4 {
        return 1.0;
    }
    let to_eye = to_eye * (1.0 / d);
    let facing = watch.rot.rotate(Vec3::Z).dot(to_eye);
    let looking = head.forward().dot(to_eye * -1.0);
    smoothstep(0.35, 0.7, facing) * smoothstep(0.7, 0.88, looking)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn flat_forward(head: Pose) -> Vec3 {
    let f = head.forward();
    let f = Vec3::new(f.x, 0.0, f.z);
    if f.length() < 1e-3 { Vec3::FORWARD } else { f.normalized() }
}

fn facing(forward: Vec3) -> Quat {
    Quat::from_arc(Vec3::Z, -forward)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Left forearm raised across the chest, palm down: the hand points to the user's right (+X), the user looks
    /// down at it from above. Text must run along the arm (+X) with its top away from the user (-Z).
    #[test]
    fn watch_reads_like_a_watch_on_both_hands() {
        for (hand, along) in [(Hand::Left, Vec3::X), (Hand::Right, Vec3::X * -1.0)] {
            // Controller: -Z (laser) along the arm toward the fingers, +Y up out of the back of the hand.
            let ctrl = Pose::new(Vec3::new(0.0, 1.2, -0.3), Quat::from_arc(Vec3::FORWARD, along));
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
    fn watch_shows_only_when_looked_at() {
        let watch = Pose::new(Vec3::new(0.0, 1.2, -0.3), Quat::from_axis_angle(Vec3::X, -std::f32::consts::FRAC_PI_2));
        let eye = Vec3::new(0.0, 1.6, 0.0);
        let at = |target: Vec3| Pose::new(eye, Quat::from_arc(Vec3::FORWARD, (target - eye).normalized()));
        assert!(gaze_visibility(at(watch.pos), watch) > 0.9, "looking at the face");
        assert!(gaze_visibility(at(Vec3::new(0.0, 1.6, -2.0)), watch) < 0.01, "looking ahead");
        let turned = Pose::new(watch.pos, Quat::from_axis_angle(Vec3::X, std::f32::consts::FRAC_PI_2));
        assert!(gaze_visibility(at(watch.pos), turned) < 0.01, "face turned away");
    }
}
