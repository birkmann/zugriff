//! Litter and debris on the ground: fallen leaves, scraps of paper, pebbles and bits of stone, twigs, the odd crushed
//! can and cigarette end. Grown at load like the grass (`detail.rs`), from the level's walkable ground faces: sparse in
//! the open, piled against walls and in corners (where the wind leaves it), thick under trees. Never collided with,
//! never baked; every vertex carries the lightmap coordinate of the ground under it, so litter in shade is as dark as
//! the ground there. Merged into 10 m cells that fade out with distance. `nodetail` leaves it out.

use crate::level::{Current, LIGHTMAP_EXPOSURE, LevelRoot};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::VisibilityRange;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::Lightmap;
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::TAU;
use zugriff_sim::compile::FaceKind;
use zugriff_sim::world::Contents;

pub struct LitterPlugin;

impl Plugin for LitterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, scatter);
    }
}

/// How much of what lies on a ground set (per square metre, in the open; walls and trees add more).
struct Mix {
    per_m2: f32,
    leaves: f32,
    paper: f32,
    stones: f32,
    twigs: f32,
    cans: f32,
    butts: f32,
}

fn mix(set: &str, map: &str) -> Option<Mix> {
    let s = set.to_ascii_lowercase();
    let town = map.contains("altstadt");
    let port = map.contains("hafen");
    // Aircraft maintenance floors are kept clear of loose debris.
    if s.contains("concrete_hangar") {
        return None;
    }
    if s.contains("grass") {
        return Some(Mix { per_m2: 0.25, leaves: 0.6, paper: 0.05, stones: 0.2, twigs: 0.15, cans: 0.0, butts: 0.0 });
    }
    if s.contains("gnd_") || s.contains("cobble") || s.contains("paving") || s.contains("asphalt") || s.contains("concrete") || s.contains("slabs") || s.contains("gravel") || s.contains("dirt") {
        return Some(if town {
            Mix { per_m2: 0.35, leaves: 0.45, paper: 0.12, stones: 0.2, twigs: 0.1, cans: 0.03, butts: 0.1 }
        } else if port {
            Mix { per_m2: 0.3, leaves: 0.1, paper: 0.2, stones: 0.4, twigs: 0.05, cans: 0.08, butts: 0.17 }
        } else {
            Mix { per_m2: 0.25, leaves: 0.25, paper: 0.08, stones: 0.45, twigs: 0.12, cans: 0.03, butts: 0.07 }
        });
    }
    None
}

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

fn lin(c: Vec3) -> [f32; 4] {
    [c.x.powf(2.2), c.y.powf(2.2), c.z.powf(2.2), 1.0]
}

#[derive(Default)]
struct Builder {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    lm: Vec<[f32; 2]>,
    idx: Vec<u32>,
}

impl Builder {
    fn vert(&mut self, p: Vec3, n: Vec3, c: [f32; 4], lm: Vec2) -> u32 {
        self.pos.push(p.to_array());
        self.nor.push(n.to_array());
        self.col.push(c);
        self.lm.push(lm.to_array());
        self.pos.len() as u32 - 1
    }

    /// A leaf lying on the ground: a pointed oval of 7 vertices around a midrib, curled up a little at the edges.
    fn leaf(&mut self, r: &mut Rng, p: Vec3, lm: Vec2) {
        let len = r.range(0.05, 0.12);
        let wid = len * r.range(0.45, 0.7);
        let yaw = r.range(0.0, TAU);
        let (f, s) = (Vec3::new(yaw.cos(), 0.0, yaw.sin()), Vec3::new(-yaw.sin(), 0.0, yaw.cos()));
        let palette = [Vec3::new(0.55, 0.36, 0.16), Vec3::new(0.68, 0.48, 0.18), Vec3::new(0.45, 0.28, 0.14), Vec3::new(0.62, 0.55, 0.22), Vec3::new(0.38, 0.4, 0.16), Vec3::new(0.72, 0.38, 0.12)];
        let base = palette[(r.next() * palette.len() as f32) as usize % palette.len()] * r.range(0.8, 1.1);
        let curl = r.range(0.0, 0.012);
        let up = Vec3::Y;
        let c_mid = lin(base * 0.85);
        let c_edge = lin(base);
        let tip = p + f * len * 0.5 + up * 0.002;
        let stem = p - f * len * 0.5 + up * 0.002;
        let a = self.vert(stem, up, c_mid, lm);
        let b = self.vert(tip, up, c_mid, lm);
        let mid = self.vert(p + up * 0.004, up, c_mid, lm);
        let mut ring = Vec::new();
        for (t, w) in [(-0.25, 0.8), (0.1, 1.0), (0.35, 0.6)] {
            for side in [-1.0f32, 1.0] {
                let q = p + f * len * t + s * wid * 0.5 * w * side + up * (0.002 + curl * w);
                ring.push(self.vert(q, up, c_edge, lm));
            }
        }
        // Fan: stem, left edge…, tip, right edge… around the middle vertex.
        let left = [a, ring[0], ring[2], ring[4], b];
        let right = [a, ring[1], ring[3], ring[5], b];
        for w in left.windows(2) {
            self.idx.extend_from_slice(&[mid, w[0], w[1]]);
        }
        for w in right.windows(2) {
            self.idx.extend_from_slice(&[mid, w[1], w[0]]);
        }
    }

    /// A scrap of paper or card: a 3×3 grid with crumpled heights, off-white, newsprint grey or brown card.
    fn paper(&mut self, r: &mut Rng, p: Vec3, lm: Vec2) {
        let size = r.range(0.08, 0.2);
        let yaw = r.range(0.0, TAU);
        let rot = Quat::from_rotation_y(yaw);
        let kinds = [Vec3::new(0.86, 0.84, 0.78), Vec3::new(0.66, 0.65, 0.6), Vec3::new(0.55, 0.42, 0.28), Vec3::new(0.8, 0.8, 0.82)];
        let c = kinds[(r.next() * kinds.len() as f32) as usize % kinds.len()] * r.range(0.85, 1.0);
        let crumple = r.range(0.0, 0.025) * if r.next() < 0.4 { 3.0 } else { 1.0 };
        let start = self.pos.len() as u32;
        for j in 0..3 {
            for i in 0..3 {
                let x = (i as f32 - 1.0) * size * 0.5 * r.range(0.8, 1.1);
                let z = (j as f32 - 1.0) * size * 0.5 * r.range(0.8, 1.1);
                let y = 0.003 + if (i, j) == (1, 1) { crumple } else { r.range(0.0, crumple * 0.6) };
                let q = p + rot * Vec3::new(x, y, z);
                let n = Vec3::new(r.range(-0.3, 0.3), 1.0, r.range(-0.3, 0.3)).normalize();
                self.vert(q, n, lin(c * r.range(0.9, 1.05)), lm);
            }
        }
        for j in 0..2u32 {
            for i in 0..2u32 {
                let a = start + j * 3 + i;
                self.idx.extend_from_slice(&[a, a + 3, a + 1, a + 1, a + 3, a + 4]);
            }
        }
    }

    /// A pebble or a chip of stone: a squashed, jittered octahedron.
    fn stone(&mut self, r: &mut Rng, p: Vec3, lm: Vec2) {
        let s = r.range(0.015, 0.06) * if r.next() < 0.15 { 2.0 } else { 1.0 };
        let greys = [Vec3::new(0.55, 0.52, 0.47), Vec3::new(0.62, 0.55, 0.45), Vec3::new(0.42, 0.41, 0.4), Vec3::new(0.7, 0.66, 0.58)];
        let c = greys[(r.next() * greys.len() as f32) as usize % greys.len()] * r.range(0.85, 1.1);
        let rot = Quat::from_rotation_y(r.range(0.0, TAU));
        let dirs = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z, Vec3::Y, Vec3::NEG_Y];
        let start = self.pos.len() as u32;
        for d in dirs {
            let k = r.range(0.7, 1.25);
            let v = Vec3::new(d.x * s * k, d.y * s * 0.55 * k, d.z * s * k * r.range(0.6, 1.0));
            let q = p + rot * v + Vec3::Y * s * 0.3;
            let top = if d.y < 0.0 { 0.55 } else { 1.0 };
            self.vert(q, (rot * d).normalize(), lin(c * top), lm);
        }
        let faces = [(0, 2, 4), (2, 1, 4), (1, 3, 4), (3, 0, 4), (2, 0, 5), (1, 2, 5), (3, 1, 5), (0, 3, 5)];
        for (a, b, cc) in faces {
            self.idx.extend_from_slice(&[start + a, start + cc, start + b]);
        }
    }

    /// A twig: a thin bent strip, one or two side shoots.
    fn twig(&mut self, r: &mut Rng, p: Vec3, lm: Vec2) {
        let len = r.range(0.12, 0.35);
        let yaw = r.range(0.0, TAU);
        let c = lin(Vec3::new(0.32, 0.24, 0.16) * r.range(0.8, 1.2));
        self.strip(p, yaw, len, r.range(0.006, 0.012), r.range(-0.6, 0.6), c, lm);
        if r.next() < 0.6 {
            let t = r.range(0.2, 0.7);
            let base = p + Vec3::new(yaw.cos(), 0.0, yaw.sin()) * (t - 0.5) * len;
            self.strip(base + Vec3::new(yaw.cos(), 0.0, yaw.sin()) * len * 0.1, yaw + r.range(-0.8, 0.8), len * 0.35, 0.005, 0.0, c, lm);
        }
    }

    fn strip(&mut self, p: Vec3, yaw: f32, len: f32, w: f32, bend: f32, c: [f32; 4], lm: Vec2) {
        let start = self.pos.len() as u32;
        const N: usize = 4;
        for i in 0..=N {
            let t = i as f32 / N as f32 - 0.5;
            let a = yaw + bend * t;
            let f = Vec3::new(a.cos(), 0.0, a.sin());
            let s = Vec3::new(-a.sin(), 0.0, a.cos());
            let q = p + f * t * len + Vec3::Y * (0.006 + 0.004 * (1.0 - 4.0 * t * t));
            self.vert(q - s * w * 0.5, Vec3::Y, c, lm);
            self.vert(q + s * w * 0.5, Vec3::Y, c, lm);
        }
        for i in 0..N as u32 {
            let a = start + i * 2;
            self.idx.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }

    /// A crushed drinks can on its side: a flattened 8-sided tube in a brand-free colour, silver ends.
    fn can(&mut self, r: &mut Rng, p: Vec3, lm: Vec2) {
        let colours = [Vec3::new(0.7, 0.08, 0.06), Vec3::new(0.1, 0.25, 0.6), Vec3::new(0.12, 0.45, 0.2), Vec3::new(0.85, 0.75, 0.2), Vec3::new(0.75, 0.75, 0.75)];
        let c = colours[(r.next() * colours.len() as f32) as usize % colours.len()];
        let yaw = r.range(0.0, TAU);
        let rot = Quat::from_rotation_y(yaw);
        let (len, rad, squash) = (0.12, 0.033, r.range(0.35, 1.0));
        let start = self.pos.len() as u32;
        const N: usize = 8;
        for e in 0..2 {
            let x = (e as f32 - 0.5) * len;
            for i in 0..N {
                let a = TAU * i as f32 / N as f32;
                let local = Vec3::new(x, rad * squash * a.sin() + rad * squash, rad * a.cos());
                let n = Vec3::new(0.0, a.sin(), a.cos());
                let col = if e == 0 && i % 2 == 0 { Vec3::splat(0.7) } else { c };
                self.vert(p + rot * local, rot * n, lin(col), lm);
            }
        }
        for i in 0..N as u32 {
            let j = (i + 1) % N as u32;
            self.idx.extend_from_slice(&[start + i, start + j, start + N as u32 + i, start + j, start + N as u32 + j, start + N as u32 + i]);
        }
    }

    /// A cigarette end: a stubby white tube with an orange filter.
    fn butt(&mut self, r: &mut Rng, p: Vec3, lm: Vec2) {
        let yaw = r.range(0.0, TAU);
        let f = Vec3::new(yaw.cos(), 0.0, yaw.sin());
        self.strip(p, yaw, 0.03, 0.008, 0.0, lin(Vec3::new(0.85, 0.83, 0.78)), lm);
        self.strip(p + f * 0.018, yaw, 0.012, 0.0085, 0.0, lin(Vec3::new(0.75, 0.48, 0.2)), lm);
    }
}

/// The baked light at atlas coordinate `lm` (see `detail::baked`); below `INDOORS` a spot is under a roof.
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

const CELL: f32 = 10.0;

/// A cell of litter (despawned to strew again at another density).
#[derive(Component)]
pub struct LitterCell;

#[allow(clippy::too_many_arguments)]
fn scatter(
    mut commands: Commands,
    current: Res<Current>,
    roots: Query<Entity, With<LevelRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    options: Res<crate::options::Options>,
    strewn: Query<Entity, With<LitterCell>>,
    mut done: Local<(u32, u32)>,
) {
    // Strewn again when the graphics settings' density changes.
    let key = (current.generation, crate::graphics::key(options.gfx.litter));
    if *done == key || !current.spawned {
        return;
    }
    let (Some(level), Some(root)) = (current.level.as_ref(), roots.iter().next()) else { return };
    *done = key;
    for e in &strewn {
        commands.entity(e).despawn();
    }
    let density = options.gfx.litter;
    if crate::args::flag("nodetail") || density <= 0.0 {
        return;
    }
    let lightmap = images.get(&current.lightmap).filter(|im| im.texture_descriptor.format == bevy::render::render_resource::TextureFormat::Rgba32Float);
    let seed = current.name.bytes().fold(0xC0FFEEu64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001B3));
    let mut rng = Rng(seed | 1);
    // Trees and shrubs drop leaves around them.
    let trees: Vec<Vec3> = level
        .entities_of("prop_model")
        .filter(|e| e.get("model").is_some_and(|m| m.contains("tree") || m.contains("shrub") || m.contains("fir")))
        .map(|e| e.origin)
        .collect();
    let mut cells: HashMap<(i32, i32), Builder> = HashMap::new();
    let mut count = 0usize;
    let web = cfg!(target_arch = "wasm32");
    for f in &level.faces {
        if f.kind != FaceKind::Opaque || f.normal.y < 0.85 || f.lm.len() != f.points.len() {
            continue;
        }
        let Some(m) = mix(level.world.surface_name(f.surface), &current.name) else { continue };
        for i in 1..f.points.len() - 1 {
            let (a, b, c) = (f.points[0], f.points[i], f.points[i + 1]);
            let (la, lb, lc) = (f.lm[0], f.lm[i], f.lm[i + 1]);
            let area = (b - a).cross(c - a).length() * 0.5;
            // Candidate spots: more than the open density, most kept only near a wall or a tree.
            let want = area * m.per_m2 * 10.0 * density * if web { 0.6 } else { 1.0 };
            let n = want.floor() as usize + usize::from(rng.next() < want.fract());
            for _ in 0..n {
                let (mut u, mut v) = (rng.next(), rng.next());
                if u + v > 1.0 {
                    (u, v) = (1.0 - u, 1.0 - v);
                }
                let p = a + (b - a) * u + (c - a) * v;
                let lm = la + (lb - la) * u + (lc - la) * v;
                if baked(lightmap, lm).is_some_and(|l| l < 0.09) {
                    continue;
                }
                // Nothing standing here.
                if level.world.trace(p + Vec3::Y * 2.5, p + Vec3::Y * 0.02, Vec3::ZERO, Contents::MASK_PLAYER).fraction < 1.0 {
                    continue;
                }
                // Walls within 1.2 m (and how close), trees within 4 m.
                let from = p + Vec3::Y * 0.15;
                let mut wall = f32::MAX;
                for d in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                    let t = level.world.trace(from, from + d * 1.2, Vec3::ZERO, Contents::MASK_SHOT);
                    if t.fraction < 1.0 {
                        wall = wall.min(t.fraction * 1.2);
                    }
                }
                let tree = trees.iter().map(|t| t.with_y(p.y).distance(p)).fold(f32::MAX, f32::min);
                let keep = if wall < 1.2 { 0.2 + 0.8 * (1.0 - wall / 1.2) } else { 0.12 } + if tree < 4.0 { 0.8 } else { 0.0 };
                if rng.next() > keep {
                    continue;
                }
                let key = ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32);
                let centre = Vec3::new((key.0 as f32 + 0.5) * CELL, 0.0, (key.1 as f32 + 0.5) * CELL);
                let cell = cells.entry(key).or_default();
                // A small cluster: litter gathers.
                let k = 1 + (rng.next() * rng.next() * 8.0) as usize;
                for _ in 0..k {
                    let q = p - centre + Vec3::new(rng.range(-0.3, 0.3), 0.0, rng.range(-0.3, 0.3));
                    let near_tree = tree < 4.0;
                    let pick = rng.next() * (m.leaves + m.paper + m.stones + m.twigs + m.cans + m.butts);
                    let mut acc = m.leaves + if near_tree { 1.0 } else { 0.0 };
                    if pick < acc {
                        cell.leaf(&mut rng, q, lm);
                        continue;
                    }
                    acc += m.paper;
                    if pick < acc {
                        cell.paper(&mut rng, q, lm);
                        continue;
                    }
                    acc += m.stones;
                    if pick < acc {
                        cell.stone(&mut rng, q, lm);
                        continue;
                    }
                    acc += m.twigs;
                    if pick < acc {
                        cell.twig(&mut rng, q, lm);
                        continue;
                    }
                    acc += m.cans;
                    if pick < acc {
                        cell.can(&mut rng, q, lm);
                        continue;
                    }
                    cell.butt(&mut rng, q, lm);
                }
                count += k;
            }
        }
    }
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        reflectance: 0.3,
        cull_mode: None,
        double_sided: false,
        lightmap_exposure: LIGHTMAP_EXPOSURE,
        ..default()
    });
    let baked_level = current.level.is_some();
    let mut tris = 0;
    for ((cx, cz), b) in cells {
        if b.idx.is_empty() {
            continue;
        }
        tris += b.idx.len() / 3;
        let n = b.lm.len();
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, b.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, b.nor)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, b.col)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; n])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, b.lm)
            .with_inserted_indices(Indices::U32(b.idx));
        let centre = Vec3::new((cx as f32 + 0.5) * CELL, 0.0, (cz as f32 + 0.5) * CELL);
        let mut e = commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(centre),
            bevy::light::NotShadowCaster,
            VisibilityRange { start_margin: 0.0..0.0, end_margin: 30.0..36.0, use_aabb: false },
            Name::new(format!("litter {cx},{cz}")),
            LitterCell,
            ChildOf(root),
        ));
        if baked_level {
            e.insert(Lightmap { image: current.lightmap.clone(), uv_rect: Rect::new(0.0, 0.0, 1.0, 1.0), bicubic_sampling: false });
        }
    }
    info!("litter: {count} pieces, {tris} triangles");
}
