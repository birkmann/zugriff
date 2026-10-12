//! The models' animation clips. The skinned models (`models/vm-*.glb` in first person, `models/soldier-*.glb` for
//! everyone else, built by Nexus Sim's `tools/infantry/build.py`) carry their clips by name. Each model is loaded as a glTF once,
//! the first time it is wanted, and gets one animation graph with a node per clip. When one of its scenes spawns, the
//! graph goes onto the scene's animation player, which `viewmodel.rs` and `body.rs` then drive by clip name.

use std::collections::HashMap;
use std::time::Duration;

use bevy::animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use bevy::animation::transition::AnimationTransitions;
use bevy::gltf::Gltf;
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Model {
    /// First person: arms and rifle, arms and pistol, empty hands (for the knife, grenades and the bomb); and arms
    /// with each other class of long gun (each falls back to the rifle while its file is missing).
    Rifle,
    Pistol,
    Hands,
    Ak,
    Sniper,
    Shotgun,
    Smg,
    Mg,
    /// Arms with the knife, arms for the grenades (each falls back to the empty hands while its file is missing).
    Knife,
    Grenade,
    /// KSK operators and OPFOR fighters; hostages (without their own model yet, the OPFOR model repainted).
    SoldierBlue,
    SoldierRed,
    Hostage,
    /// A gun's own first-person model (see `gear.rs`), loaded when first taken out; its class's shared one stands in
    /// until it is in.
    Gun(zugriff_sim::weapons::WeaponId),
}

impl Model {
    pub fn path(self) -> &'static str {
        match self {
            Model::Rifle => "models/vm-rifle.glb",
            Model::Pistol => "models/vm-pistol.glb",
            Model::Hands => "models/vm-hands.glb",
            Model::Ak => "models/vm-ak.glb",
            Model::Sniper => "models/vm-sniper.glb",
            Model::Shotgun => "models/vm-shotgun.glb",
            Model::Smg => "models/vm-smg.glb",
            Model::Mg => "models/vm-mg.glb",
            Model::Knife => "models/vm-knife.glb",
            Model::Grenade => "models/vm-grenade.glb",
            Model::SoldierBlue => "models/soldier-blue.glb",
            Model::SoldierRed => "models/soldier-red.glb",
            Model::Hostage => "models/hostage.glb",
            Model::Gun(w) => crate::gear::own(w).map_or("", |o| o.vm),
        }
    }

    /// What to use while this model's file is missing.
    pub fn fallback(self) -> Model {
        match self {
            Model::Ak | Model::Sniper | Model::Shotgun | Model::Smg | Model::Mg => Model::Rifle,
            Model::Knife | Model::Grenade => Model::Hands,
            Model::Hostage => Model::SoldierRed,
            Model::Gun(w) => crate::viewmodel::family_of(w),
            m => m,
        }
    }
    pub fn scene(self, server: &AssetServer) -> Handle<WorldAsset> {
        server.load(GltfAssetLabel::Scene(0).from_asset(self.path()))
    }
}

/// A model's clips: the graph, and each clip's node and length (s).
pub struct Clips {
    pub graph: Handle<AnimationGraph>,
    pub nodes: HashMap<String, (AnimationNodeIndex, f32)>,
}

#[derive(Resource, Default)]
pub struct Library {
    gltf: HashMap<Model, Handle<Gltf>>,
    ready: HashMap<Model, Clips>,
}

impl Library {
    /// Starts loading a model's clips (once).
    pub fn want(&mut self, server: &AssetServer, model: Model) {
        self.gltf.entry(model).or_insert_with(|| server.load(model.path()));
    }
    pub fn clips(&self, model: Model) -> Option<&Clips> {
        self.ready.get(&model)
    }

    /// Whether a model's file failed to load (it doesn't exist yet): use its fallback.
    pub fn failed(&self, server: &AssetServer, model: Model) -> bool {
        self.gltf.get(&model).is_some_and(|h| matches!(server.get_load_state(h), Some(bevy::asset::LoadState::Failed(_))))
    }

    /// Adds a node built later (a masked copy of a clip, once the model's scene is in) to a model's clips by name.
    pub fn add_node(&mut self, model: Model, name: &str, node: AnimationNodeIndex, length: f32) {
        if let Some(c) = self.ready.get_mut(&model) {
            c.nodes.insert(name.to_string(), (node, length));
        }
    }

    /// The model to show for `model`: itself once loaded, its fallback if its file is missing (or, for a gun's own
    /// model, while it is still loading).
    pub fn resolve(&self, server: &AssetServer, model: Model) -> Model {
        let waiting = matches!(model, Model::Gun(_)) && !self.ready.contains_key(&model);
        if waiting || self.failed(server, model) { model.fallback() } else { model }
    }
}

/// The root of a spawned model scene: which model it is.
#[derive(Component)]
pub struct Animated(pub Model);

/// An animation player found in a model's scene: its model, and the scene's root (`Animated`).
#[derive(Component)]
pub struct Rig {
    pub model: Model,
    pub root: Entity,
    /// The clip playing as the main one, by name.
    pub playing: Option<String>,
}

/// Each model's graph, once its glTF (and so its clips) has loaded.
pub fn build_graphs(mut lib: ResMut<Library>, server: Res<AssetServer>, gltfs: Res<Assets<Gltf>>, clips: Res<Assets<AnimationClip>>, mut graphs: ResMut<Assets<AnimationGraph>>) {
    let pending: Vec<(Model, Handle<Gltf>)> = lib.gltf.iter().filter(|(m, _)| !lib.ready.contains_key(m)).map(|(m, h)| (*m, h.clone())).collect();
    for (model, handle) in pending {
        if !server.is_loaded_with_dependencies(&handle) {
            continue;
        }
        let Some(gltf) = gltfs.get(&handle) else { continue };
        let mut graph = AnimationGraph::new();
        let root = graph.root;
        let mut nodes = HashMap::new();
        for (name, clip) in &gltf.named_animations {
            let length = clips.get(clip).map_or(1.0, |c| c.duration());
            nodes.insert(name.to_string(), (graph.add_clip(clip.clone(), 1.0, root), length));
        }
        lib.ready.insert(model, Clips { graph: graphs.add(graph), nodes });
    }
}

/// Animation players as model scenes spawn: their model's graph goes on them (as soon as it is built).
pub fn hook_players(
    mut commands: Commands,
    lib: Res<Library>,
    new: Query<Entity, Added<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    roots: Query<&Animated>,
    mut waiting: Local<Vec<Entity>>,
) {
    waiting.extend(new.iter());
    waiting.retain(|&e| {
        let mut at = e;
        for _ in 0..32 {
            if let Ok(Animated(model)) = roots.get(at) {
                let Some(clips) = lib.clips(*model) else { return true };
                if let Ok(mut ec) = commands.get_entity(e) {
                    ec.insert((AnimationGraphHandle(clips.graph.clone()), AnimationTransitions::new(), Rig { model: *model, root: at, playing: None }));
                }
                return false;
            }
            let Ok(p) = parents.get(at) else { return false };
            at = p.parent();
        }
        false
    });
}

/// Plays clip `name` on a rig as the main animation, fading from the one before over `fade` seconds; `repeat`ing or
/// once. Returns false if the model has no such clip.
pub fn play(lib: &Library, rig: &mut Rig, player: &mut AnimationPlayer, transitions: &mut AnimationTransitions, name: &str, fade: f32, repeat: bool) -> bool {
    let Some(&(node, _)) = lib.clips(rig.model).and_then(|c| c.nodes.get(name)) else { return false };
    let active = transitions.play(player, node, Duration::from_secs_f32(fade.max(0.0)));
    if repeat {
        active.repeat();
    }
    rig.playing = Some(name.to_owned());
    true
}

/// A clip's length (s), if the model has it.
pub fn length(lib: &Library, model: Model, name: &str) -> Option<f32> {
    lib.clips(model).and_then(|c| c.nodes.get(name)).map(|n| n.1)
}

/// How far the main clip is through (0..1), and whether it has finished.
pub fn progress(lib: &Library, rig: &Rig, player: &AnimationPlayer) -> (f32, bool) {
    let Some(&(node, len)) = rig.playing.as_deref().and_then(|n| lib.clips(rig.model)?.nodes.get(n)) else { return (1.0, true) };
    player.animation(node).map_or((1.0, true), |a| ((a.seek_time() / len.max(1e-3)).clamp(0.0, 1.0), a.is_finished()))
}

/// Sets the main clip's playback speed.
pub fn speed(lib: &Library, rig: &Rig, player: &mut AnimationPlayer, speed: f32) {
    if let Some(&(node, _)) = rig.playing.as_deref().and_then(|n| lib.clips(rig.model)?.nodes.get(n))
        && let Some(a) = player.animation_mut(node)
    {
        a.set_speed(speed);
    }
}
