//! Shader warm-up. The browser compiles a render pipeline the first time something new is drawn (a weapon, a smoke
//! cloud, a bullet hole), and on the web that blocks the frame — a hitch of a tenth of a second or more in the middle
//! of a fight. So when a match starts, one of everything is shown for a moment right in front of the camera, behind
//! the loading screen, and all those pipelines are compiled before play begins.

use crate::session::Frame;
use crate::view::PlayerCamera;
use bevy::prelude::*;

pub struct WarmupPlugin;

impl Plugin for WarmupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Warmup>().add_systems(Update, (start, finish).chain().after(crate::session::SessionSet));
    }
}

/// While warming up (seconds left); the loading screen stays up meanwhile.
#[derive(Resource, Default)]
pub struct Warmup {
    pub left: f32,
}

impl Warmup {
    pub fn active(&self) -> bool {
        self.left > 0.0
    }
}

#[derive(Component)]
struct WarmupItem;

/// The weapon and prop models drawn in the world (bodies, the floor).
const PROPS: [&str; 9] = ["models/m4.glb", "models/ak.glb", "models/sniper.glb", "models/shotgun.glb", "models/smg.glb", "models/mg.glb", "models/pistol.glb", "models/knife.glb", "models/grenade.glb"];

#[allow(clippy::too_many_arguments)]
fn start(
    mut commands: Commands,
    frame: Res<Frame>,
    mut warmup: ResMut<Warmup>,
    server: Res<AssetServer>,
    fx: Option<Res<crate::effects::Fx>>,
    cam: Single<Entity, With<PlayerCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !frame.started {
        return;
    }
    warmup.left = 1.4;
    let root = commands.spawn((WarmupItem, Transform::from_xyz(0.0, 0.0, -2.5), Visibility::Inherited)).id();
    commands.entity(*cam).add_child(root);
    let mut at = 0;
    let mut place = |commands: &mut Commands, bundle: (Transform,)| -> Entity {
        let i = at;
        at += 1;
        let x = (i % 6) as f32 * 0.5 - 1.25;
        let y = (i / 6) as f32 * 0.4 - 0.6;
        let mut t = bundle.0;
        t.translation += Vec3::new(x, y, 0.0);
        let e = commands.spawn((t, Visibility::Inherited, ChildOf(root))).id();
        e
    };
    for file in PROPS {
        let e = place(&mut commands, (Transform::default(),));
        commands.entity(e).insert(WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(file))));
    }
    // Every material the effects use, on the meshes they use it with.
    if let Some(fx) = fx {
        for (mesh, material, scale) in fx.warm_set() {
            let e = place(&mut commands, (Transform::from_scale(Vec3::splat(scale)),));
            commands.entity(e).insert((Mesh3d(mesh), MeshMaterial3d(material)));
        }
    }
    // A blended, lit, textured quad like the smoke's and the dust's (their materials are made per puff).
    let puff = materials.add(StandardMaterial { base_color: Color::srgba(0.7, 0.7, 0.7, 0.5), alpha_mode: AlphaMode::Blend, double_sided: true, cull_mode: None, ..default() });
    let e = place(&mut commands, (Transform::from_scale(Vec3::splat(0.3)),));
    commands.entity(e).insert((Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))), MeshMaterial3d(puff)));
    // A point light (muzzle flashes, explosions).
    let e = place(&mut commands, (Transform::default(),));
    commands.entity(e).insert(PointLight { intensity: 1000.0, range: 3.0, shadow_maps_enabled: false, ..default() });
}

fn finish(mut commands: Commands, time: Res<Time>, mut warmup: ResMut<Warmup>, items: Query<Entity, With<WarmupItem>>) {
    if warmup.left <= 0.0 {
        return;
    }
    warmup.left -= time.delta_secs();
    if warmup.left <= 0.0 {
        for e in &items {
            commands.entity(e).despawn();
        }
    }
}
