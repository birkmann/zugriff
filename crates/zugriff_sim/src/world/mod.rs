//! The collision world: convex brushes in sim space (metres, Y up) and swept-box traces against them.
//!
//! Traces follow the classic brush approach: the box is folded into each plane's distance (`d + extents·|n|`), and every
//! brush carries axial and edge bevel planes so box corners don't slip into non-axial brushes. A trace returns the first
//! blocking plane, the fraction of the move that is free, and whether it started inside a brush.

pub mod winding;

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// What a brush is to movement and bullets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contents(pub u32);

impl Contents {
    pub const NONE: Contents = Contents(0);
    /// Blocks players, grenades and bullets.
    pub const SOLID: Contents = Contents(1);
    /// Blocks players only (smooths stairs, keeps players off roofs).
    pub const PLAYER_CLIP: Contents = Contents(2);
    /// Blocks players and grenades; bullets pass through (glass, fences), losing power.
    pub const WINDOW: Contents = Contents(4);
    /// Blocks bullets and grenades but not players (rare; e.g. thin grates players walk through).
    pub const SHOT_CLIP: Contents = Contents(8);

    /// What players collide with.
    pub const MASK_PLAYER: Contents = Contents(1 | 2 | 4);
    /// What stops a bullet outright (windows are handled by penetration).
    pub const MASK_SHOT: Contents = Contents(1 | 8);
    /// What bullets touch at all.
    pub const MASK_SHOT_ALL: Contents = Contents(1 | 4 | 8);
    /// What grenades bounce off.
    pub const MASK_GRENADE: Contents = Contents(1 | 4 | 8);

    pub fn intersects(self, o: Contents) -> bool {
        self.0 & o.0 != 0
    }
}

impl std::ops::BitOr for Contents {
    type Output = Contents;
    fn bitor(self, o: Contents) -> Contents {
        Contents(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub n: Vec3,
    pub d: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Side {
    pub plane: Plane,
    /// Index into [`World::surfaces`].
    pub surface: u16,
    /// Bevel planes only exist for box traces; they are never reported as the surface that was hit if a real face is.
    pub bevel: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb { min: Vec3::splat(f32::MAX), max: Vec3::splat(f32::MIN) };

    pub fn grow(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn union(a: Aabb, b: Aabb) -> Aabb {
        Aabb { min: a.min.min(b.min), max: a.max.max(b.max) }
    }

    pub fn expand(self, e: Vec3) -> Aabb {
        Aabb { min: self.min - e, max: self.max + e }
    }

    pub fn centre(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    pub fn overlaps(&self, o: &Aabb) -> bool {
        self.min.cmple(o.max).all() && self.max.cmpge(o.min).all()
    }

    /// Whether the segment `start → start + delta` touches the box (slab test).
    pub fn hit_by_segment(&self, start: Vec3, delta: Vec3) -> bool {
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for i in 0..3 {
            let (s, d, lo, hi) = (start[i], delta[i], self.min[i], self.max[i]);
            if d.abs() < 1e-9 {
                if s < lo || s > hi {
                    return false;
                }
            } else {
                let inv = 1.0 / d;
                let (mut a, mut b) = ((lo - s) * inv, (hi - s) * inv);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                t0 = t0.max(a);
                t1 = t1.min(b);
                if t0 > t1 {
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Brush {
    pub sides: Vec<Side>,
    pub bounds: Aabb,
    pub contents: Contents,
}

impl Brush {
    /// Whether `p` is inside the brush (on the surface counts as inside).
    pub fn contains(&self, p: Vec3) -> bool {
        self.bounds.contains(p) && self.sides.iter().all(|s| s.plane.n.dot(p) - s.plane.d <= 0.0)
    }
}

/// A named surface (the texture on a face); gameplay looks up its material (footsteps, penetration, impacts) by name.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Surface {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum Node {
    Leaf { bounds: Aabb, first: u32, count: u32 },
    Inner { bounds: Aabb, left: u32, right: u32 },
}

impl Node {
    fn bounds(&self) -> &Aabb {
        match self {
            Node::Leaf { bounds, .. } | Node::Inner { bounds, .. } => bounds,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct World {
    pub brushes: Vec<Brush>,
    pub surfaces: Vec<Surface>,
    /// Brush indices in BVH leaf order.
    order: Vec<u32>,
    nodes: Vec<Node>,
}

/// The result of a trace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trace {
    /// How much of the move is free, 0..=1.
    pub fraction: f32,
    /// Where the box ends up (start + delta × fraction).
    pub end: Vec3,
    /// The normal of the blocking plane (zero if nothing was hit).
    pub normal: Vec3,
    pub surface: Option<u16>,
    pub brush: Option<u32>,
    /// The box started inside a brush.
    pub start_solid: bool,
    /// The box never left a brush.
    pub all_solid: bool,
}

impl Trace {
    pub fn hit(&self) -> bool {
        self.fraction < 1.0 || self.start_solid
    }
}

/// Keeps the box this far off planes, so the next move doesn't start touching (a 1/32 inch in the classic engines).
pub const DIST_EPSILON: f32 = 0.0008;

impl World {
    pub fn new(brushes: Vec<Brush>, surfaces: Vec<Surface>) -> World {
        let mut w = World { brushes, surfaces, order: Vec::new(), nodes: Vec::new() };
        w.build_bvh();
        w
    }

    pub fn bounds(&self) -> Aabb {
        self.nodes.first().map_or(Aabb::default(), |n| *n.bounds())
    }

    fn build_bvh(&mut self) {
        self.order = (0..self.brushes.len() as u32).collect();
        self.nodes.clear();
        if self.brushes.is_empty() {
            return;
        }
        let n = self.order.len();
        self.build_node(0, n);
    }

    fn build_node(&mut self, first: usize, count: usize) -> u32 {
        let brushes = &self.brushes;
        let bounds = self.order[first..first + count]
            .iter()
            .fold(Aabb::EMPTY, |b, &i| Aabb::union(b, brushes[i as usize].bounds));
        let index = self.nodes.len() as u32;
        if count <= 4 {
            self.nodes.push(Node::Leaf { bounds, first: first as u32, count: count as u32 });
            return index;
        }
        // Split at the median of the brush centres along the longest axis.
        let size = bounds.max - bounds.min;
        let axis = if size.x >= size.y && size.x >= size.z { 0 } else if size.y >= size.z { 1 } else { 2 };
        self.order[first..first + count]
            .sort_by(|&a, &b| brushes[a as usize].bounds.centre()[axis].total_cmp(&brushes[b as usize].bounds.centre()[axis]));
        self.nodes.push(Node::Leaf { bounds, first: 0, count: 0 });
        let half = count / 2;
        let left = self.build_node(first, half);
        let right = self.build_node(first + half, count - half);
        self.nodes[index as usize] = Node::Inner { bounds, left, right };
        index
    }

    /// Sweeps an axis-aligned box (half extents `ext`, centred on the path) from `start` to `end` against brushes whose
    /// contents intersect `mask`. `ext = Vec3::ZERO` is a ray.
    pub fn trace(&self, start: Vec3, end: Vec3, ext: Vec3, mask: Contents) -> Trace {
        let mut t = Trace {
            fraction: 1.0,
            end,
            normal: Vec3::ZERO,
            surface: None,
            brush: None,
            start_solid: false,
            all_solid: false,
        };
        if self.nodes.is_empty() {
            return t;
        }
        let delta = end - start;
        let mut stack = vec![0u32];
        while let Some(i) = stack.pop() {
            let node = &self.nodes[i as usize];
            if !node.bounds().expand(ext + Vec3::splat(DIST_EPSILON)).hit_by_segment(start, delta * t.fraction.max(1e-6)) {
                continue;
            }
            match *node {
                Node::Inner { left, right, .. } => {
                    stack.push(right);
                    stack.push(left);
                }
                Node::Leaf { first, count, .. } => {
                    for &b in &self.order[first as usize..(first + count) as usize] {
                        let brush = &self.brushes[b as usize];
                        if !brush.contents.intersects(mask) {
                            continue;
                        }
                        if !brush.bounds.expand(ext + Vec3::splat(DIST_EPSILON)).hit_by_segment(start, delta * t.fraction.max(1e-6)) {
                            continue;
                        }
                        clip_to_brush(&mut t, b, brush, start, end, ext);
                        if t.all_solid {
                            t.fraction = 0.0;
                            t.end = start;
                            return t;
                        }
                    }
                }
            }
        }
        t.end = start + delta * t.fraction;
        t
    }

    /// Whether a box at `p` overlaps any brush in `mask`.
    pub fn solid_at(&self, p: Vec3, ext: Vec3, mask: Contents) -> bool {
        self.trace(p, p, ext, mask).start_solid
    }

    pub fn surface_name(&self, s: u16) -> &str {
        self.surfaces.get(s as usize).map_or("", |s| s.name.as_str())
    }

    /// How far along unit `dir` from `p` (inside or on brush `b`) the ray leaves it, and the surface it leaves by.
    pub fn exit(&self, b: u32, p: Vec3, dir: Vec3) -> Option<(f32, u16)> {
        let brush = self.brushes.get(b as usize)?;
        let mut best: Option<(f32, u16)> = None;
        for s in &brush.sides {
            let nd = s.plane.n.dot(dir);
            if nd <= 1e-6 {
                continue;
            }
            let t = (s.plane.d - s.plane.n.dot(p)) / nd;
            if best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t.max(0.0), s.surface));
            }
        }
        best
    }
}

fn clip_to_brush(t: &mut Trace, index: u32, brush: &Brush, start: Vec3, end: Vec3, ext: Vec3) {
    let mut enter = -1.0f32;
    let mut leave = 1.0f32;
    let mut clip: Option<&Side> = None;
    let mut get_out = false;
    let mut start_out = false;
    for side in &brush.sides {
        let n = side.plane.n;
        let dist = side.plane.d + ext.x * n.x.abs() + ext.y * n.y.abs() + ext.z * n.z.abs();
        let d1 = n.dot(start) - dist;
        let d2 = n.dot(end) - dist;
        if d2 > 0.0 {
            get_out = true;
        }
        if d1 > 0.0 {
            start_out = true;
        }
        // Entirely in front of a face: the move misses this brush.
        if d1 > 0.0 && (d2 >= DIST_EPSILON || d2 >= d1) {
            return;
        }
        if d1 <= 0.0 && d2 <= 0.0 {
            continue;
        }
        if d1 > d2 {
            // Entering.
            let f = ((d1 - DIST_EPSILON) / (d1 - d2)).max(0.0);
            // Prefer a real face over a bevel at the same fraction, so the surface is right.
            if f > enter || (f == enter && clip.is_some_and(|c| c.bevel) && !side.bevel) {
                enter = f;
                clip = Some(side);
            }
        } else {
            // Leaving.
            let f = ((d1 + DIST_EPSILON) / (d1 - d2)).min(1.0);
            if f < leave {
                leave = f;
            }
        }
    }
    if !start_out {
        t.start_solid = true;
        t.brush = Some(index);
        if !get_out {
            t.all_solid = true;
        }
        return;
    }
    if enter < leave && enter > -1.0 && enter < t.fraction {
        if let Some(side) = clip {
            t.fraction = enter.max(0.0);
            t.normal = side.plane.n;
            t.surface = Some(side.surface);
            t.brush = Some(index);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An axis-aligned box brush.
    pub fn cube(min: Vec3, max: Vec3, contents: Contents) -> Brush {
        let sides = [
            (Vec3::X, max.x),
            (Vec3::NEG_X, -min.x),
            (Vec3::Y, max.y),
            (Vec3::NEG_Y, -min.y),
            (Vec3::Z, max.z),
            (Vec3::NEG_Z, -min.z),
        ]
        .into_iter()
        .map(|(n, d)| Side { plane: Plane { n, d }, surface: 0, bevel: false })
        .collect();
        Brush { sides, bounds: Aabb { min, max }, contents }
    }

    fn floor_world() -> World {
        let mut brushes = vec![cube(Vec3::new(-10.0, -1.0, -10.0), Vec3::new(10.0, 0.0, 10.0), Contents::SOLID)];
        // A wall at x = 2..3 and a clip brush at x = -3..-2.
        brushes.push(cube(Vec3::new(2.0, 0.0, -10.0), Vec3::new(3.0, 3.0, 10.0), Contents::SOLID));
        brushes.push(cube(Vec3::new(-3.0, 0.0, -10.0), Vec3::new(-2.0, 3.0, 10.0), Contents::PLAYER_CLIP));
        // Many small brushes, so the BVH has several levels.
        for i in 0..40 {
            let x = -9.0 + (i % 8) as f32 * 0.5;
            let z = 5.0 + (i / 8) as f32 * 0.5;
            brushes.push(cube(Vec3::new(x, 0.0, z), Vec3::new(x + 0.2, 0.2, z + 0.2), Contents::SOLID));
        }
        World::new(brushes, vec![Surface { name: "dev/floor".into() }])
    }

    #[test]
    fn a_ray_hits_the_floor() {
        let w = floor_world();
        let t = w.trace(Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, -5.0, 0.0), Vec3::ZERO, Contents::MASK_SHOT);
        assert!(t.hit());
        assert!((t.end.y - 0.0).abs() < 0.01, "{:?}", t.end);
        assert_eq!(t.normal, Vec3::Y);
        assert_eq!(t.brush, Some(0));
    }

    #[test]
    fn a_box_stops_its_half_width_off_a_wall() {
        let w = floor_world();
        let ext = Vec3::new(0.4, 0.9, 0.4);
        let t = w.trace(Vec3::new(0.0, 1.0, 0.0), Vec3::new(5.0, 1.0, 0.0), ext, Contents::MASK_PLAYER);
        assert!((t.end.x - 1.6).abs() < 0.01, "{:?}", t.end);
        assert_eq!(t.normal, Vec3::NEG_X);
        // Resting there, the box is not stuck.
        assert!(!w.solid_at(t.end, ext, Contents::MASK_PLAYER));
    }

    #[test]
    fn player_clip_blocks_players_but_not_bullets() {
        let w = floor_world();
        let (a, b) = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(-5.0, 1.0, 0.0));
        assert!(w.trace(a, b, Vec3::splat(0.3), Contents::MASK_PLAYER).hit());
        assert!(!w.trace(a, b, Vec3::ZERO, Contents::MASK_SHOT).hit());
    }

    #[test]
    fn starting_inside_is_reported() {
        let w = floor_world();
        let t = w.trace(Vec3::new(0.0, -0.5, 0.0), Vec3::new(0.0, -0.5, 1.0), Vec3::ZERO, Contents::MASK_SHOT);
        assert!(t.start_solid && t.all_solid);
    }

    #[test]
    fn bvh_matches_brute_force() {
        let w = floor_world();
        let brute = World { order: (0..w.brushes.len() as u32).collect(), nodes: vec![Node::Leaf { bounds: w.bounds(), first: 0, count: w.brushes.len() as u32 }], ..w.clone() };
        let mut seed = 7u32;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f32 / u32::MAX as f32) * 20.0 - 10.0
        };
        for _ in 0..500 {
            let a = Vec3::new(rnd(), rnd().abs() * 0.3, rnd());
            let b = Vec3::new(rnd(), rnd().abs() * 0.3 - 0.5, rnd());
            let ext = Vec3::splat(rnd().abs() * 0.05);
            let (x, y) = (w.trace(a, b, ext, Contents::MASK_PLAYER), brute.trace(a, b, ext, Contents::MASK_PLAYER));
            assert!((x.fraction - y.fraction).abs() < 1e-5 && x.start_solid == y.start_solid, "{a} {b}: {x:?} vs {y:?}");
        }
    }
}
