//! The sky: a cubemap painted at load for each map — deep blue overhead fading to a pale hazy horizon, soft cloud
//! banks, and the sun where the map's `light_environment` puts it — shown as the camera's skybox.

use crate::level::Current;
use crate::view::PlayerCamera;
use bevy::asset::RenderAssetUsages;
use bevy::light::Skybox;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, paint);
    }
}

const FACE: u32 = 512;

fn hash(x: i32, y: i32, z: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ (z as u32).wrapping_mul(2147483647);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

/// Smooth 3D value noise.
fn noise(p: Vec3) -> f32 {
    let i = p.floor();
    let f = p - i;
    let f = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let (x, y, z) = (i.x as i32, i.y as i32, i.z as i32);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx, dy, dz| hash(x + dx, y + dy, z + dz);
    lerp(
        lerp(lerp(c(0, 0, 0), c(1, 0, 0), f.x), lerp(c(0, 1, 0), c(1, 1, 0), f.x), f.y),
        lerp(lerp(c(0, 0, 1), c(1, 0, 1), f.x), lerp(c(0, 1, 1), c(1, 1, 1), f.x), f.y),
        f.z,
    )
}

fn fbm(p: Vec3) -> f32 {
    let mut s = 0.0;
    let mut a = 0.5;
    let mut q = p;
    for _ in 0..5 {
        s += a * noise(q);
        q *= 2.03;
        a *= 0.5;
    }
    s
}

/// The sky's colour looking along `d` (unit), with the sun towards `sun`.
fn sky(d: Vec3, sun: Vec3, warm: f32) -> Vec3 {
    let up = d.y;
    // A clear summer sky: a deep blue overhead, paler (but still blue) towards the horizon.
    let zenith = Vec3::new(0.13, 0.33, 0.78);
    let horizon = Vec3::new(0.62, 0.76, 0.92).lerp(Vec3::new(0.9, 0.8, 0.66), warm);
    let ground = Vec3::new(0.46, 0.47, 0.46);
    let mut c = if up >= 0.0 {
        let t = up.powf(0.45);
        horizon.lerp(zenith, t)
    } else {
        horizon.lerp(ground, (-up * 6.0).min(1.0))
    };
    // Clouds: patchy banks, thicker towards the horizon, lit on the sun's side.
    if up > 0.0 {
        let p = d / (up + 0.12) * 1.6;
        let n = fbm(Vec3::new(p.x, 0.0, p.z) + Vec3::new(3.1, 0.0, 7.7));
        let cover = ((n - 0.46) * 3.2).clamp(0.0, 1.0) * (1.0 - up).powf(0.6);
        let lit = 0.82 + 0.18 * d.dot(sun).max(0.0);
        c = c.lerp(Vec3::splat(lit) * Vec3::new(1.0, 0.99, 0.97), cover * 0.85);
    }
    // The sun and its glow.
    let s = d.dot(sun).max(0.0);
    c += Vec3::new(1.0, 0.9, 0.7) * (s.powf(900.0) * 3.0 + s.powf(40.0) * 0.25 + s.powf(6.0) * 0.08 * (1.0 + warm));
    c
}

/// What stands on the horizon around a map: distant hills, a tree line, rooftops or cranes, as silhouettes fading
/// into the haze (aerial perspective), so the sky doesn't meet the map's walls as an empty dome.
#[derive(Clone, Copy, PartialEq)]
pub enum Horizon {
    /// Rolling wooded hills, a church spire or two, roofs (Altstadt).
    Town,
    /// Low shore, a far city's blocks and cranes (Hafen).
    Port,
    /// Flat country, a long tree line, far hills (Fliegerhorst).
    Country,
    None,
}

impl Horizon {
    pub fn of(map: &str) -> Self {
        match map {
            m if m.contains("altstadt") => Horizon::Town,
            m if m.contains("hafen") => Horizon::Port,
            m if m.contains("flieger") => Horizon::Country,
            _ => Horizon::None,
        }
    }
}

/// 1D value noise over the azimuth (wraps at 2π when `period` divides it).
fn anoise(a: f32, freq: f32, seed: i32) -> f32 {
    let x = a * freq;
    let i = x.floor();
    let f = x - i;
    let f = f * f * (3.0 - 2.0 * f);
    let n = freq.round() as i32;
    let h = |k: i32| hash(k.rem_euclid(n.max(1)), seed, 7);
    h(i as i32) + (h(i as i32 + 1) - h(i as i32)) * f
}

fn ridge(a: f32, base: f32, amp: f32, freq: f32, seed: i32) -> f32 {
    let mut h = 0.0;
    let mut w = 1.0;
    let mut fr = freq;
    for o in 0..4 {
        h += w * anoise(a, fr, seed + o);
        w *= 0.5;
        fr *= 2.0;
    }
    base + amp * (h / 1.875)
}

/// The horizon's silhouettes over the sky colour `c` looking along `d` (elevation `up`).
fn horizon(d: Vec3, up: f32, c: Vec3, hc: Vec3, style: Horizon) -> Vec3 {
    if style == Horizon::None || up > 0.2 {
        return c;
    }
    let a = d.z.atan2(d.x) + std::f32::consts::PI; // 0..2π
    let haze = |col: Vec3, k: f32| col.lerp(c, k);
    let mut out = c;
    // Far layer: hills (lighter, bluer, mostly haze).
    let far = match style {
        Horizon::Town => ridge(a, 0.015, 0.07, 6.0, 11),
        Horizon::Port => ridge(a, 0.004, 0.025, 9.0, 21),
        Horizon::Country => ridge(a, 0.008, 0.045, 5.0, 31),
        Horizon::None => -1.0,
    };
    if up < far {
        out = haze(Vec3::new(0.3, 0.37, 0.4), 0.5);
    }
    // Near layer: tree line / rooftops / city blocks and cranes.
    let near = match style {
        Horizon::Town => {
            // Wooded hill with rooftop clusters and the odd spire.
            let t = ridge(a, 0.0, 0.035, 40.0, 41);
            let spire = if (a * 7.3).sin() > 0.995 { 0.06 } else { 0.0 };
            t + spire
        }
        Horizon::Port => {
            // Blocks of buildings (stepped) and crane jibs.
            let blocks = anoise((a * 24.0).floor() / 24.0 + 0.01, 24.0, 51) * 0.03;
            let crane = if (a * 11.0).sin() > 0.985 { 0.08 } else { 0.0 };
            blocks + crane
        }
        Horizon::Country => ridge(a, 0.004, 0.03, 60.0, 61),
        Horizon::None => -1.0,
    };
    if up < near {
        let col = match style {
            Horizon::Port => Vec3::new(0.3, 0.33, 0.36),
            _ => Vec3::new(0.16, 0.22, 0.14),
        };
        out = haze(col, 0.22);
    }
    // Below the horizon: fields and roofs in the haze, darker further down (nearer).
    if up < 0.0 {
        let k = 0.15 + (1.0 - (-up * 12.0).min(1.0)) * 0.45;
        out = Vec3::new(0.19, 0.22, 0.15).lerp(hc, k);
    }
    out
}

fn dir(face: usize, u: f32, v: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
    .normalize()
}

/// The sky cubemap of the level being played, made with the level (the level's materials reflect it too).
#[derive(Resource, Clone)]
pub struct SkyCube(pub Handle<Image>);

fn paint(mut commands: Commands, current: Res<Current>, sky: Option<Res<SkyCube>>, cam: Single<Entity, With<PlayerCamera>>, mut done: Local<u32>) {
    if *done == current.generation || !current.spawned {
        return;
    }
    let Some(sky) = sky else { return };
    *done = current.generation;
    commands.entity(*cam).insert(Skybox { image: Some(sky.0.clone()), brightness: BRIGHTNESS * crate::view::exposure_gain(), ..default() });
}

/// The sky's brightness (nits) at a texel value of 1, at the default exposure.
pub const BRIGHTNESS: f32 = 1000.0;

/// The sky of `level` as a cubemap with a box-filtered mip chain (rough surfaces reflect the blurred levels): deep blue
/// overhead fading to a pale hazy horizon, soft cloud banks, and the sun where the map's `light_environment` puts it.
pub fn cube(level: &zugriff_sim::compile::Level, style: Horizon) -> Image {
    let env = level.entities_of("light_environment").next();
    let num = |key: &str, default: f32| env.and_then(|e| e.get(key)).and_then(|v| v.parse().ok()).unwrap_or(default);
    let (pitch, yaw) = (num("pitch", -50.0).to_radians(), zugriff_sim::units::yaw_from_map_angle(num("yaw", 35.0) as f64));
    let travel = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0) * Vec3::NEG_Z;
    let sun = -travel;
    // A low sun warms the horizon.
    let warm = (1.0 - sun.y / 0.6).clamp(0.0, 1.0) * 0.8;
    let n = FACE as usize;
    // Linear floats per face first, then sRGB bytes level by level.
    let mut faces: Vec<Vec<Vec3>> = Vec::with_capacity(6);
    for face in 0..6 {
        let mut px = Vec::with_capacity(n * n);
        for y in 0..n {
            for x in 0..n {
                let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let d = dir(face, u, v);
                let hd = Vec3::new(d.x, 0.0, d.z).normalize_or(Vec3::X);
                px.push(horizon(d, d.y, sky(d, sun, warm), sky(hd, sun, warm), style));
            }
        }
        faces.push(px);
    }
    let to8 = |x: f32| (x.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8;
    // wgpu wants a cube's mips level-major: level 0 of all six faces, then level 1 of all six …
    let mut levels: Vec<Vec<Vec<Vec3>>> = vec![faces];
    let mut size = n;
    while size > 1 {
        let half = size / 2;
        let prev = levels.last().unwrap();
        let next: Vec<Vec<Vec3>> = prev
            .iter()
            .map(|f| {
                let mut out = Vec::with_capacity(half * half);
                for y in 0..half {
                    for x in 0..half {
                        let i = |dx: usize, dy: usize| f[(y * 2 + dy) * size + x * 2 + dx];
                        out.push((i(0, 0) + i(1, 0) + i(0, 1) + i(1, 1)) * 0.25);
                    }
                }
                out
            })
            .collect();
        levels.push(next);
        size = half;
    }
    let mut data = Vec::new();
    // Bevy's `Image` stores mips layer-major (all of face 0's levels, then face 1's …): `TextureDataOrder::LayerMajor`.
    for face in 0..6 {
        for level in &levels {
            for c in &level[face] {
                data.extend_from_slice(&[to8(c.x), to8(c.y), to8(c.z), 255]);
            }
        }
    }
    let mut image = Image::new_uninit(Extent3d { width: FACE, height: FACE, depth_or_array_layers: 6 }, TextureDimension::D2, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels.len() as u32;
    image.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    image.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        mag_filter: bevy::image::ImageFilterMode::Linear,
        min_filter: bevy::image::ImageFilterMode::Linear,
        mipmap_filter: bevy::image::ImageFilterMode::Linear,
        ..default()
    });
    image
}
