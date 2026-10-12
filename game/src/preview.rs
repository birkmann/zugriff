//! Item pictures, rendered by the game itself: each weapon's own model in its finish, seen from its right side (the
//! muzzle to the right), lit as in a photo studio, on transparent ground. The buy menu and the arsenal show them, so
//! a tile shows exactly the gun (and the paint) one gets.
//!
//! A picture is asked for with [`Previews::get`] (a weapon, a finish, a size) and is an image at once, filled in a few
//! frames later: a stage far below the map gets the model and a camera of its own (on a render layer nothing else is
//! on, with its own lights), the camera is fitted to the model's bounds once it has spawned and been painted, renders
//! for a moment, and is read back: once the picture has the weapon in it (the pipelines may still be compiling on the
//! first frames), the stage goes. Pictures are kept for the session.

use std::collections::HashMap;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Exposure, RenderTarget, ScalingMode};
use bevy::camera::primitives::Aabb;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use zugriff_sim::weapons::WeaponId;

use crate::finish::Painted;

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Previews>().add_systems(Startup, (lights, dump_all)).add_systems(PostUpdate, (stage, frame).chain().after(bevy::camera::visibility::VisibilitySystems::CalculateBounds));
    }
}

/// The layer the stages, their cameras and lights are on.
const LAYER: usize = 7;
/// Where the stages stand: far below any map, a few metres apart (each camera only sees its own).
const STAGE: Vec3 = Vec3::new(0.0, -4000.0, 0.0);
const SPACING: f32 = 6.0;
/// Stages rendering at once (more wait their turn).
const AT_ONCE: usize = 6;

/// How big a picture: a tile's, or the details panel's and the arsenal's.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Size {
    Tile,
    Large,
}

impl Size {
    fn px(self) -> UVec2 {
        match self {
            Size::Tile => UVec2::new(512, 192),
            Size::Large => UVec2::new(1280, 480),
        }
    }
    pub const ASPECT: f32 = 512.0 / 192.0;
}

type Key = (WeaponId, u8, Size);

#[derive(Resource, Default)]
pub struct Previews {
    images: HashMap<Key, Handle<Image>>,
    /// Asked for and not rendered yet, oldest first.
    queue: Vec<Key>,
    /// Stages up: the key, and its slot.
    live: HashMap<Key, usize>,
    done: Vec<Key>,
}

impl Previews {
    /// The picture of weapon `w` in finish `finish` (drawn over the next frames).
    pub fn get(&mut self, images: &mut Assets<Image>, w: WeaponId, finish: u8, size: Size) -> Handle<Image> {
        let key = (w, finish, size);
        if let Some(h) = self.images.get(&key) {
            return h.clone();
        }
        let px = size.px();
        let mut img = Image::new_target_texture(px.x, px.y, TextureFormat::Rgba8UnormSrgb, None);
        img.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let h = images.add(img);
        self.images.insert(key, h.clone());
        self.queue.push(key);
        h
    }

    /// Whether a picture has been drawn.
    pub fn ready(&self, w: WeaponId, finish: u8, size: Size) -> bool {
        self.done.contains(&(w, finish, size))
    }
}

#[derive(Component)]
struct Stage {
    key: Key,
    camera: Entity,
    /// The model shown (its class's shared one if the gun's own failed to load).
    scene: Handle<WorldAsset>,
    /// Seconds the camera has been rendering; whether a readback is on its way.
    shooting: Option<f32>,
    checking: bool,
    /// Frames spent waiting for the model.
    waited: u32,
}

#[derive(Component)]
struct StageCamera;

/// Studio lights: a key light from the front above, a rim light from behind, a low fill; only on the stages' layer.
fn lights(mut commands: Commands) {
    let layer = RenderLayers::layer(LAYER);
    for (dir, lux) in [(Vec3::new(-1.0, -0.9, -0.35), 26_000.0), (Vec3::new(0.9, -0.4, 0.6), 18_000.0), (Vec3::new(-0.6, 0.5, 0.2), 6_000.0)] {
        commands.spawn((DirectionalLight { illuminance: lux, shadow_maps_enabled: false, ..default() }, Transform::default().looking_to(dir, Vec3::Y), layer.clone()));
    }
}

/// Puts up stages for the pictures asked for (a few at a time).
fn stage(mut commands: Commands, server: Res<AssetServer>, mut previews: ResMut<Previews>) {
    while previews.live.len() < AT_ONCE && !previews.queue.is_empty() {
        let key = previews.queue.remove(0);
        let slot = (0..AT_ONCE).find(|s| !previews.live.values().any(|v| v == s)).unwrap_or(0);
        previews.live.insert(key, slot);
        let (w, finish, _) = key;
        let at = STAGE + Vec3::Z * SPACING * slot as f32;
        let image = previews.images[&key].clone();
        let camera = commands
            .spawn((
                Camera3d::default(),
                Camera { order: -10 - slot as isize, clear_color: ClearColorConfig::Custom(Color::NONE), is_active: false, ..default() },
                RenderTarget::Image(image.into()),
                Projection::Orthographic(OrthographicProjection { scaling_mode: ScalingMode::Fixed { width: 1.0, height: 0.375 }, near: 0.0, far: 8.0, ..OrthographicProjection::default_3d() }),
                Exposure { ev100: crate::view::ev100() },
                Msaa::Sample4,
                AmbientLight { brightness: 2600.0, color: Color::srgb(0.9, 0.93, 1.0), affects_lightmapped_meshes: false },
                Transform::from_translation(at + Vec3::X * 4.0).looking_to(Vec3::NEG_X, Vec3::Y),
                RenderLayers::layer(LAYER),
                StageCamera,
            ))
            .id();
        let scene: Handle<WorldAsset> = server.load(GltfAssetLabel::Scene(0).from_asset(crate::gear::prop_path(w)));
        commands.spawn((WorldAssetRoot(scene.clone()), Transform::from_translation(at), Visibility::Inherited, Painted { weapon: w, finish }, RenderLayers::layer(LAYER), Stage { key, camera, scene, shooting: None, checking: false, waited: 0 })).observe(checked);
    }
}

/// Fits each camera to its model once the model is in, lets it render, and reads the picture back to see it's done.
#[allow(clippy::too_many_arguments)]
fn frame(
    mut commands: Commands,
    time: Res<Time>,
    mut stages: Query<(Entity, &mut Stage, &GlobalTransform)>,
    children: Query<&Children>,
    meshes: Query<(Option<&Aabb>, &GlobalTransform, Option<&RenderLayers>), With<Mesh3d>>,
    mut cameras: Query<(&mut Camera, &mut Transform, &mut Projection), With<StageCamera>>,
    previews: Res<Previews>,
    server: Res<AssetServer>,
) {
    for (e, mut st, root_tf) in &mut stages {
        // The gun's own model missing: its class's shared one instead.
        if matches!(server.get_load_state(&st.scene), Some(bevy::asset::LoadState::Failed(_))) {
            let fallback = crate::actors::prop_file(crate::actors::prop_kind(st.key.0)).0;
            let scene: Handle<WorldAsset> = server.load(GltfAssetLabel::Scene(0).from_asset(fallback));
            if scene != st.scene {
                st.scene = scene.clone();
                st.waited = 0;
                commands.entity(e).insert(WorldAssetRoot(scene));
                continue;
            }
        }
        let Ok((mut cam, mut cam_tf, mut proj)) = cameras.get_mut(st.camera) else { continue };
        if let Some(t) = st.shooting.as_mut() {
            *t += time.delta_secs();
            if *t > 0.5 && !st.checking {
                st.checking = true;
                let image = previews.images[&st.key].clone();
                commands.entity(e).insert(Readback::texture(image));
            }
            continue;
        }
        // The model's meshes, all with bounds, on the stage's layer.
        let mut list = Vec::new();
        let mut stack = vec![e];
        while let Some(x) = stack.pop() {
            if meshes.contains(x) {
                list.push(x);
            }
            if let Ok(c) = children.get(x) {
                stack.extend(c.iter());
            }
        }
        st.waited += 1;
        if list.is_empty() || list.iter().any(|m| meshes.get(*m).is_ok_and(|(a, ..)| a.is_none())) {
            continue;
        }
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for m in &list {
            let Ok((Some(aabb), tf, layers)) = meshes.get(*m) else { continue };
            if layers.is_none() {
                commands.entity(*m).insert(RenderLayers::layer(LAYER));
            }
            let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
            for s in [Vec3::new(-1., -1., -1.), Vec3::new(1., -1., -1.), Vec3::new(-1., 1., -1.), Vec3::new(1., 1., -1.), Vec3::new(-1., -1., 1.), Vec3::new(1., -1., 1.), Vec3::new(-1., 1., 1.), Vec3::new(1., 1., 1.)] {
                let p = tf.transform_point(c + h * s);
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        // Painting happens as the meshes come in; give it (and the layers) a couple of frames.
        if st.waited < 4 {
            continue;
        }
        // Side on: the weapon's length across (its z), its height up; a margin round it, the picture's aspect kept.
        let mid = (lo + hi) * 0.5;
        let (len, tall) = (hi.z - lo.z, hi.y - lo.y);
        let width = (len * 1.1).max(tall * 1.12 * Size::ASPECT);
        cam_tf.translation = Vec3::new(root_tf.translation().x + 4.0, mid.y, mid.z);
        *proj = Projection::Orthographic(OrthographicProjection { scaling_mode: ScalingMode::Fixed { width, height: width / Size::ASPECT }, near: 0.0, far: 8.0, ..OrthographicProjection::default_3d() });
        cam.is_active = true;
        st.shooting = Some(0.0);
    }
}

/// A picture read back: done once the weapon covers enough of it; otherwise the camera keeps rendering a while.
fn checked(ev: On<ReadbackComplete>, mut commands: Commands, mut stages: Query<&mut Stage>, mut previews: ResMut<Previews>) {
    let e = ev.event().entity;
    let Ok(mut st) = stages.get_mut(e) else { return };
    commands.entity(e).remove::<Readback>();
    let data = &ev.event().data;
    let covered = data.chunks_exact(4).filter(|p| p[3] > 128).count();
    if covered * 200 < data.len() / 4 {
        st.shooting = Some(0.0);
        st.checking = false;
        return;
    }
    let key = st.key;
    // `--dump-previews <dir>`: each picture also to a PNG there (QA).
    if let Some(dir) = crate::args::get("dump-previews") {
        let px = key.2.px();
        let row = (px.x as usize * 4).div_ceil(256) * 256;
        let mut rgba = Vec::with_capacity(px.x as usize * px.y as usize * 4);
        for y in 0..px.y as usize {
            rgba.extend_from_slice(&data[y * row..y * row + px.x as usize * 4]);
        }
        let img = Image::new(
            bevy::render::render_resource::Extent3d { width: px.x, height: px.y, depth_or_array_layers: 1 },
            bevy::render::render_resource::TextureDimension::D2,
            rgba,
            TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::MAIN_WORLD,
        );
        if let Ok(d) = img.try_into_dynamic() {
            let _ = d.save(format!("{dir}/{}-{}.png", key.0.key(), crate::finish::finish(key.1).key));
        }
    }
    previews.live.remove(&key);
    previews.done.push(key);
    commands.entity(st.camera).despawn();
    commands.entity(e).despawn();
}

/// `--dump-previews <dir>`: every gun's picture (in `--finish`, or the factory's) at the large size.
fn dump_all(mut previews: ResMut<Previews>, mut images: ResMut<Assets<Image>>) {
    if crate::args::get("dump-previews").is_none() {
        return;
    }
    let f = crate::args::get("finish").and_then(|k| crate::finish::FINISHES.iter().position(|f| f.key == k)).unwrap_or(0) as u8;
    for w in WeaponId::ALL {
        if w.def().is_gun() {
            previews.get(&mut images, w, f, Size::Large);
        }
    }
}
