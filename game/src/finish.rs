//! Weapon finishes: cosmetic paint schemes for every gun. A finish is a pattern (`assets/finishes/*.png`, made by
//! `tools/finishes/build.py`) and how it sits on the weapon: its scale, how much bare metal it leaves, how glossy or
//! metallic the paint is. The weapons' paintable parts carry a `finish` UV map (TEXCOORD_1: the weapon seen from its
//! side, in metres; see `tools/models/build.py` `finish_uv`); the arms, rounds and shells don't, which is how their
//! meshes are told apart. A scene under a [`Painted`] root — the first-person rig, a gun in someone's hands or on the
//! floor, a preview — gets its paintable meshes' materials swapped for the finish's (and back, for the factory one).
//!
//! The finish travels with the gun in the sim (`Gun::finish`); a player's choices (`Options::finishes`) go into the
//! sim as their `Player::finishes`, so what they buy or are issued comes painted.

use std::collections::HashMap;

use bevy::asset::{uuid_handle, AssetEvent};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::VertexAttributeValues;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use zugriff_sim::weapons::WeaponId;

pub type FinishMaterial = ExtendedMaterial<StandardMaterial, FinishExt>;

const SHADER: Handle<Shader> = uuid_handle!("8b0f5f1e-2d4c-4a8e-b7f3-5c9e1a6d2f40");

pub struct FinishPlugin;

impl Plugin for FinishPlugin {
    fn build(&self, app: &mut App) {
        let _ = app.world_mut().resource_mut::<Assets<Shader>>().insert(SHADER.id(), Shader::from_wgsl(include_str!("finish.wgsl"), "zugriff/finish.wgsl"));
        app.add_plugins(MaterialPlugin::<FinishMaterial>::default()).init_resource::<Paints>().add_systems(PostUpdate, (mip_paints, paint).chain());
    }
}

/// How collectible a finish is (the card's colour).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    Factory,
    Troop,
    Elite,
    Master,
    Legend,
}

impl Grade {
    pub fn name(self) -> &'static str {
        match self {
            Grade::Factory => "Werk",
            Grade::Troop => "Truppe",
            Grade::Elite => "Elite",
            Grade::Master => "Meister",
            Grade::Legend => "Legende",
        }
    }
    pub fn color(self) -> Color {
        match self {
            Grade::Factory => Color::srgb(0.62, 0.66, 0.7),
            Grade::Troop => Color::srgb(0.3, 0.55, 1.0),
            Grade::Elite => Color::srgb(0.62, 0.36, 1.0),
            Grade::Master => Color::srgb(0.95, 0.3, 0.72),
            Grade::Legend => Color::srgb(1.0, 0.74, 0.2),
        }
    }
}

/// A finish: its pattern and how the paint sits on the weapon.
#[derive(Clone, Copy, Debug)]
pub struct Finish {
    pub key: &'static str,
    pub name: &'static str,
    pub grade: Grade,
    pub about: &'static str,
    /// A tile's size (m); 0: a wrap, one design over the whole weapon.
    pub tile: f32,
    /// How much of the weapon it covers, and how much bare metal it leaves bare (0 paints over everything).
    pub cover: f32,
    pub bare: f32,
    /// How much of the weapon's own light and dark shows through (its edges, wear, print).
    pub detail: f32,
    pub metallic: f32,
    pub rough: f32,
    /// The studio reflection's strength (metal and gloss paints).
    pub shine: f32,
    /// How much its brightest, most saturated strokes glow.
    pub glow: f32,
}

const PAINT: Finish = Finish { key: "", name: "", grade: Grade::Troop, about: "", tile: 0.42, cover: 1.0, bare: 0.75, detail: 0.6, metallic: 0.0, rough: 0.72, shine: 0.0, glow: 0.0 };
const METAL: Finish = Finish { tile: 0.4, bare: 0.0, detail: 0.45, metallic: 1.0, rough: 0.28, shine: 1.0, ..PAINT };

pub const FINISHES: [Finish; 14] = [
    Finish { key: "werk", name: "Werkszustand", grade: Grade::Factory, about: "Wie sie aus dem Werk kommt.", cover: 0.0, ..PAINT },
    Finish { key: "flecktarn", name: "Flecktarn", about: "Fünf Farben, kleine Flecken: der Wald.", ..PAINT },
    Finish { key: "tropentarn", name: "Tropentarn", about: "Sand, Oliv und Braun für Wüste und Steppe.", ..PAINT },
    Finish { key: "schneetarn", name: "Schneetarn", about: "Weiß mit grauen Verwehungen.", tile: 0.6, ..PAINT },
    Finish { key: "stadttarn", name: "Stadttarn", about: "Digitale Pixel in Beton und Schiefer.", tile: 0.5, ..PAINT },
    Finish { key: "tigerfell", name: "Tigerfell", grade: Grade::Elite, about: "Schwarze Streifen auf gebranntem Orange.", tile: 0.45, rough: 0.5, shine: 0.3, ..PAINT },
    Finish { key: "karbon", name: "Karbon", grade: Grade::Elite, about: "Kohlefaser in Köperbindung, klar lackiert.", tile: 0.12, bare: 0.4, detail: 0.3, rough: 0.3, shine: 0.7, ..PAINT },
    Finish { key: "kobalt", name: "Kobalt", grade: Grade::Elite, about: "Blau eloxiert, fein gebürstet.", ..METAL },
    Finish { key: "graffiti", name: "Graffiti", grade: Grade::Elite, about: "Schwarze Tags auf Weiß, mit Nasen.", tile: 0.5, rough: 0.6, shine: 0.15, ..PAINT },
    Finish { key: "damast", name: "Damast", grade: Grade::Master, about: "Gefalteter Stahl, geätzt.", tile: 0.35, rough: 0.34, ..METAL },
    Finish { key: "neonpuls", name: "Neonpuls", grade: Grade::Master, about: "Violett bis Cyan, Raster und ein Herzschlag.", tile: 0.0, bare: 0.3, rough: 0.4, shine: 0.4, glow: 1.0, detail: 0.4, ..PAINT },
    Finish { key: "glutlack", name: "Glutlack", grade: Grade::Master, about: "Rennlack in Rot und Orange mit weißem Band.", tile: 0.0, bare: 0.3, rough: 0.28, shine: 0.7, detail: 0.4, ..PAINT },
    Finish { key: "polarlicht", name: "Polarlicht", grade: Grade::Master, about: "Grüne und violette Vorhänge im Nachtblau.", tile: 0.0, bare: 0.3, rough: 0.35, shine: 0.5, glow: 1.0, detail: 0.4, ..PAINT },
    Finish { key: "gold", name: "Blattgold", grade: Grade::Legend, about: "Poliertes Gold, von Kimme bis Kolben.", rough: 0.2, ..METAL },
];

/// The finish of the gun `w` in an inventory (0 for what isn't a gun).
pub fn of(inv: &zugriff_sim::weapons::Inventory, w: WeaponId) -> u8 {
    inv.primary.filter(|g| g.id == w).or(inv.secondary.filter(|g| g.id == w)).map_or(0, |g| g.finish)
}

pub fn finish(i: u8) -> &'static Finish {
    FINISHES.get(i as usize).unwrap_or(&FINISHES[0])
}

/// A weapon scene to paint: which weapon and its finish (0: as it came from the factory).
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Painted {
    pub weapon: WeaponId,
    pub finish: u8,
}

/// A painted mesh's own material (to go back to).
#[derive(Component)]
struct Original(Handle<StandardMaterial>);

/// The size of the weapon a root carries, from its meshes' finish maps (m).
#[derive(Component)]
struct Dims(Vec2);

#[derive(AsBindGroup, Asset, TypePath, Clone, Debug)]
pub struct FinishExt {
    #[uniform(100)]
    pub shape: Vec4,
    #[uniform(101)]
    pub look: Vec4,
    #[uniform(102)]
    pub surf: Vec4,
    #[texture(103)]
    #[sampler(104)]
    pub paint: Handle<Image>,
}

impl MaterialExtension for FinishExt {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }
}

#[derive(Resource, Default)]
struct Paints {
    images: HashMap<u8, Handle<Image>>,
    /// Images given their mipmaps already.
    mipped: Vec<AssetId<Image>>,
    materials: HashMap<(AssetId<StandardMaterial>, u8), Handle<FinishMaterial>>,
    /// Each original colour map's mean luminance.
    lum: HashMap<AssetId<StandardMaterial>, f32>,
}

impl Paints {
    fn image(&mut self, server: &AssetServer, i: u8) -> Handle<Image> {
        self.images
            .entry(i)
            .or_insert_with(|| {
                server.load_builder().with_settings(|s: &mut ImageLoaderSettings| {
                    s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        mag_filter: ImageFilterMode::Linear,
                        min_filter: ImageFilterMode::Linear,
                        mipmap_filter: ImageFilterMode::Linear,
                        anisotropy_clamp: 8,
                        ..default()
                    });
                }).load(format!("finishes/{}.png", finish(i).key))
            })
            .clone()
    }
}

/// The patterns come as plain PNGs: give them a box-filtered mip chain as they load, so a gun across the map doesn't
/// shimmer.
fn mip_paints(mut events: MessageReader<AssetEvent<Image>>, mut paints: ResMut<Paints>, mut images: ResMut<Assets<Image>>) {
    for ev in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = ev else { continue };
        if paints.mipped.contains(id) || !paints.images.values().any(|h| h.id() == *id) {
            continue;
        }
        paints.mipped.push(*id);
        if let Some(mut img) = images.get_mut(*id) {
            with_mips(&mut img);
        }
    }
}

pub fn with_mips(img: &mut Image) {
    let (mut w, mut h) = (img.width() as usize, img.height() as usize);
    if img.texture_descriptor.mip_level_count > 1 || img.texture_descriptor.format.block_copy_size(None) != Some(4) {
        return;
    }
    let Some(data) = img.data.as_ref() else { return };
    let mut all = data.clone();
    let mut level = data.clone();
    let mut n = 1;
    while w > 1 && h > 1 {
        let (nw, nh) = (w / 2, h / 2);
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let s = |xx: usize, yy: usize| level[(yy * w + xx) * 4 + c] as u32;
                    next[(y * nw + x) * 4 + c] = ((s(2 * x, 2 * y) + s(2 * x + 1, 2 * y) + s(2 * x, 2 * y + 1) + s(2 * x + 1, 2 * y + 1) + 2) / 4) as u8;
                }
            }
        }
        all.extend_from_slice(&next);
        level = next;
        w = nw;
        h = nh;
        n += 1;
    }
    img.data = Some(all);
    img.texture_descriptor.mip_level_count = n;
}

/// The mean luminance (linear) of a material's colour: its map's times its factor.
fn mean_lum(m: &StandardMaterial, images: &Assets<Image>) -> f32 {
    let f = m.base_color.to_linear();
    let factor = 0.2126 * f.red + 0.7152 * f.green + 0.0722 * f.blue;
    let Some(img) = m.base_color_texture.as_ref().and_then(|h| images.get(h)) else { return factor.max(0.01) };
    let Some(data) = img.data.as_ref().filter(|_| img.texture_descriptor.format.block_copy_size(None) == Some(4)) else { return 0.1 };
    let px = (img.width() * img.height()) as usize;
    let step = (px / 8192).max(1);
    let lin = |b: u8| {
        let c = b as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let (mut sum, mut n) = (0.0f32, 0.0f32);
    // (Only the base level: mips follow it.)
    for i in (0..px.min(data.len() / 4)).step_by(step) {
        let p = &data[i * 4..i * 4 + 4];
        sum += 0.2126 * lin(p[0]) + 0.7152 * lin(p[1]) + 0.0722 * lin(p[2]);
        n += 1.0;
    }
    (sum / n.max(1.0) * factor).max(0.005)
}

/// Swaps the paintable meshes' materials under each [`Painted`] root: meshes as they spawn, and all of a root's when
/// its finish changes.
#[allow(clippy::too_many_arguments)]
fn paint(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut paints: ResMut<Paints>,
    meshes: Res<Assets<Mesh>>,
    std_mats: Res<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    mut fin_mats: ResMut<Assets<FinishMaterial>>,
    roots: Query<(Entity, Ref<Painted>, Option<&Dims>)>,
    new_meshes: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    targets: Query<(&Mesh3d, Option<&MeshMaterial3d<StandardMaterial>>, Option<&Original>)>,
    mut pending: Local<Vec<Entity>>,
) {
    // Which roots to (re)paint: those whose finish changed, and those with new meshes under them.
    let mut whole: Vec<Entity> = roots.iter().filter(|(_, p, _)| p.is_changed()).map(|(e, ..)| e).collect();
    let fresh: Vec<Entity> = new_meshes.iter().chain(std::mem::take(&mut *pending)).collect();
    for m in fresh {
        let mut e = m;
        for _ in 0..40 {
            if roots.contains(e) {
                if !whole.contains(&e) {
                    // A new scene under a root: its size may have changed with it.
                    commands.entity(e).remove::<Dims>();
                    whole.push(e);
                }
                break;
            }
            let Ok(p) = parents.get(e) else { break };
            e = p.parent();
        }
    }
    let uv1 = |mesh: &Mesh3d| -> Option<Option<Vec2>> {
        let mesh = meshes.get(&mesh.0)?;
        let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1) else { return Some(None) };
        Some(Some(uvs.iter().fold(Vec2::ZERO, |a, u| a.max(Vec2::new(u[0], u[1])))))
    };
    for root in whole {
        let Ok((_, painted, dims)) = roots.get(root) else { continue };
        let painted = *painted;
        let mut list = Vec::new();
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            if targets.contains(e) {
                list.push(e);
            }
            if let Ok(c) = children.get(e) {
                stack.extend(c.iter());
            }
        }
        // The weapon's size: the furthest reach of its finish maps.
        let mut size = dims.map(|d| d.0);
        if size.is_none() {
            let mut s = Vec2::ZERO;
            for &e in &list {
                let Ok((mesh, ..)) = targets.get(e) else { continue };
                match uv1(mesh) {
                    None => {
                        pending.push(e);
                    }
                    Some(Some(m)) => s = s.max(m),
                    Some(None) => {}
                }
            }
            if s != Vec2::ZERO {
                commands.entity(root).insert(Dims(s));
                size = Some(s);
            }
        }
        let Some(size) = size else { continue };
        for e in list {
            let Ok((mesh, std, orig)) = targets.get(e) else { continue };
            if !matches!(uv1(mesh), Some(Some(_))) {
                continue;
            }
            let Some(original) = orig.map(|o| o.0.clone()).or_else(|| std.map(|m| m.0.clone())) else { continue };
            if painted.finish == 0 || painted.finish as usize >= FINISHES.len() {
                if orig.is_some() {
                    commands.entity(e).insert(MeshMaterial3d(original)).remove::<MeshMaterial3d<FinishMaterial>>();
                }
                continue;
            }
            let Some(base) = std_mats.get(&original) else {
                pending.push(e);
                continue;
            };
            let key = (original.id(), painted.finish);
            let handle = match paints.materials.get(&key) {
                Some(h) => h.clone(),
                None => {
                    let lum = *paints.lum.entry(original.id()).or_insert_with(|| mean_lum(base, &images));
                    let f = finish(painted.finish);
                    let paint = paints.image(&server, painted.finish);
                    let bright = crate::sky::BRIGHTNESS * crate::view::exposure_gain();
                    let h = fin_mats.add(ExtendedMaterial {
                        base: base.clone(),
                        extension: FinishExt {
                            shape: Vec4::new(size.x, size.y, f.tile, lum),
                            look: Vec4::new(f.cover, f.bare, f.detail, f.metallic),
                            surf: Vec4::new(f.rough, f.shine, bright, f.glow),
                            paint,
                        },
                    });
                    paints.materials.insert(key, h.clone());
                    h
                }
            };
            commands.entity(e).insert((MeshMaterial3d(handle), Original(original))).remove::<MeshMaterial3d<StandardMaterial>>();
        }
    }
}
