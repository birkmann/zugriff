//! The graphics settings: a preset (low to ultra) or each setting on its own, applied while the game runs.
//! - Sun shadows: off, or 2–4 cascades at 1024–4096², out to 50–130 m, with a hard or soft filter.
//! - Anti-aliasing (FXAA or SMAA), sharpening, bloom.
//! - The ground: plain; detailed (the dust or wear layer over it); relief (parallax, natively).
//! - Sky reflections on glass, metal and wet-looking surfaces.
//! - How much grass and litter is strewn, the animals, and how far props, grass and litter are drawn.
//! - A frame limiter (natively): an even frame rate below what the machine could just about hold feels smoother than an
//!   uneven higher one.
//!
//! `bench.rs` measures the presets and picks the best one this machine holds at the display's refresh rate.

use crate::levelmat::LevelMaterial;
use crate::options::Options;
use crate::view::PlayerCamera;
use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::camera::visibility::{RenderLayers, VisibilityRange};
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, ShadowFilteringMethod};
use bevy::platform::collections::HashMap;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::ops::Range;

pub struct GraphicsPlugin;

impl Plugin for GraphicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (apply_camera, apply_sun, apply_ground, apply_ranges));
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Last, limit_frame_rate);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Graphics {
    /// The preset the settings come from (see [`PRESET_NAMES`]); [`CUSTOM`] once one is changed by hand.
    pub preset: u8,
    /// Sun shadows (see [`SHADOW_NAMES`]).
    pub shadows: u8,
    /// Anti-aliasing: 0 off, 1 FXAA, 2 SMAA.
    pub aa: u8,
    /// Sharpening after the anti-aliasing (0 off … 1).
    pub sharpen: f32,
    pub bloom: bool,
    /// The ground's surface: 0 plain, 1 detailed (the dust or wear layer), 2 relief (parallax; natively).
    pub ground: u8,
    /// The sky mirrored in glass, metal and polished stone.
    pub reflections: bool,
    /// How much grass and weeds grow, and litter is strewn (0 none, 1 as designed).
    pub grass: f32,
    pub litter: f32,
    /// Pigeons, gulls, crows and rats.
    pub fauna: bool,
    /// How far props, grass and litter are drawn (× their designed distances).
    pub view: f32,
    /// At most this many frames a second (0: no limit); natively.
    pub fps_limit: u32,
    /// The quick test has chosen the preset once (a first start runs it).
    pub tested: bool,
}

pub const PRESET_NAMES: [&str; 5] = ["Niedrig", "Mittel", "Hoch", "Ultra", "Eigene"];
pub const CUSTOM: u8 = 4;
pub const SHADOW_NAMES: [&str; 5] = ["Aus", "Niedrig", "Mittel", "Hoch", "Ultra"];
pub const AA_NAMES: [&str; 3] = ["Aus", "FXAA", "SMAA"];
pub const GROUND_NAMES: [&str; 3] = ["Einfach", "Detailliert", "Relief"];
/// The frame limits to step through (0: none).
pub const FPS_LIMITS: [u32; 10] = [0, 30, 50, 60, 75, 100, 120, 144, 165, 240];

impl Default for Graphics {
    fn default() -> Self {
        // Until the first start's test has chosen: high natively, medium on the web.
        let mut g = Graphics { preset: 0, shadows: 0, aa: 0, sharpen: 0.0, bloom: false, ground: 0, reflections: false, grass: 0.0, litter: 0.0, fauna: false, view: 1.0, fps_limit: 0, tested: false };
        g.apply_preset(if cfg!(target_arch = "wasm32") { 1 } else { 2 });
        g
    }
}

impl Graphics {
    /// Sets every setting a preset holds (the frame limit and whether the test ran stay).
    pub fn apply_preset(&mut self, level: u8) {
        let web = cfg!(target_arch = "wasm32");
        let (shadows, aa, sharpen, bloom, ground, reflections, grass, litter, fauna, view) = match level {
            0 => (1, 1, 0.3, false, 0, false, 0.35, 0.35, true, 0.65),
            1 => (2, 2, 0.45, true, 1, true, 0.65, 0.6, true, 0.85),
            2 => (3, 2, 0.45, true, 1, true, 1.0, 1.0, true, 1.0),
            _ => (4, 2, 0.5, true, 2, true, 1.0, 1.0, true, 1.3),
        };
        self.preset = level.min(3);
        // (The web keeps its shadow maps small, a quarter of a gigabyte at 4096² with four cascades, and has no
        // parallax.)
        self.shadows = if web { shadows.min(2) } else { shadows };
        self.aa = aa;
        self.sharpen = sharpen;
        self.bloom = bloom;
        self.ground = if web { ground.min(1) } else { ground };
        self.reflections = reflections;
        self.grass = grass;
        self.litter = litter;
        self.fauna = fauna;
        self.view = view;
    }

    /// A number that changes with every setting (for rebuilding the menu).
    pub fn hash(&self) -> u64 {
        let s = format!("{self:?}");
        s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
    }
}

/// A density as a key that notices a change (grass and litter grow again when it does).
pub fn key(density: f32) -> u32 {
    (density.max(0.0) * 100.0).round() as u32
}

/// The sun's shadows at a level: cascades, map size, reach (m), whether the soft filter.
fn shadow_setup(level: u8) -> Option<(usize, usize, f32, bool)> {
    match level {
        0 => None,
        1 => Some((2, 1024, 50.0, false)),
        2 => Some((3, 2048, 90.0, true)),
        3 => Some((4, 2048, 130.0, true)),
        _ => Some((4, 4096, 130.0, true)),
    }
}

fn apply_camera(mut commands: Commands, options: Res<Options>, cams: Query<(Entity, Option<&ContrastAdaptiveSharpening>), With<PlayerCamera>>, added: Query<(), Added<PlayerCamera>>) {
    if !options.is_changed() && added.is_empty() {
        return;
    }
    let g = &options.gfx;
    for (e, cas) in &cams {
        let mut c = commands.entity(e);
        match g.aa {
            0 => {
                c.remove::<(Smaa, Fxaa)>();
            }
            1 => {
                c.remove::<Smaa>().insert(Fxaa::default());
            }
            _ => {
                c.remove::<Fxaa>().insert(Smaa::default());
            }
        }
        let want = ContrastAdaptiveSharpening { enabled: g.sharpen > 0.0, sharpening_strength: g.sharpen.clamp(0.0, 1.0), denoise: false };
        if cas.is_none_or(|c| c.enabled != want.enabled || c.sharpening_strength != want.sharpening_strength) {
            c.insert(want);
        }
        if g.bloom {
            c.insert(Bloom { intensity: 0.1, ..Bloom::NATURAL });
        } else {
            c.remove::<Bloom>();
        }
        let soft = shadow_setup(g.shadows).is_none_or(|s| s.3);
        c.insert(if soft { ShadowFilteringMethod::Gaussian } else { ShadowFilteringMethod::Hardware2x2 });
    }
}

/// The level's sun (not the preview stages' lights, which have render layers of their own).
fn apply_sun(options: Res<Options>, mut map: ResMut<DirectionalLightShadowMap>, mut suns: Query<(Entity, &mut DirectionalLight, &mut bevy::light::CascadeShadowConfig), Without<RenderLayers>>, mut seen: Local<Vec<Entity>>) {
    // (Again for a new level's sun.)
    let now: Vec<Entity> = suns.iter().map(|(e, ..)| e).collect();
    if !options.is_changed() && *seen == now {
        return;
    }
    *seen = now;
    let setup = shadow_setup(options.gfx.shadows).filter(|_| !crate::args::flag("noshadow"));
    let size = setup.map_or(1024, |s| s.1);
    if map.size != size {
        map.size = size;
    }
    for (_, mut light, mut cascades) in &mut suns {
        light.shadow_maps_enabled = setup.is_some();
        if let Some((n, _, reach, _)) = setup {
            // The first cascade starts at the eye: the weapon in hand sits a few millimetres in front of it.
            *cascades = CascadeShadowConfigBuilder { num_cascades: n, minimum_distance: 0.001, first_cascade_far_bound: if n <= 2 { 12.0 } else { 9.0 }, maximum_distance: reach, overlap_proportion: 0.2 }.build();
        }
    }
}

/// The ground's layers and the sky's reflection, per level material: its values as made (kept the first time it is
/// seen) with the parts the settings leave out zeroed. Water keeps its reflection.
fn apply_ground(options: Res<Options>, mut events: MessageReader<AssetEvent<LevelMaterial>>, mut materials: ResMut<Assets<LevelMaterial>>, mut made: Local<HashMap<AssetId<LevelMaterial>, (Vec4, Vec4)>>, mut applied: Local<Option<(u8, bool)>>) {
    let want = (options.gfx.ground, options.gfx.reflections);
    let added = events.read().filter(|e| matches!(e, AssetEvent::Added { .. })).count() > 0;
    if !added && *applied == Some(want) {
        return;
    }
    *applied = Some(want);
    let ids: Vec<_> = materials.ids().collect();
    for id in ids {
        let Some(m) = materials.get(id) else { continue };
        let water = m.extension.params.w > 0.5;
        let (mut ground, mut reflect) = *made.entry(id).or_insert((m.extension.ground, m.extension.reflect));
        if want.0 < 2 {
            ground.z = 0.0;
        }
        if want.0 < 1 {
            ground.x = 0.0;
        }
        if !want.1 && !water {
            reflect.x = 0.0;
        }
        if (m.extension.ground != ground || m.extension.reflect != reflect)
            && let Some(mut m) = materials.get_mut(id)
        {
            m.extension.ground = ground;
            m.extension.reflect = reflect;
        }
    }
    made.retain(|id, _| materials.contains(*id));
}

/// A visibility range as it was made (the view distance scales it).
#[derive(Component)]
struct MadeRange(Range<f32>);

fn apply_ranges(mut commands: Commands, options: Res<Options>, mut new: Query<(Entity, &mut VisibilityRange), Without<MadeRange>>, mut all: Query<(&MadeRange, &mut VisibilityRange), With<MadeRange>>, mut applied: Local<f32>) {
    let k = options.gfx.view.clamp(0.3, 3.0);
    for (e, mut r) in &mut new {
        let made = r.end_margin.clone();
        r.end_margin = made.start * k..made.end * k;
        commands.entity(e).insert(MadeRange(made));
    }
    if *applied != k {
        *applied = k;
        for (made, mut r) in &mut all {
            r.end_margin = made.0.start * k..made.0.end * k;
        }
    }
}

/// Holds each frame to the limit: sleeps most of the way, then spins the last moment (sleeps overshoot).
#[cfg(not(target_arch = "wasm32"))]
fn limit_frame_rate(options: Res<Options>, bench: Res<crate::bench::Bench>, mut next: Local<Option<std::time::Instant>>) {
    use std::time::{Duration, Instant};
    let fps = options.gfx.fps_limit;
    let now = Instant::now();
    if fps == 0 || bench.running() {
        *next = None;
        return;
    }
    let period = Duration::from_secs_f64(1.0 / fps as f64);
    let Some(at) = *next else {
        *next = Some(now + period);
        return;
    };
    if at > now {
        let left = at - now;
        if left > Duration::from_micros(2000) {
            std::thread::sleep(left - Duration::from_micros(1500));
        }
        while Instant::now() < at {
            std::hint::spin_loop();
        }
    }
    // (A frame that ran long starts the count again, rather than rushing the next ones to catch up.)
    let after = Instant::now();
    *next = Some(if after > at + period { after + period } else { at + period });
}
