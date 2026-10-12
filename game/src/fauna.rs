//! Life in the maps: birds and rats. Client-side only (the sim knows nothing of them; they block nothing).
//!
//! - Birds, by map: pigeons on the old town's squares, gulls at the harbour (a few also circling high over the
//!   quay), crows on the air base's grass and aprons. A flock walks about pecking; when a player comes within a few
//!   metres, or a shot, a grenade or the bomb goes off nearby, it bursts up with flapping wings, flies off and lands
//!   at another open spot a while later.
//! - Rats (Poly Haven's street rat) in the town and the harbour: they sniff about at the foot of walls, dart along
//!   them, and bolt from players, vanishing once out of sight and turning up elsewhere later.
//!
//! The birds are built here from a few ellipsoids and two wing strips (vertex-coloured, lit by the sun and the
//! ambient light like the players); each is a root with a body and two wings the flight animates. `nofauna` leaves
//! them out.

use crate::level::{Current, LevelRoot};
use crate::session::{Frame, Session};
use crate::view::PlayerCamera;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};
use zugriff_sim::compile::FaceKind;
use zugriff_sim::game::Event;
use zugriff_sim::world::Contents;

pub struct FaunaPlugin;

impl Plugin for FaunaPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Places>().add_systems(Update, (populate, alarm, birds, rats).chain());
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Species {
    Pigeon,
    Gull,
    Crow,
}

impl Species {
    fn of(map: &str) -> Option<Self> {
        match map {
            m if m.contains("altstadt") => Some(Species::Pigeon),
            m if m.contains("hafen") => Some(Species::Gull),
            m if m.contains("flieger") => Some(Species::Crow),
            _ => None,
        }
    }
    /// Body length (m), walk speed, flight speed.
    fn size(self) -> (f32, f32, f32) {
        match self {
            Species::Pigeon => (0.32, 0.35, 7.0),
            Species::Gull => (0.52, 0.45, 8.0),
            Species::Crow => (0.45, 0.4, 7.5),
        }
    }
}

/// Where creatures may be: open ground for birds (sky above, room around), wall feet for rats.
#[derive(Resource, Default)]
struct Places {
    open: Vec<Vec3>,
    walls: Vec<(Vec3, Vec3)>,
    generation: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// On the ground, walking or pecking.
    Ground,
    /// Flying away from `from` towards `to`, then landing there.
    Flight,
    /// Circling high (gulls): round `to`, radius in `speed`'s place.
    Circle,
}

#[derive(Component)]
struct Bird {
    species: Species,
    mode: Mode,
    vel: Vec3,
    heading: f32,
    /// Seconds into the current step (walk / peck), and how long it lasts.
    t: f32,
    step: f32,
    pecking: bool,
    to: Vec3,
    flap: f32,
    wings: [Entity; 2],
    body: Entity,
    /// A spooked flock member leaves a moment after the first (so the flock doesn't lift as one).
    spooked: f32,
    flock: u32,
}

#[derive(Component)]
struct Rat {
    home: Vec3,
    along: Vec3,
    vel: Vec3,
    t: f32,
    /// Seconds hidden (after bolting), until it turns up elsewhere.
    hidden: f32,
    model: Entity,
}

fn lin(c: Vec3) -> [f32; 4] {
    [c.x.powf(2.2), c.y.powf(2.2), c.z.powf(2.2), 1.0]
}

#[derive(Default)]
struct Mb {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Mb {
    /// An ellipsoid of radii `r` at `c`, coloured `top` above and `under` below.
    fn blob(&mut self, c: Vec3, r: Vec3, top: Vec3, under: Vec3) {
        let (nu, nv) = (10u32, 7u32);
        let start = self.pos.len() as u32;
        for j in 0..=nv {
            let v = PI * j as f32 / nv as f32;
            for i in 0..=nu {
                let u = TAU * i as f32 / nu as f32;
                let n = Vec3::new(v.sin() * u.cos(), v.cos(), v.sin() * u.sin());
                self.pos.push((c + n * r).to_array());
                self.nor.push((n / r).normalize().to_array());
                self.col.push(lin(under.lerp(top, (n.y * 0.5 + 0.5).powf(0.7))));
            }
        }
        for j in 0..nv {
            for i in 0..nu {
                let a = start + j * (nu + 1) + i;
                let b = a + nu + 1;
                self.idx.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
            }
        }
    }

    /// A flat tapered strip from `root` along `dir` (the wing or tail feathers), `w0` wide at the root, `w1` at the tip.
    fn strip(&mut self, root: Vec3, dir: Vec3, side: Vec3, len: f32, w0: f32, w1: f32, c0: Vec3, c1: Vec3) {
        let start = self.pos.len() as u32;
        const N: u32 = 3;
        let n = dir.cross(side).normalize();
        for i in 0..=N {
            let t = i as f32 / N as f32;
            let p = root + dir * len * t;
            let w = w0 + (w1 - w0) * t;
            let c = lin(c0.lerp(c1, t));
            self.pos.push((p - side * w * 0.5).to_array());
            self.pos.push((p + side * w * 0.5).to_array());
            self.nor.extend_from_slice(&[n.to_array(), n.to_array()]);
            self.col.extend_from_slice(&[c, c]);
        }
        for i in 0..N {
            let a = start + i * 2;
            self.idx.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }

    fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nor)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

/// A bird's body (forward +Z, up +Y, feet at y = 0) and one wing (along +X from the shoulder at the origin), in
/// metres for a body `len` long.
fn bird_meshes(sp: Species) -> (Mesh, Mesh) {
    let (len, ..) = sp.size();
    let (body, belly, head, neck, beak, wing, tip, leg) = match sp {
        Species::Pigeon => (
            Vec3::new(0.55, 0.57, 0.62),
            Vec3::new(0.62, 0.63, 0.67),
            Vec3::new(0.38, 0.4, 0.46),
            Vec3::new(0.3, 0.45, 0.4),
            Vec3::new(0.25, 0.22, 0.22),
            Vec3::new(0.6, 0.62, 0.66),
            Vec3::new(0.2, 0.2, 0.22),
            Vec3::new(0.75, 0.35, 0.35),
        ),
        Species::Gull => (
            Vec3::new(0.92, 0.92, 0.9),
            Vec3::new(0.95, 0.95, 0.94),
            Vec3::new(0.95, 0.95, 0.94),
            Vec3::new(0.93, 0.93, 0.92),
            Vec3::new(0.95, 0.75, 0.2),
            Vec3::new(0.6, 0.63, 0.66),
            Vec3::new(0.08, 0.08, 0.08),
            Vec3::new(0.85, 0.7, 0.45),
        ),
        Species::Crow => (
            Vec3::new(0.09, 0.09, 0.1),
            Vec3::new(0.12, 0.12, 0.13),
            Vec3::new(0.08, 0.08, 0.09),
            Vec3::new(0.1, 0.1, 0.12),
            Vec3::new(0.06, 0.06, 0.06),
            Vec3::new(0.1, 0.1, 0.11),
            Vec3::new(0.05, 0.05, 0.06),
            Vec3::new(0.1, 0.1, 0.1),
        ),
    };
    let s = len;
    let mut b = Mb::default();
    let hip = 0.28 * s;
    b.blob(Vec3::new(0.0, hip + 0.1 * s, 0.0), Vec3::new(0.2, 0.2, 0.36) * s, body, belly);
    b.blob(Vec3::new(0.0, hip + 0.24 * s, 0.24 * s), Vec3::new(0.12, 0.12, 0.13) * s, neck, neck);
    b.blob(Vec3::new(0.0, hip + 0.36 * s, 0.33 * s), Vec3::new(0.1, 0.1, 0.11) * s, head, head);
    b.blob(Vec3::new(0.0, hip + 0.34 * s, 0.45 * s), Vec3::new(0.025, 0.025, 0.07) * s, beak, beak);
    for x in [-0.035, 0.035] {
        b.blob(Vec3::new(x * s * 2.4, hip + 0.4 * s, 0.39 * s), Vec3::splat(0.018 * s), Vec3::splat(0.05), Vec3::splat(0.05));
    }
    // Tail: a strip back from the rump; folded wings along the back.
    b.strip(Vec3::new(0.0, hip + 0.12 * s, -0.3 * s), Vec3::new(0.0, -0.15, -1.0).normalize(), Vec3::X, 0.3 * s, 0.14 * s, 0.18 * s, body, tip);
    for x in [-1.0, 1.0] {
        b.strip(Vec3::new(x * 0.16 * s, hip + 0.18 * s, 0.1 * s), Vec3::new(0.0, -0.05, -1.0).normalize(), Vec3::new(0.3, 1.0, 0.0).normalize(), 0.45 * s, 0.14 * s, 0.05 * s, wing, tip);
        // Legs.
        b.strip(Vec3::new(x * 0.06 * s, 0.0, 0.02 * s), Vec3::Y, Vec3::Z, hip, 0.03 * s, 0.025 * s, leg, leg);
        b.strip(Vec3::new(x * 0.06 * s, 0.005, 0.0), Vec3::Z, Vec3::X, 0.1 * s, 0.05 * s, 0.02 * s, leg, leg);
    }
    let mut w = Mb::default();
    let span = match sp {
        Species::Gull => 1.3 * s,
        _ => 1.0 * s,
    };
    w.strip(Vec3::ZERO, Vec3::X, Vec3::Z, span * 0.5, 0.3 * s, 0.14 * s, wing, tip);
    (b.mesh(), w.mesh())
}

fn populate(
    mut commands: Commands,
    current: Res<Current>,
    roots: Query<Entity, With<LevelRoot>>,
    mut places: ResMut<Places>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    server: Res<AssetServer>,
    options: Res<crate::options::Options>,
    living: Query<Entity, Or<(With<Bird>, With<Rat>)>>,
    mut done: Local<(u32, bool)>,
) {
    // (Again when the graphics settings turn the animals on or off.)
    let key = (current.generation, options.gfx.fauna && !crate::args::flag("nofauna"));
    if *done == key || !current.spawned {
        return;
    }
    let (Some(level), Some(root)) = (current.level.as_ref(), roots.iter().next()) else { return };
    *done = key;
    for e in &living {
        commands.entity(e).despawn();
    }
    if !key.1 {
        return;
    }
    let Some(species) = Species::of(&current.name) else { return };
    let seed = current.name.bytes().fold(0xB1ADu64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001B3));
    let mut rng = seed | 1;
    let mut next = move || {
        rng ^= rng >> 12;
        rng ^= rng << 25;
        rng ^= rng >> 27;
        ((rng.wrapping_mul(0x2545F4914F6CDD1D) >> 40) as f32) / (1u64 << 24) as f32
    };
    // Candidate places: points on large walkable ground faces.
    let (mut open, mut walls) = (Vec::new(), Vec::new());
    for f in &level.faces {
        if f.kind != FaceKind::Opaque || f.normal.y < 0.9 {
            continue;
        }
        let name = level.world.surface_name(f.surface);
        if !name.starts_with("mat/") || name.contains("roof") {
            continue;
        }
        for i in 1..f.points.len() - 1 {
            let (a, b, c) = (f.points[0], f.points[i], f.points[i + 1]);
            let area = (b - a).cross(c - a).length() * 0.5;
            let n = (area / 12.0) as usize + usize::from(next() < (area / 12.0).fract());
            for _ in 0..n {
                let (mut u, mut v) = (next(), next());
                if u + v > 1.0 {
                    (u, v) = (1.0 - u, 1.0 - v);
                }
                let p = a + (b - a) * u + (c - a) * v;
                // Open sky above, nothing standing here.
                if level.world.trace(p + Vec3::Y * 0.3, p + Vec3::Y * 30.0, Vec3::ZERO, Contents::MASK_SHOT).fraction < 1.0 {
                    continue;
                }
                if level.world.trace(p + Vec3::Y * 2.0, p + Vec3::Y * 0.02, Vec3::ZERO, Contents::MASK_PLAYER).fraction < 1.0 {
                    continue;
                }
                let from = p + Vec3::Y * 0.1;
                let mut wall = None;
                let mut clear = true;
                for d in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                    let t = level.world.trace(from, from + d * 2.5, Vec3::ZERO, Contents::MASK_PLAYER);
                    if t.fraction < 1.0 {
                        clear = false;
                        if t.fraction * 2.5 < 0.35 {
                            wall = Some(Vec3::new(d.z, 0.0, -d.x));
                        }
                    }
                }
                if clear {
                    open.push(p);
                }
                if let Some(along) = wall {
                    walls.push((p, along));
                }
            }
        }
    }
    places.open = open;
    places.walls = walls;
    places.generation = current.generation;
    info!("fauna: {} open places, {} wall feet", places.open.len(), places.walls.len());
    if places.open.is_empty() {
        return;
    }
    // Birds.
    let (body_mesh, wing_mesh) = bird_meshes(species);
    let (body_mesh, wing_mesh) = (meshes.add(body_mesh), meshes.add(wing_mesh));
    let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.8, cull_mode: None, double_sided: true, ..default() });
    let flocks = match species {
        Species::Pigeon => 4,
        _ => 3,
    };
    let mut flock_id = 0;
    for _ in 0..flocks {
        let centre = places.open[(next() * places.open.len() as f32) as usize % places.open.len()];
        let n = 4 + (next() * 6.0) as usize;
        flock_id += 1;
        info!("fauna: flock {flock_id} of {n} at {centre}");
        for _ in 0..n {
            let p = centre + Vec3::new(next() * 3.0 - 1.5, 0.0, next() * 3.0 - 1.5);
            spawn_bird(&mut commands, root, species, &body_mesh, &wing_mesh, &material, p, Mode::Ground, next() * TAU, flock_id);
        }
    }
    // Gulls circling high over the harbour.
    if species == Species::Gull {
        let b = level.world.bounds();
        for i in 0..6 {
            let c = b.centre() + Vec3::new(next() * 40.0 - 20.0, 0.0, next() * 40.0 - 20.0);
            let p = c.with_y(b.min.y + 18.0 + next() * 10.0) + Vec3::new(12.0, 0.0, 0.0);
            let e = spawn_bird(&mut commands, root, species, &body_mesh, &wing_mesh, &material, p, Mode::Circle, 0.0, 100 + i);
            commands.entity(e).insert(CircleOf(c.with_y(p.y), 10.0 + next() * 12.0, if next() < 0.5 { 1.0 } else { -1.0 }));
        }
    }
    // Rats (town and harbour).
    if species != Species::Crow && !places.walls.is_empty() {
        let scene = server.load(GltfAssetLabel::Scene(0).from_asset("models/props/street_rat.glb"));
        for _ in 0..5 {
            let (p, along) = places.walls[(next() * places.walls.len() as f32) as usize % places.walls.len()];
            let model = commands.spawn((WorldAssetRoot(scene.clone()), Transform::from_scale(Vec3::splat(1.6)), Visibility::Inherited)).id();
            commands.spawn((
                Transform::from_translation(p),
                Visibility::Inherited,
                Rat { home: p, along, vel: Vec3::ZERO, t: next() * 3.0, hidden: 0.0, model },
                Name::new("rat"),
                ChildOf(root),
            ))
            .add_child(model);
        }
    }
}

/// A circling gull's centre, radius and turning sense.
#[derive(Component)]
struct CircleOf(Vec3, f32, f32);

#[allow(clippy::too_many_arguments)]
fn spawn_bird(commands: &mut Commands, root: Entity, species: Species, body: &Handle<Mesh>, wing: &Handle<Mesh>, material: &Handle<StandardMaterial>, p: Vec3, mode: Mode, heading: f32, flock: u32) -> Entity {
    let (len, ..) = species.size();
    let body_e = commands.spawn((Mesh3d(body.clone()), MeshMaterial3d(material.clone()), Transform::default())).id();
    let shoulder = Vec3::new(0.0, 0.42 * len, 0.1 * len);
    let wl = commands.spawn((Mesh3d(wing.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(shoulder - Vec3::X * 0.12 * len).with_scale(Vec3::new(-1.0, 1.0, 1.0)))).id();
    let wr = commands.spawn((Mesh3d(wing.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(shoulder + Vec3::X * 0.12 * len))).id();
    let bird = Bird { species, mode, vel: Vec3::ZERO, heading, t: 0.0, step: 1.0, pecking: false, to: p, flap: 0.0, wings: [wl, wr], body: body_e, spooked: -1.0, flock };
    let e = commands
        .spawn((Transform::from_translation(p).with_rotation(Quat::from_rotation_y(heading)), Visibility::Inherited, bird, bevy::light::NotShadowCaster, Name::new("bird"), ChildOf(root)))
        .id();
    commands.entity(e).add_children(&[body_e, wl, wr]);
    e
}

/// Where danger is this frame: players' positions (and the camera), and the places of shots and blasts.
#[derive(Default)]
struct Danger {
    near: Vec<Vec3>,
    loud: Vec<(Vec3, f32)>,
}

fn danger(session: &Option<Res<Session>>, frame: &Frame, cam: Vec3) -> Danger {
    let mut d = Danger::default();
    d.near.push(cam);
    if let Some(s) = session {
        for p in &s.game.players {
            if p.alive {
                d.near.push(s.snap(p.id).origin);
            }
        }
    }
    for e in &frame.events {
        match e {
            Event::Shot { from, end, .. } => {
                d.loud.push((*from, 30.0));
                d.loud.push((*end, 12.0));
            }
            Event::Detonate { pos, .. } | Event::Exploded { pos } => d.loud.push((*pos, 45.0)),
            _ => {}
        }
    }
    d
}

/// Flocks that must leave now (a bird is threatened: its whole flock goes, one by one).
fn alarm(session: Option<Res<Session>>, frame: Res<Frame>, cam: Single<&GlobalTransform, With<PlayerCamera>>, mut birds: Query<(&Transform, &mut Bird)>) {
    let d = danger(&session, &frame, cam.translation());
    let mut flocks = Vec::new();
    for (tf, b) in &birds {
        if b.mode != Mode::Ground || b.spooked >= 0.0 {
            continue;
        }
        let p = tf.translation;
        let close = d.near.iter().any(|q| q.with_y(p.y).distance(p) < 4.0 && (q.y - p.y).abs() < 3.0);
        let loud = d.loud.iter().any(|(q, r)| q.distance(p) < *r);
        if close || loud {
            flocks.push(b.flock);
        }
    }
    if flocks.is_empty() {
        return;
    }
    let mut k = 0.0;
    for (_, mut b) in &mut birds {
        if flocks.contains(&b.flock) && b.mode == Mode::Ground && b.spooked < 0.0 {
            b.spooked = k;
            k += 0.06 + (b.heading.sin() * 0.5 + 0.5) * 0.12;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn birds(
    time: Res<Time>,
    current: Res<Current>,
    places: Res<Places>,
    session: Option<Res<Session>>,
    frame: Res<Frame>,
    cam: Single<&GlobalTransform, With<PlayerCamera>>,
    mut q: Query<(&mut Transform, &mut Bird, Option<&CircleOf>)>,
    mut parts: Query<&mut Transform, Without<Bird>>,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    let Some(level) = current.level.as_ref() else { return };
    let d = danger(&session, &frame, cam.translation());
    for (mut tf, mut b, circle) in &mut q {
        let (len, walk, fly) = b.species.size();
        let h = (tf.translation.x * 12.9898 + tf.translation.z * 78.233).sin().abs();
        match b.mode {
            Mode::Ground => {
                if b.spooked >= 0.0 {
                    b.spooked -= dt;
                    if b.spooked < 0.0 {
                        // Off: away from the nearest danger, up steeply, towards a far open place.
                        let p = tf.translation;
                        let from = d.near.iter().chain(d.loud.iter().map(|(q, _)| q)).min_by(|a, c| a.distance(p).total_cmp(&c.distance(p))).copied().unwrap_or(p - Vec3::Z);
                        let away = (p - from).with_y(0.0).normalize_or(Vec3::X);
                        let target = places
                            .open
                            .iter()
                            .filter(|o| o.distance(p) > 20.0 && (**o - p).normalize_or_zero().dot(away) > 0.0)
                            .min_by(|a, c| (a.distance(p) - 35.0).abs().total_cmp(&(c.distance(p) - 35.0).abs()))
                            .copied()
                            .unwrap_or(p + away * 40.0);
                        b.to = target;
                        b.vel = away * fly * 0.5 + Vec3::Y * fly * 0.6;
                        b.mode = Mode::Flight;
                        b.spooked = -1.0;
                        continue;
                    }
                }
                b.t += dt;
                if b.t > b.step {
                    b.t = 0.0;
                    b.pecking = h > 0.45 && !b.pecking;
                    b.step = if b.pecking { 0.4 + h * 0.8 } else { 0.8 + h * 2.0 };
                    if !b.pecking {
                        b.heading += (h - 0.5) * 2.5;
                    }
                }
                let body_pitch = if b.pecking { ((b.t * 9.0).sin().abs()) * 0.55 } else { 0.0 };
                if !b.pecking {
                    let dir = Vec3::new(b.heading.sin(), 0.0, b.heading.cos());
                    let next = tf.translation + dir * walk * dt;
                    // Stay on walkable, open ground (turn back at walls and edges).
                    let down = level.world.trace(next + Vec3::Y * 0.5, next - Vec3::Y * 0.6, Vec3::ZERO, Contents::MASK_PLAYER);
                    let blocked = level.world.trace(tf.translation + Vec3::Y * 0.15, next + dir * 0.3 + Vec3::Y * 0.15, Vec3::ZERO, Contents::MASK_PLAYER).fraction < 1.0;
                    if down.fraction < 1.0 && !blocked && (down.end.y - tf.translation.y).abs() < 0.2 {
                        tf.translation = down.end;
                    } else {
                        b.heading += PI * 0.7;
                    }
                }
                // Walking bob: the body nods with each step.
                let bob = if b.pecking { 0.0 } else { (now * 11.0 + h * 10.0).sin() * 0.06 };
                tf.rotation = Quat::from_rotation_y(b.heading);
                if let Ok(mut body) = parts.get_mut(b.body) {
                    body.rotation = Quat::from_rotation_x(body_pitch + bob);
                }
                for (i, w) in b.wings.iter().enumerate() {
                    if let Ok(mut wt) = parts.get_mut(*w) {
                        // Folded: the wing strip swept back along the body (hidden in it).
                        let s = if i == 0 { -1.0 } else { 1.0 };
                        wt.rotation = Quat::from_rotation_y(-s * 1.45) * Quat::from_rotation_z(s * 0.25);
                        wt.scale = Vec3::new(if i == 0 { -0.5 } else { 0.5 }, 1.0, 0.6);
                    }
                }
            }
            Mode::Flight => {
                let p = tf.translation;
                let to_target = b.to - p;
                let dist = to_target.with_y(0.0).length();
                // Cruise at a height over the roofs, descend to land when near.
                let cruise = b.to.y + if dist > 12.0 { 9.0 + len * 10.0 } else { dist * 0.6 };
                let want = (to_target.with_y(0.0).normalize_or(Vec3::X) * fly).with_y((cruise - p.y).clamp(-2.5, 3.5));
                b.vel = b.vel.lerp(want, (dt * 1.8).min(1.0));
                tf.translation += b.vel * dt;
                let flat = b.vel.with_y(0.0);
                if flat.length() > 0.1 {
                    b.heading = flat.x.atan2(flat.z);
                }
                let climb = (b.vel.y / fly).clamp(-0.6, 0.6);
                tf.rotation = Quat::from_rotation_y(b.heading) * Quat::from_rotation_x(-climb * 0.6);
                // Flap hard climbing, glide when level or descending.
                let rate = if b.vel.y > 0.5 { 13.0 } else if dist < 8.0 { 9.0 } else { 6.0 };
                b.flap += dt * rate;
                let glide = b.vel.y < -0.3 && dist > 4.0;
                let amp = if glide { 0.08 } else { 0.9 };
                let angle = (b.flap).sin() * amp + 0.15;
                set_wings(&mut parts, &b, angle);
                if let Ok(mut body) = parts.get_mut(b.body) {
                    body.rotation = Quat::IDENTITY;
                }
                // Landed.
                if dist < 0.4 && (p.y - b.to.y).abs() < 0.3 {
                    tf.translation = b.to;
                    b.mode = Mode::Ground;
                    b.vel = Vec3::ZERO;
                    b.t = 0.0;
                }
            }
            Mode::Circle => {
                let Some(CircleOf(c, r, sense)) = circle else { continue };
                let a = now * sense * fly / r * 0.6 + h * TAU;
                let p = *c + Vec3::new(a.cos() * r, (now * 0.3 + h * 5.0).sin() * 2.0, a.sin() * r);
                let vel = (p - tf.translation) / dt.max(1e-3);
                tf.translation = p;
                if vel.with_y(0.0).length() > 0.1 {
                    b.heading = vel.x.atan2(vel.z);
                }
                // Banked into the turn, mostly gliding, a few beats now and then.
                tf.rotation = Quat::from_rotation_y(b.heading) * Quat::from_rotation_z(-sense * 0.35);
                b.flap += dt * 7.0;
                let beating = (now * 0.4 + h * 9.0).sin() > 0.75;
                set_wings(&mut parts, &b, if beating { b.flap.sin() * 0.6 } else { 0.05 });
            }
        }
    }
}

fn set_wings(parts: &mut Query<&mut Transform, Without<Bird>>, b: &Bird, angle: f32) {
    for (i, w) in b.wings.iter().enumerate() {
        if let Ok(mut wt) = parts.get_mut(*w) {
            let s = if i == 0 { -1.0 } else { 1.0 };
            wt.rotation = Quat::from_rotation_z(s * angle);
            wt.scale = Vec3::new(s, 1.0, 1.0);
        }
    }
}

fn rats(
    time: Res<Time>,
    current: Res<Current>,
    places: Res<Places>,
    session: Option<Res<Session>>,
    frame: Res<Frame>,
    cam: Single<&GlobalTransform, With<PlayerCamera>>,
    mut q: Query<(&mut Transform, &mut Rat, &mut Visibility)>,
) {
    let dt = time.delta_secs().min(0.05);
    let Some(level) = current.level.as_ref() else { return };
    if places.walls.is_empty() {
        return;
    }
    let d = danger(&session, &frame, cam.translation());
    for (mut tf, mut r, mut vis) in &mut q {
        let _ = r.model;
        if r.hidden > 0.0 {
            r.hidden -= dt;
            if r.hidden <= 0.0 {
                // Turn up at a wall foot no one is near.
                let i = ((time.elapsed_secs() * 7919.0) as usize) % places.walls.len();
                let (p, along) = places.walls[i];
                if d.near.iter().all(|q| q.distance(p) > 15.0) {
                    r.home = p;
                    r.along = along;
                    tf.translation = p;
                    *vis = Visibility::Inherited;
                } else {
                    r.hidden = 1.0;
                }
            }
            continue;
        }
        let p = tf.translation;
        let threat = d.near.iter().chain(d.loud.iter().filter(|(q, rr)| q.distance(p) < *rr * 0.6).map(|(q, _)| q)).find(|q| q.distance(p) < 5.0).copied();
        r.t -= dt;
        if let Some(t) = threat {
            // Bolt along the wall, away.
            let away = if (p - t).dot(r.along) >= 0.0 { r.along } else { -r.along };
            r.vel = away * 4.5;
            r.t = 1.2;
        } else if r.t <= 0.0 {
            // Sniff about, then a dash along the wall.
            let h = (time.elapsed_secs() * 3.7 + p.x).sin();
            if h > 0.2 {
                r.vel = r.along * h.signum() * 2.2 * if h > 0.6 { 1.0 } else { -1.0 };
                r.t = 0.4 + h * 0.6;
            } else {
                r.vel = Vec3::ZERO;
                r.t = 1.0 + h.abs() * 2.5;
            }
        }
        if r.vel.length_squared() > 0.01 {
            let next = p + r.vel * dt;
            let down = level.world.trace(next + Vec3::Y * 0.4, next - Vec3::Y * 0.6, Vec3::ZERO, Contents::MASK_PLAYER);
            let blocked = level.world.trace(p + Vec3::Y * 0.05, next + r.vel.normalize() * 0.15 + Vec3::Y * 0.05, Vec3::ZERO, Contents::MASK_PLAYER).fraction < 1.0;
            if down.fraction < 1.0 && !blocked && (down.end.y - p.y).abs() < 0.25 {
                tf.translation = down.end;
                tf.rotation = Quat::from_rotation_y(r.vel.x.atan2(r.vel.z));
            } else if threat.is_some() {
                // Cornered: gone (down a drain, behind a pipe).
                *vis = Visibility::Hidden;
                r.hidden = 20.0 + (p.x * 3.1).sin().abs() * 25.0;
                r.vel = Vec3::ZERO;
            } else {
                r.vel = -r.vel;
            }
            // Far from home after a bolt: gone, turns up elsewhere later.
            if threat.is_none() && tf.translation.distance(r.home) > 8.0 {
                *vis = Visibility::Hidden;
                r.hidden = 15.0;
            }
        }
    }
}
