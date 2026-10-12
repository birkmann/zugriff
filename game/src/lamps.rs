//! Lamps (`light_zg` in the map, written by `zkit` for lamp models): their light on the level is baked into the
//! lightmap; here each gets a glowing bulb (bloom does the halo) and a live light without shadows, which lights what
//! moves (players, the weapon in hand) and puts highlights on the baked floors. `nolamps` leaves the live lights out.

use crate::level::{Current, LevelRoot};
use bevy::prelude::*;

pub struct LampsPlugin;

impl Plugin for LampsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, spawn);
    }
}

/// Live lumens per watt of the bake's lamp power. The bake's sun (4 W/m²) is the live sun's 32 000 lux; a lamp of
/// P watts gives P / (4π d²) in the bake, so the same light live is P × 8000 lumens.
const LUMENS_PER_WATT: f32 = 8000.0;
/// How bright a bulb glows (nits per unit of its colour).
const BULB_NITS: f32 = 60_000.0;

fn spawn(mut commands: Commands, current: Res<Current>, roots: Query<Entity, With<LevelRoot>>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut done: Local<u32>) {
    if *done == current.generation || !current.spawned {
        return;
    }
    let (Some(level), Some(root)) = (current.level.as_ref(), roots.iter().next()) else { return };
    *done = current.generation;
    let live = !crate::args::flag("nolamps");
    let vec = |v: Option<&str>, default: Vec3| -> Vec3 {
        let c: Vec<f32> = v.unwrap_or("").split_whitespace().filter_map(|c| c.parse().ok()).collect();
        // Map space (z up) → sim (y up).
        if c.len() >= 3 { Vec3::new(c[0], c[2], -c[1]).normalize_or(default) } else { default }
    };
    let bulb = meshes.add(Sphere::new(0.035));
    for e in level.entities_of("light_zg") {
        let rgb: Vec<f32> = e.get("color").unwrap_or("255 255 255").split_whitespace().filter_map(|c| c.parse::<f32>().ok()).map(|c| c / 255.0).collect();
        let color = Color::srgb(rgb.first().copied().unwrap_or(1.0), rgb.get(1).copied().unwrap_or(1.0), rgb.get(2).copied().unwrap_or(1.0));
        let power: f32 = e.get("power").and_then(|v| v.parse().ok()).unwrap_or(100.0);
        let dir = vec(e.get("dir"), Vec3::NEG_Y);
        let kind = e.get("kind").unwrap_or("point");
        let glow = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::from(color) * BULB_NITS, unlit: true, ..default() });
        // The bulb, or the tube of a strip light.
        let (mesh, rot) = if kind == "area" {
            let size: Vec<f32> = e.get("size").unwrap_or("0.9 0.08").split_whitespace().filter_map(|c| c.parse().ok()).collect();
            let along = vec(e.get("along"), Vec3::X);
            (meshes.add(Cuboid::new(size.first().copied().unwrap_or(0.9) * 0.95, 0.022, 0.03)), Quat::from_rotation_arc(Vec3::X, along))
        } else {
            (bulb.clone(), Quat::IDENTITY)
        };
        commands.spawn((Mesh3d(mesh), MeshMaterial3d(glow), Transform::from_translation(e.origin).with_rotation(rot), bevy::light::NotShadowCaster, ChildOf(root)));
        if !live {
            continue;
        }
        let intensity = power * LUMENS_PER_WATT;
        let at = Transform::from_translation(e.origin + dir * 0.05);
        match kind {
            "spot" => {
                let half = e.get("cone").and_then(|v| v.parse::<f32>().ok()).unwrap_or(90.0).to_radians() * 0.5;
                commands.spawn((
                    SpotLight { color, intensity, range: 12.0, radius: 0.04, shadow_maps_enabled: false, outer_angle: half, inner_angle: half * 0.55, affects_lightmapped_mesh_diffuse: false, ..default() },
                    at.looking_to(dir, if dir.y.abs() > 0.9 { Vec3::X } else { Vec3::Y }),
                    ChildOf(root),
                ));
            }
            _ => {
                commands.spawn((
                    PointLight { color, intensity, range: 10.0, radius: if kind == "area" { 0.4 } else { 0.04 }, shadow_maps_enabled: false, affects_lightmapped_mesh_diffuse: false, ..default() },
                    at,
                    ChildOf(root),
                ));
            }
        }
    }
}
