//! Convex polygons on planes (f64), for building brush faces and clipping them.

use glam::DVec3;

/// A plane `n·p = d` with the normal pointing out of the solid side's owner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DPlane {
    pub n: DVec3,
    pub d: f64,
}

impl DPlane {
    /// The plane through three points of a map face: normal = (p0 − p1) × (p2 − p1), which points out of the brush.
    pub fn from_points(p: [DVec3; 3]) -> Option<DPlane> {
        let n = (p[0] - p[1]).cross(p[2] - p[1]);
        let len = n.length();
        if len < 1e-9 {
            return None;
        }
        let n = n / len;
        Some(DPlane { n, d: n.dot(p[1]) })
    }

    pub fn distance(&self, p: DVec3) -> f64 {
        self.n.dot(p) - self.d
    }

    pub fn same(&self, o: &DPlane, eps: f64) -> bool {
        self.n.dot(o.n) > 1.0 - 1e-9 && (self.d - o.d).abs() < eps
    }
}

/// Points of a convex polygon, counter-clockwise seen from the side the plane normal points to.
pub type Winding = Vec<DVec3>;

/// A square on `plane`, `size` wide, counter-clockwise about the normal.
pub fn base_winding(plane: &DPlane, size: f64) -> Winding {
    let n = plane.n;
    // A reference axis not parallel to the normal.
    let up = if n.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
    let right = up.cross(n).normalize();
    let up = n.cross(right);
    let c = n * plane.d;
    // right × up = n, so r, u, −r, −u runs counter-clockwise about n.
    vec![c - right * size - up * size, c + right * size - up * size, c + right * size + up * size, c - right * size + up * size]
}

/// Splits a winding by a plane into (front, back) parts; either may be empty. Points within `eps` count as on the plane
/// and go to both sides.
pub fn split(w: &Winding, plane: &DPlane, eps: f64) -> (Winding, Winding) {
    let dists: Vec<f64> = w.iter().map(|p| plane.distance(*p)).collect();
    let side = |d: f64| if d > eps { 1 } else if d < -eps { -1 } else { 0 };
    let (mut front, mut back) = (Vec::new(), Vec::new());
    if dists.iter().all(|&d| side(d) >= 0) && dists.iter().any(|&d| side(d) > 0) {
        return (w.clone(), Vec::new());
    }
    if dists.iter().all(|&d| side(d) <= 0) && dists.iter().any(|&d| side(d) < 0) {
        return (Vec::new(), w.clone());
    }
    if dists.iter().all(|&d| side(d) == 0) {
        // Coplanar: the caller decides; report it as neither.
        return (Vec::new(), Vec::new());
    }
    for i in 0..w.len() {
        let (p, q) = (w[i], w[(i + 1) % w.len()]);
        let (dp, dq) = (dists[i], dists[(i + 1) % w.len()]);
        let (sp, sq) = (side(dp), side(dq));
        if sp >= 0 {
            front.push(p);
        }
        if sp <= 0 {
            back.push(p);
        }
        if (sp > 0 && sq < 0) || (sp < 0 && sq > 0) {
            let t = dp / (dp - dq);
            let m = p + (q - p) * t;
            front.push(m);
            back.push(m);
        }
    }
    (front, back)
}

/// Keeps the part of `w` behind `plane` (the inside of a brush). Coplanar windings are kept.
pub fn clip_back(w: &Winding, plane: &DPlane, eps: f64) -> Winding {
    if w.iter().all(|p| plane.distance(*p).abs() <= eps) {
        return w.clone();
    }
    split(w, plane, eps).1
}

pub fn area(w: &Winding) -> f64 {
    if w.len() < 3 {
        return 0.0;
    }
    let mut a = DVec3::ZERO;
    for i in 1..w.len() - 1 {
        a += (w[i] - w[0]).cross(w[i + 1] - w[0]);
    }
    a.length() * 0.5
}

pub fn normal(w: &Winding) -> DVec3 {
    let mut a = DVec3::ZERO;
    for i in 1..w.len().saturating_sub(1) {
        a += (w[i] - w[0]).cross(w[i + 1] - w[0]);
    }
    a.normalize_or_zero()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_winding_faces_the_normal() {
        for n in [DVec3::X, DVec3::NEG_Y, DVec3::Z, DVec3::new(1.0, 2.0, -3.0).normalize()] {
            let w = base_winding(&DPlane { n, d: 5.0 }, 100.0);
            assert!(normal(&w).dot(n) > 0.999, "{n}");
            assert!(w.iter().all(|p| (p.dot(n) - 5.0).abs() < 1e-9));
        }
    }

    #[test]
    fn split_halves_a_square() {
        let w = base_winding(&DPlane { n: DVec3::Z, d: 0.0 }, 10.0);
        let (f, b) = split(&w, &DPlane { n: DVec3::X, d: 0.0 }, 1e-6);
        assert!((area(&f) - 200.0).abs() < 1e-6);
        assert!((area(&b) - 200.0).abs() < 1e-6);
        assert!(normal(&f).dot(DVec3::Z) > 0.999 && normal(&b).dot(DVec3::Z) > 0.999);
    }
}
