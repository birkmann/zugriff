//! The player's view: mouse look in the classic units, and the camera — at the eye of the player (or whom they
//! spectate), kicked by the recoil's punch, zoomed through a scope; a death cam that looks at the killer; a slow
//! orbit over the map behind the main menu.
//!
//! Mouse: `sensitivity × 0.022°` per count, like the classic shooters, so players can bring their own setting (scaled
//! down in a scope). On the web the page asks for raw (unaccelerated) pointer lock; natively Bevy's mouse motion is
//! raw device motion already.

use crate::input::Ui;
use crate::level::Current;
use crate::options::Options;
use crate::session::Session;
use bevy::anti_alias::smaa::Smaa;
use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use zugriff_sim::game::PlayerId;

pub struct ViewPlugin;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MouseLook;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        // (The sun's shadow maps and the camera's anti-aliasing, sharpening and bloom follow the graphics settings:
        // graphics.rs.)
        app.init_resource::<Look>()
            .init_resource::<Spectate>()
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (grab_cursor, face_spawn, mouse_look).chain().in_set(MouseLook))
            .add_systems(PostUpdate, place_camera.before(TransformSystems::Propagate))
            .add_systems(Update, sky_probe);
    }
}

/// The camera's exposure (EV100) for a sunlit map: the sun is a physical 32 000 lux and the lightmap is scaled to it
/// (`level::LIGHTMAP_EXPOSURE`), so the camera must be set like a real one on a bright day. Bevy's default (9.7, an
/// indoor value) overexposed everything in sun about eightfold: walls and ground clipped to white and sun and shade
/// looked alike. `ev` overrides it for tuning.
pub fn ev100() -> f32 {
    crate::args::get("ev").and_then(|v| v.parse().ok()).unwrap_or(EV100)
}
pub const EV100: f32 = 12.0;

/// How much brighter something must be at [`ev100`] than at Bevy's default exposure to look the same on screen: for
/// what was tuned by eye as a picture (the sky's brightness, glowing emissives). Lights are not scaled: the ambient
/// for players, the viewmodel's fill and the muzzle flashes were tuned against the physical sun and the lightmap,
/// which the exposure change leaves as they were.
pub fn exposure_gain() -> f32 {
    2f32.powf(ev100() - bevy::camera::Exposure::EV100_BLENDER)
}

/// A number from the launch options, for tuning the look (`--sat 1.2`); `default` without it.
fn tune(name: &str, default: f32) -> f32 {
    crate::args::get(name).and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Degrees per mouse count at sensitivity 1 (the classic `m_yaw`/`m_pitch`).
pub const DEGREES_PER_COUNT: f32 = 0.022;

/// A horizontal field of view at 4:3, kept vertical so wider screens see more at the sides ("Hor+").
pub fn vertical_fov(horizontal_4_3_degrees: f32) -> f32 {
    2.0 * ((horizontal_4_3_degrees.to_radians() * 0.5).tan() * 0.75).atan()
}

/// Where the player looks (radians). Yaw 0 looks down −Z; pitch is positive up.
#[derive(Resource, Default)]
pub struct Look {
    pub yaw: f32,
    pub pitch: f32,
    /// The mouse is captured.
    pub locked: bool,
}

/// Dead: whom the camera follows, and since when the local player is dead.
#[derive(Resource, Default)]
pub struct Spectate {
    pub target: Option<PlayerId>,
    pub dead_for: f32,
    pub killer: Option<PlayerId>,
    /// Camera shake (explosions nearby), decaying.
    pub shake: f32,
}

#[derive(Component)]
pub struct PlayerCamera;

fn spawn_camera(mut commands: Commands, options: Res<Options>) {
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: vertical_fov(options.fov), near: 0.002, far: 600.0, ..default() }),
        Hdr,
        bevy::camera::Exposure { ev100: ev100() },
        // A filmic curve with real contrast (AgX kept everything grey) and a touch more colour; the darks are left alone
        // (crushing them turned players in shade into silhouettes).
        Tonemapping::TonyMcMapface,
        // A warm, crisp daylight grade: a touch warmer overall, richer colour, a little more midtone contrast, and the
        // shadows lifted slightly towards blue (the open sky fills them) so shade reads as shade, not as black.
        bevy::render::view::ColorGrading {
            global: bevy::render::view::ColorGradingGlobal { post_saturation: tune("sat", 1.1), temperature: tune("temp", 0.01), ..default() },
            shadows: bevy::render::view::ColorGradingSection { saturation: 1.05, lift: tune("lift", 0.0), ..default() },
            midtones: bevy::render::view::ColorGradingSection { contrast: tune("contrast", 1.08), saturation: 1.05, ..default() },
            highlights: bevy::render::view::ColorGradingSection { saturation: 1.0, ..default() },
        },
        // Crisp textures after SMAA, as a sharp game image is.
        bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening { enabled: true, sharpening_strength: 0.45, denoise: false },
        Bloom { intensity: 0.1, ..Bloom::NATURAL },
        (
            // Soft-edged sun shadows (a wider filter than the default 2×2), as a real sun's are.
            bevy::light::ShadowFilteringMethod::Gaussian,
            Msaa::Off,
            Smaa::default(),
        ),
        // Sky light for what isn't baked (players, the weapon, dropped guns). The level's own sky light is in its
        // lightmap: adding this on top of it too washed every shadow out.
        AmbientLight { brightness: 1900.0, color: Color::srgb(0.78, 0.84, 0.95), affects_lightmapped_meshes: false },
        // A light haze towards the sky's horizon colour: far buildings and cranes fall back, the maps get depth.
        DistanceFog {
            color: Color::srgb(0.72, 0.79, 0.87),
            directional_light_color: Color::srgba(1.0, 0.92, 0.78, 0.3),
            directional_light_exponent: 24.0,
            falloff: FogFalloff::from_visibility_squared(1500.0),
        },
        Transform::from_xyz(0.0, 2.0, 0.0),
        PlayerCamera,
        children![(
            // A fill light for the weapon in hand (the classic viewmodel light): dark gunmetal and gloves went black
            // in shade next to bright walls. Behind and above the eye, reaching only the rig a few centimetres in
            // front (the world is further than its range).
            PointLight { intensity: 1400.0, range: 0.45, radius: 0.05, color: Color::srgb(1.0, 0.97, 0.92), shadow_maps_enabled: false, affects_lightmapped_mesh_diffuse: false, ..default() },
            Transform::from_xyz(0.03, 0.12, 0.16),
        )],
    ));
}

/// Ambient light for what isn't baked (players, the weapon in hand) where the camera is: open sky outside, much less
/// under a roof. A few rays up from the eye measure how much sky is open (a poor man's light probe; the level's own
/// light is in its lightmap). Eased, so walking through a door doesn't flicker.
fn sky_probe(time: Res<Time>, current: Res<Current>, mut cam: Single<(&GlobalTransform, &mut AmbientLight), With<PlayerCamera>>, mut state: Local<(f32, f32)>) {
    let (open, timer) = &mut *state;
    *timer -= time.delta_secs();
    let (tf, ambient) = &mut *cam;
    if *timer <= 0.0 {
        *timer = 0.15;
        if let Some(level) = &current.level {
            let eye = tf.translation();
            // Eleven directions over the upper hemisphere, weighted towards the zenith as the sky's light is.
            let dirs = [
                Vec3::Y,
                Vec3::new(0.6, 0.8, 0.0), Vec3::new(-0.6, 0.8, 0.0), Vec3::new(0.0, 0.8, 0.6), Vec3::new(0.0, 0.8, -0.6),
                Vec3::new(0.62, 0.5, 0.62), Vec3::new(-0.62, 0.5, 0.62), Vec3::new(0.62, 0.5, -0.62), Vec3::new(-0.62, 0.5, -0.62),
                Vec3::new(0.95, 0.3, 0.0), Vec3::new(-0.95, 0.3, 0.0),
            ];
            let weights = [2.0, 1.2, 1.2, 1.2, 1.2, 0.8, 0.8, 0.8, 0.8, 0.5, 0.5];
            let (mut seen, mut total) = (0.0, 0.0);
            for (d, w) in dirs.iter().zip(weights) {
                let t = level.world.trace(eye, eye + d.normalize() * 60.0, Vec3::ZERO, zugriff_sim::world::Contents::MASK_SHOT);
                if t.fraction >= 1.0 {
                    seen += w;
                }
                total += w;
            }
            let target = seen / total;
            *open += (target - *open) * 0.35;
        }
    }
    // Under open sky the sky's light (the bake's sky seen through the lightmap's exposure); indoors what bounces.
    // (×1.8 over the sky's physical share: players' kit is near black, and in shade it read as a cut-out against the
    // lightmapped walls; the lift keeps them readable, as players are in any shooter)
    let brightness = (900.0 + 2300.0 * *open) * 1.8;
    if (ambient.brightness - brightness).abs() > 1.0 {
        ambient.brightness = brightness;
    }
}

/// Faces the local player the way their spawn points, whenever they (re)spawn.
fn face_spawn(session: Option<Res<Session>>, frame: Res<crate::session::Frame>, mut look: ResMut<Look>) {
    let Some(s) = session else { return };
    for e in &frame.events {
        if let zugriff_sim::game::Event::Spawn { player, yaw } = e
            && *player == s.me
        {
            look.yaw = *yaw;
            look.pitch = 0.0;
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn pointer_locked() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().and_then(|w| w.document()).and_then(|d| d.pointer_lock_element()).is_some()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        true
    }
}

/// Tells the page whether a click should capture the mouse (in a match, no menu open).
#[cfg(target_arch = "wasm32")]
fn set_page_lock(want: bool) {
    if let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
        let _ = body.set_attribute("data-lock", if want { "1" } else { "0" });
    }
}

/// Natively: click to capture the mouse in a match, Escape (the menu) releases it. On the web the page captures on
/// click when the game says so, and Escape is the browser's.
fn grab_cursor(mut look: ResMut<Look>, ui: Res<Ui>, session: Option<Res<Session>>, buttons: Res<ButtonInput<MouseButton>>, mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>) {
    let want = session.is_some() && !ui.menu;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (&buttons, &mut cursor);
        set_page_lock(want);
        look.locked = pointer_locked() && want;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if want && buttons.just_pressed(MouseButton::Left) && cursor.grab_mode != CursorGrabMode::Locked {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
        if !want && cursor.grab_mode != CursorGrabMode::None {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
        // (`--qa-lock`: as if captured, for key tests without a real mouse grab.)
        look.locked = cursor.grab_mode == CursorGrabMode::Locked || (want && crate::args::flag("qa-lock"));
    }
}

fn mouse_look(motion: Res<AccumulatedMouseMotion>, fixed: Res<Time<Fixed>>, options: Res<Options>, ui: Res<Ui>, session: Option<Res<Session>>, mut look: ResMut<Look>) {
    // A bot playing for the local player: the view follows its aim.
    if let Some(s) = session.as_ref().filter(|s| s.me().bot) {
        look.yaw = s.me().yaw;
        look.pitch = s.me().pitch;
        return;
    }
    if !look.locked || ui.buy {
        return;
    }
    // Scoped: slower in proportion to the zoom.
    let zoom = session.as_ref().map_or(1.0, |s| zoom_factor(s, s.me, fixed.overstep_fraction()));
    let k = (options.sensitivity * DEGREES_PER_COUNT * zoom).to_radians();
    let dy = if options.invert { -motion.delta.y } else { motion.delta.y };
    look.yaw = crate::session::wrap(look.yaw - motion.delta.x * k);
    look.pitch = (look.pitch - dy * k).clamp(-89f32.to_radians(), 89f32.to_radians());
}

/// The field-of-view factor of a player's scope (1 unscoped), `a` of the way from the last tick to this one.
pub fn zoom_factor(s: &Session, id: PlayerId, a: f32) -> f32 {
    let p = &s.game.players[id];
    if !p.alive {
        return 1.0;
    }
    if p.inv.zoom > 0 {
        return p.inv.def().zoom.get(p.inv.zoom as usize - 1).copied().unwrap_or(1.0);
    }
    // Down the sights: a gentle zoom by class, eased with the raise (interpolated between ticks, or it steps).
    let a = s.lerp(id, a).ads;
    let e = a * a * (3.0 - 2.0 * a);
    1.0 + (zugriff_sim::weapons::ads_zoom(p.inv.def().class) - 1.0) * e
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn place_camera(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    session: Option<Res<Session>>,
    current: Res<Current>,
    look: Res<Look>,
    options: Res<Options>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut spec: ResMut<Spectate>,
    frame: Res<crate::session::Frame>,
    cam: Single<(&mut Transform, &mut Projection), With<PlayerCamera>>,
) {
    let (mut tf, mut proj) = cam.into_inner();
    let dt = time.delta_secs();
    let base_fov = vertical_fov(options.fov);
    spec.shake = (spec.shake - dt * 1.5).max(0.0);
    let Some(s) = session else {
        // A fixed view for screenshots (`pose`), else the menu's slow orbit high over the map.
        if let Some([x, y, z, yaw, pitch]) = crate::args::pose() {
            *tf = Transform::from_xyz(x, y, z).with_rotation(Quat::from_euler(EulerRot::YXZ, yaw.to_radians(), pitch.to_radians(), 0.0));
        } else if let Some(level) = &current.level {
            let b = level.world.bounds();
            let c = b.centre();
            let r = ((b.max - b.min).length() * 0.32).clamp(15.0, 70.0);
            let t = time.elapsed_secs() * 0.04;
            let eye = c + Vec3::new(t.sin() * r, r * 0.45 + 6.0, t.cos() * r);
            *tf = Transform::from_translation(eye).looking_at(c, Vec3::Y);
        }
        if let Projection::Perspective(p) = &mut *proj {
            p.fov = base_fov;
        }
        return;
    };
    let a = fixed.overstep_fraction();
    let me = s.me;
    for e in &frame.events {
        if let zugriff_sim::game::Event::Kill { victim, killer, .. } = e
            && *victim == me
        {
            spec.killer = *killer;
            spec.dead_for = 0.0;
        }
        if let zugriff_sim::game::Event::Exploded { pos } | zugriff_sim::game::Event::Detonate { kind: zugriff_sim::weapons::WeaponId::He, pos, .. } = e {
            let d = pos.distance(tf.translation);
            spec.shake = spec.shake.max((1.0 - d / 25.0).clamp(0.0, 1.0));
        }
    }
    let alive = s.me().alive;
    let eye_rot = |id: PlayerId, own_look: bool| -> (Vec3, Quat) {
        let snap = s.lerp(id, a);
        let params = &s.game.params;
        let eye_h = params.stand_eye + (params.crouch_eye - params.stand_eye) * snap.duck;
        let eye = snap.origin + Vec3::Y * eye_h;
        let (yaw, pitch) = if own_look { (look.yaw, look.pitch) } else { (snap.yaw, snap.pitch) };
        let rot = Quat::from_euler(EulerRot::YXZ, yaw - snap.punch.y.to_radians(), pitch + snap.punch.x.to_radians(), 0.0);
        (eye, rot)
    };
    let viewed;
    if alive {
        spec.target = None;
        let (eye, rot) = eye_rot(me, true);
        tf.translation = eye;
        tf.rotation = rot;
        viewed = me;
    } else {
        spec.dead_for += dt;
        if spec.dead_for < 2.5 {
            // The death cam: rising behind and above where we fell, turned towards the killer.
            let snap = s.snap(me);
            let k = (spec.dead_for / 0.8).min(1.0);
            let target = spec.killer.filter(|&k| k != me).map(|k| s.lerp(k, a).origin + Vec3::Y * 1.2);
            let away = target.map_or(Vec3::Z, |t| (snap.origin - t).with_y(0.0).normalize_or(Vec3::Z));
            let mut eye = snap.origin + Vec3::Y * (1.2 + 1.3 * k) + away * (0.6 + 1.6 * k);
            // Not into a wall.
            if let Some(level) = &current.level {
                let from = snap.origin + Vec3::Y * 1.0;
                let t = level.world.trace(from, eye, Vec3::splat(0.15), zugriff_sim::world::Contents::MASK_SHOT);
                eye = t.end;
            }
            tf.translation = eye;
            let look_at = target.unwrap_or(snap.origin);
            let want = Transform::from_translation(eye).looking_at(look_at, Vec3::Y).rotation;
            tf.rotation = tf.rotation.slerp(want, (dt * 5.0).min(1.0));
            viewed = me;
        } else {
            // Spectate a living team-mate (left/right click to change); anyone alive if the team is out.
            let team = s.me().team;
            let mut candidates: Vec<PlayerId> = s.game.players.iter().filter(|p| p.alive && p.team == team).map(|p| p.id).collect();
            if candidates.is_empty() {
                candidates = s.game.players.iter().filter(|p| p.alive).map(|p| p.id).collect();
            }
            let current_i = spec.target.and_then(|t| candidates.iter().position(|&c| c == t));
            let step = if buttons.just_pressed(MouseButton::Left) {
                1
            } else if buttons.just_pressed(MouseButton::Right) {
                -1
            } else {
                0
            };
            spec.target = match (current_i, candidates.len()) {
                (_, 0) => None,
                (Some(i), n) => Some(candidates[(i as i32 + step).rem_euclid(n as i32) as usize]),
                (None, _) => Some(candidates[0]),
            };
            match spec.target {
                Some(t) => {
                    let (eye, rot) = eye_rot(t, false);
                    tf.translation = eye;
                    tf.rotation = rot;
                    viewed = t;
                }
                None => {
                    viewed = me;
                }
            }
        }
    }
    // `--chase <player>` (for checks): behind and above that player, looking at them (how bodies move, seen from
    // outside).
    if let Some(id) = crate::args::get("chase").and_then(|v| v.parse::<usize>().ok()).filter(|&i| i < s.game.players.len()) {
        let snap = s.lerp(id, a);
        let back = Quat::from_rotation_y(snap.yaw) * Vec3::new(0.0, 0.0, 1.0);
        let body = snap.origin + Vec3::Y * 1.1;
        let mut eye = body + back * 4.0 + Vec3::Y * 1.4;
        if let Some(level) = &current.level {
            eye = level.world.trace(body, eye, Vec3::splat(0.2), zugriff_sim::world::Contents::MASK_SHOT).end;
        }
        *tf = Transform::from_translation(eye).looking_at(body, Vec3::Y);
    }
    // Explosions shake the view.
    if spec.shake > 0.0 {
        let t = time.elapsed_secs() * 40.0;
        let k = spec.shake * spec.shake * 0.03;
        tf.rotation *= Quat::from_euler(EulerRot::YXZ, (t * 1.3).sin() * k, (t * 1.7).cos() * k, 0.0);
    }
    if let Projection::Perspective(p) = &mut *proj {
        p.fov = 2.0 * ((base_fov * 0.5).tan() * zoom_factor(&s, viewed, a)).atan();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ninety_at_four_by_three() {
        // tan(v/2) = tan(45°) × 3/4 → v ≈ 73.74°.
        assert!((vertical_fov(90.0).to_degrees() - 73.74).abs() < 0.01);
    }
}
