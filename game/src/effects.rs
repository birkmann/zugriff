//! What the sim's events look like: tracers, muzzle light, impacts (sparks off metal, dust off concrete and plaster,
//! splinters off wood, shards off glass) with bullet holes, blood, brass from the weapon in hand, grenades in flight,
//! explosions, smoke clouds, the bomb, and weapons lying on the floor.
//!
//! Particles are small meshes that fly and fade; puffs are camera-facing quads with a soft round texture; decals are
//! quads laid on the surface with a depth bias, the oldest recycled.

use crate::actors::rand01;
use crate::level::Current;
use crate::session::{Frame, Session};
use crate::view::PlayerCamera;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::{HashMap, VecDeque};
use zugriff_sim::game::{Bomb, Event, SMOKE_RADIUS, SMOKE_TIME};
use zugriff_sim::material::Material;
use zugriff_sim::weapons::{Class, WeaponId};
use zugriff_sim::world::Contents;

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .init_resource::<Decals>()
            .add_systems(Update, (spawn_effects, particles, billboards, lights, tracers, brass, grenades, smokes, bomb, dropped, clear_on_new_round).chain().after(crate::viewmodel::ViewmodelSet));
    }
}

#[derive(Resource)]
pub struct Fx {
    cube: Handle<Mesh>,
    quad: Handle<Mesh>,
    sphere: Handle<Mesh>,
    casing: Handle<Mesh>,
    streak: Handle<Mesh>,
    spark: Handle<StandardMaterial>,
    tracer: Handle<StandardMaterial>,
    brass: Handle<StandardMaterial>,
    blood: Handle<StandardMaterial>,
    splinter: Handle<StandardMaterial>,
    shard: Handle<StandardMaterial>,
    grit: Handle<StandardMaterial>,
    fire: Handle<StandardMaterial>,
    /// Smoke: billowy puffs (a few variants: colour with alpha, and normals) on a quad with tangents.
    clouds: Vec<(Handle<Image>, Handle<Image>)>,
    /// Flame for explosions (additive, unlit): a few turbulent blobs.
    fires: Vec<Handle<StandardMaterial>>,
    cloud_quad: Handle<Mesh>,
    holes: HashMap<u8, Handle<StandardMaterial>>,
    blood_decal: Handle<StandardMaterial>,
    scorch: Handle<StandardMaterial>,
    olive: Handle<StandardMaterial>,
    led_on: Handle<StandardMaterial>,
    led_off: Handle<StandardMaterial>,
}

impl Fx {
    /// Every mesh/material pair the effects draw, for the shader warm-up.
    pub fn warm_set(&self) -> Vec<(Handle<Mesh>, Handle<StandardMaterial>, f32)> {
        let mut v = vec![
            (self.cube.clone(), self.spark.clone(), 0.05),
            (self.cube.clone(), self.blood.clone(), 0.05),
            (self.cube.clone(), self.splinter.clone(), 0.05),
            (self.cube.clone(), self.shard.clone(), 0.05),
            (self.cube.clone(), self.grit.clone(), 0.05),
            (self.cube.clone(), self.olive.clone(), 0.1),
            (self.cube.clone(), self.led_on.clone(), 0.05),
            (self.cube.clone(), self.led_off.clone(), 0.05),
            (self.streak.clone(), self.tracer.clone(), 0.3),
            (self.sphere.clone(), self.fire.clone(), 0.1),
            (self.casing.clone(), self.brass.clone(), 2.0),
            (self.quad.clone(), self.blood_decal.clone(), 0.2),
            (self.quad.clone(), self.scorch.clone(), 0.2),
        ];
        for m in self.holes.values() {
            v.push((self.quad.clone(), m.clone(), 0.2));
        }
        v
    }
}

#[derive(Component)]
struct Particle {
    vel: Vec3,
    life: f32,
    max: f32,
    gravity: f32,
    drag: f32,
    shrink: bool,
}

#[derive(Component)]
struct Puff {
    vel: Vec3,
    life: f32,
    max: f32,
    grow: f32,
    alpha: f32,
    material: Handle<StandardMaterial>,
    base: f32,
}

#[derive(Component)]
struct Billboard;

/// A billboard's turn about the view axis (puffs, flames: so the same texture doesn't read as a stamp).
#[derive(Component)]
struct Roll(f32);

#[derive(Component)]
struct Flicker {
    life: f32,
    max: f32,
    intensity: f32,
}

#[derive(Component)]
struct Tracer {
    vel: Vec3,
    left: f32,
}

#[derive(Component)]
struct Brass {
    vel: Vec3,
    spin: Vec3,
    life: f32,
    rest: bool,
}

#[derive(Component)]
struct GrenadeModel(u32);

#[derive(Component)]
struct SmokeCloud {
    id: u32,
    age: f32,
    /// Each puff, its material, and its offset from the cloud's foot (xyz) and size (w).
    puffs: Vec<(Entity, Handle<StandardMaterial>, Vec4)>,
}

#[derive(Component)]
struct BombModel {
    led: Entity,
    flash: f32,
}

#[derive(Component)]
struct DroppedModel(u32);

/// Everything spawned by this module for the round (cleared on a new round or match).
#[derive(Component)]
struct RoundFx;

#[derive(Resource, Default)]
struct Decals(VecDeque<Entity>);

const MAX_DECALS: usize = 220;

/// A cloud puff for smoke: billowy fbm heights inside a round falloff. Returns the colour (white, darker in the
/// folds, the heights as alpha) and a tangent-space normal map from the heights' slopes, so the live sun shades the
/// billows.
fn cloud_images(size: u32, seed: u32) -> (Image, Image) {
    let s = size as usize;
    let vnoise = |x: f32, y: f32, o: u32| -> f32 {
        let (xi, yi) = (x.floor(), y.floor());
        let (fx, fy) = (x - xi, y - yi);
        let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let h = |i: i32, j: i32| noise((i + 1000) as u32 ^ o.wrapping_mul(7919), (j + 1000) as u32 ^ o.wrapping_mul(104729));
        let (i, j) = (xi as i32, yi as i32);
        let a = h(i, j) + (h(i + 1, j) - h(i, j)) * ux;
        let b = h(i, j + 1) + (h(i + 1, j + 1) - h(i, j + 1)) * ux;
        a + (b - a) * uy
    };
    let mut height = vec![0f32; s * s];
    for y in 0..s {
        for x in 0..s {
            let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            let mut f = 0.0;
            let mut amp = 0.5;
            let mut freq = 1.7;
            for o in 0..4 {
                f += amp * vnoise(u * freq + 7.3, v * freq + 3.1, seed + o);
                amp *= 0.5;
                freq *= 2.1;
            }
            // Round overall, the rim eaten into by the noise.
            let fall = (1.0 - r * r).max(0.0);
            height[y * s + x] = (fall * (0.55 + 0.9 * f) - 0.25).max(0.0);
        }
    }
    let mut col = vec![0u8; s * s * 4];
    let mut nor = vec![0u8; s * s * 4];
    for y in 0..s {
        for x in 0..s {
            let hgt = |xx: i32, yy: i32| height[(yy.clamp(0, s as i32 - 1) as usize) * s + xx.clamp(0, s as i32 - 1) as usize];
            let h = hgt(x as i32, y as i32);
            let dx = hgt(x as i32 + 1, y as i32) - hgt(x as i32 - 1, y as i32);
            let dy = hgt(x as i32, y as i32 + 1) - hgt(x as i32, y as i32 - 1);
            let n = Vec3::new(-dx * 4.5, dy * 4.5, 1.0).normalize();
            let i = (y * s + x) * 4;
            let shade = (0.72 + 0.28 * (h * 3.0).min(1.0)) * 255.0;
            col[i] = shade as u8;
            col[i + 1] = shade as u8;
            col[i + 2] = (shade * 1.01).min(255.0) as u8;
            col[i + 3] = ((h * 2.1).clamp(0.0, 1.0).powf(1.1) * 255.0) as u8;
            nor[i] = ((n.x * 0.5 + 0.5) * 255.0) as u8;
            nor[i + 1] = ((n.y * 0.5 + 0.5) * 255.0) as u8;
            nor[i + 2] = ((n.z * 0.5 + 0.5) * 255.0) as u8;
            nor[i + 3] = 255;
        }
    }
    let ext = Extent3d { width: size, height: size, depth_or_array_layers: 1 };
    (
        Image::new(ext, TextureDimension::D2, col, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD),
        Image::new(ext, TextureDimension::D2, nor, TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD),
    )
}

/// Flame: turbulent fbm inside a round falloff, as colour (white-yellow at the hottest, through orange to a dim red
/// rim) with the heat as alpha, for additive blending.
fn fire_image(size: u32, seed: u32) -> Image {
    let (col, _) = cloud_images(size, seed);
    let mut data = col.data.clone().unwrap_or_default();
    for px in data.chunks_mut(4) {
        let h = px[3] as f32 / 255.0;
        let t = h.powf(0.8);
        let (r, g, b) = (1.0, (0.25 + 0.75 * t * t).min(1.0), (0.05 + 0.8 * t.powf(4.0)).min(1.0));
        px[0] = (r * 255.0) as u8;
        px[1] = (g * 255.0) as u8;
        px[2] = (b * 255.0) as u8;
        px[3] = ((h * 1.3).min(1.0) * 255.0) as u8;
    }
    Image::new(Extent3d { width: size, height: size, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}

fn noise(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

/// A bullet hole: a dark core, a rim and cracks, on transparent.
fn hole_image(core: [u8; 3], rim: [u8; 3], cracks: bool, star: bool) -> Image {
    let size = 64u32;
    let mut data = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            let ang = v.atan2(u);
            let i = ((y * size + x) * 4) as usize;
            let jag = 0.06 * (ang * 7.0).sin() + 0.04 * (ang * 13.0).cos();
            let (c, a) = if r < 0.16 + jag * 0.5 {
                (core, 1.0)
            } else if r < 0.42 + jag {
                (rim, (1.0 - (r - 0.16) / 0.3).clamp(0.0, 1.0) * 0.85)
            } else if (cracks || star) && ((ang * if star { 6.0 } else { 4.0 }).sin().abs() > 0.97) && r < if star { 0.98 } else { 0.8 } {
                (if star { [230, 240, 245] } else { core }, 0.5 * (1.0 - r))
            } else {
                ([0, 0, 0], 0.0)
            };
            data[i] = c[0];
            data[i + 1] = c[1];
            data[i + 2] = c[2];
            data[i + 3] = (a * 255.0) as u8;
        }
    }
    Image::new(Extent3d { width: size, height: size, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}

fn splat_image(rgb: [u8; 3]) -> Image {
    let size = 64u32;
    let mut data = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            let ang = v.atan2(u);
            let edge = 0.55 + 0.2 * (ang * 5.0).sin() * (ang * 3.0).cos() + 0.15 * noise(x / 4, y / 4);
            let drop = noise(x / 3 + 17, y / 3 + 5) > 0.93 && r < 0.95;
            let a = if r < edge { 0.9 } else if drop { 0.8 } else { 0.0 };
            let i = ((y * size + x) * 4) as usize;
            let k = 0.8 + 0.2 * noise(x, y);
            data[i] = (rgb[0] as f32 * k) as u8;
            data[i + 1] = (rgb[1] as f32 * k) as u8;
            data[i + 2] = (rgb[2] as f32 * k) as u8;
            data[i + 3] = (a * 255.0) as u8;
        }
    }
    Image::new(Extent3d { width: size, height: size, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}

fn mat_key(m: Material) -> u8 {
    match m {
        Material::Metal => 1,
        Material::Wood => 2,
        Material::Glass => 3,
        Material::Dirt | Material::Grass => 4,
        _ => 0,
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    let unlit = |c: Color, e: LinearRgba| StandardMaterial { base_color: c, emissive: e, unlit: true, ..default() };
    let decal = |img: Handle<Image>| StandardMaterial { base_color_texture: Some(img), alpha_mode: AlphaMode::Blend, perceptual_roughness: 0.9, depth_bias: 40.0, ..default() };
    let mut holes = HashMap::new();
    holes.insert(0, materials.add(decal(images.add(hole_image([20, 18, 16], [70, 66, 60], true, false)))));
    holes.insert(1, materials.add(decal(images.add(hole_image([12, 12, 14], [150, 150, 155], false, false)))));
    holes.insert(2, materials.add(decal(images.add(hole_image([30, 18, 8], [90, 60, 30], true, false)))));
    holes.insert(3, materials.add(decal(images.add(hole_image([40, 50, 55], [200, 215, 225], false, true)))));
    holes.insert(4, materials.add(decal(images.add(hole_image([30, 24, 16], [60, 48, 34], false, false)))));
    let clouds = (0..4)
        .map(|i| {
            let (c, n) = cloud_images(128, 11 + i * 17);
            (images.add(c), images.add(n))
        })
        .collect();
    let fires = (0..3)
        .map(|i| {
            let img = fire_image(128, 31 + i * 13);
            materials.add(StandardMaterial {
                base_color: LinearRgba::rgb(2.6, 1.35, 0.5).into(),
                base_color_texture: Some(images.add(img)),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                double_sided: true,
                cull_mode: None,
                ..default()
            })
        })
        .collect();
    let mut cq = Mesh::from(Rectangle::new(1.0, 1.0));
    let _ = cq.generate_tangents();
    let cloud_quad = meshes.add(cq);
    commands.insert_resource(Fx {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        sphere: meshes.add(Sphere::new(1.0).mesh().uv(12, 8)),
        casing: meshes.add(Cylinder::new(0.0045, 0.03)),
        streak: meshes.add(Cuboid::new(0.018, 0.018, 1.0)),
        spark: materials.add(unlit(Color::srgb(1.0, 0.85, 0.4), LinearRgba::rgb(40.0, 22.0, 6.0))),
        tracer: materials.add(unlit(Color::srgb(1.0, 0.8, 0.5), LinearRgba::rgb(25.0, 14.0, 5.0))),
        brass: materials.add(StandardMaterial { base_color: Color::srgb(0.8, 0.62, 0.26), metallic: 0.9, perceptual_roughness: 0.3, ..default() }),
        blood: materials.add(StandardMaterial { base_color: Color::srgb(0.45, 0.02, 0.02), perceptual_roughness: 0.4, ..default() }),
        splinter: materials.add(StandardMaterial { base_color: Color::srgb(0.55, 0.4, 0.22), perceptual_roughness: 0.9, ..default() }),
        shard: materials.add(StandardMaterial { base_color: Color::srgba(0.8, 0.9, 0.95, 0.6), alpha_mode: AlphaMode::Blend, perceptual_roughness: 0.05, ..default() }),
        grit: materials.add(StandardMaterial { base_color: Color::srgb(0.45, 0.43, 0.4), perceptual_roughness: 1.0, ..default() }),
        fire: materials.add(StandardMaterial { base_color: Color::srgba(1.0, 0.6, 0.2, 0.9), emissive: LinearRgba::rgb(80.0, 30.0, 6.0), unlit: true, alpha_mode: AlphaMode::Add, ..default() }),
        clouds,
        fires,
        cloud_quad,
        holes,
        blood_decal: materials.add(decal(images.add(splat_image([110, 6, 6])))),
        scorch: materials.add(decal(images.add(splat_image([14, 12, 10])))),
        olive: materials.add(StandardMaterial { base_color: Color::srgb(0.24, 0.26, 0.19), perceptual_roughness: 0.8, ..default() }),
        led_on: materials.add(unlit(Color::srgb(1.0, 0.1, 0.05), LinearRgba::rgb(60.0, 2.0, 1.0))),
        led_off: materials.add(StandardMaterial { base_color: Color::srgb(0.2, 0.02, 0.02), ..default() }),
    });
}

fn clear_on_new_round(mut commands: Commands, frame: Res<Frame>, session: Option<Res<Session>>, fx: Query<Entity, With<RoundFx>>, mut decals: ResMut<Decals>) {
    let new_round = frame.started || session.is_none() || frame.events.iter().any(|e| matches!(e, Event::RoundStart { .. }));
    if !new_round || fx.is_empty() {
        return;
    }
    for e in &fx {
        commands.entity(e).despawn();
    }
    decals.0.clear();
}

fn decal(commands: &mut Commands, decals: &mut Decals, fx: &Fx, material: Handle<StandardMaterial>, pos: Vec3, normal: Vec3, size: f32) {
    let n = normal.normalize_or(Vec3::Y);
    let up = if n.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
    let rot = Transform::from_translation(pos + n * 0.004).looking_to(-n, up).rotation * Quat::from_rotation_z(rand01() * std::f32::consts::TAU);
    let e = commands.spawn((Mesh3d(fx.quad.clone()), MeshMaterial3d(material), Transform::from_translation(pos + n * 0.004).with_rotation(rot).with_scale(Vec3::splat(size)), bevy::light::NotShadowCaster, RoundFx)).id();
    decals.0.push_back(e);
    while decals.0.len() > MAX_DECALS {
        if let Some(old) = decals.0.pop_front() {
            commands.entity(old).try_despawn();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn burst(commands: &mut Commands, fx: &Fx, material: &Handle<StandardMaterial>, at: Vec3, dir: Vec3, n: usize, speed: f32, size: f32, life: f32, gravity: f32) {
    for _ in 0..n {
        let r = Vec3::new(rand01() - 0.5, rand01() - 0.5, rand01() - 0.5) * 2.0;
        let v = (dir + r * 0.8).normalize_or(Vec3::Y) * speed * (0.4 + rand01() * 0.8);
        let s = size * (0.6 + rand01() * 0.8);
        commands.spawn((
            Mesh3d(fx.cube.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(s)).with_rotation(Quat::from_euler(EulerRot::XYZ, rand01() * 6.0, rand01() * 6.0, 0.0)),
            Particle { vel: v, life: 0.0, max: life * (0.6 + rand01() * 0.6), gravity, drag: 1.5, shrink: true },
            bevy::light::NotShadowCaster,
            RoundFx,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn puff(commands: &mut Commands, fx: &Fx, materials: &mut Assets<StandardMaterial>, at: Vec3, vel: Vec3, color: Color, size: f32, grow: f32, life: f32, alpha: f32, lit: bool) -> (Entity, Handle<StandardMaterial>) {
    // A billowy cloud texture (lit ones with its normals, so the sun shades them), turned at random.
    let (tex, nor) = fx.clouds[(rand01() * fx.clouds.len() as f32) as usize % fx.clouds.len()].clone();
    // Smoke and dust scatter light through themselves: a glow of their own colour keeps the shaded side from going
    // black (lit only by the ambient, a puff in shade against a lightmapped wall was a dark hole).
    // (scaled by hand: `LinearRgba * f` scales alpha too, and the emissive's alpha weights the exposure)
    let c = LinearRgba::from(color);
    let glow = LinearRgba::rgb(c.red * 300.0, c.green * 300.0, c.blue * 300.0);
    let material = materials.add(StandardMaterial {
        base_color: color.with_alpha(0.0),
        base_color_texture: Some(tex),
        normal_map_texture: lit.then_some(nor),
        emissive: if lit { glow } else { LinearRgba::BLACK },
        alpha_mode: AlphaMode::Blend,
        unlit: !lit,
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let e = commands
        .spawn((
            Mesh3d(fx.cloud_quad.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(size)),
            Puff { vel, life: 0.0, max: life, grow, alpha, material: material.clone(), base: size },
            Billboard,
            Roll(rand01() * std::f32::consts::TAU),
            bevy::light::NotShadowCaster,
            RoundFx,
        ))
        .id();
    (e, material)
}

fn flash_light(commands: &mut Commands, at: Vec3, color: Color, intensity: f32, range: f32, life: f32) {
    commands.spawn((PointLight { color, intensity, range, shadow_maps_enabled: false, ..default() }, Transform::from_translation(at), Flicker { life: 0.0, max: life, intensity }, RoundFx));
}

#[allow(clippy::too_many_arguments)]
fn spawn_effects(
    mut commands: Commands,
    frame: Res<Frame>,
    session: Option<Res<Session>>,
    fx: Res<Fx>,
    current: Res<Current>,
    markers: Res<crate::viewmodel::Markers>,
    spectate: Res<crate::view::Spectate>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut decals: ResMut<Decals>,
    cam: Single<&GlobalTransform, With<PlayerCamera>>,
) {
    let Some(s) = session else { return };
    if crate::args::flag("nofx") {
        return;
    }
    let viewed = s.viewed(spectate.target);
    let first_person = s.game.players[viewed].alive;
    let cam_pos = cam.translation();
    for e in &frame.events {
        match e {
            Event::Shot { player, weapon, from, end, tracer, .. } => {
                let mine = *player == viewed && first_person;
                let muzzle = if mine { markers.muzzle.unwrap_or(*from) } else { *from + (*end - *from).normalize_or_zero() * 0.6 - Vec3::Y * 0.15 };
                let class = weapon.def().class;
                if class != Class::Knife {
                    flash_light(&mut commands, muzzle, Color::srgb(1.0, 0.75, 0.4), if class == Class::Pistol { 40_000.0 } else { 90_000.0 }, 7.0, 0.05);
                }
                if *tracer || (!mine && rand01() < 0.25) {
                    let dir = (*end - muzzle).normalize_or_zero();
                    let dist = muzzle.distance(*end);
                    if dist > 2.0 {
                        let speed = 220.0;
                        commands.spawn((
                            Mesh3d(fx.streak.clone()),
                            MeshMaterial3d(fx.tracer.clone()),
                            Transform::from_translation(muzzle + dir * 1.0).looking_to(dir, Vec3::Y).with_scale(Vec3::new(1.0, 1.0, 1.4)),
                            Tracer { vel: dir * speed, left: (dist - 1.0) / speed },
                            bevy::light::NotShadowCaster,
                            RoundFx,
                        ));
                    }
                }
                // Brass from the weapon in hand.
                if mine && class != Class::Shotgun && class != Class::Knife
                    && let Some(ej) = markers.ejection
                {
                    let v = markers.right * (1.8 + rand01()) + Vec3::Y * (1.2 + rand01() * 0.8) + s.game.players[viewed].mover.velocity;
                    commands.spawn((Mesh3d(fx.casing.clone()), MeshMaterial3d(fx.brass.clone()), Transform::from_translation(ej), Brass { vel: v, spin: Vec3::new(rand01() * 30.0, rand01() * 20.0, 10.0), life: 0.0, rest: false }, bevy::light::NotShadowCaster, RoundFx));
                }
            }
            Event::Impact { pos, normal, material, exit } => {
                let n = *normal;
                let out = if *exit { n } else { n };
                match material {
                    Material::Metal => burst(&mut commands, &fx, &fx.spark, *pos + out * 0.01, out, 6, 7.0, 0.012, 0.35, 9.0),
                    Material::Wood => burst(&mut commands, &fx, &fx.splinter, *pos + out * 0.01, out, 5, 3.5, 0.02, 0.8, 9.0),
                    Material::Glass => burst(&mut commands, &fx, &fx.shard, *pos + out * 0.01, out, 6, 3.0, 0.02, 0.9, 9.0),
                    _ => {
                        burst(&mut commands, &fx, &fx.grit, *pos + out * 0.01, out, 9, 4.0, 0.012, 0.7, 9.0);
                        // a few quick sparks off stone and concrete
                        if !matches!(material, Material::Dirt | Material::Grass | Material::Flesh) {
                            burst(&mut commands, &fx, &fx.spark, *pos + out * 0.01, out, 3, 6.0, 0.008, 0.12, 9.0);
                        }
                    }
                }
                let (color, size) = match material {
                    Material::Dirt | Material::Grass => (Color::srgb(0.42, 0.35, 0.26), 0.35),
                    Material::Wood => (Color::srgb(0.5, 0.42, 0.3), 0.22),
                    Material::Metal => (Color::srgb(0.6, 0.6, 0.6), 0.12),
                    Material::Glass => (Color::srgb(0.85, 0.9, 0.95), 0.1),
                    Material::Brick => (Color::srgb(0.62, 0.42, 0.34), 0.3),
                    _ => (Color::srgb(0.7, 0.68, 0.64), 0.3),
                };
                // Dust: a quick jet out of the hole, and a slower cloud that hangs and drifts down.
                let dim = |c: Color| { let l = LinearRgba::from(c); Color::linear_rgb(l.red * 0.55, l.green * 0.55, l.blue * 0.55) };
                puff(&mut commands, &fx, &mut materials, *pos + out * 0.05, out * 3.2 + Vec3::Y * 0.3, dim(color), size * 0.6, 2.4, 0.4, 0.45, true);
                puff(&mut commands, &fx, &mut materials, *pos + out * 0.12, out * 0.9 + Vec3::Y * 0.15, dim(color), size, 2.8, 1.5, 0.35, true);
                if !matches!(material, Material::Flesh) && cam_pos.distance(*pos) < 60.0 {
                    let m = fx.holes[&mat_key(*material)].clone();
                    let size = if *material == Material::Glass { 0.12 } else { 0.055 };
                    decal(&mut commands, &mut decals, &fx, m, *pos, n, size);
                }
            }
            Event::Hit { pos, dir, damage, .. } => {
                let back = -*dir;
                burst(&mut commands, &fx, &fx.blood, *pos, back + *dir * 0.5, (*damage as usize / 12).clamp(2, 8), 2.0, 0.018, 0.5, 9.0);
                puff(&mut commands, &fx, &mut materials, *pos, *dir * 0.6, Color::srgb(0.5, 0.03, 0.03), 0.25, 1.4, 0.5, 0.8, true);
                // Blood on the wall behind, or the floor below.
                if let Some(level) = &current.level {
                    let t = level.world.trace(*pos, *pos + *dir * 2.5, Vec3::ZERO, Contents::MASK_SHOT);
                    if t.hit() && !t.start_solid {
                        decal(&mut commands, &mut decals, &fx, fx.blood_decal.clone(), t.end, t.normal, 0.25 + *damage as f32 * 0.004);
                    } else {
                        let t = level.world.trace(*pos, *pos - Vec3::Y * 2.2, Vec3::ZERO, Contents::MASK_SHOT);
                        if t.hit() && !t.start_solid {
                            decal(&mut commands, &mut decals, &fx, fx.blood_decal.clone(), t.end, t.normal, 0.3);
                        }
                    }
                }
            }
            Event::HostageHit { pos, dir, .. } => {
                burst(&mut commands, &fx, &fx.blood, *pos, -*dir, 4, 2.0, 0.018, 0.5, 9.0);
            }
            Event::Detonate { kind, pos, .. } => match kind {
                WeaponId::He => explosion(&mut commands, &fx, &mut materials, &mut decals, *pos, 1.0, &current),
                WeaponId::Flash => {
                    flash_light(&mut commands, *pos + Vec3::Y * 0.2, Color::WHITE, 4_000_000.0, 30.0, 0.12);
                    puff(&mut commands, &fx, &mut materials, *pos + Vec3::Y * 0.2, Vec3::ZERO, Color::WHITE, 0.6, 4.0, 0.25, 1.0, false);
                    burst(&mut commands, &fx, &fx.spark, *pos, Vec3::Y, 10, 4.0, 0.01, 0.4, 5.0);
                }
                _ => {
                    // Smoke: a hiss and a cloud that billows out (the cloud system builds it).
                    commands.spawn((Transform::from_translation(*pos), Visibility::Inherited, SmokeCloud { id: match e {
                        Event::Detonate { grenade, .. } => *grenade,
                        _ => 0,
                    }, age: 0.0, puffs: Vec::new() }, RoundFx));
                }
            },
            Event::Exploded { pos } => explosion(&mut commands, &fx, &mut materials, &mut decals, *pos, 3.0, &current),
            Event::KnifeHit { pos, material: Some(m), .. } if *m != Material::Flesh => {
                burst(&mut commands, &fx, &fx.spark, *pos, Vec3::Y, 3, 2.0, 0.01, 0.2, 9.0);
            }
            _ => {}
        }
    }
}

fn explosion(commands: &mut Commands, fx: &Fx, materials: &mut Assets<StandardMaterial>, decals: &mut Decals, pos: Vec3, scale: f32, current: &Current) {
    let c = pos + Vec3::Y * 0.3 * scale;
    flash_light(commands, c, Color::srgb(1.0, 0.7, 0.35), 3_000_000.0 * scale, 18.0 * scale, 0.35);
    // The fireball: flame billboards bursting out from the centre, each flaring and shrinking away in a third of a
    // second (their material is shared: the fade is in the scale).
    for i in 0..(10.0 * scale.sqrt()) as usize {
        let d = Vec3::new(rand01() - 0.5, rand01() * 0.8 - 0.1, rand01() - 0.5).normalize_or(Vec3::Y);
        let m = fx.fires[i % fx.fires.len()].clone();
        commands.spawn((
            Mesh3d(fx.quad.clone()),
            MeshMaterial3d(m),
            Transform::from_translation(c + d * 0.3 * scale).with_scale(Vec3::splat(0.2)),
            Particle { vel: d * (3.0 + rand01() * 3.0) * scale.sqrt(), life: 0.0, max: 0.22 + rand01() * 0.18, gravity: -2.0, drag: 6.0, shrink: false },
            Fireball((0.9 + rand01() * 0.7) * scale),
            Billboard,
            Roll(rand01() * std::f32::consts::TAU),
            bevy::light::NotShadowCaster,
            RoundFx,
        ));
    }
    burst(commands, fx, &fx.spark, c, Vec3::Y, (18.0 * scale) as usize, 11.0, 0.02, 0.6, 9.0);
    burst(commands, fx, &fx.grit, c, Vec3::Y, (10.0 * scale) as usize, 7.0, 0.04, 1.2, 9.0);
    // Smoke: dark and dense at first, rising and spreading, a few lighter dust puffs low down.
    for i in 0..(12.0 * scale) as usize {
        let r = Vec3::new(rand01() - 0.5, rand01() * 0.7, rand01() - 0.5) * 1.8 * scale;
        let grey = 0.17 + rand01() * 0.14;
        puff(commands, fx, materials, c + r, Vec3::Y * (0.8 + rand01() * 0.9) + r * 0.5, Color::srgb(grey, grey * 0.97, grey * 0.93), 1.1 * scale, 1.9, 2.6 + i as f32 * 0.18, 0.9, true);
    }
    for _ in 0..(6.0 * scale) as usize {
        let r = Vec3::new(rand01() - 0.5, 0.0, rand01() - 0.5) * 3.0 * scale;
        puff(commands, fx, materials, pos + r + Vec3::Y * 0.3, r * 0.8, Color::srgb(0.42, 0.38, 0.32), 0.8 * scale, 2.2, 1.8, 0.6, true);
    }
    if let Some(level) = &current.level {
        let t = level.world.trace(c, c - Vec3::Y * 2.0, Vec3::ZERO, Contents::MASK_SHOT);
        if t.hit() && !t.start_solid {
            decal(commands, decals, fx, fx.scorch.clone(), t.end, t.normal, 1.6 * scale);
        }
    }
}

/// A fireball grows to this radius.
#[derive(Component)]
struct Fireball(f32);

fn particles(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Particle, &mut Transform, Option<&Fireball>)>, mut puffs: Query<(Entity, &mut Puff, &mut Transform), Without<Particle>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let dt = time.delta_secs();
    for (e, mut p, mut tf, fireball) in &mut q {
        p.life += dt;
        if p.life >= p.max {
            commands.entity(e).despawn();
            continue;
        }
        if let Some(Fireball(r)) = fireball {
            let k = p.life / p.max;
            // Flares up fast, then shrinks away as it drifts out.
            tf.scale = Vec3::splat(r * (k * 5.0).min(1.0) * (1.0 - k).powf(0.7));
            let drag = p.drag;
            p.vel *= (1.0 - drag * dt).max(0.0);
            tf.translation += p.vel * dt;
            continue;
        }
        p.vel.y -= p.gravity * dt;
        let drag = p.drag;
        p.vel *= (1.0 - drag * dt).max(0.0);
        tf.translation += p.vel * dt;
        if p.shrink {
            let k = 1.0 - p.life / p.max;
            let s = tf.scale.x;
            tf.scale = Vec3::splat((s * (0.97 + 0.03 * k)).max(0.001));
        }
    }
    for (e, mut p, mut tf) in &mut puffs {
        p.life += dt;
        if p.life >= p.max {
            materials.remove(&p.material);
            commands.entity(e).despawn();
            continue;
        }
        let k = p.life / p.max;
        let v = p.vel;
        tf.translation += v * dt;
        p.vel *= (1.0 - 1.5 * dt).max(0.0);
        tf.scale = Vec3::splat(p.base * (1.0 + p.grow * k.sqrt()));
        let a = p.alpha * (1.0 - k) * (k * 8.0).min(1.0);
        if let Some(mut m) = materials.get_mut(&p.material) {
            m.base_color.set_alpha(a);
        }
    }
}

fn billboards(cam: Single<&GlobalTransform, With<PlayerCamera>>, mut q: Query<(&mut Transform, Option<&Roll>), With<Billboard>>) {
    let cam_rot = cam.to_scale_rotation_translation().1;
    for (mut tf, roll) in &mut q {
        tf.rotation = roll.map_or(cam_rot, |r| cam_rot * Quat::from_rotation_z(r.0));
    }
}

fn lights(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Flicker, &mut PointLight)>) {
    let dt = time.delta_secs();
    for (e, mut f, mut l) in &mut q {
        f.life += dt;
        if f.life >= f.max {
            commands.entity(e).despawn();
            continue;
        }
        l.intensity = f.intensity * (1.0 - f.life / f.max);
    }
}

fn tracers(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Tracer, &mut Transform)>) {
    let dt = time.delta_secs();
    for (e, mut t, mut tf) in &mut q {
        t.left -= dt;
        if t.left <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        tf.translation += t.vel * dt;
    }
}

fn brass(mut commands: Commands, time: Res<Time>, current: Res<Current>, mut q: Query<(Entity, &mut Brass, &mut Transform)>, mut sounds: MessageWriter<crate::audio::Cue>) {
    let dt = time.delta_secs();
    let Some(level) = &current.level else { return };
    for (e, mut b, mut tf) in &mut q {
        b.life += dt;
        if b.life > 8.0 {
            commands.entity(e).despawn();
            continue;
        }
        if b.rest {
            continue;
        }
        b.vel.y -= 9.8 * dt;
        let to = tf.translation + b.vel * dt;
        let t = level.world.trace(tf.translation, to, Vec3::splat(0.005), Contents::MASK_GRENADE);
        tf.translation = t.end;
        if t.hit() {
            let speed = b.vel.length();
            b.vel = (b.vel - t.normal * b.vel.dot(t.normal) * 1.5) * 0.35;
            if speed > 1.0 {
                sounds.write(crate::audio::Cue::Brass(tf.translation));
            }
            if t.normal.y > 0.7 && b.vel.length() < 0.4 {
                b.rest = true;
                tf.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_y(rand01() * 6.0);
            }
        }
        let spin = b.spin * dt;
        tf.rotation *= Quat::from_euler(EulerRot::XYZ, spin.x, spin.y, spin.z);
    }
}

fn grenades(mut commands: Commands, server: Res<AssetServer>, session: Option<Res<Session>>, fixed: Res<Time<Fixed>>, time: Res<Time>, mut q: Query<(Entity, &GrenadeModel, &mut Transform)>) {
    let Some(s) = session else { return };
    let a = fixed.overstep_fraction();
    let mut have = Vec::new();
    for (e, g, mut tf) in &mut q {
        match s.grenade_pos(g.0, a) {
            Some(p) => {
                tf.translation = p;
                if let Some(n) = s.game.grenades.iter().find(|n| n.id == g.0)
                    && !n.rest
                {
                    let spin = n.spin * time.delta_secs();
                    tf.rotation *= Quat::from_euler(EulerRot::XYZ, spin.x, spin.y, spin.z);
                }
                have.push(g.0);
            }
            None => commands.entity(e).despawn(),
        }
    }
    for n in &s.game.grenades {
        if have.contains(&n.id) {
            continue;
        }
        let scene = server.load(GltfAssetLabel::Scene(0).from_asset("models/grenade.glb"));
        commands.spawn((WorldAssetRoot(scene), Transform::from_translation(n.pos), Visibility::Inherited, GrenadeModel(n.id), RoundFx));
    }
}

fn smokes(mut commands: Commands, time: Res<Time>, fx: Res<Fx>, frame: Res<Frame>, mut materials: ResMut<Assets<StandardMaterial>>, mut clouds: Query<(Entity, &mut SmokeCloud, &Transform)>, mut puffs: Query<&mut Transform, (Without<SmokeCloud>, With<Billboard>)>) {
    let dt = time.delta_secs();
    let gone: Vec<u32> = frame.events.iter().filter_map(|e| if let Event::SmokeGone { smoke } = e { Some(*smoke) } else { None }).collect();
    for (e, mut cloud, tf) in &mut clouds {
        cloud.age += dt;
        let centre = tf.translation;
        if cloud.puffs.is_empty() {
            // Puffs filling a dome as wide as the sim's smoke (it blocks sight through SMOKE_RADIUS), bigger towards
            // the middle, lighter at the top where the sun catches it and greyer in the shadowed base.
            const N: usize = 60;
            for i in 0..N {
                let a = i as f32 * 2.39996;
                let h = (i as f32 + 0.5) / N as f32;
                let r = SMOKE_RADIUS * 0.85 * (1.0 - h * h).sqrt().max(0.3) * (0.35 + 0.65 * rand01().sqrt());
                let off = Vec3::new(a.cos() * r, 0.25 + h * SMOKE_RADIUS * 0.85, a.sin() * r);
                let grey = 0.56 + 0.3 * h + rand01() * 0.06;
                let (tex, nor) = fx.clouds[i % fx.clouds.len()].clone();
                let material = materials.add(StandardMaterial {
                    emissive: LinearRgba::rgb(grey, grey, grey) * 700.0,
                    base_color: Color::srgba(grey, grey, grey * 1.01, 0.0),
                    base_color_texture: Some(tex),
                    normal_map_texture: Some(nor),
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 1.0,
                    reflectance: 0.0,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                });
                let size = 2.6 + 1.6 * (1.0 - h) + rand01() * 1.0;
                let p = commands.spawn((Mesh3d(fx.cloud_quad.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(centre).with_scale(Vec3::splat(0.5)), Billboard, bevy::light::NotShadowCaster, RoundFx)).id();
                cloud.puffs.push((p, material, off.extend(size)));
            }
        }
        let grow = (cloud.age / 1.6).min(1.0);
        let fade = ((SMOKE_TIME - cloud.age) / 3.0).clamp(0.0, 1.0);
        let alpha = 0.97 * grow.powf(0.5) * fade;
        let t = cloud.age;
        for (i, (p, m, off)) in cloud.puffs.iter().enumerate() {
            if let Ok(mut ptf) = puffs.get_mut(*p) {
                // Billowing: a slow drift, and each puff turning slowly about the view axis.
                let drift = Vec3::new((t * 0.2 + i as f32).sin(), (t * 0.13 + i as f32 * 0.7).sin() * 0.4, (t * 0.17 + i as f32 * 1.3).cos()) * 0.3;
                ptf.translation = centre + off.truncate() * (0.25 + 0.75 * grow.powf(0.7)) + drift;
                ptf.scale = Vec3::splat(off.w * (0.35 + 0.65 * grow));
                let roll = (i as f32 * 1.7) + t * (if i % 2 == 0 { 0.05 } else { -0.04 });
                ptf.rotation *= Quat::from_rotation_z(roll);
            }
            if let Some(mut mat) = materials.get_mut(m) {
                mat.base_color.set_alpha(alpha);
            }
        }
        if gone.contains(&cloud.id) || cloud.age > SMOKE_TIME + 0.5 {
            for (p, m, _) in cloud.puffs.drain(..) {
                commands.entity(p).try_despawn();
                materials.remove(&m);
            }
            commands.entity(e).despawn();
        }
    }
}

fn bomb(mut commands: Commands, session: Option<Res<Session>>, fx: Res<Fx>, time: Res<Time>, frame: Res<Frame>, mut q: Query<(Entity, &mut BombModel, &mut Transform, &mut Visibility)>, mut leds: Query<&mut MeshMaterial3d<StandardMaterial>>, mut meshes: ResMut<Assets<Mesh>>) {
    let Some(s) = session else {
        for (e, ..) in &q {
            commands.entity(e).despawn();
        }
        return;
    };
    let at = match s.game.bomb {
        Bomb::Planted { pos, .. } => Some((pos, true)),
        Bomb::Dropped { pos, .. } => Some((pos - Vec3::Y * 0.06, false)),
        Bomb::Defused { pos } => Some((pos, false)),
        _ => None,
    };
    if q.is_empty() {
        let body = commands.spawn((Mesh3d(meshes.add(Cuboid::new(0.2, 0.1, 0.28))), MeshMaterial3d(fx.olive.clone()), Transform::from_xyz(0.0, 0.05, 0.0))).id();
        let led = commands.spawn((Mesh3d(meshes.add(Cuboid::new(0.02, 0.012, 0.02))), MeshMaterial3d(fx.led_off.clone()), Transform::from_xyz(0.05, 0.106, 0.07))).id();
        let keypad = commands.spawn((Mesh3d(meshes.add(Cuboid::new(0.08, 0.01, 0.1))), MeshMaterial3d(fx.olive.clone()), Transform::from_xyz(-0.03, 0.104, 0.0))).id();
        let mut root = commands.spawn((Transform::default(), Visibility::Hidden, BombModel { led, flash: 0.0 }));
        root.add_children(&[body, led, keypad]);
        return;
    }
    let beeped = frame.events.iter().any(|e| matches!(e, Event::Beep { .. }));
    for (_, mut b, mut tf, mut vis) in &mut q {
        b.flash -= time.delta_secs();
        if beeped {
            b.flash = 0.08;
        }
        match at {
            Some((pos, planted)) => {
                *vis = Visibility::Inherited;
                tf.translation = pos;
                if let Ok(mut m) = leds.get_mut(b.led) {
                    m.0 = if planted && b.flash > 0.0 { fx.led_on.clone() } else { fx.led_off.clone() };
                }
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

fn dropped(mut commands: Commands, server: Res<AssetServer>, session: Option<Res<Session>>, mut q: Query<(Entity, &DroppedModel, &mut Transform)>) {
    let Some(s) = session else {
        for (e, ..) in &q {
            commands.entity(e).despawn();
        }
        return;
    };
    let mut have = Vec::new();
    for (e, d, mut tf) in &mut q {
        match s.game.dropped.iter().find(|x| x.id == d.0) {
            Some(x) => {
                tf.translation = x.pos + Vec3::Y * 0.03;
                have.push(d.0);
            }
            None => commands.entity(e).despawn(),
        }
    }
    for d in &s.game.dropped {
        if have.contains(&d.id) {
            continue;
        }
        let scene = server.load(GltfAssetLabel::Scene(0).from_asset(crate::gear::prop_path(d.gun.id)));
        // Lying on its side, in its finish.
        let rot = Quat::from_rotation_y(d.yaw) * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let painted = crate::finish::Painted { weapon: d.gun.id, finish: d.gun.finish };
        commands.spawn((WorldAssetRoot(scene), Transform::from_translation(d.pos).with_rotation(rot), Visibility::Inherited, DroppedModel(d.id), RoundFx, painted));
    }
}
