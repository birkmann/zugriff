//! Sound through kira: one listener on the camera, a pool of spatial emitters for sounds in the world (attenuation,
//! panning), and the recorded samples (`assets/sounds`, built by `tools/sounds/build.py`; a few variants of each,
//! picked at random). The synthesised bank (`sfx.rs`) stands in for any sample not loaded (yet), and still makes the
//! musical stings and the flashbang's ringing. The sim's events choose what plays: shots (a far version past 25 m),
//! footsteps by surface, reloads timed to the weapon, impacts, grenades, the bomb, the interface.
//!
//! Browsers only start audio after a user gesture, so the mixer is created on the first click or key press.

use crate::sfx::{self, Gun, Surface};
use crate::view::PlayerCamera;
use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::prelude::*;
use kira::effect::compressor::CompressorBuilder;
use kira::listener::ListenerHandle;
use kira::sound::static_sound::StaticSoundData;
use kira::track::{MainTrackBuilder, SpatialTrackBuilder, SpatialTrackDistances, SpatialTrackHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, PlaybackRate, StartTime, Tween};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use zugriff_sim::game::{Event, Team};
use zugriff_sim::material::Material;
use zugriff_sim::weapons::{Class, WeaponId};

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<Clip>()
            .register_asset_loader(ClipLoader)
            .add_message::<Cue>()
            .add_systems(Startup, load_samples)
            .add_systems(Update, (start_on_gesture, absorb_samples, follow_camera, play_events, play_cues, volume).chain().after(crate::session::SessionSet));
    }
}

/// A recorded sound, decoded on the asset loader's thread.
#[derive(Asset, TypePath)]
pub struct Clip(StaticSoundData);

#[derive(Default, TypePath)]
struct ClipLoader;

impl AssetLoader for ClipLoader {
    type Asset = Clip;
    type Settings = ();
    type Error = crate::level::LoadError;

    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<Clip, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|e| crate::level::LoadError(e.to_string()))?;
        StaticSoundData::from_cursor(std::io::Cursor::new(bytes)).map(Clip).map_err(|e| crate::level::LoadError(e.to_string()))
    }

    fn extensions(&self) -> &[&str] {
        &["ogg"]
    }
}

/// The samples still loading: each sound's name (`shot_rifle`, `step_metal`, ...) and one variant's file.
#[derive(Resource, Default)]
struct Samples(Vec<(String, Handle<Clip>)>);

/// Every variant of every sound in the build's index (`sounds/<name>_<n>.ogg`).
fn load_samples(mut commands: Commands, server: Res<AssetServer>) {
    let index: BTreeMap<String, usize> = serde_json::from_str(include_str!("../../assets/sounds/index.json")).unwrap_or_default();
    let files = index.iter().flat_map(|(name, &n)| (0..n).map(move |i| (name.clone(), i)));
    commands.insert_resource(Samples(files.map(|(name, i)| (name.clone(), server.load(format!("sounds/{name}_{i}.ogg")))).collect()));
}

/// Hands the loaded samples to the mixer (once it exists: on the web, after the first click).
fn absorb_samples(mixer: Option<NonSendMut<Mixer>>, samples: Option<ResMut<Samples>>, clips: Res<Assets<Clip>>) {
    let (Some(mut m), Some(mut samples)) = (mixer, samples) else { return };
    if samples.0.is_empty() {
        return;
    }
    samples.0.retain(|(name, handle)| match clips.get(handle) {
        Some(clip) => {
            m.samples.entry(name.clone()).or_default().push(clip.0.clone());
            false
        }
        None => true,
    });
    if samples.0.is_empty() {
        info!("audio: {} sounds recorded ({} variants)", m.samples.len(), m.samples.values().map(Vec::len).sum::<usize>());
    }
}

/// The sample set for a sound the synthesised bank also has.
fn sample_name(k: Key) -> Option<String> {
    let surface = |s: Surface| format!("{s:?}").to_lowercase();
    Some(match k {
        Key::Shot(g, far) => format!("shot_{}{}", format!("{g:?}").to_lowercase(), if far { "_far" } else { "" }),
        Key::Cycle(bolt) => (if bolt { "cycle_bolt" } else { "cycle_pump" }).into(),
        Key::Shell => "shell_in".into(),
        Key::Dry => "dry".into(),
        Key::Draw => "draw".into(),
        Key::Zoom => "zoom".into(),
        Key::Step(s) => format!("step_{}", surface(s)),
        Key::Land => "land".into(),
        Key::Impact(s) => format!("impact_{}", surface(s)),
        Key::Helmet => "helmet".into(),
        Key::Hurt => "hurt".into(),
        Key::Whoosh(stab) => (if stab { "whoosh_stab" } else { "whoosh" }).into(),
        Key::Bounce => "bounce".into(),
        Key::Brass => "brass".into(),
        Key::Boom(big, far) => format!("boom{}{}", if big { "_big" } else { "" }, if far { "_far" } else { "" }),
        Key::Flashbang => "flashbang".into(),
        Key::Hiss => "hiss".into(),
        Key::Beep => "beep".into(),
        Key::Keypad => "keypad".into(),
        Key::Defuse => "defuse".into(),
        Key::Click => "click".into(),
        Key::Hover => "hover".into(),
        Key::Buy => "buy".into(),
        Key::Pickup => "pickup".into(),
        Key::Radio => "radio".into(),
        Key::Sting(_) => return None,
    })
}

/// A sound asked for by the client itself (not a sim event).
#[derive(Message, Clone, Copy)]
pub enum Cue {
    Brass(Vec3),
    UiClick,
    UiHover,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Key {
    Shot(Gun, bool),
    Cycle(bool),
    Shell,
    Dry,
    Draw,
    Zoom,
    Step(Surface),
    Land,
    Impact(Surface),
    Helmet,
    Hurt,
    Whoosh(bool),
    Bounce,
    Brass,
    Boom(bool, bool),
    Flashbang,
    Hiss,
    Beep,
    Keypad,
    Defuse,
    Click,
    Hover,
    Buy,
    Pickup,
    Radio,
    Sting(u8),
}

/// kira's manager and handles hold a `SendWrapper` on the web, so the whole mixer lives in a non-send resource.
pub struct Mixer {
    manager: AudioManager<DefaultBackend>,
    listener: ListenerHandle,
    emitters: Vec<SpatialTrackHandle>,
    next: usize,
    bank: HashMap<Key, Vec<StaticSoundData>>,
    reloads: HashMap<(u32, bool), StaticSoundData>,
    samples: HashMap<String, Vec<StaticSoundData>>,
    tinnitus: Option<kira::sound::static_sound::StaticSoundHandle>,
    listener_pos: Vec3,
    volume: f32,
    seed: u32,
}

fn v(p: Vec3) -> mint::Vector3<f32> {
    mint::Vector3 { x: p.x, y: p.y, z: p.z }
}

fn q(r: Quat) -> mint::Quaternion<f32> {
    mint::Quaternion { v: mint::Vector3 { x: r.x, y: r.y, z: r.z }, s: r.w }
}

fn now() -> Tween {
    Tween { duration: std::time::Duration::ZERO, ..default() }
}

fn bank() -> HashMap<Key, Vec<StaticSoundData>> {
    let mut b: HashMap<Key, Vec<StaticSoundData>> = HashMap::new();
    let mut put = |k: Key, f: &dyn Fn(u32) -> StaticSoundData, n: u32| {
        b.insert(k, (0..n).map(|i| f(i * 7919 + 13)).collect());
    };
    for g in [Gun::Pistol, Gun::Heavy, Gun::Smg, Gun::Rifle, Gun::Sniper, Gun::Shotgun, Gun::Mg] {
        put(Key::Shot(g, false), &|s| sfx::gunshot(g, false, s), 3);
        put(Key::Shot(g, true), &|s| sfx::gunshot(g, true, s), 2);
    }
    put(Key::Cycle(true), &|s| sfx::cycle(true, s), 1);
    put(Key::Cycle(false), &|s| sfx::cycle(false, s), 1);
    put(Key::Shell, &sfx::shell_in, 2);
    put(Key::Dry, &sfx::dry_fire, 1);
    put(Key::Draw, &sfx::draw, 2);
    put(Key::Zoom, &sfx::zoom, 1);
    for s in [Surface::Concrete, Surface::Metal, Surface::Wood, Surface::Dirt, Surface::Grass, Surface::Tile, Surface::Glass, Surface::Flesh] {
        put(Key::Step(s), &|x| sfx::footstep(s, x), 4);
        put(Key::Impact(s), &|x| sfx::impact(s, x), 3);
    }
    put(Key::Land, &sfx::land, 2);
    put(Key::Helmet, &sfx::helmet, 2);
    put(Key::Hurt, &sfx::hurt, 3);
    put(Key::Whoosh(false), &|s| sfx::whoosh(false, s), 2);
    put(Key::Whoosh(true), &|s| sfx::whoosh(true, s), 1);
    put(Key::Bounce, &sfx::bounce, 3);
    put(Key::Brass, &sfx::brass, 4);
    for big in [false, true] {
        for far in [false, true] {
            put(Key::Boom(big, far), &|s| sfx::explosion(big, far, s), 1);
        }
    }
    put(Key::Flashbang, &sfx::flashbang, 1);
    put(Key::Hiss, &sfx::smoke_hiss, 1);
    put(Key::Beep, &|_| sfx::beep(), 1);
    put(Key::Keypad, &sfx::keypad, 1);
    put(Key::Defuse, &sfx::defuse_click, 1);
    put(Key::Click, &|_| sfx::ui_click(), 1);
    put(Key::Hover, &|_| sfx::ui_hover(), 1);
    put(Key::Buy, &|_| sfx::buy(), 1);
    put(Key::Pickup, &|_| sfx::pickup(), 1);
    put(Key::Radio, &|_| sfx::radio(), 1);
    for k in 0..3 {
        put(Key::Sting(k), &|_| sfx::sting(k), 1);
    }
    b
}

/// On Linux the sound server's own ALSA device (PipeWire, else PulseAudio) when it has one: ALSA's `default` is only
/// routed through the server when the distribution's glue package is installed; without it, it is the first card
/// (often a monitor's HDMI output), and Bluetooth headphones or the chosen output hear nothing.
#[cfg(target_os = "linux")]
fn server_device() -> Option<kira::backend::cpal::cpal::Device> {
    use kira::backend::cpal::cpal::traits::{DeviceTrait, HostTrait};
    let devices: Vec<_> = kira::backend::cpal::cpal::default_host().output_devices().ok()?.collect();
    ["pipewire", "pulse"].into_iter().find_map(|want| devices.iter().find(|d| d.id().is_ok_and(|id| id.id() == want)).cloned())
}

#[cfg(not(target_os = "linux"))]
fn server_device() -> Option<kira::backend::cpal::cpal::Device> {
    None
}

impl Mixer {
    fn new() -> Result<Mixer, String> {
        // Recordings are loud: a few shots and an explosion at once would clip, so the mix is squeezed above -10 dB.
        let settings = |device| AudioManagerSettings {
            backend_settings: kira::backend::cpal::CpalBackendSettings { device, config: None },
            main_track_builder: MainTrackBuilder::new().with_effect(
                CompressorBuilder::new().threshold(-10.0).ratio(4.0).attack_duration(Duration::from_millis(2)).release_duration(Duration::from_millis(180)),
            ),
            ..default()
        };
        let server = server_device().and_then(|device| {
            AudioManager::<DefaultBackend>::new(settings(Some(device))).inspect_err(|e| warn!("audio: sound server device failed ({e}), using the default")).ok()
        });
        let mut manager = match server {
            Some(m) => m,
            None => AudioManager::<DefaultBackend>::new(settings(None)).map_err(|e| e.to_string())?,
        };
        let listener = manager.add_listener(v(Vec3::ZERO), q(Quat::IDENTITY)).map_err(|e| e.to_string())?;
        let mut emitters = Vec::new();
        for _ in 0..40 {
            let builder = SpatialTrackBuilder::new().distances(SpatialTrackDistances { min_distance: 2.5, max_distance: 90.0 }).persist_until_sounds_finish(true);
            emitters.push(manager.add_spatial_sub_track(&listener, v(Vec3::ZERO), builder).map_err(|e| e.to_string())?);
        }
        Ok(Mixer { manager, listener, emitters, next: 0, bank: bank(), reloads: HashMap::new(), tinnitus: None, listener_pos: Vec3::ZERO, volume: 1.0, seed: 1, samples: HashMap::new() })
    }

    /// One of a sound's recorded variants, or its synthesised stand-in.
    fn pick(&mut self, k: Key) -> Option<StaticSoundData> {
        match sample_name(k) {
            Some(name) => self.pick_named(&name, k),
            None => self.pick_synth(k),
        }
    }

    /// One of the variants of the sample set `name`, or the synthesised `fallback`.
    fn pick_named(&mut self, name: &str, fallback: Key) -> Option<StaticSoundData> {
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        match self.samples.get(name).filter(|l| !l.is_empty()) {
            Some(list) => list.get((self.seed >> 16) as usize % list.len()).cloned(),
            None => self.pick_synth(fallback),
        }
    }

    fn pick_synth(&mut self, k: Key) -> Option<StaticSoundData> {
        let list = self.bank.get(&k)?;
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        list.get((self.seed >> 16) as usize % list.len()).cloned()
    }

    /// Plays a sound at a place in the world, `db` louder or softer, `rate` faster or slower.
    fn at(&mut self, k: Key, pos: Vec3, db: f32, rate: f64) {
        let Some(s) = self.pick(k) else { return };
        self.play_at(s, pos, db, rate);
    }

    fn play_at(&mut self, s: StaticSoundData, pos: Vec3, db: f32, rate: f64) {
        if pos.distance(self.listener_pos) > 110.0 {
            return;
        }
        let i = self.next % self.emitters.len();
        self.next += 1;
        let e = &mut self.emitters[i];
        e.set_position(v(pos), now());
        let _ = e.play(s.volume(Decibels(db)).playback_rate(PlaybackRate(rate)));
    }

    /// Plays a sound without a position (the interface, the player's own weapon).
    fn flat(&mut self, k: Key, db: f32, rate: f64) {
        let Some(s) = self.pick(k) else { return };
        let _ = self.manager.play(s.volume(Decibels(db)).playback_rate(PlaybackRate(rate)));
    }

    fn jitter(&mut self) -> f64 {
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        0.95 + ((self.seed >> 16) % 100) as f64 / 1000.0
    }
}

fn gun_of(w: WeaponId) -> Gun {
    match w.def().class {
        Class::Pistol if w == WeaponId::Rev357 => Gun::Heavy,
        Class::Pistol => Gun::Pistol,
        Class::Smg => Gun::Smg,
        Class::Sniper if w.def().cycle > 1.0 => Gun::Sniper,
        Class::Sniper => Gun::Rifle,
        Class::Shotgun => Gun::Shotgun,
        Class::Mg => Gun::Mg,
        _ => Gun::Rifle,
    }
}

/// Which recorded shots a weapon fires: the Kalashnikovs have their own.
fn shot_set(w: WeaponId) -> String {
    match w {
        WeaponId::Ak74 | WeaponId::Akm | WeaponId::Pkm => "ak".into(),
        _ => format!("{:?}", gun_of(w)).to_lowercase(),
    }
}

/// Which recorded reload a weapon has (none for the shotguns, loaded shell by shell).
fn reload_kit(w: WeaponId) -> Option<&'static str> {
    Some(match w.def().class {
        Class::Pistol => "pistol",
        Class::Smg => "smg",
        Class::Shotgun => return None,
        _ if matches!(w, WeaponId::Ak74 | WeaponId::Akm | WeaponId::Pkm | WeaponId::Svd) => "ak",
        _ => "rifle",
    })
}

fn surface(m: Material) -> Surface {
    match m {
        Material::Metal => Surface::Metal,
        Material::Wood => Surface::Wood,
        Material::Dirt => Surface::Dirt,
        Material::Grass => Surface::Grass,
        Material::Tile => Surface::Tile,
        Material::Glass => Surface::Glass,
        Material::Flesh => Surface::Flesh,
        _ => Surface::Concrete,
    }
}

fn start_on_gesture(world: &mut World) {
    if world.get_non_send::<Mixer>().is_some() || world.get_non_send::<Silent>().is_some() {
        return;
    }
    // Natively at once; a browser only lets a page make sound after a click or a key press.
    let clicked = world.resource::<ButtonInput<MouseButton>>().get_just_pressed().next().is_some() || world.resource::<ButtonInput<KeyCode>>().get_just_pressed().next().is_some();
    if cfg!(target_arch = "wasm32") && !clicked {
        return;
    }
    match Mixer::new() {
        Ok(m) => {
            info!("audio: started");
            world.insert_non_send(m);
        }
        Err(e) => {
            warn!("audio: unavailable ({e})");
            world.insert_non_send(Silent);
        }
    }
}

struct Silent;

fn follow_camera(mixer: Option<NonSendMut<Mixer>>, cam: Single<&GlobalTransform, With<PlayerCamera>>) {
    let Some(mut m) = mixer else { return };
    let (_, rot, pos) = cam.to_scale_rotation_translation();
    m.listener.set_position(v(pos), now());
    m.listener.set_orientation(q(rot), now());
    m.listener_pos = pos;
}

fn volume(mixer: Option<NonSendMut<Mixer>>, options: Res<crate::options::Options>) {
    let Some(mut m) = mixer else { return };
    if (m.volume - options.volume).abs() > 1e-3 {
        m.volume = options.volume;
        let db = if options.volume <= 0.001 { -60.0 } else { 20.0 * options.volume.log10() };
        m.manager.main_track().set_volume(Decibels(db), now());
    }
}

fn play_cues(mixer: Option<NonSendMut<Mixer>>, mut cues: MessageReader<Cue>) {
    let Some(mut m) = mixer else {
        cues.clear();
        return;
    };
    for c in cues.read() {
        match *c {
            Cue::Brass(p) => {
                let r = m.jitter();
                m.at(Key::Brass, p, -8.0, r);
            }
            Cue::UiClick => m.flat(Key::Click, -4.0, 1.0),
            Cue::UiHover => m.flat(Key::Hover, -10.0, 1.0),
        }
    }
}

fn play_events(mixer: Option<NonSendMut<Mixer>>, frame: Res<crate::session::Frame>, session: Option<Res<crate::session::Session>>, spectate: Res<crate::view::Spectate>) {
    let Some(mut m) = mixer else { return };
    let Some(s) = session else { return };
    let viewed = s.viewed(spectate.target);
    let me = s.me;
    let pos_of = |id: usize| s.game.players[id].mover.origin + Vec3::Y * 1.2;
    for e in &frame.events {
        match e {
            Event::Shot { player, weapon, from, .. } => {
                let g = gun_of(*weapon);
                let r = m.jitter();
                let own = *player == viewed;
                let far = !own && from.distance(m.listener_pos) > 25.0;
                let name = format!("shot_{}{}", shot_set(*weapon), if far { "_far" } else { "" });
                if let Some(snd) = m.pick_named(&name, Key::Shot(g, far)) {
                    let snd = snd.playback_rate(PlaybackRate(r));
                    if own {
                        let _ = m.manager.play(snd.volume(Decibels(-1.0)));
                    } else {
                        m.play_at(snd, *from, if far { 2.0 } else { 0.0 }, 1.0);
                    }
                }
                if weapon.def().class == Class::Shotgun && weapon.def().cycle > 0.5 || weapon.def().cycle > 1.0 {
                    let bolt = weapon.def().class == Class::Sniper;
                    let at = *from;
                    let snd = m.pick(Key::Cycle(bolt));
                    if let Some(snd) = snd {
                        let delayed = snd.start_time(kira::StartTime::Delayed(std::time::Duration::from_millis(350)));
                        if *player == viewed {
                            let _ = m.manager.play(delayed.volume(Decibels(-6.0)));
                        } else {
                            m.play_at(delayed, at, -8.0, 1.0);
                        }
                    }
                }
            }
            Event::DryFire { player } => {
                let p = pos_of(*player);
                m.at(Key::Dry, p, -4.0, 1.0);
            }
            Event::Reload { player, weapon } => {
                let d = weapon.def();
                let secs = d.shells.map_or(d.reload, |(start, _)| start);
                let own = *player == viewed;
                let p = pos_of(*player);
                // Recorded: the magazine out, in, and the slide or bolt, spread over the reload (the shotguns load
                // shell by shell, each its own sound).
                let parts = reload_kit(*weapon).map(|kit| {
                    let bolt = if d.class == Class::Sniper && d.cycle > 1.0 { "cycle_bolt".to_string() } else { format!("reload_{kit}_bolt") };
                    [(format!("reload_{kit}_out"), 0.1), (format!("reload_{kit}_in"), 0.48), (bolt, if kit == "pistol" { 0.8 } else { 0.72 })]
                });
                let recorded = parts.as_ref().is_some_and(|parts| parts.iter().all(|(name, _)| m.samples.contains_key(name)));
                if recorded {
                    for (name, at) in parts.into_iter().flatten() {
                        let Some(snd) = m.pick_named(&name, Key::Draw) else { continue };
                        let snd = snd.start_time(StartTime::Delayed(Duration::from_secs_f32(secs * at)));
                        if own {
                            let _ = m.manager.play(snd.volume(Decibels(-4.0)));
                        } else {
                            m.play_at(snd, p, -6.0, 1.0);
                        }
                    }
                } else if d.shells.is_none() {
                    let pistol = d.class == Class::Pistol;
                    let key = ((secs * 10.0) as u32, pistol);
                    let snd = m.reloads.entry(key).or_insert_with(|| sfx::reload(secs, pistol, key.0 * 31 + 7)).clone();
                    if own {
                        let _ = m.manager.play(snd.volume(Decibels(-4.0)));
                    } else {
                        m.play_at(snd, p, -6.0, 1.0);
                    }
                }
            }
            Event::Shell { player } => {
                let p = pos_of(*player);
                if *player == viewed {
                    m.flat(Key::Shell, -5.0, 1.0);
                } else {
                    m.at(Key::Shell, p, -8.0, 1.0);
                }
            }
            Event::Draw { player, .. } if *player == viewed => m.flat(Key::Draw, -8.0, 1.0),
            Event::Zoom { player, .. } if *player == viewed => m.flat(Key::Zoom, -6.0, 1.0),
            Event::Footstep { player, pos, material, loud } => {
                let r = m.jitter();
                let db = if *player == viewed { -9.0 } else if *loud { -2.0 } else { -16.0 };
                m.at(Key::Step(surface(*material)), *pos, db, r);
            }
            Event::Land { player, pos, speed } => {
                let db = (-12.0 + speed * 1.5).min(0.0) + if *player == viewed { -4.0 } else { 0.0 };
                m.at(Key::Land, *pos, db, 1.0);
            }
            Event::Impact { pos, material, exit, .. } => {
                let r = m.jitter();
                m.at(Key::Impact(surface(*material)), *pos, if *exit { -10.0 } else { -5.0 }, r);
            }
            Event::Hit { victim, pos, zone, armor, .. } => {
                let r = m.jitter();
                if *zone == zugriff_sim::hitbox::Zone::Head && *armor {
                    m.at(Key::Helmet, *pos, 0.0, r);
                } else {
                    m.at(Key::Impact(Surface::Flesh), *pos, -2.0, r);
                }
                if *victim == me {
                    m.flat(Key::Hurt, -3.0, r);
                }
            }
            Event::KnifeSwing { player, stab } => {
                let p = pos_of(*player);
                m.at(Key::Whoosh(*stab), p, -6.0, 1.0);
            }
            Event::KnifeHit { pos, material, .. } => {
                let s = material.map_or(Surface::Concrete, surface);
                m.at(Key::Impact(s), *pos, -3.0, 0.8);
            }
            Event::Throw { player, .. } => {
                let p = pos_of(*player);
                m.at(Key::Whoosh(false), p, -10.0, 0.8);
            }
            Event::GrenadeBounce { pos, .. } => {
                let r = m.jitter();
                m.at(Key::Bounce, *pos, -6.0, r);
            }
            Event::Detonate { kind, pos, .. } => match kind {
                WeaponId::He => {
                    let far = pos.distance(m.listener_pos) > 30.0;
                    m.at(Key::Boom(false, far), *pos, 4.0, 1.0);
                }
                WeaponId::Flash => m.at(Key::Flashbang, *pos, 3.0, 1.0),
                _ => m.at(Key::Hiss, *pos, -4.0, 1.0),
            },
            Event::Exploded { pos } => {
                let far = pos.distance(m.listener_pos) > 40.0;
                m.at(Key::Boom(true, far), *pos, 6.0, 1.0);
            }
            Event::Flashed { player, seconds } if *player == me => {
                if let Some(mut old) = m.tinnitus.take() {
                    old.stop(now());
                }
                let snd = sfx::tinnitus(*seconds + 0.5);
                m.tinnitus = m.manager.play(snd.volume(Decibels(-4.0))).ok();
            }
            Event::PlantStart { player } => {
                let p = pos_of(*player);
                m.at(Key::Keypad, p, -3.0, 1.0);
            }
            Event::Beep { pos, .. } => m.at(Key::Beep, *pos, 0.0, 1.0),
            Event::DefuseStart { player, .. } => {
                let p = pos_of(*player);
                m.at(Key::Defuse, p, -2.0, 1.0);
            }
            Event::Defused { player } => {
                let p = pos_of(*player);
                m.at(Key::Defuse, p, 0.0, 1.3);
            }
            Event::Pickup { player, .. } | Event::BombPickup { player } if *player == viewed => m.flat(Key::Pickup, -6.0, 1.0),
            Event::Bought { player, .. } if *player == me => m.flat(Key::Buy, -6.0, 1.0),
            Event::Radio { player, .. } if s.game.players[*player].team == s.me().team => m.flat(Key::Radio, -10.0, 1.0),
            Event::RoundStart { .. } => m.flat(Key::Sting(2), -8.0, 1.0),
            Event::RoundEnd { winner, .. } => {
                let won = *winner == Some(s.me().team);
                m.flat(Key::Sting(if won { 0 } else { 1 }), -4.0, 1.0);
            }
            Event::MatchEnd { winner } => {
                let won = *winner == Some(s.me().team);
                m.flat(Key::Sting(if won { 0 } else { 1 }), 0.0, 0.8);
            }
            _ => {}
        }
    }
    let _ = Team::Ksk;
}
