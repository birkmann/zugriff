//! Procedural development textures (the blockout look) until the real material library lands. Each is 512² with a full
//! mip chain, so distant floors don't shimmer.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Texel coordinates from the map are divided by this for every surface, dev or real, so a map's texture scale means
/// the same world size whatever the image resolution (at scale 0.25 one tile is 128 units, 3.25 m).
pub const SIZE: u32 = 512;

/// How a surface looks and behaves in the renderer.
pub struct DevLook {
    pub base: [u8; 3],
    pub line: [u8; 3],
    /// Grid spacing in texels (64 texels = 16 units at the default 0.25 scale).
    pub grid: u32,
    pub roughness: f32,
    pub alpha: f32,
    /// A crate: a thick border instead of a grid.
    pub framed: bool,
    /// No lines at all.
    pub plain: bool,
}

pub fn look(name: &str) -> DevLook {
    let n = name.to_ascii_lowercase();
    let d = |base, line, grid, roughness| DevLook { base, line, grid, roughness, alpha: 1.0, framed: false, plain: false };
    match n.as_str() {
        "dev/ground" => d([92, 88, 80], [70, 67, 61], 256, 0.95),
        "dev/floor" => d([118, 116, 110], [90, 88, 84], 128, 0.85),
        "dev/concrete" => d([150, 148, 142], [118, 116, 110], 128, 0.9),
        "dev/wall" => d([196, 150, 96], [160, 118, 72], 128, 0.9),
        "dev/plaster" => d([214, 206, 188], [182, 174, 156], 256, 0.92),
        "dev/ceiling" => d([180, 180, 184], [150, 150, 154], 128, 0.9),
        "dev/trim" => d([70, 76, 84], [52, 56, 62], 64, 0.6),
        "dev/crate" => DevLook { framed: true, ..d([150, 112, 62], [96, 68, 36], 512, 0.8) },
        "dev/glass" => DevLook { alpha: 0.28, ..d([170, 200, 220], [140, 170, 190], 512, 0.05) },
        "dev/water" => DevLook { plain: true, ..d([22, 40, 48], [22, 40, 48], 512, 0.04) },
        _ => {
            // Unknown names get a stable colour from their hash, so missing textures are obvious but not ugly.
            let h = n.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
            let c = [(h & 0xff) as u8 / 2 + 80, ((h >> 8) & 0xff) as u8 / 2 + 80, ((h >> 16) & 0xff) as u8 / 2 + 80];
            d(c, [c[0] / 2, c[1] / 2, c[2] / 2], 128, 0.85)
        }
    }
}

fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

/// A dev texture for `name` with its mip chain.
pub fn dev_image(name: &str) -> Image {
    let l = look(name);
    let s = SIZE;
    let seed = name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let mut rgba = vec![0u8; (s * s * 4) as usize];
    for y in 0..s {
        for x in 0..s {
            let on_line = if l.plain {
                false
            } else if l.framed {
                let b = 28;
                x < b || y < b || x >= s - b || y >= s - b || (x as i32 - y as i32).abs() < b as i32 / 2
            } else {
                x % l.grid < 3 || y % l.grid < 3
            };
            let c = if on_line { l.line } else { l.base };
            // A little grain and some large blotches, so surfaces read as surfaces.
            let grain = noise(x, y, seed) * 0.08 - 0.04;
            let blot = noise(x / 64, y / 64, seed ^ 0x9e37) * 0.1 - 0.05;
            let k = 1.0 + grain + blot;
            let i = ((y * s + x) * 4) as usize;
            for ch in 0..3 {
                rgba[i + ch] = (c[ch] as f32 * k).clamp(0.0, 255.0) as u8;
            }
            rgba[i + 3] = (l.alpha * 255.0) as u8;
        }
    }
    with_mips(s, s, rgba)
}

/// An sRGB RGBA8 image with a box-filtered mip chain and a trilinear, anisotropic, repeating sampler.
pub fn with_mips(w: u32, h: u32, base: Vec<u8>) -> Image {
    let mut data = base.clone();
    let (mut cw, mut ch) = (w as usize, h as usize);
    let mut level = base;
    let mut count = 1;
    while cw > 1 || ch > 1 {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(cw - 1);
                        let sy = (y * 2 + dy).min(ch - 1);
                        sum += u32::from(level[(sy * cw + sx) * 4 + c]);
                    }
                    next[(y * nw + x) * 4 + c] = ((sum + 2) / 4) as u8;
                }
            }
        }
        data.extend_from_slice(&next);
        level = next;
        cw = nw;
        ch = nh;
        count += 1;
    }
    let mut img = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = count;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    img
}

fn repeat() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    })
}

/// A material from `textures/<set>/`: `color.ktx2` (BC7 sRGB), `normal.ktx2` (BC5) and `orm.ktx2` (BC7 linear:
/// occlusion, roughness, metal), as written by `zg-tex`.
pub fn material(assets: &AssetServer, set: &str) -> StandardMaterial {
    let load = |file: &str, srgb: bool| -> Handle<Image> {
        assets.load_builder().with_settings(move |s: &mut ImageLoaderSettings| {
            s.is_srgb = srgb;
            s.sampler = repeat();
        }).load(format!("textures/{set}/{file}"))
    };
    let orm = load("orm.ktx2", false);
    StandardMaterial {
        base_color_texture: Some(load("color.ktx2", true)),
        normal_map_texture: Some(load("normal.ktx2", false)),
        metallic_roughness_texture: Some(orm.clone()),
        occlusion_texture: Some(orm),
        perceptual_roughness: 1.0,
        metallic: 1.0,
        reflectance: 0.4,
        ..default()
    }
}

/// The decal atlas (`textures/decals/`): `color.ktx2` (BC7 sRGB with straight alpha, cells padded so mips don't
/// bleed) and `normal.ktx2` (BC5, same layout).
pub fn decal_atlas(assets: &AssetServer) -> (Handle<Image>, Handle<Image>) {
    let load = |file: &str, srgb: bool| -> Handle<Image> {
        assets.load_builder().with_settings(move |s: &mut ImageLoaderSettings| {
            s.is_srgb = srgb;
            s.sampler = repeat();
        }).load(format!("textures/decals/{file}"))
    };
    (load("color.ktx2", true), load("normal.ktx2", false))
}

/// Whether a decal darkens what is under it (`"blend": "multiply"` in `decals.json`) rather than being painted on.
pub fn decal_multiplies(name: &str) -> bool {
    static MULTIPLY: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    MULTIPLY
        .get_or_init(|| {
            let doc: serde_json::Value = serde_json::from_str(include_str!("../../assets/textures/decals/decals.json")).unwrap_or_default();
            doc["decals"].as_object().map_or_else(Vec::new, |m| m.iter().filter(|(_, d)| d["blend"] == "multiply").map(|(n, _)| n.clone()).collect())
        })
        .iter()
        .any(|n| n == name)
}
