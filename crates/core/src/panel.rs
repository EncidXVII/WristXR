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
}

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

    /// Watch on the given hand: behind the pointer tip, facing up out of the controller, top toward the tip.
    pub fn on_wrist(id: u32, hand: Hand, aspect: f32) -> Self {
        let pose = Pose::new(Vec3::new(0.0, 0.04, 0.16), Quat::from_axis_angle(Vec3::X, -std::f32::consts::FRAC_PI_2));
        let mut p = Self::new(id, Source::Wrist, pose, WRIST_WIDTH_M, aspect);
        p.anchor = Anchor::Hand(hand);
        p
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

fn flat_forward(head: Pose) -> Vec3 {
    let f = head.forward();
    let f = Vec3::new(f.x, 0.0, f.z);
    if f.length() < 1e-3 { Vec3::FORWARD } else { f.normalized() }
}

fn facing(forward: Vec3) -> Quat {
    Quat::from_arc(Vec3::Z, -forward)
}
