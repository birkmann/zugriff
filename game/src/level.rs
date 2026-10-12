//! Loads `.map` files as assets (compiled by `zugriff_sim` on load) and turns the level's faces into meshes: one mesh
//! per surface, with the surface's material and the baked lightmap. Maps are switched with [`LoadMap`]; the bots'
//! navigation graph (`maps/<name>.nav`, written by `zg-compile --nav`) loads alongside, and is built on the spot if
//! the file is missing or was made for another version of the map.

use crate::textures;
use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext, RenderAssetUsages};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::Lightmap;
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use zugriff_sim::compile::{self, FaceKind, Level};
use zugriff_sim::nav::{self, Nav};
use zugriff_sim::{lightmap, map};

pub struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        crate::levelmat::register(app);
        app.add_plugins((crate::props::PropsPlugin, crate::lamps::LampsPlugin, crate::detail::DetailPlugin, crate::wires::WiresPlugin, crate::backdrop::BackdropPlugin, crate::litter::LitterPlugin, crate::fauna::FaunaPlugin));
        app.init_asset::<MapAsset>()
            .init_asset::<NavAsset>()
            .register_asset_loader(MapLoader)
            .register_asset_loader(NavLoader)
            .init_resource::<Current>()
            .add_message::<LoadMap>()
            .add_systems(Update, (switch_map, spawn_when_loaded, resolve_nav).chain());
    }
}

/// Scales the bake's values to the renderer's photometric light. The bake stores irradiance / π (its 4 W/m² sun
/// bakes as 4/π), the live sun is 32 000 lux (a white wall in it shows 32 000/π nits): 32 000 / 4 = 8000 makes the
/// baked sky, bounce and lamps sit right next to the live sun. (At 3000 everything in shade was 2.7× too dark.)
pub const LIGHTMAP_EXPOSURE: f32 = 8000.0;

/// Asks for a map by name (`maps/<name>.map`).
#[derive(Message, Clone)]
pub struct LoadMap(pub String);

/// A compiled level.
#[derive(Asset, TypePath)]
pub struct MapAsset(pub Arc<Level>);

#[derive(Asset, TypePath)]
pub struct NavAsset(pub Option<Nav>);

#[derive(Default, TypePath)]
struct MapLoader;

#[derive(Default, TypePath)]
struct NavLoader;

#[derive(Debug)]
pub struct LoadError(pub String);

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LoadError {}

impl AssetLoader for MapLoader {
    type Asset = MapAsset;
    type Settings = ();
    type Error = LoadError;

    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<MapAsset, LoadError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|e| LoadError(e.to_string()))?;
        let src = String::from_utf8(bytes).map_err(|e| LoadError(e.to_string()))?;
        let parsed = map::parse(&src).map_err(|e| LoadError(e.to_string()))?;
        let mut level = compile::compile(&parsed);
        lightmap::chart(&mut level, lightmap::LUXEL, lightmap::ATLAS_WIDTH);
        for w in &level.warnings {
            warn!("map: {w}");
        }
        Ok(MapAsset(Arc::new(level)))
    }

    fn extensions(&self) -> &[&str] {
        &["map"]
    }
}

impl AssetLoader for NavLoader {
    type Asset = NavAsset;
    type Settings = ();
    type Error = LoadError;

    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<NavAsset, LoadError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|e| LoadError(e.to_string()))?;
        Ok(NavAsset(Nav::from_bytes(&bytes)))
    }

    fn extensions(&self) -> &[&str] {
        &["nav"]
    }
}

/// The level being played.
#[derive(Resource, Default)]
pub struct Current {
    pub name: String,
    pub handle: Handle<MapAsset>,
    /// The baked light (`maps/<name>.lightmap.hdr`); the level is drawn with dynamic light only if it is missing.
    pub lightmap: Handle<Image>,
    pub nav_handle: Handle<NavAsset>,
    pub spawned: bool,
    pub level: Option<Arc<Level>>,
    pub nav: Option<Arc<Nav>>,
    /// Bumped every time a level is spawned (so others can notice a new map).
    pub generation: u32,
}

impl Current {
    /// The level and its navigation, once both are ready.
    pub fn ready(&self) -> Option<(Arc<Level>, Arc<Nav>)> {
        Some((self.level.clone()?, self.nav.clone()?))
    }
}

/// Everything spawned for the level (despawned together on a map change).
#[derive(Component)]
pub struct LevelRoot;

fn switch_map(mut commands: Commands, mut requests: MessageReader<LoadMap>, mut current: ResMut<Current>, assets: Res<AssetServer>, roots: Query<Entity, With<LevelRoot>>) {
    let Some(LoadMap(name)) = requests.read().last().cloned() else { return };
    if name == current.name && current.spawned {
        return;
    }
    for r in &roots {
        commands.entity(r).despawn();
    }
    info!("loading map {name}");
    current.handle = assets.load(format!("maps/{name}.map"));
    current.lightmap = assets.load(format!("maps/{name}.lightmap.hdr"));
    current.nav_handle = assets.load(format!("maps/{name}.nav"));
    current.name = name;
    current.spawned = false;
    current.level = None;
    current.nav = None;
}

fn resolve_nav(mut current: ResMut<Current>, assets: Res<AssetServer>, navs: Res<Assets<NavAsset>>) {
    if current.nav.is_some() {
        return;
    }
    let Some(level) = current.level.clone() else { return };
    let loaded = match assets.get_load_state(&current.nav_handle) {
        Some(bevy::asset::LoadState::Loaded) => navs.get(&current.nav_handle).and_then(|n| n.0.clone()),
        Some(bevy::asset::LoadState::Failed(_)) | None => None,
        _ => return,
    };
    let want = nav::geometry_hash(&level.world);
    let nav = match loaded {
        Some(n) if n.hash == want => n,
        other => {
            if other.is_some() {
                warn!("maps/{}.nav is stale; building the navigation now", current.name);
            } else {
                warn!("no maps/{}.nav; building the navigation now", current.name);
            }
            Nav::build(&level, &zugriff_sim::movement::MoveParams::default())
        }
    };
    info!("navigation: {} nodes, {} links", nav.nodes.len(), nav.links.len());
    current.nav = Some(Arc::new(nav));
}

fn spawn_when_loaded(
    mut commands: Commands,
    mut current: ResMut<Current>,
    assets: Res<AssetServer>,
    maps: Res<Assets<MapAsset>>,
    mut meshes: ResMut<Assets<Mesh>>,
    (mut materials, mut level_materials): (ResMut<Assets<StandardMaterial>>, ResMut<Assets<crate::levelmat::LevelMaterial>>),
    mut images: ResMut<Assets<Image>>,
) {
    if current.spawned || current.name.is_empty() {
        return;
    }
    let Some(MapAsset(level)) = maps.get(&current.handle) else {
        return;
    };
    let level = level.clone();
    // Wait for the lightmap to load or fail.
    let baked = match assets.get_load_state(&current.lightmap) {
        Some(bevy::asset::LoadState::Loaded) => true,
        Some(bevy::asset::LoadState::Failed(_)) | None => false,
        _ => return,
    };
    if !baked {
        warn!("no baked lightmap for this map; using dynamic light only");
    }
    current.spawned = true;
    current.generation += 1;
    current.level = Some(level.clone());
    let root = commands.spawn((LevelRoot, Transform::default(), Visibility::default(), Name::new("level"))).id();
    // The sky first: the skybox and the level's reflections share it.
    let sky = images.add(crate::sky::cube(&level, crate::sky::Horizon::of(&current.name)));
    commands.insert_resource(crate::sky::SkyCube(sky.clone()));

    // One mesh per surface (and per opaque/translucent kind).
    let mut groups: HashMap<(u16, FaceKind), Vec<&compile::RenderFace>> = HashMap::new();
    for f in &level.faces {
        groups.entry((f.surface, f.kind)).or_default().push(f);
    }
    let mut tris = 0;
    let mut dev_images: HashMap<String, Handle<Image>> = HashMap::new();
    let mut decal_atlas: Option<(Handle<Image>, Handle<Image>)> = None;
    for ((surface, kind), faces) in groups {
        let name = level.world.surface_name(surface).to_string();
        let (mut pos, mut nor, mut uv, mut lm, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for f in faces {
            let base = pos.len() as u32;
            for (i, (p, t)) in f.points.iter().zip(&f.uv).enumerate() {
                pos.push(p.to_array());
                nor.push(f.normal.to_array());
                uv.push([t.x / textures::SIZE as f32, t.y / textures::SIZE as f32]);
                lm.push(f.lm.get(i).map_or([0.0, 0.0], |t| [t.x, t.y]));
            }
            for i in 1..f.points.len() as u32 - 1 {
                idx.extend_from_slice(&[base, base + i, base + i + 1]);
            }
            tris += f.points.len() - 2;
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, lm)
            .with_inserted_indices(Indices::U32(idx));
        if let Err(e) = mesh.generate_tangents() {
            warn!("tangents for {name}: {e}");
        }
        // Texture sets get the level material (the standard one with the repeat hidden), dev textures the plain one.
        let mut set_material = name.strip_prefix("mat/").map(|set| crate::levelmat::material(&assets, set, sky.clone()));
        if name.eq_ignore_ascii_case("dev/water") {
            set_material = Some(crate::levelmat::water(sky.clone()));
        }
        let mut dev_material = set_material.is_none().then(|| {
            // Decals (`decal/<name>`): thin panels in front of a wall or floor, their UVs on one cell of the decal atlas;
            // blended over what is behind, and lit by their own square of the lightmap.
            if let Some(decal) = name.strip_prefix("decal/") {
                let (color, normal) = decal_atlas.get_or_insert_with(|| textures::decal_atlas(&assets)).clone();
                // Grime and damp darken what is under them (unlit: their colour scales the lit surface), the rest are
                // painted on and lit.
                if textures::decal_multiplies(decal) {
                    return StandardMaterial { base_color_texture: Some(color), alpha_mode: AlphaMode::Multiply, unlit: true, depth_bias: 16.0, ..default() };
                }
                return StandardMaterial { base_color_texture: Some(color), normal_map_texture: Some(normal), alpha_mode: AlphaMode::Blend, perceptual_roughness: 0.9, reflectance: 0.3, depth_bias: 16.0, ..default() };
            }
            let look = textures::look(&name);
            let image = dev_images.entry(name.clone()).or_insert_with(|| images.add(textures::dev_image(&name))).clone();
            StandardMaterial { base_color_texture: Some(image), perceptual_roughness: look.roughness, reflectance: if look.plain { 0.6 } else { 0.3 }, ..default() }
        });
        let material = set_material.as_mut().map_or_else(|| dev_material.as_mut().unwrap(), |m| &mut m.base);
        // The bake's sun is 4 W/m² in Blender units; this matches it to the live sun's 32 000 lux.
        material.lightmap_exposure = LIGHTMAP_EXPOSURE;
        // Both sides cast the sun's shadow: a room's ceiling faces down, away from the sun, and its roof above is not
        // drawn (nodraw), so with back faces culled the sun shone through every roof onto the floors below.
        material.cull_mode = None;
        if kind == FaceKind::Translucent {
            material.alpha_mode = AlphaMode::Blend;
            material.double_sided = true;
            material.cull_mode = None;
        }
        let mut e = commands.spawn((Mesh3d(meshes.add(mesh)), Name::new(name), ChildOf(root)));
        match (set_material, dev_material) {
            (Some(m), _) => e.insert(MeshMaterial3d(level_materials.add(m))),
            (_, Some(m)) => e.insert(MeshMaterial3d(materials.add(m))),
            _ => unreachable!(),
        };
        if baked && kind == FaceKind::Opaque {
            e.insert(Lightmap { image: current.lightmap.clone(), uv_rect: Rect::new(0.0, 0.0, 1.0, 1.0), bicubic_sampling: true });
        }
    }

    // The sun from `light_environment` (pitch below the horizon, yaw in map degrees).
    let env = level.entities_of("light_environment").next();
    let num = |key: &str, default: f32| env.and_then(|e| e.get(key)).and_then(|v| v.parse().ok()).unwrap_or(default);
    let (pitch, yaw) = (num("pitch", -50.0).to_radians(), zugriff_sim::units::yaw_from_map_angle(num("yaw", 35.0) as f64));
    commands.spawn((
        // The sun is live on everything, the level included: its shadows stay sharp (a baked sun smeared them over
        // 20 cm luxels) and it falls through windows onto floors. The lightmap holds the rest: sky, lamps, bounce.
        DirectionalLight {
            illuminance: 32_000.0,
            shadow_maps_enabled: !crate::args::flag("noshadow"),
            color: Color::srgb(1.0, 0.95, 0.88),
            affects_lightmapped_mesh_diffuse: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0)),
        // The first cascade starts at the eye: the weapon in hand sits a few millimetres in front of it, and outside the
        // cascade it flipped between lit and shadowed as the view turned.
        // (The web draws three: every cascade is another pass over everything that casts.)
        CascadeShadowConfigBuilder {
            num_cascades: if cfg!(target_arch = "wasm32") { 3 } else { 4 },
            minimum_distance: 0.001,
            first_cascade_far_bound: 9.0,
            maximum_distance: if cfg!(target_arch = "wasm32") { 90.0 } else { 130.0 },
            overlap_proportion: 0.2,
        }
        .build(),
        ChildOf(root),
    ));
    info!(
        "level {}: {} faces → {tris} triangles, {} brushes, {} entities, {} volumes",
        current.name,
        level.faces.len(),
        level.world.brushes.len(),
        level.entities.len(),
        level.volumes.len()
    );
}
