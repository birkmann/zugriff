//! The level's material for the texture sets (`mat/<set>`): the standard PBR material (colour, normal, ORM, the
//! lightmap) with `levelmat.wgsl` on top, which hides how a texture repeats (see there). How much of it each set gets
//! is in [`tuning`]; the `nodetail` launch option turns it off for comparison.

use bevy::asset::uuid_handle;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub type LevelMaterial = ExtendedMaterial<StandardMaterial, LevelDetail>;

/// The baked light (in the lightmap's units, after `LIGHTMAP_EXPOSURE`) on a wall that sees half the open sky: the
/// sky's reflection fades out below it, so rooms and covered corners don't shine with a sky they can't see.
const OPEN_SKY: f32 = 1800.0;

const SHADER: Handle<Shader> = uuid_handle!("3c1f2a8e-5b7d-4e60-9a1c-7d2e4f6b8a01");

pub fn register(app: &mut App) {
    let _ = app.world_mut().resource_mut::<Assets<Shader>>().insert(SHADER.id(), Shader::from_wgsl(include_str!("levelmat.wgsl"), "zugriff/levelmat.wgsl"));
    app.add_plugins(MaterialPlugin::<LevelMaterial>::default());
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct LevelDetail {
    /// x: the second sample's scale, y: how much of it, z: the variation's strength, w: 1 for water (ripples).
    #[uniform(100)]
    pub params: Vec4,
    /// The set's colour again, for the second sample.
    #[texture(101)]
    #[sampler(102)]
    pub color: Handle<Image>,
    /// The sky (`sky::cube`), for reflections.
    #[texture(103, dimension = "cube")]
    #[sampler(104)]
    pub sky: Handle<Image>,
    /// x: the sky's brightness (nits at a texel of 1), y: the baked light (lightmap units) of a spot that sees the open
    /// sky, z: how strongly the texture's own occlusion darkens the baked light.
    #[uniform(105)]
    pub reflect: Vec4,
    /// The set's normal and ORM maps again (the ground path samples the set itself, twice, to hide its repeat). A
    /// ground set (`gnd_*`) keeps its height in the ORM's blue channel (it has no metal).
    #[texture(106)]
    pub normal: Handle<Image>,
    #[texture(107)]
    pub orm: Handle<Image>,
    /// A ground set's overlay (dust, mud): colour, normal, ORM (height in blue), laid in world space.
    #[texture(108)]
    pub over_color: Handle<Image>,
    #[texture(109)]
    pub over_normal: Handle<Image>,
    #[texture(110)]
    pub over_orm: Handle<Image>,
    /// x: the overlay's coverage (0 off), y: its scale (repeats per metre), z: parallax depth (UV units), w: 1 for a
    /// ground set.
    #[uniform(111)]
    pub ground: Vec4,
    /// The material's colour factor (linear), which the ground path applies itself.
    #[uniform(112)]
    pub tint: Vec4,
}

/// A ground set's overlay and how it is laid: (overlay set, coverage, repeats per metre, parallax depth).
pub fn ground(set: &str) -> Option<(&'static str, f32, f32, f32)> {
    if !set.starts_with("gnd_") || crate::args::flag("nodetail") {
        return None;
    }
    // (the web draws no parallax: every step is two more texture reads per pixel of ground)
    let px = if cfg!(target_arch = "wasm32") { 0.0 } else { 1.0 };
    Some(match set {
        "gnd_setts" | "gnd_setts_old" => ("gnd_dust", 0.55, 1.0 / 3.2, 0.03 * px),
        "gnd_setts_moss" => ("gnd_mud", 0.45, 1.0 / 2.6, 0.03 * px),
        "gnd_flagstone" => ("gnd_dust", 0.5, 1.0 / 3.2, 0.025 * px),
        // Yards and aprons: worn and patched, grimy, not dusty.
        "gnd_asphalt" => ("gnd_asphalt_damaged", 0.42, 1.0 / 4.4, 0.008 * px),
        "gnd_asphalt_damaged" => ("gnd_mud", 0.3, 1.0 / 3.6, 0.008 * px),
        "gnd_concrete" => ("gnd_concrete_damaged", 0.42, 1.0 / 5.0, 0.008 * px),
        "gnd_concrete_damaged" => ("gnd_mud", 0.25, 1.0 / 3.6, 0.008 * px),
        "gnd_dust" => ("gnd_mud", 0.35, 1.0 / 2.4, 0.015 * px),
        _ => ("gnd_dust", 0.35, 1.0 / 3.2, 0.0),
    })
}

impl MaterialExtension for LevelDetail {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }
}

/// How a set hides its repeat: organic sets blend in a second sample at another scale, patterned ones (whose joints a
/// second sample would smear) only vary in brightness and tint.
pub fn tuning(set: &str) -> Vec4 {
    if crate::args::flag("nodetail") {
        return Vec4::ZERO;
    }
    // Glass and lamps are fitted per window or lamp: no variation over them.
    if set.starts_with("glass") || set.starts_with("lamp") {
        return Vec4::ZERO;
    }
    let patterned = [
        "brick", "tiles", "planks", "wood", "crate", "roof", "corrugated", "container", "cobble", "paving", "slabs", "plate", "ceiling", "frame", "door",
        "sign", "grille", "shutter", "stone", "pavers", "sheet", "grate", "factory", "precast",
    ];
    if patterned.iter().any(|p| set.contains(p)) {
        Vec4::new(0.0, 0.0, 0.13, 0.0)
    } else if set.starts_with("metal_painted") || set.starts_with("concrete_painted") {
        Vec4::new(0.41, 0.45, 0.12, 0.0)
    } else {
        Vec4::new(0.43, 0.6, 0.18, 0.0)
    }
}

/// Water (`dev/water`): dark and glossy, the sky's reflection on it, ripples moving over it (the shader's `w` flag).
pub fn water(sky: Handle<Image>) -> LevelMaterial {
    let base = StandardMaterial { base_color: Color::srgb(0.05, 0.085, 0.09), perceptual_roughness: 0.05, metallic: 0.0, reflectance: 0.5, ..default() };
    let reflect = if crate::args::flag("noreflect") { Vec4::new(0.0, 1.0, 1.0, 0.0) } else { Vec4::new(crate::sky::BRIGHTNESS * crate::view::exposure_gain(), OPEN_SKY, 1.0, 0.0) };
    ExtendedMaterial {
        base,
        extension: LevelDetail {
            params: Vec4::new(0.0, 0.0, 0.0, 1.0),
            color: Handle::default(),
            sky,
            reflect,
            normal: Handle::default(),
            orm: Handle::default(),
            over_color: Handle::default(),
            over_normal: Handle::default(),
            over_orm: Handle::default(),
            ground: Vec4::ZERO,
            tint: Vec4::ONE,
        },
    }
}

/// A set's material: [`crate::textures::material`] with the detail on top.
pub fn material(assets: &AssetServer, set: &str, sky: Handle<Image>) -> LevelMaterial {
    let mut base = crate::textures::material(assets, set);
    // Bare metal shows mostly what it reflects, and the level has no reflection probes: fully metallic sheet went
    // black in shade. A third of the texture's metalness keeps its sheen and lets it take the lightmap's light.
    base.metallic = 0.35;
    // Sets whose colour map is off for the look: the lawn is a loud yellow-green in the source.
    if set.contains("grass") {
        base.base_color = Color::srgb(0.78, 0.8, 0.62);
    }
    // Plaster002 is near paper-white: in the sun it glared.
    if set == "plaster_white" {
        base.base_color = Color::srgb(0.88, 0.86, 0.81);
    }
    // The clay tiles' orange is loud in full sun: a little weathered.
    if set == "roof_clay" {
        base.base_color = Color::srgb(0.82, 0.76, 0.74);
    }
    let color = base.base_color_texture.clone().unwrap_or_default();
    let normal = base.normal_map_texture.clone().unwrap_or_default();
    let orm = base.metallic_roughness_texture.clone().unwrap_or_default();
    let reflect = if crate::args::flag("noreflect") { Vec4::new(0.0, 1.0, 1.0, 0.0) } else { Vec4::new(crate::sky::BRIGHTNESS * crate::view::exposure_gain(), OPEN_SKY, 1.0, 0.0) };
    let (over_color, over_normal, over_orm, ground_params) = match ground(set) {
        Some((over, amount, scale, depth)) => {
            let m = crate::textures::material(assets, over);
            // (ground sets carry height, not metal, in blue: no metal)
            base.metallic = 0.0;
            // The photos' setts and asphalt are cool and dark for a sunny town: warmed and lifted a little.
            if set.starts_with("gnd_setts") || set == "gnd_flagstone" {
                base.base_color = Color::linear_rgb(1.25, 1.17, 1.02);
            } else if set.starts_with("gnd_asphalt") {
                // (the photo's asphalt is brown: towards a cool dark grey)
                base.base_color = Color::linear_rgb(1.0, 1.12, 1.28);
            } else if set == "gnd_concrete" {
                base.base_color = Color::linear_rgb(0.95, 0.98, 1.05);
            }
            (m.base_color_texture.unwrap_or_default(), m.normal_map_texture.unwrap_or_default(), m.metallic_roughness_texture.unwrap_or_default(), Vec4::new(amount, scale, depth, 1.0))
        }
        None => (color.clone(), normal.clone(), orm.clone(), Vec4::ZERO),
    };
    // (w: the overlay is dust, lifted and dried in the shader)
    let dusty = ground(set).is_some_and(|g| g.0 == "gnd_dust");
    let tint = base.base_color.to_linear().to_vec3().extend(if dusty { 1.0 } else { 0.0 });
    ExtendedMaterial { base, extension: LevelDetail { params: tuning(set), color, sky, reflect, normal, orm, over_color, over_normal, over_orm, ground: ground_params, tint } }
}
