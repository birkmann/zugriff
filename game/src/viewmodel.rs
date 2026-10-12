//! The first-person view: the weapon in hand and the arms holding it. The rig hangs off the camera at a tenth of its
//! size and a tenth of the distance (the classic trick): it looks the same, but sits within a few centimetres of the
//! eye, where no wall ever comes (the player's hull keeps walls 40 cm away), and it is lit and shadowed by the world
//! where the player stands. It is widened by [`MAGNIFY`] in view space to look as if seen with a narrower field of
//! view than the world's, as the genre does. The models
//! (`models/vm-*.glb`, from CC-BY packs: see `assets/models/CREDITS.md`) are skinned arms with the weapon, in the
//! camera's frame, with their own clips:
//! - arms and rifle (`draw`, `fire`, `reload`, `reload_empty`, `idle`) for every long gun; arms and pistol (`idle`,
//!   `walk`, `fire`, `reload`, `reload_empty`) for pistols; empty hands (`idle`, `walk`) holding the knife, a grenade
//!   or the bomb;
//! - clips follow the sim: a round restarts `fire`, a reload plays over the reload's own length, a new weapon plays
//!   `draw` (or comes up from below);
//! - over them, on the whole rig: sway as the view turns, the walk's bob, the dip of a landing, breathing, each round's
//!   kick, the knife's slash and stab, the grenade's wind-up and throw, the bomb's plant; the muzzle flash at the
//!   `muzzle` marker. Scoped, the rig hides (the scope overlay is the HUD's).

use bevy::animation::graph::{AnimationGraph, AnimationNodeType};
use bevy::animation::transition::AnimationTransitions;
use bevy::animation::AnimationTargetId;
use bevy::gltf::GltfMaterialName;
use std::collections::{HashMap, HashSet};
use bevy::transform::TransformSystems;
use bevy::prelude::*;
use std::f32::consts::PI;
use zugriff_sim::game::Event;
use zugriff_sim::weapons::{Action, Class, WeaponId};

use crate::actors::rand01;
use crate::anim::{Key, Pose, Spring, ease, sample};
use crate::clips::{self, Animated, Library, Model, Rig};
use crate::session::{Frame, Session};
use crate::view::{PlayerCamera, Spectate};

pub struct ViewmodelPlugin;

/// The viewmodel's systems (the muzzle's place is known after them).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewmodelSet;

impl Plugin for ViewmodelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewState>().init_resource::<Markers>().add_systems(PostStartup, setup).add_systems(Update, (tag, mask_fire, animate, show_grenade, paint).chain().in_set(ViewmodelSet).after(crate::session::SessionSet))
            .add_systems(PostUpdate, hold_sight.after(bevy::app::AnimationSystems).before(TransformSystems::Propagate));
    }
}

/// The rig's scale: a tenth, widened across the view so it looks as if seen with a 60° field of view (the world's is
/// wider).
pub const SHRINK: f32 = 0.1;
pub const MAGNIFY: f32 = 1.28;
/// Not aimed, the rig is drawn this much further ahead (rig units), and widened to look as big at [`PULL_REF`] ahead
/// as it would without: as if seen from further back with a narrower field of view. That flattens the perspective,
/// so what is close to the eye (the support hand on the magazine well, the forearms) isn't oversized next to the gun.
/// Down the sights it goes back to the eye, where the sight pictures were fitted. Further back, parts of the rigs that
/// sit behind the eye (the stock's end, the pistol pack's upper arms) come into view: the pistol gets less.
fn pull(model: Model) -> f32 {
    let pistol = model == Model::Pistol || matches!(model, Model::Gun(w) if w.def().class == Class::Pistol);
    if pistol { 0.1 } else { 0.2 }
}
const PULL_REF: f32 = 0.45;
/// Over the packs' own framing (m): a little lower and further right, as the classic shooters have it.
const HIP: Vec3 = Vec3::new(0.03, -0.028, 0.0);
/// Metres run per bob cycle (one dip, half a sway): about 0.85 s at full speed, a calm step rather than a sprint's.
const BOB_CYCLE: f32 = 5.4;
/// Full running speed (250 units/s), where the bob and a walk clip reach their full pace.
const RUN: f32 = 250.0 * 0.0254;

/// The node on the camera that carries the rig.
#[derive(Component)]
pub struct ViewAnchor;

#[derive(Component)]
pub struct ViewRoot(Model);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Muzzle,
    Ejection,
    Hand,
    /// Where the eye goes aimed down the sights (the sight line runs along −Z through it).
    SightEye,
    /// A gun's own model's rear and front sight (the line the eye looks along, aimed).
    SightRear,
    SightFront,
    /// A point above the rear sight, square up in the gun's own frame (the aimed gun is stood upright by it).
    SightUp,
    /// The grenade in the grenade rig's hand (hidden once thrown).
    Grenade,
}

#[derive(Component)]
pub struct Marker {
    model: Model,
    kind: Kind,
}

#[derive(Component)]
pub struct Flash;

/// What the empty hands hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HeldKind {
    Knife,
    Grenade,
    Bomb,
}

#[derive(Component)]
pub struct Held(HeldKind);

/// World positions of the weapon's muzzle and ejection port this frame (for tracers and brass).
#[derive(Resource, Default)]
pub struct Markers {
    pub muzzle: Option<Vec3>,
    pub ejection: Option<Vec3>,
    /// The view's right and forward, for throwing brass.
    pub right: Vec3,
}

#[derive(Resource, Default)]
pub struct ViewState {
    sway: Spring,
    kick: Spring,
    kick_rot: Spring,
    dip: Spring,
    last_look: Option<Vec2>,
    time: f32,
    stride: f32,
    spawned: bool,
    shown: Option<WeaponId>,
    reloading: bool,
    fired_ago: f32,
    thrown_ago: f32,
    viewed: Option<usize>,
    /// The muzzle flash's quad, and its side (flame) and front (star) materials.
    flash: Option<(Handle<Mesh>, Handle<StandardMaterial>, Handle<StandardMaterial>)>,
    /// Aimed: the rig on show, the rear sight's place in it that `animate` aligned (from the last frame's
    /// transforms), where that point is in the `sight-eye` marker's frame, and how far aimed; [`hold_sight`] corrects
    /// for where this frame's clip put it.
    sight: Option<(Entity, Vec3, Vec3, f32)>,
    /// The knife swing's time last frame (a new swing starts its clip).
    swing_t: Option<f32>,
    /// The grenade in hand is to be lobbed (underhand), as its pin was pulled; whether it's in the hand.
    lob: bool,
    grenade_hidden: bool,
    /// A grenade has been thrown (so `thrown_ago` means something).
    threw: bool,
    /// The arms' materials, made single-sided (by the original's id).
    one_sided: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
}

fn setup(mut commands: Commands, cam: Single<Entity, With<PlayerCamera>>) {
    let anchor = commands.spawn((ViewAnchor, Transform::from_scale(Vec3::new(SHRINK * MAGNIFY, SHRINK * MAGNIFY, SHRINK)), Visibility::Hidden)).id();
    commands.entity(*cam).add_child(anchor);
}

/// The first-person model of a weapon: its own (see `gear.rs`), or its class's.
pub fn model_of(w: WeaponId) -> Model {
    if crate::gear::own(w).is_some() { Model::Gun(w) } else { family_of(w) }
}

/// The shared first-person model of a weapon's class.
pub fn family_of(w: WeaponId) -> Model {
    match w.def().class {
        Class::Pistol => Model::Pistol,
        Class::Knife => Model::Knife,
        Class::Grenade => Model::Grenade,
        Class::Bomb => Model::Hands,
        Class::Smg => Model::Smg,
        Class::Shotgun => Model::Shotgun,
        Class::Sniper => Model::Sniper,
        Class::Mg => Model::Mg,
        _ if matches!(w, WeaponId::Akm | WeaponId::Ak74) => Model::Ak,
        _ => Model::Rifle,
    }
}

fn spawn(commands: &mut Commands, server: &AssetServer, lib: &mut Library, view: Entity) {
    for model in [Model::Rifle, Model::Pistol, Model::Hands, Model::Ak, Model::Sniper, Model::Shotgun, Model::Smg, Model::Mg, Model::Knife, Model::Grenade] {
        spawn_one(commands, server, lib, view, model);
    }
}

fn spawn_one(commands: &mut Commands, server: &AssetServer, lib: &mut Library, view: Entity, model: Model) {
    lib.want(server, model);
    let root = commands.spawn((WorldAssetRoot(model.scene(server)), Transform::IDENTITY, Visibility::Hidden, ViewRoot(model), Animated(model))).id();
    commands.entity(view).add_child(root);
}

/// The viewmodels' nodes as they load: on the viewmodel's layer, their markers found; the knife, a grenade and the
/// bomb put in the empty hands, the flash at the muzzles.
#[allow(clippy::too_many_arguments)]
fn tag(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut state: ResMut<ViewState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    named: Query<(Entity, &Name), Added<Name>>,
    new_meshes: Query<(Entity, Option<&GltfMaterialName>, Option<&MeshMaterial3d<StandardMaterial>>), Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    roots: Query<&ViewRoot>,
) {
    let root_of = |mut e: Entity| {
        for _ in 0..40 {
            if let Ok(r) = roots.get(e) {
                return Some(r.0);
            }
            e = parents.get(e).ok()?.parent();
        }
        None
    };
    for (e, material_name, material) in &new_meshes {
        if root_of(e).is_none() {
            continue;
        }
        commands.entity(e).insert(bevy::light::NotShadowCaster);
        // The packs export the arms double-sided: with the camera inside a sleeve (close to the eye, or where the
        // near plane cuts it) its inside was drawn. Single-sided, what the eye is inside of isn't drawn at all.
        let is_arms = material_name.is_some_and(|n| matches!(&**n, "arms" | "Hand_D" | "Glove_D" | "material"));
        if let (true, Some(material)) = (is_arms, material) {
            let id = material.0.id();
            let one = match state.one_sided.get(&id) {
                Some(h) => h.clone(),
                None => {
                    let Some(mut m) = materials.get(id).cloned() else { continue };
                    m.double_sided = false;
                    m.cull_mode = Some(bevy::render::render_resource::Face::Back);
                    let h = materials.add(m);
                    state.one_sided.insert(id, h.clone());
                    h
                }
            };
            commands.entity(e).insert(MeshMaterial3d(one));
        }
    }
    let (flash_mesh, flash_side, flash_front) = state
        .flash
        .get_or_insert_with(|| {
            let mesh = meshes.add(Rectangle::new(1.0, 1.0));
            let (side, front) = flash_images();
            let mut mat = |img: Image| StandardMaterial {
                base_color: LinearRgba::rgb(3.2, 2.2, 1.2).into(),
                base_color_texture: Some(images.add(img)),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                double_sided: true,
                cull_mode: None,
                ..default()
            };
            (mesh, materials.add(mat(side)), materials.add(mat(front)))
        })
        .clone();
    for (e, name) in &named {
        let kind = match name.as_str() {
            "muzzle" => Kind::Muzzle,
            "ejection" => Kind::Ejection,
            "hand-r" => Kind::Hand,
            "sight-eye" => Kind::SightEye,
            "sight-rear" => Kind::SightRear,
            "sight-front" => Kind::SightFront,
            "sight-up" => Kind::SightUp,
            "grenade" => Kind::Grenade,
            _ => continue,
        };
        let Some(model) = root_of(e) else { continue };
        commands.entity(e).insert(Marker { model, kind });
        match kind {
            Kind::Muzzle => {
                // Two crossed flame quads along the bore (their u runs from the muzzle out) and a star facing forward,
                // under one parent the flash system scales in for a frame or two.
                commands.entity(e).with_children(|c| {
                    c.spawn((Flash, Transform::from_scale(Vec3::splat(1e-4)), Visibility::Inherited)).with_children(|f| {
                        let along = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
                        for roll in [0.0, std::f32::consts::FRAC_PI_2] {
                            f.spawn((
                                Mesh3d(flash_mesh.clone()),
                                MeshMaterial3d(flash_side.clone()),
                                Transform::from_xyz(0.0, 0.0, -0.5).with_rotation(Quat::from_rotation_z(roll) * along).with_scale(Vec3::new(1.0, 0.55, 1.0)),
                                bevy::light::NotShadowCaster,
                            ));
                        }
                        f.spawn((Mesh3d(flash_mesh.clone()), MeshMaterial3d(flash_front.clone()), Transform::from_xyz(0.0, 0.0, -0.08).with_scale(Vec3::splat(0.7)), bevy::light::NotShadowCaster));
                    });
                });
            }
            Kind::Hand if model == Model::Hands => {
                let knife = server.load(GltfAssetLabel::Scene(0).from_asset("models/knife.glb"));
                let grenade = server.load(GltfAssetLabel::Scene(0).from_asset("models/grenade.glb"));
                let pack = meshes.add(Cuboid::new(0.16, 0.08, 0.22));
                let olive = materials.add(StandardMaterial { base_color: Color::srgb(0.24, 0.26, 0.19), perceptual_roughness: 0.8, ..default() });
                let keypad = meshes.add(Cuboid::new(0.07, 0.012, 0.09));
                let pad = materials.add(StandardMaterial { base_color: Color::srgb(0.08, 0.08, 0.08), emissive: { let g = crate::view::exposure_gain(); LinearRgba::rgb(0.6 * g, 0.05 * g, 0.02 * g) }, ..default() });
                commands.entity(e).with_children(|c| {
                    c.spawn((WorldAssetRoot(knife), Transform::IDENTITY, Visibility::Hidden, Held(HeldKind::Knife)));
                    c.spawn((WorldAssetRoot(grenade), Transform::IDENTITY, Visibility::Hidden, Held(HeldKind::Grenade)));
                    c.spawn((Transform::IDENTITY, Visibility::Hidden, Held(HeldKind::Bomb))).with_children(|b| {
                        b.spawn((Mesh3d(pack), MeshMaterial3d(olive), Transform::from_xyz(0.0, 0.05, 0.0), bevy::light::NotShadowCaster));
                        b.spawn((Mesh3d(keypad), MeshMaterial3d(pad), Transform::from_xyz(0.0, 0.096, 0.02), bevy::light::NotShadowCaster));
                    });
                });
            }
            _ => {}
        }
    }
}

/// The rig on show wears the finish of the gun in the viewed player's hands.
fn paint(mut commands: Commands, session: Option<Res<Session>>, spectate: Res<Spectate>, mut roots: Query<(Entity, &ViewRoot, &Visibility, Option<&mut crate::finish::Painted>)>) {
    let Some(s) = session else { return };
    let inv = &s.game.players[s.viewed(spectate.target)].inv;
    let want = crate::finish::Painted { weapon: inv.active, finish: crate::finish::of(inv, inv.active) };
    for (e, _, vis, painted) in &mut roots {
        if *vis == Visibility::Hidden {
            continue;
        }
        match painted {
            Some(mut p) => {
                p.set_if_neq(want);
            }
            None => {
                commands.entity(e).insert(want);
            }
        }
    }
}

/// A knife swing's offset of the whole rig at `t` (s).
fn swing_frame(stab: bool, t: f32) -> Pose {
    let keys: &[Key] = if stab {
        &[
            Key { t: 0.0, pose: Pose::IDENTITY },
            Key { t: 0.2, pose: Pose::new([0.02, 0.03, 0.1], [20.0, 0.0, 0.0]) },
            Key { t: 0.3, pose: Pose::new([-0.06, 0.02, -0.22], [-10.0, 15.0, 0.0]) },
            Key { t: 0.5, pose: Pose::new([-0.05, 0.01, -0.18], [-10.0, 15.0, 0.0]) },
            Key { t: 1.0, pose: Pose::IDENTITY },
        ]
    } else {
        &[
            Key { t: 0.0, pose: Pose::IDENTITY },
            Key { t: 0.06, pose: Pose::new([0.06, 0.05, 0.02], [8.0, -22.0, -14.0]) },
            Key { t: 0.15, pose: Pose::new([-0.1, -0.04, -0.08], [-10.0, 34.0, 18.0]) },
            Key { t: 0.4, pose: Pose::IDENTITY },
        ]
    };
    sample(keys, t)
}

struct Want {
    clip: &'static str,
    restart: bool,
    speed: f32,
    repeat: bool,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn animate(
    mut commands: Commands,
    (time, fixed, look_now): (Res<Time>, Res<Time<Fixed>>, Res<crate::view::Look>),
    server: Res<AssetServer>,
    session: Option<Res<Session>>,
    spectate: Res<Spectate>,
    frame: Res<Frame>,
    ui: Res<crate::input::Ui>,
    warmup: Res<crate::warmup::Warmup>,
    mut lib: ResMut<Library>,
    mut state: ResMut<ViewState>,
    mut markers_out: ResMut<Markers>,
    anchor: Single<(Entity, &mut Visibility, &GlobalTransform, &mut Transform), (With<ViewAnchor>, Without<ViewRoot>, Without<Held>, Without<Flash>)>,
    mut roots: Query<(Entity, &ViewRoot, &mut Transform, &mut Visibility, &GlobalTransform), (Without<Flash>, Without<Held>)>,
    (markers, mut flashes, mut held, globals): (
        Query<(&Marker, &GlobalTransform)>,
        Query<&mut Transform, (With<Flash>, Without<ViewRoot>, Without<Held>)>,
        Query<(&Held, &mut Visibility, &mut Transform, &ChildOf), (Without<ViewRoot>, Without<Flash>)>,
        Query<&GlobalTransform>,
    ),
    mut rigs: Query<(&mut Rig, &mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let dt = time.delta_secs();
    let (view, mut anchor_vis, view_global, mut anchor_tf) = anchor.into_inner();
    let viewed = session.as_ref().map(|s| s.viewed(spectate.target));
    let on = session.as_ref().zip(viewed).filter(|(s, v)| s.game.players[*v].alive && !ui.menu && crate::args::get("chase").is_none());
    *anchor_vis = if on.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    let Some((s, viewed)) = on else {
        for (_, _, _, mut vis, _) in &mut roots {
            *vis = Visibility::Hidden;
        }
        state.last_look = None;
        state.shown = None;
        state.sight = None;
        markers_out.muzzle = None;
        return;
    };
    if !state.spawned {
        state.spawned = true;
        spawn(&mut commands, &server, &mut lib, view);
        return;
    }
    if state.viewed != Some(viewed) {
        state.viewed = Some(viewed);
        state.shown = None;
    }
    let p = &s.game.players[viewed];
    let inv = &p.inv;
    let weapon = inv.active;
    state.time += dt;
    state.fired_ago += dt;
    state.thrown_ago += dt;
    let mut fired_now = false;
    for e in &frame.events {
        match e {
            Event::Shot { player, .. } if *player == viewed => {
                fired_now = true;
                state.fired_ago = 0.0;
            }
            Event::Throw { player, .. } if *player == viewed => {
                state.thrown_ago = 0.0;
                state.threw = true;
            }
            Event::Land { player, speed, .. } if *player == viewed => state.dip.kick(Vec3::Y * -(speed * 0.08).min(1.2)),
            _ => {}
        }
    }
    // A throw's follow-through plays out on the grenade rig before the next weapon comes up.
    let throwing = state.threw && state.thrown_ago < 0.45;
    let wanted = if throwing { Model::Grenade } else { model_of(weapon) };
    // A gun's own model comes when it is first taken out (its class's stands in meanwhile).
    if matches!(wanted, Model::Gun(_)) && !roots.iter().any(|(_, r, ..)| r.0 == wanted) {
        spawn_one(&mut commands, &server, &mut lib, view, wanted);
    }
    let model = lib.resolve(&server, wanted);
    // The look's lag: the weapon trails the turn, and springs back. Per frame, as the camera turns: our own look, or
    // the viewed player's interpolated between ticks (the ticks' own steps made the weapon jitter).
    let snap = s.lerp(viewed, fixed.overstep_fraction());
    let ads = snap.ads;
    let own = viewed == s.me && !s.me().bot;
    let look = if own { Vec2::new(look_now.yaw, look_now.pitch) } else { Vec2::new(snap.yaw, snap.pitch) };
    let turned = state.last_look.map_or(Vec2::ZERO, |l| Vec2::new(crate::session::wrap(look.x - l.x), look.y - l.y));
    state.last_look = Some(look);
    let lag = (turned / dt.max(1e-3) * 0.01).clamp_length_max(0.07) * (1.0 - 0.8 * ads);
    state.sway.step(lag.extend(0.0), 9.0, dt);
    if fired_now {
        // The whole gun kicks back and up a little, by the weapon's recoil: a light tap for an SMG or a rifle burst,
        // a harder shove for the revolver, the shotgun and the sniper rifles; less down the sights.
        let r = inv.def().recoil;
        let k = (0.3 + 0.2 * r.up_base).clamp(0.35, 1.1) * (1.0 - 0.5 * ads);
        let side = (rand01() - 0.5) * (0.3 + r.lateral_base);
        state.kick.kick(Vec3::new(0.0, 0.03 * k, 1.1 * k));
        state.kick_rot.kick(Vec3::new(1.0 * k, side * 0.5 * k, side * k));
    }
    state.kick.step(Vec3::ZERO, 28.0, dt);
    state.kick_rot.step(Vec3::ZERO, 24.0, dt);
    state.dip.step(Vec3::ZERO, 10.0, dt);
    let speed = p.mover.horizontal_speed();
    if p.mover.on_ground {
        state.stride = (state.stride + speed * dt) % (2.0 * BOB_CYCLE);
    }

    let root_entity = roots.iter().find(|(_, r, ..)| r.0 == model).map(|(e, ..)| e);
    let root_global = roots.iter().find(|(_, r, ..)| r.0 == model).map(|(.., g)| *g);
    // Down the sights: the sight's marker (where it is in the rig, whatever the clip is doing) onto the view's axis,
    // a little in front of the eye (the near plane is 2 cm away at the rig's scale).
    // The marker's frame rides the weapon; the rig's sight line (rear sight to front sight) is found in it, turned onto
    // the view's axis and moved onto it, so the front sight covers the middle of the screen, where the shots go.
    let marker = root_global.and_then(|rg| markers.iter().find(|(m, _)| m.model == model && m.kind == Kind::SightEye).map(|(_, g)| rg.affine().inverse() * g.affine()));
    let aim = ease(ads);
    let steady = 1.0 - 0.85 * aim;
    let pull = pull(model) * (1.0 - aim);
    let widen = MAGNIFY * (PULL_REF + pull) / PULL_REF;
    anchor_tf.translation = Vec3::new(0.0, 0.0, -pull * SHRINK);
    anchor_tf.scale = Vec3::new(SHRINK * widen, SHRINK * widen, SHRINK);
    // Without measured sights: the marker itself, a little in front of the eye (the near plane is 2 cm away at the
    // rig's scale).
    let marked = || {
        let Model::Gun(w) = model else { return None };
        let eye = crate::gear::own(w)?.eye?;
        let find = |k: Kind| markers.iter().find(|(m, _)| m.model == model && m.kind == k).map(|(_, g)| g.affine());
        let (sight, rear, front, muzzle) = (find(Kind::SightEye)?, find(Kind::SightRear)?, find(Kind::SightFront)?, find(Kind::Muzzle)?);
        let into = sight.inverse();
        let (r, f, mz) = (into.transform_point3(rear.translation.into()), into.transform_point3(front.translation.into()), into.transform_point3(muzzle.translation.into()));
        // The gun's own up, square to the sight line: its `sight-up` marker, else from the bore (the muzzle) to the
        // sights. Stood upright by it, whatever cant the pack's idle gives the gun (the pistols too, put in the pistol
        // pack's hand canted as its own pistol is). (The packs' muzzle markers can sit off the bore: the pistols' by
        // 8–11 mm to the side, which leant them by up to 10°.)
        let axis = (f - r).normalize_or(Vec3::NEG_Z);
        let up = match find(Kind::SightUp) {
            Some(u) => into.transform_point3(u.translation.into()) - r,
            None => r - mz,
        }
        .reject_from(axis);
        let upright = Upright::Along(up);
        let _ = w;
        let eye = crate::args::get("eye").and_then(|v| v.parse().ok()).unwrap_or(eye);
        Some((r, f, eye, upright))
    };
    let (rear, front, eye, upright) = sights(model)
        .map(|(r, f, e, roll)| (r, f, e, Upright::Roll(roll)))
        .or_else(marked)
        .unwrap_or((Vec3::ZERO, Vec3::NEG_Z, 0.06, Upright::Roll(0.0)));
    let ads_pose = marker.map_or(Pose { pos: HIP, rot: Quat::IDENTITY }, |m| {
        let (r, f) = (m.transform_point3(rear), m.transform_point3(front));
        let onto = Quat::from_rotation_arc((f - r).normalize_or(Vec3::NEG_Z), Vec3::NEG_Z);
        let roll = match upright {
            Upright::Roll(deg) => deg.to_radians(),
            // (the turn about the view's axis that brings the gun's up onto the view's)
            Upright::Along(up) => {
                let u = onto * m.transform_vector3(up);
                if u.truncate().length() > 1e-6 { u.x.atan2(u.y) } else { 0.0 }
            }
        };
        let rot = Quat::from_rotation_z(roll) * onto;
        let rr = rot * r;
        Pose { pos: Vec3::new(-rr.x, -rr.y, -rr.z - eye), rot }
    });
    state.sight = root_entity.zip(marker).filter(|_| aim > 0.0).map(|(e, m)| (e, m.transform_point3(rear), rear, aim));
    let mut pose = Pose { pos: HIP, rot: Quat::IDENTITY }.lerp(ads_pose, aim);
    // Walking: a slow, smooth figure of eight (a dip per cycle, a sway every two); breathing at rest (both calmer
    // aimed). Softer where the rig has a walk clip of its own, which bobs the arms too.
    let phase = state.stride / BOB_CYCLE * 2.0 * PI;
    let own_walk = clips::length(&lib, model, "walk").is_some();
    let stride = if p.mover.on_ground { (speed / RUN).min(1.0) } else { 0.0 } * steady * if own_walk { 0.5 } else { 1.0 };
    let breath = (state.time * 1.7).sin() * 0.002 * steady;
    pose.pos += Vec3::new((phase * 0.5).sin() * 0.007, (phase.cos() - 1.0) * 0.004, 0.0) * stride + Vec3::Y * breath;
    // Crouched a little lower; a landing dips it.
    // (Not aimed: down the sights the rear and front sight have to stay on the view's axis, crouched or not.)
    pose.pos += Vec3::Y * (-0.008 * p.mover.duck * (1.0 - aim) + state.dip.x.y * 0.02);
    // The look's lag and the kick.
    pose.rot = Quat::from_euler(EulerRot::YXZ, state.sway.x.x, state.sway.x.y, -state.sway.x.x * 0.5) * pose.rot;
    pose.pos += Vec3::new(-state.sway.x.x * 0.04, -state.sway.x.y * 0.04, 0.0) + state.kick.x * 0.02;
    pose.rot *= Quat::from_euler(EulerRot::YXZ, state.kick_rot.x.y * 0.02, state.kick_rot.x.x * 0.02, state.kick_rot.x.z * 0.02);
    // Taking out a weapon without a draw clip: up from below.
    let has_draw = clips::length(&lib, model, "draw").is_some();
    if let Action::Drawing(left) = inv.action
        && !has_draw
    {
        let off = 1.0 - ease(1.0 - left / inv.def().draw.max(1e-3));
        pose = pose.then(Pose::new([0.0, -0.2 * off, 0.05 * off], [-50.0 * off, 0.0, 20.0 * off]));
    }
    let mut grenade_shown = true;
    match inv.def().class {
        // The knife rig swings its arm in its own clips; the empty hands (while its file is missing) are moved whole.
        Class::Knife if model == Model::Hands => {
            if let Some(sw) = inv.swing {
                pose = pose.then(swing_frame(sw.stab, sw.t));
            }
        }
        Class::Grenade => match inv.action {
            Action::PinPulled { lob } => {
                // Raised, pin out, ready: overhand up by the shoulder, underhand low.
                let back = if lob { Pose::new([0.0, -0.06, 0.02], [18.0, 0.0, 0.0]) } else { Pose::new([0.03, 0.035, 0.05], [-18.0, -6.0, 6.0]) };
                pose = pose.then(Pose::IDENTITY.lerp(back, 0.9));
            }
            _ if state.thrown_ago < 0.45 => {
                let t = (state.thrown_ago / 0.3).min(1.0);
                pose = pose.then(Pose::new([-0.05, 0.08 - 0.3 * t, -0.2 + 0.2 * t], [50.0 - 80.0 * t, 20.0, 0.0]));
                grenade_shown = false;
            }
            _ => {}
        },
        Class::Bomb => {
            if let Action::Planting(t) = inv.action {
                let k = ease((t / 0.4).min(1.0));
                pose = pose.then(Pose::new([0.0, -0.12 * k, -0.08 * k], [-40.0 * k, 0.0, 0.0]));
            }
        }
        _ => {}
    }
    // The grenade rig throws in its own clips; the grenade leaves the hand with the throw.
    state.grenade_hidden = !grenade_shown || (model == Model::Grenade && throwing);
    let scoped = inv.zoom > 0 || crate::gear::in_optic(inv);
    // The warm-up shows every rig for a moment (their shaders compile behind the loading screen).
    let warming = warmup.active();
    for (_, r, mut rtf, mut vis, _) in &mut roots {
        if warming && r.0 != model {
            *vis = Visibility::Inherited;
            *rtf = Pose { pos: HIP, rot: Quat::IDENTITY }.transform();
        } else if r.0 == model && !scoped {
            *vis = Visibility::Inherited;
            *rtf = pose.transform();
        } else {
            *vis = Visibility::Hidden;
        }
    }
    let active_held = match inv.def().class {
        Class::Knife => Some(HeldKind::Knife),
        Class::Grenade => Some(HeldKind::Grenade),
        Class::Bomb => Some(HeldKind::Bomb),
        _ => None,
    };
    for (h, mut vis, mut htf, parent) in &mut held {
        let show = Some(h.0) == active_held && (h.0 != HeldKind::Grenade || grenade_shown);
        *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
        let scale = globals.get(parent.parent()).map_or(1.0, |g| g.to_scale_rotation_translation().0.x / view_global.to_scale_rotation_translation().0.x);
        if scale > 1e-6 {
            let hold = match h.0 {
                HeldKind::Knife => Transform::from_xyz(0.0, 0.015, 0.0).with_rotation(Quat::from_euler(EulerRot::YXZ, 0.35, 0.95, -0.2)),
                HeldKind::Grenade => Transform::from_xyz(0.0, 0.125, 0.045),
                HeldKind::Bomb => Transform::from_xyz(0.02, 0.08, 0.06).with_rotation(Quat::from_euler(EulerRot::YXZ, 0.3, 0.2, 0.0)),
            };
            *htf = hold.with_scale(Vec3::splat(1.0 / scale));
        }
    }
    for mut ftf in &mut flashes {
        let lit = state.fired_ago < 0.035 && inv.def().class != Class::Shotgun || state.fired_ago < 0.05 && inv.def().class == Class::Shotgun;
        // A new shape every frame it shows: size, length and the roll of the petals.
        ftf.scale = if lit { Vec3::new(1.0, 1.0, 0.8 + 0.5 * rand01()) * (0.24 + 0.12 * rand01()) } else { Vec3::splat(1e-4) };
        ftf.rotation = Quat::from_rotation_z(rand01() * PI);
        ftf.translation = Vec3::new(0.0, 0.0, -0.01);
    }
    // Where the muzzle and the ejection port would be at full size in the world, for tracers and brass.
    let eye = view_global.translation();
    let full = |p: Vec3| eye + (p - eye) / SHRINK;
    markers_out.muzzle = markers.iter().find(|(m, _)| m.model == model && m.kind == Kind::Muzzle).map(|(_, g)| full(g.translation()));
    markers_out.ejection = markers.iter().find(|(m, _)| m.model == model && m.kind == Kind::Ejection).map(|(_, g)| full(g.translation()));
    markers_out.right = view_global.right().as_vec3();

    // The clips.
    let Some(root) = root_entity else { return };
    let Some((mut rig, mut player, mut transitions)) = rigs.iter_mut().find(|(r, ..)| r.root == root) else { return };
    let len = |name: &str| clips::length(&lib, model, name);
    let first = !throwing && state.shown != Some(weapon);
    if first {
        state.shown = Some(weapon);
        state.reloading = false;
    }
    let (_, finished) = clips::progress(&lib, &rig, &player);
    let playing = rig.playing.clone().unwrap_or_default();
    let moving = speed > 1.0 && p.mover.on_ground;
    let rest = if moving && len("walk").is_some() { "walk" } else { "idle" };
    let empty = inv.gun().is_some_and(|g| g.clip == 0);
    // A knife swing just begun (its time went back to the start, or there was none).
    let swing = inv.swing.filter(|sw| state.swing_t.is_none_or(|t| sw.t < t));
    state.swing_t = inv.swing.map(|sw| sw.t);
    // The grenade: the pin pulled and the arm wound up (held there), then the throw or the lob once it's gone.
    let grenade = if throwing {
        Some(if state.lob { "lob" } else { "throw" })
    } else if let (Class::Grenade, Action::PinPulled { lob }) = (inv.def().class, inv.action) {
        state.lob = lob;
        Some(if lob { "pin_lob" } else { "pin" })
    } else {
        None
    };
    let want = if let Some(clip) = grenade.filter(|c| len(c).is_some()) {
        Want { clip, restart: false, speed: 1.0, repeat: false }
    } else if let Some(sw) = swing.filter(|sw| len(if sw.stab { "stab" } else { "slash" }).is_some()) {
        // The slash as long as the knife's cycle; the stab at its own pace (the arm is back before the next one).
        let (clip, speed) = if sw.stab { ("stab", 1.0) } else { ("slash", (len("slash").unwrap_or(0.5) / inv.def().cycle.max(0.1)).clamp(0.8, 2.0)) };
        Want { clip, restart: true, speed, repeat: false }
    } else if let Action::Reloading { total, .. } = inv.action {
        let clip = if empty && len("reload_empty").is_some() { "reload_empty" } else { "reload" };
        let start = !state.reloading;
        state.reloading = true;
        let length = if inv.def().shells.is_some() { total.max(0.3) * 1.5 } else { total };
        Want { clip, restart: start, speed: len(clip).unwrap_or(1.0) / length.max(0.1), repeat: false }
    } else if first && has_draw {
        state.reloading = false;
        Want { clip: "draw", restart: true, speed: len("draw").unwrap_or(1.0) / inv.def().draw.max(0.1), repeat: false }
    } else {
        state.reloading = false;
        let one_shot = matches!(playing.as_str(), "fire" | "draw" | "reload" | "reload_empty" | "slash" | "stab" | "throw" | "lob");
        if one_shot && !finished && !first {
            Want { clip: "", restart: false, speed: 0.0, repeat: false }
        } else {
            Want { clip: rest, restart: playing != rest, speed: 1.0, repeat: true }
        }
    };
    if !want.clip.is_empty() && (want.restart || playing != want.clip) {
        let fade = match want.clip {
            "reload" | "reload_empty" => 0.12,
            "slash" | "stab" | "throw" | "lob" => 0.06,
            _ => 0.18,
        };
        if clips::play(&lib, &mut rig, &mut player, &mut transitions, want.clip, fade, want.repeat) {
            clips::speed(&lib, &rig, &mut player, want.speed);
        }
    }
    // The walk clip at the feet's pace: one of its cycles per bob cycle at full speed (the packs' clips are a
    // sprint's, 0.6-1 s), slower when walking.
    if rig.playing.as_deref() == Some("walk") {
        let pace = len("walk").unwrap_or(0.85) / (BOB_CYCLE / RUN) * (speed / RUN).clamp(0.45, 1.0);
        clips::speed(&lib, &rig, &mut player, pace.clamp(0.3, 1.0));
    }
    // A shot cycles the gun's own parts (slide, bolt, hammer) over whatever the arms are doing; see `mask_fire`.
    if let Some(&(node, length)) = lib.clips(model).and_then(|c| c.nodes.get("fire_parts")) {
        if fired_now {
            let speed = (length / inv.def().cycle.max(0.05)).clamp(1.0, 3.0);
            player.start(node).set_speed(speed);
        } else if player.animation(node).is_some_and(|a| a.is_finished()) {
            player.stop(node);
        }
    }
}

/// The grenade rig's grenade: out of the hand once thrown (until the next one is drawn).
fn show_grenade(state: Res<ViewState>, mut grenades: Query<(&Marker, &mut Visibility)>) {
    for (m, mut vis) in &mut grenades {
        if m.kind == Kind::Grenade {
            vis.set_if_neq(if state.grenade_hidden { Visibility::Hidden } else { Visibility::Inherited });
        }
    }
}

/// How the aimed gun is stood upright about the view's axis: a measured roll (degrees), or the gun's own up (in the
/// `sight-eye` marker's frame) turned onto the view's.
#[derive(Clone, Copy)]
enum Upright {
    Roll(f32),
    Along(Vec3),
}

/// Each rig's iron sights, in its `sight-eye` marker's frame (which rides the weapon): the rear sight's and the front
/// sight's aim points. The markers from the model build assumed the weapon lies straight ahead of the eye in the idle
/// pose, which most packs don't (the pistol and the shotgun point a few degrees aside, the SMG's and the MG's markers
/// sit centimetres off); these were measured on the rigs' geometry in the idle's first frame and checked by rendering
/// the sight picture, in Blender and in the game: the rifle's aperture and post, the AK's leaf and post (post level
/// with the leaf), the MG's aperture and hooded post (the hood centred), the pistol's notch and blade. Third: how far
/// behind the rear sight the eye is (rig units), clear of the stock and the arms (for the pistol just behind the slide:
/// further back the eye sits between the pack's arms and the sleeves fill the view); fourth: a
/// roll (degrees) that stands a canted gun upright. The SMG pack holds its gun canted with the support hand over the
/// sights, and along the shotgun's rib the eye ends up inside the sleeve or the receiver, so those two have none:
/// aimed, they keep the crosshair.
fn sights(model: Model) -> Option<(Vec3, Vec3, f32, f32)> {
    Some(match model {
        Model::Rifle => (Vec3::new(-0.0108, -0.0398, -0.1570), Vec3::new(-0.0108, -0.0405, -0.5616), 0.21, 0.0),
        Model::Ak => (Vec3::new(0.0060, -0.0016, -0.2664), Vec3::new(-0.0019, -0.0041, -0.5816), 0.25, 0.0),
        Model::Mg => (Vec3::new(0.0155, 0.0788, -0.0152), Vec3::new(0.0055, 0.0532, -0.5002), 0.08, 0.0),
        Model::Pistol => (Vec3::new(-0.0455, -0.0112, -0.3241), Vec3::new(-0.0595, -0.0091, -0.4825), 0.12, -10.6),
        _ => return None,
    })
}

/// Whether aiming this weapon brings its iron sights onto the middle of the screen (the HUD then fades the
/// crosshair's arms and keeps its dot).
pub fn has_sights(w: WeaponId) -> bool {
    match crate::gear::own(w) {
        Some(o) => o.eye.is_some(),
        None => sights(model_of(w)).is_some(),
    }
}

/// Firing moves only the gun's own parts (slide, bolt, trigger, hammer): the pack's `fire` clip masked to the bones
/// under the weapon's body bone (the one its markers ride), played over the idle as `fire_parts`. The arms and the
/// gun as a whole stay where the idle has them; `animate` adds a small procedural kick instead. Built once per model,
/// when its scene is in.
fn mask_fire(
    mut lib: ResMut<Library>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    roots: Query<(Entity, &ViewRoot)>,
    markers: Query<(Entity, &Marker)>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    targets: Query<&AnimationTargetId>,
    mut done: Local<HashSet<Model>>,
) {
    for (root, &ViewRoot(model)) in &roots {
        if done.contains(&model) || matches!(model, Model::Hands) {
            continue;
        }
        let Some(clips) = lib.clips(model) else { continue };
        let Some(&(fire, length)) = clips.nodes.get("fire") else {
            done.insert(model);
            continue;
        };
        let Some(body) = markers.iter().find(|(_, m)| m.model == model && m.kind == Kind::Muzzle).and_then(|(e, _)| parents.get(e).ok()).map(|p| p.parent()) else { continue };
        let parts: HashSet<AnimationTargetId> = children.iter_descendants(body).filter_map(|e| targets.get(e).ok().copied()).collect();
        let handle = clips.graph.clone();
        let Some(mut graph) = graphs.get_mut(&handle) else { continue };
        let Some(AnimationNodeType::Clip(clip)) = graph.get(fire).map(|n| n.node_type.clone()) else {
            done.insert(model);
            continue;
        };
        let mut masked = 0;
        for e in children.iter_descendants(root) {
            if let Ok(t) = targets.get(e)
                && !parts.contains(t)
            {
                graph.add_target_to_mask_group(*t, 0);
                masked += 1;
            }
        }
        // Weighted well above the idle, so the parts follow the shot, not a blend of the two.
        let root_node = graph.root;
        let node = graph.add_clip_with_mask(clip, 1, 20.0, root_node);
        lib.add_node(model, "fire_parts", node, length);
        debug!("{model:?}: fire clip masked to {} parts ({masked} other targets)", parts.len());
        done.insert(model);
    }
}

/// Aimed, the sight has to sit on the view's axis in the frame that is drawn. `animate` aligns it from the last
/// frame's transforms (the clips advance after it); here, after the clips and before the transforms propagate, the rig
/// is shifted by as much as this frame's clip moved the sight. Without it the sight picture lagged a frame behind every
/// shot and breath, and at a few millimetres from the eye that is a visible jitter.
fn hold_sight(state: Res<ViewState>, mut roots: Query<(&ViewRoot, &mut Transform)>, markers: Query<(Entity, &Marker)>, locals: Query<(&Transform, &ChildOf), Without<ViewRoot>>) {
    let Some((root, used, at, aim)) = state.sight else { return };
    let Ok((r, mut rtf)) = roots.get_mut(root) else { return };
    let Some((mut e, _)) = markers.iter().find(|(_, m)| m.model == r.0 && m.kind == Kind::SightEye) else { return };
    // The sight in the rig's frame, from the local transforms up to the rig's root.
    let mut m = bevy::math::Affine3A::IDENTITY;
    for _ in 0..64 {
        let Ok((tf, parent)) = locals.get(e) else { return };
        m = tf.compute_affine() * m;
        e = parent.parent();
        if e == root {
            let shift = rtf.rotation * (used - m.transform_point3(at)) * aim;
            rtf.translation += shift;
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_weapon_has_a_viewmodel() {
        for w in WeaponId::ALL {
            let _ = model_of(w);
        }
        assert_eq!(family_of(WeaponId::Akm), Model::Ak);
        assert_eq!(family_of(WeaponId::Akm).fallback(), Model::Rifle);
        assert_eq!(family_of(WeaponId::G95), Model::Rifle);
        assert_eq!(family_of(WeaponId::P8), Model::Pistol);
        // A gun with its own model stands in with its class's while that loads.
        assert_eq!(model_of(WeaponId::G36), Model::Gun(WeaponId::G36));
        assert_eq!(model_of(WeaponId::G36).fallback(), Model::Rifle);
        assert_eq!(model_of(WeaponId::Ak74), Model::Ak);
        assert_eq!(model_of(WeaponId::He), Model::Grenade);
        assert_eq!(model_of(WeaponId::Knife).fallback(), Model::Hands);
    }

    #[test]
    fn a_knife_swing_returns_to_rest() {
        for stab in [false, true] {
            let end = swing_frame(stab, 5.0);
            assert!(end.pos.length() < 1e-5 && end.rot.angle_between(Quat::IDENTITY) < 1e-4);
        }
    }
}


/// The muzzle flash's two textures (white-hot to orange, alpha as intensity, for additive blending): the flame seen
/// from the side (u from the muzzle to the tip: a hot core fanning into ragged tongues) and the star seen from in
/// front (a bright core with five uneven spikes).
pub fn flash_images() -> (Image, Image) {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let colour = |i: f32| -> [u8; 4] {
        let i = i.clamp(0.0, 1.0);
        // Orange at the edges, yellow, white at the core.
        let (r, g, b) = (1.0, 0.45 + 0.5 * i.powf(0.6), 0.12 + 0.8 * i.powf(2.0));
        [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, (i * 255.0) as u8]
    };
    let hash = |x: f32| ((x * 12.9898).sin() * 43758.547).fract().abs();
    let (w, h) = (128u32, 64u32);
    let mut side = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let v = (y as f32 + 0.5) / h as f32 * 2.0 - 1.0;
            // Tongues: the width wobbles along the length.
            let wob = 0.55 + 0.25 * (u * 23.0).sin() * (u * 7.0 + 1.3).cos() + 0.15 * hash((u * 16.0).floor());
            let width = (0.9 - 0.75 * u) * wob;
            let across = (1.0 - (v.abs() / width.max(0.05))).max(0.0);
            let core = (-u * 7.0).exp() * (1.0 - v.abs() * 2.2).max(0.0);
            let i = across.powf(1.6) * (1.0 - u).powf(1.3) + core * 0.8;
            side.extend_from_slice(&colour(i));
        }
    }
    let s = 128u32;
    let mut front = Vec::with_capacity((s * s * 4) as usize);
    for y in 0..s {
        for x in 0..s {
            let px = (x as f32 + 0.5) / s as f32 * 2.0 - 1.0;
            let py = (y as f32 + 0.5) / s as f32 * 2.0 - 1.0;
            let r = (px * px + py * py).sqrt();
            let a = py.atan2(px);
            let spikes = ((a * 5.0 + 0.4).cos() * 0.5 + 0.5).powf(6.0) * (0.7 + 0.3 * (a * 3.0).sin());
            let i = (-r * 7.0).exp() * 1.2 + spikes * (1.0 - r).max(0.0).powf(1.5) * 0.9;
            front.extend_from_slice(&colour(i));
        }
    }
    let img = |w: u32, h: u32, data: Vec<u8>| Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    (img(w, h, side), img(s, s, front))
}
