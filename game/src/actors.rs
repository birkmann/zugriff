//! Players seen from outside (bots, and whoever the camera isn't): `models/soldier-*.glb`, skinned characters with the
//! Universal Animation Library's clips retargeted onto them and a rifle hold baked into their upper bodies, plus two
//! sockets — `weapon-socket` on the chest for guns, `hand-socket` in the right hand for the knife and grenades.
//!
//! - Clips follow the player: idle, walk, run, crouched still or walking, in the air; the playback speed follows the
//!   ground speed (backwards: the walk in reverse), and walking sideways the hips turn toward it while the chest turns
//!   back to the aim.
//! - One-shots: the flinch of a hit, the knife's jab, a grenade's throw; killed, the death clip, held at its end.
//! - After the clips, the chest bends to the aim's pitch; the weapon in hand shows, with its muzzle flash.
//!
//! Hostages are the OPFOR model repainted in civilian colours, without weapons.

use std::f32::consts::PI;

use bevy::animation::transition::AnimationTransitions;
use bevy::prelude::*;
use zugriff_sim::game::{Event, PlayerId, Team};
use zugriff_sim::weapons::{Action, Class, WeaponId};

use crate::clips::{self, Animated, Library, Model, Rig};
use crate::session::{Frame, Session, wrap};

pub struct ActorsPlugin;

impl Plugin for ActorsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Library>()
            .add_systems(PostUpdate, bend_to_aim.after(bevy::app::AnimationSystems).before(TransformSystems::Propagate))
            .add_systems(
                Update,
                (sync, find_parts, hostage_fallback, clips::build_graphs, clips::hook_players, repaint_hostages, arm_sockets, animate, paint_props, gun_props, hostages).chain().after(crate::session::SessionSet),
            );
    }
}

/// A player's body.
#[derive(Component)]
pub struct Actor {
    pub id: PlayerId,
    pub team: Team,
    /// Seconds since the last round it fired (muzzle flash), was hit (flinch).
    fired: f32,
    hit: f32,
    swung: f32,
    thrown: f32,
}

/// A hostage's body.
#[derive(Component)]
pub struct HostageActor(pub usize);

/// The model's root under an actor (turned to face −Z, further as the hips follow a sideways walk).
#[derive(Component)]
pub struct Root;

#[derive(Component)]
pub struct Socket {
    actor: Entity,
    hand: bool,
    armed: bool,
}

/// A weapon model in an actor's hands (one per kind; the one in use is shown).
#[derive(Component)]
pub struct Prop {
    actor: Entity,
    kind: PropKind,
    scene: Option<Handle<WorldAsset>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropKind {
    Rifle,
    Ak,
    Sniper,
    Shotgun,
    Smg,
    Mg,
    Pistol,
    Knife,
    Grenade,
    Bomb,
}

pub fn prop_kind(w: WeaponId) -> PropKind {
    match w.def().class {
        Class::Pistol => PropKind::Pistol,
        Class::Knife => PropKind::Knife,
        Class::Grenade => PropKind::Grenade,
        Class::Bomb => PropKind::Bomb,
        Class::Smg => PropKind::Smg,
        Class::Shotgun => PropKind::Shotgun,
        Class::Sniper => PropKind::Sniper,
        Class::Mg => PropKind::Mg,
        _ if matches!(w, WeaponId::Akm | WeaponId::Ak74) => PropKind::Ak,
        _ => PropKind::Rifle,
    }
}

/// A weapon's model file on a body or on the floor, and what stands in while it's missing.
pub fn prop_file(k: PropKind) -> (&'static str, Option<PropKind>) {
    match k {
        PropKind::Rifle => ("models/m4.glb", None),
        PropKind::Ak => ("models/ak.glb", Some(PropKind::Rifle)),
        PropKind::Sniper => ("models/sniper.glb", Some(PropKind::Rifle)),
        PropKind::Shotgun => ("models/shotgun.glb", Some(PropKind::Rifle)),
        PropKind::Smg => ("models/smg.glb", Some(PropKind::Rifle)),
        PropKind::Mg => ("models/mg.glb", Some(PropKind::Rifle)),
        PropKind::Pistol => ("models/pistol.glb", None),
        PropKind::Knife => ("models/knife.glb", None),
        PropKind::Grenade => ("models/grenade.glb", None),
        PropKind::Bomb => ("", None),
    }
}

#[derive(Component)]
pub struct Chest {
    actor: Entity,
    /// This bone's share of turning back from the hips' turn (waist, lower spine and chest together make the whole),
    /// and of bending to the aim's pitch (lower spine and chest: a back bent at one joint looked broken).
    share: f32,
    pitch: f32,
    /// The rotation written last frame and the turn it added: a bone no clip animates keeps what was written, and the
    /// turn is taken off again before this frame's (else it added up frame after frame and bent the body double).
    written: Option<(Quat, Quat)>,
}

#[derive(Component)]
pub struct PropFlash {
    actor: Entity,
}

#[derive(Component, Default)]
pub struct Gait {
    twist: f32,
    one_shot: Option<&'static str>,
    /// Seconds off the ground (a step down a kerb or a stair isn't a jump), the clip chosen last and its speed.
    air: f32,
    clip: &'static str,
    rate: f32,
}

/// Marks a hostage's materials as repainted.
#[derive(Component)]
struct Repainted;

pub fn model_for(team: Team) -> Model {
    match team {
        Team::Ksk => Model::SoldierBlue,
        Team::Opfor => Model::SoldierRed,
    }
}

/// One actor per player (rebuilt when a player changes sides), one per hostage; placed each frame.
#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    server: Res<AssetServer>,
    session: Option<Res<Session>>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    frame: Res<Frame>,
    spectate: Res<crate::view::Spectate>,
    mut actors: Query<(Entity, &mut Actor, &mut Transform, &mut Visibility), Without<HostageActor>>,
    mut hostage_actors: Query<(Entity, &HostageActor, &mut Transform, &mut Visibility), Without<Actor>>,
) {
    let Some(s) = session else {
        for (e, ..) in &actors {
            commands.entity(e).despawn();
        }
        for (e, ..) in &hostage_actors {
            commands.entity(e).despawn();
        }
        return;
    };
    if frame.started {
        for (e, ..) in &actors {
            commands.entity(e).despawn();
        }
        for (e, ..) in &hostage_actors {
            commands.entity(e).despawn();
        }
        return;
    }
    let dt = time.delta_secs();
    let a = fixed.overstep_fraction();
    let viewed = s.viewed(spectate.target);
    let first_person = s.game.players[viewed].alive;
    let mut have: Vec<PlayerId> = Vec::new();
    for (e, mut actor, mut tf, mut vis) in &mut actors {
        let Some(p) = s.game.players.get(actor.id) else {
            commands.entity(e).despawn();
            continue;
        };
        if p.team != actor.team {
            commands.entity(e).despawn();
            continue;
        }
        have.push(actor.id);
        actor.fired += dt;
        actor.hit += dt;
        actor.swung += dt;
        actor.thrown += dt;
        for ev in &frame.events {
            match ev {
                Event::Shot { player, .. } if *player == actor.id => actor.fired = 0.0,
                Event::Hit { victim, .. } if *victim == actor.id => actor.hit = 0.0,
                Event::KnifeSwing { player, .. } if *player == actor.id => actor.swung = 0.0,
                Event::Throw { player, .. } if *player == actor.id => actor.thrown = 0.0,
                _ => {}
            }
        }
        let snap = s.lerp(actor.id, a);
        tf.translation = snap.origin;
        tf.rotation = Quat::from_rotation_y(snap.yaw);
        // Hidden: the body whose eyes the camera uses. The dead stay where they fell until the next round.
        *vis = if actor.id == viewed && first_person { Visibility::Hidden } else { Visibility::Inherited };
    }
    let bare = crate::args::flag("noactors");
    for p in &s.game.players {
        if have.contains(&p.id) {
            continue;
        }
        let model = model_for(p.team);
        if bare {
            commands.spawn((Actor { id: p.id, team: p.team, fired: 9.0, hit: 9.0, swung: 9.0, thrown: 9.0 }, Transform::from_translation(p.mover.origin), Visibility::Inherited));
            continue;
        }
        let e = commands
            .spawn((Actor { id: p.id, team: p.team, fired: 9.0, hit: 9.0, swung: 9.0, thrown: 9.0 }, Transform::from_translation(p.mover.origin), Visibility::Inherited, Name::new(format!("actor {}", p.name))))
            .id();
        commands.entity(e).with_children(|c| {
            c.spawn((Root, WorldAssetRoot(model.scene(&server)), Animated(model), Transform::from_rotation(Quat::from_rotation_y(PI)), Visibility::Inherited));
        });
    }
    // Hostages.
    let mut have_h: Vec<usize> = Vec::new();
    for (e, h, mut tf, mut vis) in &mut hostage_actors {
        let Some(host) = s.game.hostages.get(h.0) else {
            commands.entity(e).despawn();
            continue;
        };
        have_h.push(h.0);
        tf.translation = s.hostage_pos(h.0, a);
        tf.rotation = Quat::from_rotation_y(host.yaw);
        *vis = if host.rescued { Visibility::Hidden } else { Visibility::Inherited };
    }
    for (i, h) in s.game.hostages.iter().enumerate() {
        if have_h.contains(&i) {
            continue;
        }
        let e = commands.spawn((HostageActor(i), Transform::from_translation(h.mover.origin), Visibility::Inherited, Name::new(format!("hostage {i}")))).id();
        commands.entity(e).with_children(|c| {
            c.spawn((Root, WorldAssetRoot(Model::Hostage.scene(&server)), Animated(Model::Hostage), Transform::from_rotation(Quat::from_rotation_y(PI)), Visibility::Inherited));
        });
    }
}

fn owner(mut e: Entity, parents: &Query<&ChildOf>, is_owner: impl Fn(Entity) -> bool) -> Option<Entity> {
    for _ in 0..48 {
        if is_owner(e) {
            return Some(e);
        }
        e = parents.get(e).ok()?.parent();
    }
    None
}

/// The models' parts as their scenes spawn: sockets, the chest bone, muzzles.
#[allow(clippy::too_many_arguments)]
fn find_parts(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut lib: ResMut<Library>,
    roots: Query<(), Added<Root>>,
    named: Query<(Entity, &Name), Added<Name>>,
    parents: Query<&ChildOf>,
    actors: Query<(), Or<(With<Actor>, With<HostageActor>)>>,
    props: Query<&Prop>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut flash: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !roots.is_empty() {
        lib.want(&server, Model::SoldierBlue);
        lib.want(&server, Model::SoldierRed);
        lib.want(&server, Model::Hostage);
    }
    for (e, name) in &named {
        let n = name.as_str();
        let Some(actor) = owner(e, &parents, |x| actors.contains(x)) else { continue };
        match n {
            "weapon-socket" | "hand-socket" => {
                commands.entity(e).insert(Socket { actor, hand: n == "hand-socket", armed: false });
            }
            "muzzle" if owner(e, &parents, |x| props.contains(x)).is_some() => {
                // A star burst (the viewmodel's star texture on three crossed quads), right whichever way it is seen.
                let (mesh, material) = flash
                    .get_or_insert_with(|| {
                        let mesh = meshes.add(Rectangle::new(1.0, 1.0));
                        let (_, star) = crate::viewmodel::flash_images();
                        let material = materials.add(StandardMaterial {
                            base_color: LinearRgba::rgb(1.9, 1.05, 0.35).into(),
                            base_color_texture: Some(images.add(star)),
                            unlit: true,
                            alpha_mode: AlphaMode::Add,
                            double_sided: true,
                            cull_mode: None,
                            ..default()
                        });
                        (mesh, material)
                    })
                    .clone();
                commands.entity(e).with_children(|c| {
                    c.spawn((PropFlash { actor }, Transform::from_scale(Vec3::splat(1e-4)), Visibility::Inherited)).with_children(|f| {
                        for rot in [Quat::IDENTITY, Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)] {
                            f.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::from_rotation(rot), bevy::light::NotShadowCaster));
                        }
                    });
                });
            }
            _ if n.starts_with("CC_Base_Spine02") || n.contains(":Spine2_") || n.starts_with("Spine2_") => {
                commands.entity(e).insert(Chest { actor, share: 0.4, pitch: 0.4, written: None });
            }
            // (The hips' turn taken back over three bones: all of it in the chest bent the torso like a corkscrew.)
            _ if n.starts_with("CC_Base_Spine01") => {
                commands.entity(e).insert(Chest { actor, share: 0.3, pitch: 0.3, written: None });
            }
            _ if n.starts_with("CC_Base_Waist") => {
                commands.entity(e).insert(Chest { actor, share: 0.3, pitch: 0.0, written: None });
            }
            _ => {}
        }
    }
}

/// Until the civilian model exists, hostages wear the OPFOR model.
fn hostage_fallback(lib: Res<Library>, server: Res<AssetServer>, mut roots: Query<(&mut Animated, &mut WorldAssetRoot), With<Root>>) {
    if !lib.failed(&server, Model::Hostage) {
        return;
    }
    for (mut a, mut scene) in &mut roots {
        if a.0 == Model::Hostage {
            a.0 = Model::SoldierRed;
            scene.0 = Model::SoldierRed.scene(&server);
        }
    }
}

/// Hostages in the stand-in model wear civilian colours: its materials are copied and tinted once the scene is in.
fn repaint_hostages(
    mut commands: Commands,
    new: Query<(Entity, &MeshMaterial3d<StandardMaterial>), (Added<MeshMaterial3d<StandardMaterial>>, Without<Repainted>)>,
    parents: Query<&ChildOf>,
    hostages: Query<&Children, With<HostageActor>>,
    animated: Query<&Animated>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (e, m) in &new {
        let Some(h) = owner(e, &parents, |x| hostages.contains(x)) else { continue };
        let stand_in = hostages.get(h).is_ok_and(|ch| ch.iter().any(|c| animated.get(c).is_ok_and(|a| a.0 == Model::SoldierRed)));
        if !stand_in {
            continue;
        }
        let Some(base) = materials.get(&m.0).cloned() else { continue };
        let mut tinted = base;
        // A washed-out shirt and trousers: lift the camouflage towards pale denim blue.
        let c = tinted.base_color.to_srgba();
        tinted.base_color = Color::srgba(c.red * 0.55 + 0.35, c.green * 0.55 + 0.38, c.blue * 0.55 + 0.5, c.alpha);
        commands.entity(e).insert((MeshMaterial3d(materials.add(tinted)), Repainted));
    }
}

/// The weapons into the sockets once the sockets are placed (a socket carries its bone's scale; the weapons are
/// modelled at real size).
fn arm_sockets(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut sockets: Query<(Entity, &mut Socket, &GlobalTransform)>,
    hostages: Query<(), With<HostageActor>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (e, mut socket, global) in &mut sockets {
        if socket.armed {
            continue;
        }
        let scale = global.to_scale_rotation_translation().0.x;
        if !(scale > 1e-6) {
            continue;
        }
        socket.armed = true;
        if hostages.contains(socket.actor) {
            continue;
        }
        let actor = socket.actor;
        let props: &[PropKind] = if socket.hand { &[PropKind::Knife, PropKind::Grenade] } else { &[PropKind::Rifle, PropKind::Ak, PropKind::Sniper, PropKind::Shotgun, PropKind::Smg, PropKind::Mg, PropKind::Pistol] };
        let inv = Vec3::splat(1.0 / scale);
        commands.entity(e).with_children(|c| {
            for kind in props {
                let scene: Handle<WorldAsset> = server.load(GltfAssetLabel::Scene(0).from_asset(prop_file(*kind).0));
                c.spawn((WorldAssetRoot(scene.clone()), Transform::from_scale(inv), Visibility::Hidden, Prop { actor, kind: *kind, scene: Some(scene) }));
            }
            if !socket.hand {
                // Guns with a model of their own go here (see `gun_props`).
                c.spawn((Transform::from_scale(inv), Visibility::Hidden, GunProp { actor, weapon: None }));
            }
            if socket.hand {
                // The bomb: a pack of charges with a keypad, drawn from boxes.
                let pack = meshes.add(Cuboid::new(0.2, 0.1, 0.28));
                let olive = materials.add(StandardMaterial { base_color: Color::srgb(0.25, 0.27, 0.2), perceptual_roughness: 0.8, ..default() });
                c.spawn((Mesh3d(pack), MeshMaterial3d(olive), Transform::from_scale(inv).with_translation(Vec3::new(0.0, 0.05, 0.0) * inv.x), Visibility::Hidden, Prop { actor, kind: PropKind::Bomb, scene: None }));
            }
        });
    }
}

/// The chest socket's place for a gun with a model of its own: its scene is swapped for the gun in hand's.
#[derive(Component)]
pub struct GunProp {
    actor: Entity,
    weapon: Option<zugriff_sim::weapons::WeaponId>,
}

fn gun_props(
    mut commands: Commands,
    server: Res<AssetServer>,
    session: Option<Res<Session>>,
    actors: Query<&Actor>,
    mut props: Query<(Entity, &mut GunProp, &mut Visibility, Option<&mut crate::finish::Painted>, Option<&Children>)>,
) {
    let Some(s) = session else { return };
    for (e, mut gp, mut vis, painted, children) in &mut props {
        let Ok(actor) = actors.get(gp.actor) else { continue };
        let p = &s.game.players[actor.id];
        let want = Some(p.inv.active).filter(|w| crate::gear::own(*w).is_some());
        if want != gp.weapon {
            gp.weapon = want;
            for c in children.into_iter().flatten() {
                commands.entity(*c).despawn();
            }
            if let Some(w) = want {
                let scene: Handle<WorldAsset> = server.load(GltfAssetLabel::Scene(0).from_asset(crate::gear::prop_path(w)));
                commands.entity(e).with_children(|c| {
                    c.spawn((WorldAssetRoot(scene), Transform::IDENTITY, Visibility::Inherited));
                });
            }
        }
        vis.set_if_neq(if want.is_some() && p.alive { Visibility::Inherited } else { Visibility::Hidden });
        if let Some(w) = want {
            let paint = crate::finish::Painted { weapon: w, finish: crate::finish::of(&p.inv, w) };
            match painted {
                Some(mut x) => {
                    x.set_if_neq(paint);
                }
                None => {
                    commands.entity(e).insert(paint);
                }
            }
        }
    }
}

/// A gun in someone's hands wears its finish.
fn paint_props(mut commands: Commands, session: Option<Res<Session>>, actors: Query<&Actor>, mut props: Query<(Entity, &Prop, &Visibility, Option<&mut crate::finish::Painted>)>) {
    let Some(s) = session else { return };
    for (e, prop, vis, painted) in &mut props {
        if *vis == Visibility::Hidden {
            continue;
        }
        let Ok(actor) = actors.get(prop.actor) else { continue };
        let inv = &s.game.players[actor.id].inv;
        let want = crate::finish::Painted { weapon: inv.active, finish: crate::finish::of(inv, inv.active) };
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

/// The clip for how a body is moving, its playback speed (negative: in reverse) and the hips' turn toward it.
pub fn locomotion(velocity_local: Vec3, crouch: f32, on_ground: bool) -> (&'static str, f32, f32) {
    gait_clip("", velocity_local, crouch, on_ground)
}

/// [`locomotion`], keeping the clip from before (`prev`) near the speeds where they change, so a body at the edge of
/// walking and running doesn't flick between them.
pub fn gait_clip(prev: &str, velocity_local: Vec3, crouch: f32, on_ground: bool) -> (&'static str, f32, f32) {
    let speed = velocity_local.with_y(0.0).length();
    if !on_ground {
        return ("jump", 1.0, 0.0);
    }
    let heading = (-velocity_local.x).atan2(-velocity_local.z);
    let backwards = heading.abs() > 1.95;
    // Sideways, the legs turn nearly all the way (the chest turns back to the aim): at 63° the feet slid.
    let twist = if speed < 0.3 { 0.0 } else if backwards { wrap(heading - PI) } else { heading }.clamp(-1.45, 1.45);
    let sign = if backwards { -1.0 } else { 1.0 };
    let (run_at, walk_at) = match prev {
        "run" => (3.3, 0.4),
        "walk" => (4.4, 0.25),
        _ => (3.9, 0.4),
    };
    let (clip, nominal) = if crouch > 0.5 {
        if speed > 0.3 { ("crouch_walk", 1.2) } else { ("crouch_idle", 0.0) }
    } else if speed > run_at {
        ("run", 4.6)
    } else if speed > walk_at {
        ("walk", 1.6)
    } else {
        ("idle", 0.0)
    };
    let rate = if nominal > 0.0 { (speed / nominal).clamp(0.6, 1.5) * sign } else { 1.0 };
    (clip, rate, twist)
}

#[allow(clippy::type_complexity)]
fn animate(
    time: Res<Time>,
    lib: Res<Library>,
    server: Res<AssetServer>,
    session: Option<Res<Session>>,
    mut actors: Query<(Entity, &Transform, &Actor, Option<&mut Gait>, &Children)>,
    mut roots: Query<&mut Transform, (With<Root>, Without<Actor>)>,
    mut rigs: Query<(&mut Rig, &mut AnimationPlayer, &mut AnimationTransitions)>,
    parents: Query<&ChildOf>,
    mut props: Query<(&Prop, &mut Visibility)>,
    mut flashes: Query<(&PropFlash, &mut Transform), (Without<Root>, Without<Actor>)>,
    mut commands: Commands,
) {
    let Some(s) = session else { return };
    let dt = time.delta_secs();
    for (e, tf, actor, gait, children) in &mut actors {
        let Some(mut gait) = gait else {
            commands.entity(e).insert(Gait::default());
            continue;
        };
        let p = &s.game.players[actor.id];
        let local = tf.rotation.inverse() * p.mover.velocity;
        // In the air only after a moment, or going up fast (a real jump): a step down isn't a hop.
        gait.air = if p.mover.on_ground { 0.0 } else { gait.air + dt };
        let airborne = !p.mover.on_ground && (gait.air > 0.2 || p.mover.velocity.y > 2.5);
        let (clip, rate, twist) = gait_clip(gait.clip, local, p.mover.duck, !airborne || !p.alive);
        gait.clip = clip;
        // The playback speed eases (the steps don't jerk with every change of pace); reversing flips it at once.
        gait.rate = if gait.rate * rate <= 0.0 { rate } else { gait.rate + (rate - gait.rate) * (6.0 * dt).min(1.0) };
        let rate = gait.rate;
        gait.twist += (if p.alive { twist } else { 0.0 } - gait.twist) * (6.0 * dt).min(1.0);
        for c in children.iter() {
            if let Ok(mut rtf) = roots.get_mut(c) {
                rtf.rotation = Quat::from_rotation_y(gait.twist) * Quat::from_rotation_y(PI);
            }
        }
        let one_shot = if !p.alive {
            None
        } else if actor.hit < dt {
            Some("hit")
        } else if actor.swung < dt {
            Some("knife")
        } else if actor.thrown < dt {
            Some("throw")
        } else {
            None
        };
        let Some((mut rig, mut player, mut transitions)) = rigs.iter_mut().find(|(r, ..)| parents.get(r.root).is_ok_and(|q| q.parent() == e)) else { continue };
        let playing = rig.playing.clone().unwrap_or_default();
        let (_, finished) = clips::progress(&lib, &rig, &player);
        if !p.alive {
            if playing != "death" {
                clips::play(&lib, &mut rig, &mut player, &mut transitions, "death", 0.12, false);
            }
            continue;
        }
        if let Some(shot) = one_shot {
            gait.one_shot = Some(shot);
            clips::play(&lib, &mut rig, &mut player, &mut transitions, shot, 0.06, false);
            continue;
        }
        if gait.one_shot.is_some() && !finished {
            continue;
        }
        gait.one_shot = None;
        if playing != clip {
            clips::play(&lib, &mut rig, &mut player, &mut transitions, clip, 0.2, true);
        }
        clips::speed(&lib, &rig, &mut player, rate);
    }
    let missing = |k: PropKind| matches!(prop_file(k).0, f if !f.is_empty() && matches!(server.get_load_state(&server.get_handle::<WorldAsset>(GltfAssetLabel::Scene(0).from_asset(f)).unwrap_or_default()), Some(bevy::asset::LoadState::Failed(_))));
    for (prop, mut vis) in &mut props {
        let Ok((_, _, actor, _, _)) = actors.get(prop.actor) else { continue };
        let p = &s.game.players[actor.id];
        let want = prop_kind(p.inv.active);
        let active = match prop_file(want).1 {
            Some(fb) if missing(want) => fb,
            _ => want,
        };
        let _ = &prop.scene;
        let thrown = prop.kind == PropKind::Grenade && actor.thrown < 0.4;
        let planting_hidden = false;
        let own = crate::gear::own(p.inv.active).is_some();
        *vis = if p.alive && active == prop.kind && !own && !thrown && !planting_hidden { Visibility::Inherited } else { Visibility::Hidden };
    }
    for (f, mut ftf) in &mut flashes {
        let fired = actors.get(f.actor).is_ok_and(|(_, _, a, ..)| a.fired < 0.05 && s.game.players[a.id].alive);
        ftf.scale = if fired { Vec3::splat(0.13 * (0.8 + 0.4 * rand01())) } else { Vec3::splat(1e-4) };
        ftf.rotation = Quat::from_rotation_z(rand01() * 3.14);
        ftf.translation = Vec3::new(0.0, 0.07, 0.0);
    }
    let _ = Action::Idle;
}

/// Hostages: idle, walking when led, the death clip when killed.
fn hostages(
    time: Res<Time>,
    lib: Res<Library>,
    session: Option<Res<Session>>,
    hostages: Query<(Entity, &Transform, &HostageActor, &Children)>,
    mut roots: Query<&mut Transform, (With<Root>, Without<HostageActor>, Without<Actor>)>,
    mut rigs: Query<(&mut Rig, &mut AnimationPlayer, &mut AnimationTransitions)>,
    parents: Query<&ChildOf>,
) {
    let Some(s) = session else { return };
    let _ = time;
    for (e, tf, h, children) in &hostages {
        let Some(host) = s.game.hostages.get(h.0) else { continue };
        for c in children.iter() {
            if let Ok(mut rtf) = roots.get_mut(c) {
                rtf.rotation = Quat::from_rotation_y(PI);
            }
        }
        let Some((mut rig, mut player, mut transitions)) = rigs.iter_mut().find(|(r, ..)| parents.get(r.root).is_ok_and(|q| q.parent() == e)) else { continue };
        let playing = rig.playing.clone().unwrap_or_default();
        if !host.alive {
            if playing != "death" {
                clips::play(&lib, &mut rig, &mut player, &mut transitions, "death", 0.12, false);
            }
            continue;
        }
        let local = tf.rotation.inverse() * host.mover.velocity;
        let (clip, rate, _) = locomotion(local, 0.0, true);
        // Held hostages crouch, waiting.
        let clip = if host.leader.is_none() && clip == "idle" { "crouch_idle" } else { clip };
        if playing != clip {
            clips::play(&lib, &mut rig, &mut player, &mut transitions, clip, 0.25, true);
        }
        clips::speed(&lib, &rig, &mut player, rate);
    }
}

/// After the clips: the chest bends to the aim's pitch and turns back from the hips' turn.
fn bend_to_aim(session: Option<Res<Session>>, actors: Query<(&Transform, &Actor, Option<&Gait>)>, mut chests: Query<(&mut Chest, &ChildOf, &mut Transform), Without<Actor>>, globals: Query<&GlobalTransform>, fixed: Res<Time<Fixed>>) {
    let Some(s) = session else { return };
    let a = fixed.overstep_fraction();
    for (mut chest, parent, mut tf) in &mut chests {
        let Ok((atf, actor, gait)) = actors.get(chest.actor) else { continue };
        // The bone as the clip left it: if no clip wrote it since last frame, without last frame's turn.
        if let Some((written, turn)) = chest.written
            && tf.rotation == written
        {
            tf.rotation = turn.inverse() * tf.rotation;
        }
        chest.written = None;
        if !s.game.players[actor.id].alive {
            continue;
        }
        let Ok(parent_global) = globals.get(parent.parent()) else { continue };
        let parent_rot = parent_global.to_scale_rotation_translation().1;
        let right = atf.rotation * Vec3::X;
        let twist = gait.map_or(0.0, |g| g.twist) * chest.share;
        let pitch = s.lerp(actor.id, a).pitch.clamp(-1.1, 1.1) * chest.pitch;
        let world = Quat::from_rotation_y(-twist) * Quat::from_axis_angle(right, pitch);
        let turn = parent_rot.inverse() * world * parent_rot;
        tf.rotation = turn * tf.rotation;
        chest.written = Some((tf.rotation, turn));
    }
}

/// A cheap random number for looks (not the sim's).
pub fn rand01() -> f32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEED: AtomicU32 = AtomicU32::new(0x1234_5678);
    let mut x = SEED.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    SEED.store(x, Ordering::Relaxed);
    x as f32 / u32::MAX as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_by_speed_and_stance() {
        assert_eq!(locomotion(Vec3::ZERO, 0.0, true).0, "idle");
        assert_eq!(locomotion(Vec3::new(0.0, 0.0, -1.5), 0.0, true).0, "walk");
        assert_eq!(locomotion(Vec3::new(0.0, 0.0, -5.5), 0.0, true).0, "run");
        assert_eq!(locomotion(Vec3::new(0.0, 0.0, -1.0), 1.0, true).0, "crouch_walk");
        assert_eq!(locomotion(Vec3::new(0.0, 3.0, -2.0), 0.0, false).0, "jump");
    }
}
