//! Lightmap charts: every visible face gets its own rectangle in one atlas, projected onto the face's plane at a fixed
//! luxel size (the classic approach — no seams inside a face, stable across rebuilds). The bake tool
//! (`tools/lightmap/bake.py`) renders the light into that atlas; the client samples it with these
//! coordinates. Packing is deterministic, so the same map always gives the same layout.

use crate::compile::{FaceKind, Level};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    /// Metres per luxel.
    pub luxel: f32,
    /// A hash of the charted geometry, so a stale bake is noticed.
    pub hash: u64,
}

/// The default resolution (metres per luxel) and atlas width; the bake tool and the client use the same.
pub const LUXEL: f32 = 0.2;
pub const ATLAS_WIDTH: u32 = 1024;

/// Empty luxels around each chart, so bilinear filtering and the bake's margin don't bleed between faces.
const PAD: u32 = 2;

struct Chart {
    /// A face, or (at `usize::MAX - i`) the i-th placed model.
    face: usize,
    origin: Vec2,
    u: Vec3,
    v: Vec3,
    w: u32,
    h: u32,
    x: u32,
    y: u32,
}

/// A placed model's chart side in luxels, when the map gives none: models are charted finer than walls (they are
/// small and seen close), from the `lmsize` key the map scripts write (the model's surface area → a square).
const PROP_SIDE_DEFAULT: u32 = 16;

/// Charts the level's faces at `luxel` metres per luxel into an atlas at most `max` wide (height grows as needed, in
/// multiples of 4). Translucent faces are skipped (they are not lightmapped). Every `prop_model` point entity gets a
/// square of `lmsize` luxels (its own lightmap UVs map into it; see [`Level::prop_charts`]).
pub fn chart(level: &mut Level, luxel: f32, max: u32) {
    let mut charts = Vec::new();
    for (i, f) in level.faces.iter().enumerate() {
        if f.kind != FaceKind::Opaque {
            continue;
        }
        let n = f.normal;
        let reference = if n.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
        let u = reference.cross(n).normalize();
        let v = n.cross(u);
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in &f.points {
            let q = Vec2::new(p.dot(u), p.dot(v));
            lo = lo.min(q);
            hi = hi.max(q);
        }
        let size = ((hi - lo) / luxel).ceil();
        let (w, h) = (size.x as u32 + 1 + 2 * PAD, size.y as u32 + 1 + 2 * PAD);
        charts.push(Chart { face: i, origin: lo, u, v, w: w.min(max), h: h.min(max), x: 0, y: 0 });
    }
    let props: Vec<usize> = level.entities.iter().enumerate().filter(|(_, e)| e.classname == "prop_model").map(|(i, _)| i).collect();
    for (k, &i) in props.iter().enumerate() {
        let side = level.entities[i].get("lmsize").and_then(|v| v.parse::<u32>().ok()).unwrap_or(PROP_SIDE_DEFAULT).clamp(4, 256);
        let w = side + 2 * PAD;
        charts.push(Chart { face: usize::MAX - k, origin: Vec2::ZERO, u: Vec3::X, v: Vec3::Y, w, h: w, x: 0, y: 0 });
    }
    // Shelf packing, tallest first (ties by face index, so the layout is stable).
    charts.sort_by(|a, b| b.h.cmp(&a.h).then(b.w.cmp(&a.w)).then(a.face.cmp(&b.face)));
    let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
    for c in &mut charts {
        if x + c.w > max {
            x = 0;
            y += shelf;
            shelf = 0;
        }
        c.x = x;
        c.y = y;
        x += c.w;
        shelf = shelf.max(c.h);
    }
    let width = max;
    let height = (y + shelf).div_ceil(4) * 4;
    let mut hash = 0xcbf29ce484222325u64;
    let mut mix = |v: f32| {
        hash ^= v.to_bits() as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    };
    level.prop_charts.clear();
    for c in &charts {
        if c.face > level.faces.len() {
            let e = props[usize::MAX - c.face];
            let (x0, y0) = ((c.x + PAD) as f32, (c.y + PAD) as f32);
            let (x1, y1) = ((c.x + c.w - PAD) as f32, (c.y + c.h - PAD) as f32);
            level.prop_charts.push(crate::compile::PropChart { entity: e, rect: [x0 / width as f32, y0 / height as f32, x1 / width as f32, y1 / height as f32] });
            let ent = &level.entities[e];
            for v in [ent.origin.x, ent.origin.y, ent.origin.z, ent.yaw, c.w as f32] {
                mix(v);
            }
            for (k, v) in &ent.props {
                if k == "model" || k == "scale" || k == "angles" {
                    for b in v.bytes() {
                        mix(b as f32);
                    }
                }
            }
            continue;
        }
        let f = &mut level.faces[c.face];
        f.lm = f
            .points
            .iter()
            .map(|p| {
                let q = (Vec2::new(p.dot(c.u), p.dot(c.v)) - c.origin) / luxel;
                // Luxel centres: the chart's first luxel centre sits at PAD + 0.5.
                Vec2::new((c.x as f32 + PAD as f32 + 0.5 + q.x) / width as f32, (c.y as f32 + PAD as f32 + 0.5 + q.y) / height as f32)
            })
            .collect();
        for p in &f.points {
            mix(p.x);
            mix(p.y);
            mix(p.z);
        }
    }
    mix(luxel);
    level.lightmap = Some(Atlas { width, height, luxel, hash });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, map};

    fn arena() -> Level {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/maps/dev_arena.map")).unwrap();
        compile::compile(&map::parse(&src).unwrap())
    }

    #[test]
    fn charts_stay_inside_the_atlas_and_do_not_overlap() {
        let mut level = arena();
        chart(&mut level, 0.2, 1024);
        let atlas = level.lightmap.unwrap();
        assert_eq!(atlas.width, 1024);
        let mut rects: Vec<(Vec2, Vec2)> = Vec::new();
        for f in level.faces.iter().filter(|f| f.kind == FaceKind::Opaque) {
            assert_eq!(f.lm.len(), f.points.len());
            let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for t in &f.lm {
                assert!(t.cmpge(Vec2::ZERO).all() && t.cmple(Vec2::ONE).all(), "{t}");
                lo = lo.min(*t);
                hi = hi.max(*t);
            }
            let px = Vec2::new(atlas.width as f32, atlas.height as f32);
            let (lo, hi) = (lo * px, hi * px);
            for (a, b) in &rects {
                let apart = hi.x <= a.x + 0.01 || lo.x >= b.x - 0.01 || hi.y <= a.y + 0.01 || lo.y >= b.y - 0.01;
                assert!(apart, "charts overlap: {lo}..{hi} vs {a}..{b}");
            }
            rects.push((lo, hi));
        }
    }

    #[test]
    fn a_luxel_is_the_requested_size() {
        let mut level = arena();
        chart(&mut level, 0.2, 1024);
        let atlas = level.lightmap.unwrap();
        let f = level.faces.iter().find(|f| f.kind == FaceKind::Opaque && f.points.len() >= 3).unwrap();
        let world = f.points[0].distance(f.points[1]);
        let d = (f.lm[0] - f.lm[1]) * Vec2::new(atlas.width as f32, atlas.height as f32);
        assert!((d.length() * 0.2 - world).abs() < 1e-3, "{} vs {world}", d.length() * 0.2);
    }

    #[test]
    fn placed_models_get_their_own_squares() {
        let mut level = arena();
        let before = {
            let mut l = arena();
            chart(&mut l, 0.2, 1024);
            l.lightmap.unwrap().hash
        };
        for (i, side) in [(0, "40"), (1, "9")] {
            level.entities.push(crate::compile::PointEntity { classname: "prop_model".into(), origin: Vec3::new(i as f32, 0.0, 0.0), yaw: 0.0, props: vec![("model".into(), "crate".into()), ("lmsize".into(), side.into())] });
        }
        chart(&mut level, 0.2, 1024);
        let atlas = level.lightmap.unwrap();
        assert_ne!(atlas.hash, before, "props change the bake");
        assert_eq!(level.prop_charts.len(), 2);
        let px = Vec2::new(atlas.width as f32, atlas.height as f32);
        for c in &level.prop_charts {
            let (lo, hi) = (Vec2::new(c.rect[0], c.rect[1]) * px, Vec2::new(c.rect[2], c.rect[3]) * px);
            let side: f32 = level.entities[c.entity].get("lmsize").unwrap().parse().unwrap();
            assert!(((hi - lo) - Vec2::splat(side)).length() < 1e-3, "{lo}..{hi}");
            for f in level.faces.iter().filter(|f| f.kind == FaceKind::Opaque) {
                let (mut flo, mut fhi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
                for t in &f.lm {
                    flo = flo.min(*t * px);
                    fhi = fhi.max(*t * px);
                }
                let apart = hi.x <= flo.x + 0.01 || lo.x >= fhi.x - 0.01 || hi.y <= flo.y + 0.01 || lo.y >= fhi.y - 0.01;
                assert!(apart, "prop square {lo}..{hi} overlaps a face {flo}..{fhi}");
            }
        }
    }

    #[test]
    fn the_layout_is_deterministic() {
        let (mut a, mut b) = (arena(), arena());
        chart(&mut a, 0.2, 1024);
        chart(&mut b, 0.2, 1024);
        assert_eq!(a.lightmap, b.lightmap);
        assert!(a.faces.iter().zip(&b.faces).all(|(x, y)| x.lm == y.lm));
    }
}
