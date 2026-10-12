//! Placed models (`prop_model` in the map: `model`, `origin`, `angle`, `scale`): barriers, bins, lamps, crates, shrubs
//! (`models/props/<model>.glb`, CC0 from Poly Haven, see `assets/models/CREDITS.md`). They are baked into the level's
//! lightmap like the walls: each has its own square of the atlas (`Level::prop_charts`), which its second UV set maps
//! into, so a crate in a dark hall is as dark as the hall. Their collision is a clip brush in the map, written next to
//! the entity by the map scripts (`zkit.prop`).

use crate::level::{Current, LIGHTMAP_EXPOSURE, LevelRoot};
use bevy::pbr::Lightmap;
use bevy::prelude::*;
use std::collections::HashSet;

pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn, light).chain());
    }
}

/// A placed model's root, with its square in the lightmap atlas, how far away it is still drawn, and whether it
/// casts the sun's shadow.
#[derive(Component)]
struct Placed {
    rect: Rect,
    range: Option<f32>,
    shadow: bool,
}

/// The size (metres, largest side) of each model, from `props.json`.
fn model_size(model: &str) -> f32 {
    static SIZES: std::sync::OnceLock<std::collections::HashMap<String, f32>> = std::sync::OnceLock::new();
    SIZES
        .get_or_init(|| {
            let doc: serde_json::Value = serde_json::from_str(include_str!("../../assets/models/props/props.json")).unwrap_or_default();
            doc.as_object()
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), v["size"].as_array().map_or(1.0, |a| a.iter().filter_map(|x| x.as_f64()).fold(0.0, f64::max) as f32)))
                        .collect()
                })
                .unwrap_or_default()
        })
        .get(model)
        .copied()
        .unwrap_or(1.0)
}

/// How far a model of `size` metres is drawn (small things vanish first: pots, signs, chairs — a draw each), and
/// whether it casts a shadow worth its draw in four cascades.
fn draw_rules(size: f32) -> (Option<f32>, bool) {
    match size {
        s if s < 0.6 => (Some(28.0), false),
        s if s < 1.5 => (Some(45.0), true),
        s if s < 3.5 => (Some(80.0), true),
        _ => (None, true),
    }
}

fn spawn(mut commands: Commands, current: Res<Current>, server: Res<AssetServer>, roots: Query<Entity, With<LevelRoot>>, mut done: Local<u32>) {
    if *done == current.generation || !current.spawned {
        return;
    }
    let (Some(level), Some(root)) = (current.level.as_ref(), roots.iter().next()) else { return };
    *done = current.generation;
    for c in &level.prop_charts {
        let e = &level.entities[c.entity];
        let Some(model) = e.get("model").filter(|m| !m.is_empty()) else { continue };
        let scale = e.get("scale").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
        let (range, shadow) = draw_rules(model_size(model) * scale);
        let scene = server.load(GltfAssetLabel::Scene(0).from_asset(format!("models/props/{model}.glb")));
        let [x0, y0, x1, y1] = c.rect;
        commands.spawn((
            WorldAssetRoot(scene),
            // A model's front is its +Z, a view's yaw looks down −Z: half a turn more faces it along the map angle.
            Transform::from_translation(e.origin).with_rotation(Quat::from_rotation_y(e.yaw + std::f32::consts::PI)).with_scale(Vec3::splat(scale)),
            Visibility::Inherited,
            Placed { rect: Rect::new(x0, y0, x1, y1), range, shadow },
            Name::new(format!("prop {model}")),
            ChildOf(root),
        ));
    }
}

/// Gives each placed model's meshes their square of the lightmap as they appear, and their materials the level's
/// lightmap exposure.
#[allow(clippy::type_complexity)]
fn light(
    mut commands: Commands,
    current: Res<Current>,
    images: Res<Assets<Image>>,
    meshes: Res<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new: Query<(Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>), Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    placed: Query<&Placed>,
    mut exposed: Local<HashSet<AssetId<StandardMaterial>>>,
) {
    let baked = images.contains(&current.lightmap);
    for (e, mesh, material) in &new {
        // Up the hierarchy to the placed model, if this mesh belongs to one.
        let mut at = e;
        let mut rect = None;
        for _ in 0..16 {
            if let Ok(p) = placed.get(at) {
                rect = Some(p.rect);
                if let Some(r) = p.range {
                    commands.entity(e).insert(bevy::camera::visibility::VisibilityRange { start_margin: 0.0..0.0, end_margin: r..r + 4.0, use_aabb: true });
                }
                if !p.shadow {
                    commands.entity(e).insert(bevy::light::NotShadowCaster);
                }
                break;
            }
            let Ok(parent) = parents.get(at) else { break };
            at = parent.parent();
        }
        let Some(rect) = rect else { continue };
        let has_uv1 = meshes.get(&mesh.0).is_some_and(|m| m.contains_attribute(Mesh::ATTRIBUTE_UV_1));
        if !baked || !has_uv1 {
            continue;
        }
        commands.entity(e).insert(Lightmap { image: current.lightmap.clone(), uv_rect: rect, bicubic_sampling: true });
        if exposed.insert(material.0.id())
            && let Some(mut m) = materials.get_mut(&material.0)
        {
            m.lightmap_exposure = LIGHTMAP_EXPOSURE;
        }
    }
}
