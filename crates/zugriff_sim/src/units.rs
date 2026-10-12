//! Map space (inches, Z up — what TrenchBroom and the layout scripts use) versus sim space (metres, Y up, like Bevy).
//!
//! The axis change `(x, y, z) → (x, z, −y)` is a proper rotation, so plane normals and windings keep their handedness.

use glam::{DVec3, Vec3};

/// One map unit in metres (an inch): a 72-unit player hull is 1.83 m tall.
pub const METRES_PER_UNIT: f64 = 0.0254;

/// A map-space direction (unit length kept) in sim space.
pub fn dir_to_sim(v: DVec3) -> DVec3 {
    DVec3::new(v.x, v.z, -v.y)
}

/// A map-space position in sim space.
pub fn pos_to_sim(v: DVec3) -> DVec3 {
    dir_to_sim(v) * METRES_PER_UNIT
}

/// A map-space position as `f32` sim space.
pub fn to_sim(v: DVec3) -> Vec3 {
    pos_to_sim(v).as_vec3()
}

/// The yaw (radians, about sim +Y, 0 = −Z like a Bevy camera) of a map `angle` in degrees (0 = map +X, counter-clockwise
/// seen from above).
pub fn yaw_from_map_angle(degrees: f64) -> f32 {
    // Map +X is sim +X; a Bevy yaw of 0 looks down −Z, and +90° looks down −X.
    (degrees.to_radians() - std::f64::consts::FRAC_PI_2) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes() {
        assert_eq!(dir_to_sim(DVec3::Z), DVec3::Y);
        assert_eq!(dir_to_sim(DVec3::Y), DVec3::NEG_Z);
        assert!((pos_to_sim(DVec3::new(0.0, 0.0, 72.0)).y - 1.8288).abs() < 1e-9);
    }

    #[test]
    fn yaw_faces_the_map_angle() {
        // A Bevy camera with yaw θ looks along (−sin θ, 0, −cos θ).
        let look = |yaw: f32| Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        assert!(look(yaw_from_map_angle(0.0)).distance(Vec3::X) < 1e-6);
        assert!(look(yaw_from_map_angle(90.0)).distance(Vec3::NEG_Z) < 1e-6);
    }
}
