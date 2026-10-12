//! The small toolkit of the on-foot mode's procedural motion, layered over the models' clips: poses, keyframed tracks
//! and springs (`viewmodel.rs` moves the first-person rig with them).

use bevy::prelude::*;

/// A node's offset from its rest pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: Vec3,
    pub rot: Quat,
}

impl Default for Pose {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Pose {
    pub const IDENTITY: Pose = Pose { pos: Vec3::ZERO, rot: Quat::IDENTITY };

    /// Position (m) and rotation as pitch (about x), yaw (about y) and roll (about z), in degrees.
    pub fn new(pos: [f32; 3], deg: [f32; 3]) -> Self {
        let [x, y, z] = deg.map(f32::to_radians);
        Pose { pos: Vec3::from(pos), rot: Quat::from_euler(EulerRot::YXZ, y, x, z) }
    }
    pub fn lerp(self, to: Pose, t: f32) -> Pose {
        Pose { pos: self.pos.lerp(to.pos, t), rot: self.rot.slerp(to.rot, t) }
    }
    /// This pose, then `then` in its frame.
    pub fn then(self, then: Pose) -> Pose {
        Pose { pos: self.pos + self.rot * then.pos, rot: self.rot * then.rot }
    }
    pub fn transform(self) -> Transform {
        Transform::from_translation(self.pos).with_rotation(self.rot)
    }
}

/// Smooth in and out.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A keyframed animation: poses at times (s), eased between; before the first key it holds the first, after the
/// last the last.
#[derive(Clone, Copy)]
pub struct Key {
    pub t: f32,
    pub pose: Pose,
}

pub fn sample(keys: &[Key], t: f32) -> Pose {
    let Some(first) = keys.first() else { return Pose::IDENTITY };
    if t <= first.t {
        return first.pose;
    }
    for w in keys.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t <= b.t {
            let span = (b.t - a.t).max(1e-6);
            return a.pose.lerp(b.pose, ease((t - a.t) / span));
        }
    }
    keys.last().map_or(Pose::IDENTITY, |k| k.pose)
}

/// A critically damped spring toward zero, for kicks that settle (recoil, look lag, landing dip).
#[derive(Clone, Copy, Default, Debug)]
pub struct Spring {
    pub x: Vec3,
    pub v: Vec3,
}

impl Spring {
    /// Moves toward `target` with natural frequency `w` (rad/s): the exact solution, so any frame length is stable.
    pub fn step(&mut self, target: Vec3, w: f32, dt: f32) {
        let x0 = self.x - target;
        let k = self.v + w * x0;
        let e = (-w * dt).exp();
        self.x = target + (x0 + k * dt) * e;
        self.v = (self.v - w * k * dt) * e;
    }
    pub fn kick(&mut self, dv: Vec3) {
        self.v += dv;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_hold_at_the_ends_and_ease_between() {
        let keys = [Key { t: 0.2, pose: Pose::new([0.0, 0.0, 0.0], [0.0; 3]) }, Key { t: 1.2, pose: Pose::new([1.0, 0.0, 0.0], [0.0, 90.0, 0.0]) }];
        assert_eq!(sample(&keys, 0.0).pos, Vec3::ZERO);
        assert_eq!(sample(&keys, 5.0).pos, Vec3::X);
        let mid = sample(&keys, 0.7);
        assert!((mid.pos.x - 0.5).abs() < 1e-4, "{mid:?}");
        // Eased: slow at the start.
        assert!(sample(&keys, 0.3).pos.x < 0.1 * 0.5 * 3.0);
        let (yaw, _, _) = mid.rot.to_euler(EulerRot::YXZ);
        assert!((yaw.to_degrees() - 45.0).abs() < 0.5, "{}", yaw.to_degrees());
    }

    #[test]
    fn spring_settles_without_overshoot() {
        let mut s = Spring::default();
        s.kick(Vec3::X * 5.0);
        let mut peak: f32 = 0.0;
        for _ in 0..200 {
            s.step(Vec3::ZERO, 20.0, 1.0 / 60.0);
            peak = peak.max(s.x.x);
            assert!(s.x.x > -1e-3, "overshot to {}", s.x.x);
        }
        assert!(peak > 0.05 && s.x.length() < 1e-3, "peak {peak}, rest {}", s.x);
        // A long frame stays stable.
        let mut s = Spring::default();
        s.kick(Vec3::Y);
        s.step(Vec3::ZERO, 40.0, 0.5);
        assert!(s.x.length() < 1.0 && s.x.is_finite());
    }

}
