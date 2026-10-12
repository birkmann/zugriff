//! The land beyond a map's edge: a wide plane under the horizon painted into the sky (`sky::Horizon`), so looking out
//! past a fence or over a quay shows fields or water running off into the haze instead of the sky's lower half.
//! Not lightmapped (the sun and the ambient light it), not collided with, drawn below the map's lowest floor.

use crate::level::{Current, LevelRoot};
use crate::sky::Horizon;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

pub struct BackdropPlugin;

impl Plugin for BackdropPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, lay);
    }
}

/// A square of `half` metres around `centre` at height `y`, its texture repeating every `tile` metres.
fn plane(centre: Vec3, half: f32, y: f32, tile: f32) -> Mesh {
    let (x0, x1, z0, z1) = (centre.x - half, centre.x + half, centre.z - half, centre.z + half);
    let pos = vec![[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]];
    let uv: Vec<[f32; 2]> = pos.iter().map(|p| [p[0] / tile, p[2] / tile]).collect();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]));
    let _ = m.generate_tangents();
    m
}

#[allow(clippy::too_many_arguments)]
fn lay(
    mut commands: Commands,
    current: Res<Current>,
    roots: Query<Entity, With<LevelRoot>>,
    sky: Option<Res<crate::sky::SkyCube>>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut level_materials: ResMut<Assets<crate::levelmat::LevelMaterial>>,
    mut done: Local<u32>,
) {
    if *done == current.generation || !current.spawned {
        return;
    }
    let (Some(level), Some(root), Some(sky)) = (current.level.as_ref(), roots.iter().next(), sky) else { return };
    *done = current.generation;
    let style = Horizon::of(&current.name);
    if style == Horizon::None {
        return;
    }
    let b = level.world.bounds();
    let c = b.centre();
    // Just under the lowest floor players walk on (the bounds' bottom is the safety floor, below that).
    let floor = level.faces.iter().filter(|f| f.normal.y > 0.9).map(|f| f.points[0].y).fold(f32::MAX, f32::min);
    let (set, y, tile) = match style {
        Horizon::Port => (None, -2.53, 8.0),
        _ => (Some("grass"), floor - 0.42, 3.0),
    };
    let material = match set {
        Some(set) => {
            let mut m = crate::levelmat::material(&assets, set, sky.0.clone());
            // Far off and seen flat: darker, as a field seen at a glance is.
            m.base.base_color = Color::srgb(0.62, 0.66, 0.5);
            m
        }
        None => crate::levelmat::water(sky.0.clone()),
    };
    commands.spawn((
        Mesh3d(meshes.add(plane(c, 900.0, y, tile))),
        MeshMaterial3d(level_materials.add(material)),
        Transform::IDENTITY,
        bevy::light::NotShadowCaster,
        Name::new("backdrop"),
        ChildOf(root),
    ));
}
