use serde::{Deserialize, Serialize};
use std::f32::consts::PI;
use std::ops::{Add, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    pub const X: Self = Self::new(1.0, 0.0, 0.0);
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);
    pub const Z: Self = Self::new(0.0, 0.0, 1.0);
    /// OpenVR convention: -Z is forward.
    pub const FORWARD: Self = Self::new(0.0, 0.0, -1.0);

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Self) -> Self {
        Self::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Self {
        let l = self.length();
        if l > 0.0 { self * (1.0 / l) } else { self }
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}
impl Mul<f32> for Vec3 {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Default for Quat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Quat {
    pub const IDENTITY: Self = Self { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let a = axis.normalized();
        let (s, c) = (angle * 0.5).sin_cos();
        Self { x: a.x * s, y: a.y * s, z: a.z * s, w: c }
    }
    /// Shortest rotation taking unit vector `from` to unit vector `to`.
    pub fn from_arc(from: Vec3, to: Vec3) -> Self {
        let d = from.dot(to);
        if d < -0.999_999 {
            let mut axis = from.cross(Vec3::X);
            if axis.length() < 1e-6 {
                axis = from.cross(Vec3::Y);
            }
            return Self::from_axis_angle(axis, PI);
        }
        let c = from.cross(to);
        Self { x: c.x, y: c.y, z: c.z, w: 1.0 + d }.normalized()
    }
    pub fn conjugate(self) -> Self {
        Self { x: -self.x, y: -self.y, z: -self.z, w: self.w }
    }
    pub fn normalized(self) -> Self {
        let l = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if l > 0.0 { Self { x: self.x / l, y: self.y / l, z: self.z / l, w: self.w / l } } else { Self::IDENTITY }
    }
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let u = Vec3::new(self.x, self.y, self.z);
        let t = u.cross(v) * 2.0;
        v + t * self.w + u.cross(t)
    }
}

impl Mul for Quat {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        let a = self;
        Self {
            w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
            x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
            y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        }
    }
}

/// Rigid transform: rotate, then translate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    pub pos: Vec3,
    pub rot: Quat,
}

impl Pose {
    pub const IDENTITY: Self = Self { pos: Vec3::ZERO, rot: Quat::IDENTITY };

    pub const fn new(pos: Vec3, rot: Quat) -> Self {
        Self { pos, rot }
    }
    /// Pose at `from` whose forward (-Z) points at `at`.
    pub fn aim(from: Vec3, at: Vec3) -> Self {
        Self::new(from, Quat::from_arc(Vec3::FORWARD, (at - from).normalized()))
    }
    pub fn inverse(self) -> Self {
        let r = self.rot.conjugate();
        Self::new(-r.rotate(self.pos), r)
    }
    pub fn transform_point(self, p: Vec3) -> Vec3 {
        self.pos + self.rot.rotate(p)
    }
    pub fn forward(self) -> Vec3 {
        self.rot.rotate(Vec3::FORWARD)
    }
    /// Row-major 3x4 matrix (OpenVR `HmdMatrix34_t` layout).
    pub fn to_mat34(self) -> [[f32; 4]; 3] {
        let Quat { x, y, z, w } = self.rot;
        let p = self.pos;
        [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y), p.x],
            [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x), p.y],
            [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y), p.z],
        ]
    }
}

impl Mul for Pose {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self::new(self.transform_point(o.pos), (self.rot * o.rot).normalized())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn inverse_roundtrip() {
        let p = Pose::new(Vec3::new(1.0, 2.0, 3.0), Quat::from_axis_angle(Vec3::new(1.0, 1.0, 0.0), 0.7));
        let q = p * p.inverse();
        assert!(close(q.pos, Vec3::ZERO));
        assert!(close(q.rot.rotate(Vec3::X), Vec3::X));
    }

    #[test]
    fn aim_points_forward_at_target() {
        let p = Pose::aim(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 0.0));
        assert!(close(p.forward(), Vec3::X));
        let back = Pose::aim(Vec3::ZERO, Vec3::Z);
        assert!(close(back.forward(), Vec3::Z));
    }

    #[test]
    fn mat34_matches_rotate() {
        let p = Pose::new(Vec3::new(0.5, 0.0, -1.0), Quat::from_axis_angle(Vec3::Y, 1.0));
        let m = p.to_mat34();
        let v = Vec3::new(0.3, -0.2, 0.9);
        let r = Vec3::new(
            m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z + m[0][3],
            m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z + m[1][3],
            m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z + m[2][3],
        );
        assert!(close(r, p.transform_point(v)));
    }
}
