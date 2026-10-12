//! Detail: grass on the lawns, tufts in gravel and dirt, weeds where paving meets a wall. Grown at load from the
//! level's ground faces (by their texture set), never written into the map, never collided with, never baked.
//!
//! Each blade is a thin bent strip coloured by vertex (dark at the root, lighter and drier at the tip). The blades of an
//! 8 m cell are one mesh, so a map is a few hundred draws. Every vertex carries, as its lightmap coordinate, the atlas
//! coordinate of the ground at the blade's root: grass in a building's shade is as dark as the ground there, and the
//! live sun lights and shadows it like the rest of the level. Cells fade out with distance (`VisibilityRange`).
//! `nodetail` (the level shader's switch) leaves them out too.

use crate::level::{Current, LIGHTMAP_EXPOSURE, LevelRoot};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::VisibilityRange;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::Lightmap;
use bevy::prelude::*;
use std::collections::HashMap;
use zugriff_sim::compile::FaceKind;
use zugriff_sim::world::Contents;

pub struct DetailPlugin;

impl Plugin for DetailPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, grow);
    }
}

/// What grows on a ground texture set.
#[derive(Clone, Copy)]
struct Rule {
    /// Clumps per square metre.
    density: f32,
    /// Only within this distance (m) of a wall (weeds along a facade's foot); 0: anywhere.
    near_wall: f32,
    kind: Kind,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// A lawn: dense green blades, a few dry ones, the odd flower.
    Lawn,
    /// Sparse, drier tufts in gravel and dirt.
    Tuft,
    /// Weeds in the joints at a wall's foot: short, broad-leaved and grassy.
    Weed,
}

fn rule(set: &str) -> Option<Rule> {
    let s = set.to_ascii_lowercase();
    Some(if s.contains("grass") {
        Rule { density: 16.0, near_wall: 0.0, kind: Kind::Lawn }
    } else if s.contains("dirt") {
        Rule { density: 0.9, near_wall: 0.0, kind: Kind::Tuft }
    } else if s.contains("gravel") {
        Rule { density: 0.35, near_wall: 0.0, kind: Kind::Tuft }
    } else if s.contains("gnd_dust") {
        Rule { density: 0.6, near_wall: 0.0, kind: Kind::Tuft }
    } else if s.contains("cobble") || s.contains("paving") || s.contains("asphalt") || s.contains("concrete_floor") || s.contains("slabs") || s.contains("gnd_") {
        Rule { density: 6.0, near_wall: 0.4, kind: Kind::Weed }
    } else {
        return None;
    })
}

/// A small deterministic generator (xorshift*), so a map grows the same every time.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        ((self.0.wrapping_mul(0x2545F4914F6CDD1D) >> 40) as f32) / (1u64 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

/// One cell's merged blades.
#[derive(Default)]
struct Builder {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    lm: Vec<[f32; 2]>,
    idx: Vec<u32>,
}

fn lin(c: Vec3) -> Vec3 {
    Vec3::new(c.x.powf(2.2), c.y.powf(2.2), c.z.powf(2.2))
}

impl Builder {
    /// A blade from `root`: `h` tall, `w` wide at the root, bent `lean` metres towards `dir` (horizontal), coloured
    /// from `base` to `tip` (sRGB).
    #[allow(clippy::too_many_arguments)]
    fn blade(&mut self, root: Vec3, lm: Vec2, dir: Vec3, h: f32, w: f32, lean: f32, base: Vec3, tip: Vec3, facing: Vec3) {
        let side = facing.cross(Vec3::Y).normalize_or(Vec3::X);
        let start = self.pos.len() as u32;
        const SEG: usize = 3;
        for i in 0..=SEG {
            let t = i as f32 / SEG as f32;
            // A curve: rising, bending over more towards the tip.
            let c = root + Vec3::Y * h * t + dir * lean * t * t;
            let half = w * 0.5 * (1.0 - t).powf(0.8);
            // Lit like the ground under it, tipping a little towards the blade's face (so a lawn isn't a flat mat).
            let n = (Vec3::Y * 0.8 + facing * 0.2).normalize();
            let col = lin(base.lerp(tip, t.powf(0.8)));
            // A little darker at the root, where the blades shade each other.
            let ao = 0.55 + 0.45 * t.sqrt();
            let rgba = [col.x * ao, col.y * ao, col.z * ao, 1.0];
            if i < SEG {
                for s in [-1.0, 1.0] {
                    self.pos.push((c + side * half * s).to_array());
                    self.nor.push(n.to_array());
                    self.col.push(rgba);
                    self.lm.push(lm.to_array());
                }
            } else {
                self.pos.push(c.to_array());
                self.nor.push(n.to_array());
                self.col.push(rgba);
                self.lm.push(lm.to_array());
            }
        }
        for i in 0..(SEG as u32 - 1) {
            let a = start + i * 2;
            self.idx.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
        let a = start + (SEG as u32 - 1) * 2;
        self.idx.extend_from_slice(&[a, a + 1, a + 2]);
    }

    /// A clump of blades at `p` (the ground's lightmap coordinate `lm` there).
    fn clump(&mut self, rng: &mut Rng, p: Vec3, lm: Vec2, kind: Kind) {
        // Per-clump colour: a green that runs from fresh to sun-dried.
        let dry = rng.next();
        let (n, h0, h1, spread) = match kind {
            Kind::Lawn => (8, 0.1, 0.26, 0.16),
            Kind::Tuft => (9, 0.1, 0.32, 0.1),
            Kind::Weed => (6, 0.06, 0.2, 0.08),
        };
        let dry = match kind {
            Kind::Lawn => dry * 0.55,
            Kind::Tuft => 0.3 + dry * 0.7,
            Kind::Weed => dry * 0.4,
        };
        // (sRGB; the lawn texture's own greens are a warm yellow-green.)
        let green_base = Vec3::new(0.17, 0.22, 0.06).lerp(Vec3::new(0.3, 0.27, 0.12), dry);
        let green_tip = Vec3::new(0.4, 0.48, 0.17).lerp(Vec3::new(0.64, 0.58, 0.34), dry);
        for _ in 0..n {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let r = spread * rng.next().sqrt();
            let root = p + Vec3::new(a.cos() * r, 0.0, a.sin() * r);
            let face_a = rng.range(0.0, std::f32::consts::TAU);
            let facing = Vec3::new(face_a.cos(), 0.0, face_a.sin());
            let dir = (Vec3::new(a.cos(), 0.0, a.sin()) + facing * 0.3).normalize_or(Vec3::X);
            let h = rng.range(h0, h1);
            let v = rng.range(-0.04, 0.04);
            let (w, lean) = match kind {
                Kind::Weed if rng.next() < 0.5 => (rng.range(0.03, 0.05), h * 0.6),
                _ => (rng.range(0.008, 0.016), h * rng.range(0.15, 0.55)),
            };
            self.blade(root, lm, dir, h, w, lean, green_base + Vec3::splat(v), green_tip + Vec3::splat(v), facing);
        }
        // Now and then a flower on a lawn: a small bright head on a stalk.
        if kind == Kind::Lawn && rng.next() < 0.06 {
            let head = if rng.next() < 0.5 { Vec3::new(0.95, 0.85, 0.2) } else { Vec3::new(0.95, 0.95, 0.92) };
            let h = rng.range(0.22, 0.34);
            let facing = Vec3::new(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0)).normalize_or(Vec3::X);
            self.blade(p, lm, facing, h, 0.006, 0.02, green_base, green_tip, facing);
            let top = p + Vec3::Y * h + facing * 0.02;
            for k in 0..2 {
                let f = if k == 0 { facing } else { facing.cross(Vec3::Y) };
                self.blade(top - Vec3::Y * 0.015, lm, f, 0.03, 0.035, 0.0, head, head, f);
            }
        }
    }
}

const CELL: f32 = 8.0;

/// The baked light (the atlas's raw irradiance / π, before `LIGHTMAP_EXPOSURE`) at atlas coordinate `lm`, if the
/// lightmap is loaded and readable.
fn baked(image: Option<&Image>, lm: Vec2) -> Option<f32> {
    let im = image?;
    let (w, h) = (im.width() as usize, im.height() as usize);
    let data = im.data.as_ref()?;
    let x = ((lm.x * w as f32) as usize).min(w.saturating_sub(1));
    let y = ((lm.y * h as f32) as usize).min(h.saturating_sub(1));
    let i = (y * w + x) * 16;
    let f = |o: usize| data.get(i + o..i + o + 4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    Some(0.2126 * f(0)? + 0.7152 * f(4)? + 0.0722 * f(8)?)
}

/// Below this much baked light a spot is under a roof: nothing grows there (weeds on a warehouse floor).
const INDOORS: f32 = 0.09;

/// A cell of grown blades (despawned to grow again at another density).
#[derive(Component)]
pub struct DetailCell;

#[allow(clippy::too_many_arguments)]
fn grow(
    mut commands: Commands,
    current: Res<Current>,
    roots: Query<Entity, With<LevelRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    options: Res<crate::options::Options>,
    grown: Query<Entity, With<DetailCell>>,
    mut done: Local<(u32, u32)>,
) {
    // Grown again when the graphics settings' density changes.
    let key = (current.generation, crate::graphics::key(options.gfx.grass));
    if *done == key || !current.spawned {
        return;
    }
    let (Some(level), Some(root)) = (current.level.as_ref(), roots.iter().next()) else { return };
    *done = key;
    for e in &grown {
        commands.entity(e).despawn();
    }
    let density = options.gfx.grass;
    if crate::args::flag("nodetail") || density <= 0.0 {
        return;
    }
    let seed = current.name.bytes().fold(0x9E3779B97F4A7C15u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001B3));
    let mut rng = Rng(seed | 1);
    // (An RGBA float lightmap; anything else and every spot counts as open.)
    let lightmap = images.get(&current.lightmap).filter(|im| im.texture_descriptor.format == bevy::render::render_resource::TextureFormat::Rgba32Float);
    let mut indoors = 0usize;
    let mut cells: HashMap<(i32, i32, bool), Builder> = HashMap::new();
    let (mut clumps, mut blocked) = (0usize, 0usize);
    for f in &level.faces {
        if f.kind != FaceKind::Opaque || f.normal.y < 0.8 || f.lm.len() != f.points.len() {
            continue;
        }
        let Some(r) = rule(level.world.surface_name(f.surface)) else { continue };
        for i in 1..f.points.len() - 1 {
            let (a, b, c) = (f.points[0], f.points[i], f.points[i + 1]);
            let (la, lb, lc) = (f.lm[0], f.lm[i], f.lm[i + 1]);
            let area = (b - a).cross(c - a).length() * 0.5;
            // (The web draws about two thirds as many: its frame is dearer per triangle.)
            let want = area * r.density * density * if cfg!(target_arch = "wasm32") { 0.65 } else { 1.0 };
            let n = want.floor() as usize + usize::from(rng.next() < want.fract());
            for _ in 0..n {
                let (mut u, mut v) = (rng.next(), rng.next());
                if u + v > 1.0 {
                    (u, v) = (1.0 - u, 1.0 - v);
                }
                let p = a + (b - a) * u + (c - a) * v;
                let lm = la + (lb - la) * u + (lc - la) * v;
                // Nothing standing on the spot (a crate, a wall's foot, a prop's clip box).
                // (from 2.5 m: a start inside a raised box — a van's body over its wheels — wouldn't count as a hit)
                let t = level.world.trace(p + Vec3::Y * 2.5, p + Vec3::Y * 0.02, Vec3::ZERO, Contents::MASK_PLAYER);
                if t.fraction < 1.0 {
                    blocked += 1;
                    continue;
                }
                if baked(lightmap, lm).is_some_and(|b| b < INDOORS) {
                    indoors += 1;
                    continue;
                }
                if r.near_wall > 0.0 {
                    let from = p + Vec3::Y * 0.12;
                    let near = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z].iter().any(|d| level.world.trace(from, from + *d * r.near_wall, Vec3::ZERO, Contents::MASK_SHOT).fraction < 1.0);
                    if !near {
                        continue;
                    }
                }
                // Most lawn clumps are only drawn near the eye; the rest carry the lawn's texture further out.
                let near = r.kind == Kind::Lawn && rng.next() < 0.65;
                let key = ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32, near);
                let centre = Vec3::new((key.0 as f32 + 0.5) * CELL, 0.0, (key.1 as f32 + 0.5) * CELL);
                cells.entry(key).or_default().clump(&mut rng, p + Vec3::Y * 0.005 - centre, lm, r.kind);
                clumps += 1;
            }
        }
    }
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.75,
        reflectance: 0.35,
        cull_mode: None,
        double_sided: false,
        lightmap_exposure: LIGHTMAP_EXPOSURE,
        ..default()
    });
    let baked = current.level.is_some();
    let mut tris = 0;
    for ((cx, cz, near), b) in cells {
        tris += b.idx.len() / 3;
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, b.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, b.nor)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, b.col)
            // (Bevy's fragment shader wants a first UV set whenever there is a second.)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; b.lm.len()])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, b.lm)
            .with_inserted_indices(Indices::U32(b.idx));
        let centre = Vec3::new((cx as f32 + 0.5) * CELL, 0.0, (cz as f32 + 0.5) * CELL);
        let mut e = commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            // At the cell's centre (its vertices are relative to it): the visibility range measures from here.
            Transform::from_translation(centre),
            // Blades are too thin to shadow anything that reads; the sun's shadow maps are spared them.
            bevy::light::NotShadowCaster,
            // Measured from the cell's centre: the cell's half-diagonal on top of the distance the blades still read.
            VisibilityRange { start_margin: 0.0..0.0, end_margin: if near { 24.0..30.0 } else { 55.0..65.0 }, use_aabb: false },
            Name::new(format!("detail {cx},{cz} @ {centre}")),
            DetailCell,
            ChildOf(root),
        ));
        if baked {
            e.insert(Lightmap { image: current.lightmap.clone(), uv_rect: Rect::new(0.0, 0.0, 1.0, 1.0), bicubic_sampling: false });
        }
    }
    info!("detail: {clumps} clumps ({blocked} spots taken, {indoors} indoors), {tris} triangles; lightmap readable: {}", lightmap.is_some());
}
