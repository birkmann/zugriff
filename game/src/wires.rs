//! Overhead cables (`zg_wire` in the map: from `origin` to `end`, sagging `sag` metres in the middle): power and
//! phone lines strung across the lanes. Thin tubes along a parabola, built at load, not baked (they are lit by the
//! sun and the ambient like the players), not collided with.

use crate::level::{Current, LevelRoot};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

pub struct WiresPlugin;

impl Plugin for WiresPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, string);
    }
}

fn map_vec(v: &str) -> Option<Vec3> {
    let c: Vec<f32> = v.split_whitespace().filter_map(|c| c.parse().ok()).collect();
    // Map units (inches, z up) → sim metres (y up).
    (c.len() == 3).then(|| Vec3::new(c[0], c[2], -c[1]) * 0.0254)
}

fn string(mut commands: Commands, current: Res<Current>, roots: Query<Entity, With<LevelRoot>>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut done: Local<u32>) {
    if *done == current.generation || !current.spawned {
        return;
    }
    let (Some(level), Some(root)) = (current.level.as_ref(), roots.iter().next()) else { return };
    *done = current.generation;
    let (mut pos, mut nor, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    const SEG: usize = 20;
    const SIDES: usize = 5;
    for e in level.entities_of("zg_wire") {
        let Some(end) = e.get("end").and_then(map_vec) else { continue };
        let start = e.origin;
        let sag: f32 = e.get("sag").and_then(|v| v.parse().ok()).unwrap_or(0.4);
        let r: f32 = e.get("radius").and_then(|v| v.parse().ok()).unwrap_or(0.011);
        let point = |t: f32| start.lerp(end, t) - Vec3::Y * sag * 4.0 * t * (1.0 - t);
        for i in 0..=SEG {
            let t = i as f32 / SEG as f32;
            let p = point(t);
            let d = (point((t + 0.01).min(1.0)) - point((t - 0.01).max(0.0))).normalize_or(Vec3::X);
            let side = d.cross(Vec3::Y).normalize_or(Vec3::Z);
            let up = side.cross(d);
            let base = pos.len() as u32;
            for k in 0..SIDES {
                let a = std::f32::consts::TAU * k as f32 / SIDES as f32;
                let n = side * a.cos() + up * a.sin();
                pos.push((p + n * r).to_array());
                nor.push(n.to_array());
            }
            if i > 0 {
                let prev = base - SIDES as u32;
                for k in 0..SIDES as u32 {
                    let k1 = (k + 1) % SIDES as u32;
                    idx.extend_from_slice(&[prev + k, base + k, base + k1, prev + k, base + k1, prev + k1]);
                }
            }
        }
    }
    info!("wires: {} cables", level.entities_of("zg_wire").count());
    if idx.is_empty() {
        return;
    }
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_indices(Indices::U32(idx));
    let material = materials.add(StandardMaterial { base_color: Color::srgb(0.05, 0.05, 0.05), perceptual_roughness: 0.55, ..default() });
    commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), Transform::IDENTITY, Name::new("wires"), ChildOf(root)));
}
