//! Compiles a parsed `.map` into a [`Level`]: collision brushes (with bevels), the visible faces with hidden parts cut
//! away, point entities and trigger volumes (buy zones, bomb sites, places …).
//!
//! Brush entities by class:
//! - `worldspawn`, `func_detail`, `func_wall`: solid and drawn; they hide faces of other brushes they cover.
//! - `func_illusionary`: drawn only.
//! - `func_glass`, `func_window`: drawn, block players and grenades, bullets pass (with penetration loss).
//! - `func_buyzone`, `func_bomb_target`, `func_hostage_rescue`, `func_ladder`, `zg_place`, `trigger_*`: volumes.
//!
//! Tool textures: `tools/clip` (player clip brush), `tools/nodraw` (solid, not drawn), `tools/skip`, `tools/hint`
//! (neither), `tools/trigger` (for volumes; ignored on solid brushes).

use crate::map::{BrushDef, FaceDef, MapFile};
use crate::units::{self, METRES_PER_UNIT};
use crate::world::winding::{self, DPlane, Winding};
use crate::world::{Aabb, Brush, Contents, Plane, Side, Surface, World};
use glam::{DVec3, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// Map-space tolerance (units).
const EPS: f64 = 0.01;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderFace {
    pub surface: u16,
    /// Sim space, counter-clockwise seen from the front.
    pub points: Vec<Vec3>,
    pub normal: Vec3,
    /// Texture coordinates in texels (divide by the texture size).
    pub uv: Vec<Vec2>,
    /// The entity the face belongs to (0 = worldspawn).
    pub entity: u32,
    pub kind: FaceKind,
    /// Lightmap atlas coordinates (0..1), filled by [`crate::lightmap::chart`].
    pub lm: Vec<Vec2>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FaceKind {
    Opaque,
    /// Glass and other see-through brushes.
    Translucent,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PointEntity {
    pub classname: String,
    pub origin: Vec3,
    /// About +Y; 0 looks down −Z.
    pub yaw: f32,
    pub props: Vec<(String, String)>,
}

impl PointEntity {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.props.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Volume {
    pub classname: String,
    pub props: Vec<(String, String)>,
    pub brushes: Vec<Brush>,
    pub bounds: Aabb,
}

impl Volume {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.props.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn contains(&self, p: Vec3) -> bool {
        self.bounds.contains(p) && self.brushes.iter().any(|b| b.contains(p))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Level {
    pub world: World,
    pub faces: Vec<RenderFace>,
    pub entities: Vec<PointEntity>,
    pub volumes: Vec<Volume>,
    pub warnings: Vec<String>,
    /// The lightmap atlas the faces' `lm` coordinates point into, once charted.
    pub lightmap: Option<crate::lightmap::Atlas>,
    /// Each placed model's (`prop_model`) square in the lightmap atlas, once charted; the model's second UV set
    /// (0..1) maps into it.
    #[serde(default)]
    pub prop_charts: Vec<PropChart>,
}

/// A `prop_model`'s square in the lightmap atlas.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropChart {
    /// Index into [`Level::entities`].
    pub entity: usize,
    /// Atlas coordinates (0..1): min x, min y, max x, max y.
    pub rect: [f32; 4],
}

impl Level {
    pub fn entities_of<'a>(&'a self, classname: &'a str) -> impl Iterator<Item = &'a PointEntity> + 'a {
        self.entities.iter().filter(move |e| e.classname == classname)
    }

    pub fn volumes_of<'a>(&'a self, classname: &'a str) -> impl Iterator<Item = &'a Volume> + 'a {
        self.volumes.iter().filter(move |v| v.classname == classname)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Solid,
    Illusionary,
    Window,
    Volume,
}

fn role(classname: &str) -> Option<Role> {
    Some(match classname {
        "worldspawn" | "func_detail" | "func_wall" => Role::Solid,
        "func_illusionary" => Role::Illusionary,
        "func_glass" | "func_window" => Role::Window,
        "func_buyzone" | "func_bomb_target" | "func_hostage_rescue" | "func_ladder" | "zg_place" => Role::Volume,
        c if c.starts_with("trigger_") => Role::Volume,
        _ => return None,
    })
}

fn is_tool(tex: &str) -> bool {
    tex.to_ascii_lowercase().starts_with("tools/")
}

fn drawn(tex: &str) -> bool {
    !is_tool(tex)
}

/// A brush's faces as map-space windings (one per distinct plane that has area).
struct Built<'a> {
    faces: Vec<(&'a FaceDef, DPlane, Winding)>,
    bounds_map: (DVec3, DVec3),
}

fn build_brush(def: &BrushDef) -> Option<Built<'_>> {
    let mut planes: Vec<(&FaceDef, DPlane)> = Vec::new();
    for f in &def.faces {
        let p = DPlane::from_points(f.points)?;
        if !planes.iter().any(|(_, q)| q.same(&p, EPS)) {
            planes.push((f, p));
        }
    }
    let mut faces = Vec::new();
    let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
    for (i, (f, p)) in planes.iter().enumerate() {
        let mut w = winding::base_winding(p, 1.0e5);
        for (j, (_, q)) in planes.iter().enumerate() {
            if i != j && !w.is_empty() {
                w = winding::split(&w, q, EPS).1;
            }
        }
        if w.len() >= 3 && winding::area(&w) > EPS {
            for v in &w {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
            faces.push((*f, *p, w));
        }
    }
    (faces.len() >= 4).then_some(Built { faces, bounds_map: (lo, hi) })
}

fn texel(f: &FaceDef, p: DVec3) -> Vec2 {
    let u = DVec3::new(f.u[0], f.u[1], f.u[2]);
    let v = DVec3::new(f.v[0], f.v[1], f.v[2]);
    let sx = if f.scale[0].abs() < 1e-9 { 1.0 } else { f.scale[0] };
    let sy = if f.scale[1].abs() < 1e-9 { 1.0 } else { f.scale[1] };
    Vec2::new((p.dot(u) / sx + f.u[3]) as f32, (p.dot(v) / sy + f.v[3]) as f32)
}

fn sim_plane(p: &DPlane) -> Plane {
    Plane { n: units::dir_to_sim(p.n).normalize().as_vec3(), d: (p.d * METRES_PER_UNIT) as f32 }
}

/// Collision sides of a built brush: its faces plus axial and edge bevels (sim space).
fn collision_brush(b: &Built, surface_of: &mut impl FnMut(&str) -> u16, contents: Contents) -> Brush {
    let mut sides: Vec<Side> = b
        .faces
        .iter()
        .map(|(f, p, _)| Side { plane: sim_plane(p), surface: surface_of(&f.texture), bevel: false })
        .collect();
    let verts: Vec<DVec3> = b.faces.iter().flat_map(|(_, _, w)| w.iter().map(|v| units::pos_to_sim(*v))).collect();
    let mut bounds = Aabb::EMPTY;
    for v in &verts {
        bounds.grow(v.as_vec3());
    }
    let first_surface = sides[0].surface;
    let has = |sides: &Vec<Side>, n: Vec3, d: f32| {
        sides.iter().any(|s| s.plane.n.dot(n) > 0.9999 && (s.plane.d - d).abs() < 1e-3)
    };
    let support = |n: DVec3| verts.iter().map(|v| v.dot(n)).fold(f64::MIN, f64::max);
    // Axial bevels.
    for axis in [DVec3::X, DVec3::NEG_X, DVec3::Y, DVec3::NEG_Y, DVec3::Z, DVec3::NEG_Z] {
        let d = support(axis);
        if !sides.iter().any(|s| s.plane.n.as_dvec3().dot(axis) > 0.9999) {
            sides.push(Side { plane: Plane { n: axis.as_vec3(), d: d as f32 }, surface: first_surface, bevel: true });
        }
    }
    // Edge bevels: planes through an edge, parallel to an axis, that touch the brush only along that edge.
    for (f, _, w) in &b.faces {
        for i in 0..w.len() {
            let (a, c) = (units::pos_to_sim(w[i]), units::pos_to_sim(w[(i + 1) % w.len()]));
            let edge = c - a;
            if edge.length() < 1e-4 {
                continue;
            }
            for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
                let n = edge.cross(axis);
                if n.length() < 1e-4 {
                    continue;
                }
                for n in [n.normalize(), -n.normalize()] {
                    let d = n.dot(a);
                    // Only a supporting plane through the edge is a bevel.
                    if (support(n) - d).abs() > 1e-5 {
                        continue;
                    }
                    let (nf, df) = (n.as_vec3(), d as f32);
                    if !has(&sides, nf, df) {
                        sides.push(Side { plane: Plane { n: nf, d: df }, surface: surface_of(&f.texture), bevel: true });
                    }
                }
            }
        }
    }
    Brush { sides, bounds, contents }
}

/// Keeps the parts of `w` (on `plane`, belonging to brush `mine`) outside brush `other` (index `theirs`).
fn outside_of(w: Winding, plane: &DPlane, mine: usize, other: &Built, theirs: usize, out: &mut Vec<Winding>) {
    let mut rest = w;
    for (_, q, _) in &other.faces {
        if rest.is_empty() {
            return;
        }
        let on = rest.iter().all(|p| q.distance(*p).abs() <= EPS);
        if on {
            // Coplanar with one of their faces: facing the same way, the later brush keeps the face; facing each
            // other, both are hidden.
            if q.n.dot(plane.n) > 0.0 && theirs < mine {
                out.push(rest);
                return;
            }
            continue;
        }
        let (front, back) = winding::split(&rest, q, EPS);
        if front.len() >= 3 {
            out.push(front);
        }
        rest = back;
    }
    // What is left is inside `other`.
}

#[derive(Clone, Copy)]
struct Info {
    role: Role,
    contents: Contents,
    entity: u32,
    occludes: bool,
}

/// Compiles a map. Problems that don't stop the build (bad brushes, unknown classes) end up in `warnings`.
pub fn compile(map: &MapFile) -> Level {
    let mut level = Level::default();
    let mut surfaces: Vec<Surface> = Vec::new();
    let mut surface_of = |name: &str| -> u16 {
        if let Some(i) = surfaces.iter().position(|s| s.name == name) {
            return i as u16;
        }
        surfaces.push(Surface { name: name.to_string() });
        (surfaces.len() - 1) as u16
    };

    let mut built: Vec<(Built, Info)> = Vec::new();
    for (ei, ent) in map.entities.iter().enumerate() {
        let class = ent.classname();
        if ent.brushes.is_empty() {
            if class == "worldspawn" {
                continue;
            }
            let origin = ent.origin().map_or(Vec3::ZERO, units::to_sim);
            let yaw = ent
                .get("angles")
                .and_then(|a| a.split_whitespace().nth(1).and_then(|y| y.parse().ok()))
                .or_else(|| ent.get("angle").and_then(|a| a.parse().ok()))
                .map_or(0.0, units::yaw_from_map_angle);
            level.entities.push(PointEntity { classname: class.to_string(), origin, yaw, props: ent.props.clone() });
            continue;
        }
        let role = role(class).unwrap_or_else(|| {
            level.warnings.push(format!("entity {ei}: unknown brush class \"{class}\", treated as solid"));
            Role::Solid
        });
        let mut volume = Volume { classname: class.to_string(), props: ent.props.clone(), brushes: Vec::new(), bounds: Aabb::EMPTY };
        for (bi, def) in ent.brushes.iter().enumerate() {
            let Some(b) = build_brush(def) else {
                level.warnings.push(format!("entity {ei} brush {bi}: degenerate, skipped"));
                continue;
            };
            if role == Role::Volume {
                let brush = collision_brush(&b, &mut surface_of, Contents::NONE);
                volume.bounds = Aabb::union(volume.bounds, brush.bounds);
                volume.brushes.push(brush);
                continue;
            }
            let tex: Vec<String> = def.faces.iter().map(|f| f.texture.to_ascii_lowercase()).collect();
            let all = |name: &str| tex.iter().all(|t| t == name);
            if all("tools/trigger") || all("tools/skip") || all("tools/hint") {
                level.warnings.push(format!("entity {ei} brush {bi}: tool-only brush in a {class}, skipped"));
                continue;
            }
            let (contents, occludes) = match role {
                _ if all("tools/clip") => (Contents::PLAYER_CLIP, false),
                Role::Solid => (Contents::SOLID, true),
                Role::Window => (Contents::WINDOW, false),
                _ => (Contents::NONE, false),
            };
            built.push((b, Info { role, contents, entity: ei as u32, occludes }));
        }
        if role == Role::Volume && !volume.brushes.is_empty() {
            level.volumes.push(volume);
        }
    }

    // Collision.
    let mut brushes = Vec::new();
    for (b, info) in &built {
        if info.contents != Contents::NONE {
            brushes.push(collision_brush(b, &mut surface_of, info.contents));
        }
    }

    // Visible faces, cut by the solid brushes that cover them.
    for (i, (b, info)) in built.iter().enumerate() {
        if info.contents == Contents::PLAYER_CLIP {
            continue;
        }
        let kind = if info.role == Role::Window { FaceKind::Translucent } else { FaceKind::Opaque };
        for (f, plane, w) in &b.faces {
            if !drawn(&f.texture) {
                continue;
            }
            let mut pieces = vec![w.clone()];
            for (j, (other, oinfo)) in built.iter().enumerate() {
                if i == j || !oinfo.occludes {
                    continue;
                }
                let (lo, hi) = other.bounds_map;
                let (wlo, whi) = w.iter().fold((DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)), |(l, h), p| (l.min(*p), h.max(*p)));
                if (wlo - DVec3::splat(EPS)).cmpgt(hi).any() || (whi + DVec3::splat(EPS)).cmplt(lo).any() {
                    continue;
                }
                let mut next = Vec::new();
                for p in pieces {
                    outside_of(p, plane, i, other, j, &mut next);
                }
                pieces = next;
                if pieces.is_empty() {
                    break;
                }
            }
            let surface = surface_of(&f.texture);
            for p in pieces {
                if winding::area(&p) <= EPS {
                    continue;
                }
                level.faces.push(RenderFace {
                    surface,
                    points: p.iter().map(|v| units::to_sim(*v)).collect(),
                    normal: units::dir_to_sim(plane.n).as_vec3(),
                    uv: p.iter().map(|v| texel(f, *v)).collect(),
                    entity: info.entity,
                    kind,
                    lm: Vec::new(),
                });
            }
        }
    }

    level.world = World::new(brushes, surfaces);
    level
}

#[cfg(test)]
mod tests {
    use super::tests_support::block;
    use super::*;
    use crate::map;

    fn level(world_brushes: &[String], extra: &str) -> Level {
        let src = format!("{{\n\"classname\" \"worldspawn\"\n{}}}\n{extra}", world_brushes.concat());
        compile(&map::parse(&src).unwrap())
    }

    fn total_area(l: &Level) -> f32 {
        l.faces
            .iter()
            .map(|f| {
                let mut a = Vec3::ZERO;
                for i in 1..f.points.len() - 1 {
                    a += (f.points[i] - f.points[0]).cross(f.points[i + 1] - f.points[0]);
                }
                a.length() * 0.5
            })
            .sum()
    }

    #[test]
    fn a_block_becomes_six_outward_faces_and_a_brush() {
        let l = level(&[block([-64.0, -64.0, -16.0], [64.0, 64.0, 0.0], "dev/floor")], "");
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        assert_eq!(l.faces.len(), 6);
        for f in &l.faces {
            // Outward: the face normal points away from the block's centre.
            let c = f.points.iter().copied().sum::<Vec3>() / f.points.len() as f32;
            assert!(f.normal.dot(c - Vec3::new(0.0, -0.2032, 0.0)) > 0.0);
            // Counter-clockwise about the normal.
            let n = (f.points[1] - f.points[0]).cross(f.points[2] - f.points[0]).normalize();
            assert!(n.dot(f.normal) > 0.99);
        }
        assert_eq!(l.world.brushes.len(), 1);
        // Top face at map z = 0 → sim y = 0, 128 units = 3.25 m wide.
        let top = l.faces.iter().find(|f| f.normal.y > 0.9).unwrap();
        assert!(top.points.iter().all(|p| p.y.abs() < 1e-5));
        assert!((total_area(&l) - (2.0 * 3.2512 * 3.2512 + 4.0 * 3.2512 * 0.4064)).abs() < 1e-3);
    }

    #[test]
    fn touching_faces_are_hidden() {
        // Two blocks side by side: the two faces where they touch disappear.
        let l = level(&[block([0.0, 0.0, 0.0], [64.0, 64.0, 64.0], "a"), block([64.0, 0.0, 0.0], [128.0, 64.0, 64.0], "b")], "");
        assert_eq!(l.faces.len(), 10);
    }

    #[test]
    fn a_covered_face_is_cut_to_what_shows() {
        // A floor with a crate on it: the floor top loses the crate's footprint.
        let l = level(&[block([-128.0, -128.0, -16.0], [128.0, 128.0, 0.0], "floor"), block([0.0, 0.0, 0.0], [64.0, 64.0, 64.0], "crate")], "");
        let floor_top: f32 = l
            .faces
            .iter()
            .filter(|f| f.normal.y > 0.9 && f.points[0].y.abs() < 1e-5)
            .map(|f| {
                let mut a = Vec3::ZERO;
                for i in 1..f.points.len() - 1 {
                    a += (f.points[i] - f.points[0]).cross(f.points[i + 1] - f.points[0]);
                }
                a.length() * 0.5
            })
            .sum();
        let m = METRES_PER_UNIT as f32;
        assert!((floor_top - (256.0 * 256.0 - 64.0 * 64.0) * m * m).abs() < 1e-3, "{floor_top}");
    }

    #[test]
    fn entities_volumes_and_clip() {
        let extra = format!(
            "{{\n\"classname\" \"info_player_ksk\"\n\"origin\" \"32 0 40\"\n\"angle\" \"90\"\n}}\n\
             {{\n\"classname\" \"func_bomb_target\"\n\"site\" \"A\"\n{}}}\n",
            block([0.0, 0.0, 0.0], [128.0, 128.0, 64.0], "tools/trigger")
        );
        let l = level(&[block([-64.0, -64.0, -16.0], [64.0, 64.0, 0.0], "floor"), block([64.0, -64.0, 0.0], [72.0, 64.0, 128.0], "tools/clip")], &extra);
        let spawn = l.entities_of("info_player_ksk").next().unwrap();
        assert!((spawn.origin - Vec3::new(0.8128, 1.016, 0.0)).length() < 1e-4);
        let site = l.volumes_of("func_bomb_target").next().unwrap();
        assert_eq!(site.get("site"), Some("A"));
        assert!(site.contains(Vec3::new(1.0, 0.5, -1.0)));
        assert!(!site.contains(Vec3::new(-1.0, 0.5, -1.0)));
        // The clip brush collides but draws nothing.
        assert_eq!(l.world.brushes.iter().filter(|b| b.contents == Contents::PLAYER_CLIP).count(), 1);
        assert_eq!(l.faces.len(), 6);
    }

    #[test]
    fn a_box_slides_along_a_diagonal_wall_without_snagging() {
        // A wedge (a block cut diagonally) needs edge bevels for a box to stop in the right place near its corner.
        let wedge = "{\n\
( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) w [ 0 -1 0 0 ] [ 0 0 -1 0 ] 0 1 1\n\
( 0 0 0 ) ( 0 0 1 ) ( 1 0 0 ) w [ 1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1\n\
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) w [ -1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1\n\
( 0 0 128 ) ( 0 1 128 ) ( 1 0 128 ) w [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1\n\
( 0 128 0 ) ( 128 0 0 ) ( 0 128 1 ) w [ 1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1\n\
}\n";
        let l = level(&[wedge.to_string()], "");
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        let b = &l.world.brushes[0];
        assert!(b.sides.iter().any(|s| s.bevel), "bevels were added");
        // Coming at the wedge's sharp corner (map (128, 0)) along map −X, just inside its y range: without the axial
        // bevel the box would stop on the diagonal pushed out by the box (an invisible wall ≈ 6 units early).
        let ext = Vec3::new(0.2, 0.9, 0.2);
        let m = METRES_PER_UNIT as f32;
        let start = Vec3::new(128.0 * m + 1.0, 1.0, -2.0 * m);
        let t = l.world.trace(start, start + Vec3::new(-1.0, 0.0, 0.0), ext, Contents::MASK_PLAYER);
        assert!(t.hit());
        assert!(!l.world.solid_at(t.end, ext, Contents::MASK_PLAYER));
        assert!((t.end.x - (128.0 * m + 0.2)).abs() < 0.005, "{:?}", t.end);
        assert_eq!(t.normal, Vec3::X);
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    /// An axis-aligned brush in map units, textured `tex` on all sides.
    pub fn block(min: [f64; 3], max: [f64; 3], tex: &str) -> String {
        let [x0, y0, z0] = min;
        let [x1, y1, z1] = max;
        format!(
            "{{\n\
    ( {x0} {y0} {z0} ) ( {x0} {} {z0} ) ( {x0} {y0} {} ) {tex} [ 0 -1 0 0 ] [ 0 0 -1 0 ] 0 1 1\n\
    ( {x0} {y0} {z0} ) ( {x0} {y0} {} ) ( {} {y0} {z0} ) {tex} [ 1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1\n\
    ( {x0} {y0} {z0} ) ( {} {y0} {z0} ) ( {x0} {} {z0} ) {tex} [ -1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1\n\
    ( {x1} {y1} {z1} ) ( {x1} {} {z1} ) ( {} {y1} {z1} ) {tex} [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1\n\
    ( {x1} {y1} {z1} ) ( {} {y1} {z1} ) ( {x1} {y1} {} ) {tex} [ -1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1\n\
    ( {x1} {y1} {z1} ) ( {x1} {y1} {} ) ( {x1} {} {z1} ) {tex} [ 0 1 0 0 ] [ 0 0 -1 0 ] 0 1 1\n\
    }}\n",
            y0 + 1.0,
            z0 + 1.0,
            z0 + 1.0,
            x0 + 1.0,
            x0 + 1.0,
            y0 + 1.0,
            y1 + 1.0,
            x1 + 1.0,
            x1 + 1.0,
            z1 + 1.0,
            z1 + 1.0,
            y1 + 1.0,
        )
    }
}
