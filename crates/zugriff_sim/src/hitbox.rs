//! Where a player can be hit: a sphere for the head and capsules for the chest, the stomach and the legs, placed from
//! the feet and the crouch alone. Rounds test these instead of the rendered model, so a hit is cheap, the same for
//! everyone, and easy to test.

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Standing and fully crouched heights of the body (m), matching the movement hull (72 and 54 units).
pub const STAND: f32 = 1.83;
pub const CROUCH: f32 = 1.37;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum Zone {
    Head,
    Chest,
    Stomach,
    Legs,
}

impl Zone {
    /// Damage multiplier.
    pub fn multiplier(self) -> f32 {
        match self {
            Zone::Head => 4.0,
            Zone::Chest => 1.0,
            Zone::Stomach => 1.25,
            Zone::Legs => 0.75,
        }
    }

    /// Whether a vest protects it (the head needs a helmet).
    pub fn armoured(self, helmet: bool) -> bool {
        match self {
            Zone::Head => helmet,
            Zone::Chest | Zone::Stomach => true,
            Zone::Legs => false,
        }
    }
}

pub fn height(crouch: f32) -> f32 {
    STAND + (CROUCH - STAND) * crouch.clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug)]
pub struct Hitboxes {
    head: (Vec3, f32),
    chest: (Vec3, Vec3, f32),
    stomach: (Vec3, Vec3, f32),
    legs: (Vec3, Vec3, f32),
}

impl Hitboxes {
    /// For a body at `feet`, `crouch` (0..1) down, facing `yaw` (the head sits a little forward of the spine).
    pub fn of(feet: Vec3, crouch: f32, yaw: f32) -> Hitboxes {
        let h = height(crouch);
        let fwd = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        let up = |y: f32| feet + Vec3::Y * y;
        Hitboxes {
            head: (up(h - 0.14) + fwd * 0.03, 0.115),
            chest: (up(h * 0.62), up(h - 0.36), 0.2),
            stomach: (up(h * 0.47), up(h * 0.6), 0.18),
            legs: (up(0.12), up(h * 0.46), 0.16),
        }
    }

    /// The nearest zone a ray (unit `dir`) meets within `max` metres, and how far along.
    pub fn ray(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<(Zone, f32)> {
        [
            (Zone::Head, ray_sphere(origin, dir, self.head.0, self.head.1)),
            (Zone::Chest, ray_capsule(origin, dir, self.chest.0, self.chest.1, self.chest.2)),
            (Zone::Stomach, ray_capsule(origin, dir, self.stomach.0, self.stomach.1, self.stomach.2)),
            (Zone::Legs, ray_capsule(origin, dir, self.legs.0, self.legs.1, self.legs.2)),
        ]
        .into_iter()
        .filter_map(|(z, t)| t.filter(|t| *t <= max).map(|t| (z, t)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// A cheap bound: whether the ray passes within reach of the body at all.
    pub fn near_ray(&self, origin: Vec3, dir: Vec3, max: f32) -> bool {
        let centre = (self.legs.0 + self.head.0) * 0.5;
        let reach = (self.head.0 - self.legs.0).length() * 0.5 + 0.35;
        let t = (centre - origin).dot(dir).clamp(0.0, max);
        (origin + dir * t).distance_squared(centre) <= reach * reach
    }

    pub fn head(&self) -> Vec3 {
        self.head.0
    }

    pub fn chest(&self) -> Vec3 {
        (self.chest.0 + self.chest.1) * 0.5
    }

    pub fn stomach(&self) -> Vec3 {
        (self.stomach.0 + self.stomach.1) * 0.5
    }

    /// Distance from `p` to the body's surface (0 inside), for blasts.
    pub fn distance(&self, p: Vec3) -> f32 {
        let (a, b, r) = self.chest;
        let (c, _, _) = self.legs;
        let d1 = segment_distance(p, a, b) - r;
        let d2 = segment_distance(p, c, a) - 0.2;
        d1.min(d2).max(0.0)
    }
}

fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn ray_sphere(o: Vec3, d: Vec3, c: Vec3, r: f32) -> Option<f32> {
    let oc = o - c;
    let b = oc.dot(d);
    let cc = oc.length_squared() - r * r;
    if cc < 0.0 {
        return Some(0.0);
    }
    let h = b * b - cc;
    if h < 0.0 {
        return None;
    }
    let t = -b - h.sqrt();
    (t >= 0.0).then_some(t)
}

fn ray_capsule(o: Vec3, d: Vec3, a: Vec3, b: Vec3, r: f32) -> Option<f32> {
    let ba = b - a;
    let oa = o - a;
    let baba = ba.dot(ba);
    let bard = ba.dot(d);
    let baoa = ba.dot(oa);
    let rdoa = d.dot(oa);
    let oaoa = oa.dot(oa);
    let k2 = baba - bard * bard;
    if k2.abs() > 1e-9 {
        let k1 = baba * rdoa - baoa * bard;
        let k0 = baba * oaoa - baoa * baoa - r * r * baba;
        let h = k1 * k1 - k2 * k0;
        if h < 0.0 {
            return None;
        }
        let t = (-k1 - h.sqrt()) / k2;
        let y = baoa + t * bard;
        if y > 0.0 && y < baba && t >= 0.0 {
            return Some(t);
        }
    }
    [ray_sphere(o, d, a, r), ray_sphere(o, d, b, r)].into_iter().flatten().min_by(f32::total_cmp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone_at(b: &Hitboxes, y: f32) -> Option<Zone> {
        b.ray(Vec3::new(0.0, y, 20.0), Vec3::NEG_Z, 100.0).map(|h| h.0)
    }

    #[test]
    fn zones_by_height() {
        let b = Hitboxes::of(Vec3::ZERO, 0.0, 0.0);
        assert_eq!(zone_at(&b, 1.7), Some(Zone::Head));
        assert_eq!(zone_at(&b, 1.3), Some(Zone::Chest));
        assert_eq!(zone_at(&b, 0.95), Some(Zone::Stomach));
        assert_eq!(zone_at(&b, 0.4), Some(Zone::Legs));
        assert_eq!(zone_at(&b, 2.0), None);
        assert!(b.near_ray(Vec3::new(0.0, 1.0, 20.0), Vec3::NEG_Z, 100.0));
        assert!(!b.near_ray(Vec3::new(3.0, 1.0, 20.0), Vec3::NEG_Z, 100.0));
    }

    #[test]
    fn crouching_lowers_the_head() {
        let b = Hitboxes::of(Vec3::ZERO, 1.0, 0.0);
        assert_eq!(zone_at(&b, 1.7), None);
        assert_eq!(zone_at(&b, 1.25), Some(Zone::Head));
    }
}
