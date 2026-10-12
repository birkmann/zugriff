//! The match being played: the sim's [`Game`] and its [`Bots`], ticked at the sim's fixed rate with the local
//! player's input. Each tick's events are collected into [`Frame`] for the effects, the sound and the HUD; the
//! positions before the tick are kept so everything drawn can be interpolated between ticks.

use crate::input::Held;
use crate::level::{Current, LoadMap};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use zugriff_sim::bots::{Bots, Difficulty};
use zugriff_sim::game::{Config, Event, Game, Mode, Phase, PlayerId, Team, UserCmd, TICK_HZ};
use zugriff_sim::rng::Rng;

pub struct SessionPlugin;

/// Starting a match (the systems that read the session run after it).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionSet;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(TICK_HZ as f64))
            .init_resource::<Frame>()
            .init_resource::<MatchSettings>()
            .init_resource::<Pending>()
            .init_resource::<crate::perf::SimCost>()
            .add_systems(First, clear_frame)
            .add_systems(Update, start_when_ready.in_set(SessionSet))
            .add_systems(FixedUpdate, tick);
    }
}

/// What the player chose in the menu.
#[derive(Resource, Clone, Debug)]
pub struct MatchSettings {
    pub map: String,
    pub mode: Mode,
    pub difficulty: Difficulty,
    /// None: whichever side needs a player.
    pub team: Option<Team>,
    /// Players per side (the human included on theirs).
    pub per_side: usize,
    pub max_rounds: u32,
    pub warmup: f32,
    /// A room on a server (`net.rs` starts it), not a match of our own.
    pub online: bool,
}

impl Default for MatchSettings {
    fn default() -> Self {
        MatchSettings { map: "zg_hafen".into(), mode: Mode::Defuse, difficulty: Difficulty::Normal, team: Some(Team::Ksk), per_side: 5, max_rounds: 16, warmup: 20.0, online: false }
    }
}

/// The mode a map is made for, from its name and contents.
pub fn mode_for(level: &zugriff_sim::compile::Level) -> Mode {
    if level.volumes_of("func_bomb_target").next().is_some() {
        Mode::Defuse
    } else if level.entities_of("info_hostage").next().is_some() {
        Mode::Rescue
    } else {
        Mode::Deathmatch
    }
}

/// A match asked for, waiting for its map to load.
#[derive(Resource, Default)]
pub struct Pending(pub Option<MatchSettings>);

/// Where a player was at the last tick (for interpolation).
#[derive(Clone, Copy, Debug, Default)]
pub struct Snap {
    pub origin: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub duck: f32,
    pub punch: Vec2,
    /// Down the sights, 0..1 (drives the zoom and the weapon's raise).
    pub ads: f32,
    /// The weapon's current spread (tan of the cone's half-angle; opens the crosshair).
    pub spread: f32,
}

#[derive(Resource)]
pub struct Session {
    pub game: Game,
    pub bots: Bots,
    pub me: PlayerId,
    pub settings: MatchSettings,
    pub paused: bool,
    pub prev: Vec<Snap>,
    pub prev_hostages: Vec<Vec3>,
    pub prev_grenades: Vec<(u32, Vec3)>,
    /// Ticks run since the match started.
    pub ticks: u64,
    /// Played on a server: `net.rs` ticks it from snapshots and predicts the local player.
    pub online: bool,
}

impl Session {
    pub fn snap(&self, id: PlayerId) -> Snap {
        let p = &self.game.players[id];
        let spread = p.inv.spread(p.mover.horizontal_speed() / p.inv.def().speed.max(0.1), p.mover.on_ground, p.mover.ducked);
        Snap { origin: p.mover.origin, yaw: p.yaw, pitch: p.pitch, duck: p.mover.duck, punch: p.punch, ads: p.inv.ads, spread }
    }

    /// A player's state between the last two ticks (`a` = 0 at the previous tick, 1 at the latest).
    pub fn lerp(&self, id: PlayerId, a: f32) -> Snap {
        let now = self.snap(id);
        let Some(prev) = self.prev.get(id).copied() else { return now };
        // A teleport (spawn): no smoothing across it.
        if prev.origin.distance(now.origin) > 3.0 {
            return now;
        }
        Snap {
            origin: prev.origin.lerp(now.origin, a),
            yaw: prev.yaw + wrap(now.yaw - prev.yaw) * a,
            pitch: prev.pitch + (now.pitch - prev.pitch) * a,
            duck: prev.duck + (now.duck - prev.duck) * a,
            punch: prev.punch.lerp(now.punch, a),
            ads: prev.ads + (now.ads - prev.ads) * a,
            spread: prev.spread + (now.spread - prev.spread) * a,
        }
    }

    pub fn hostage_pos(&self, i: usize, a: f32) -> Vec3 {
        let now = self.game.hostages[i].mover.origin;
        match self.prev_hostages.get(i) {
            Some(p) if p.distance(now) < 3.0 => p.lerp(now, a),
            _ => now,
        }
    }

    pub fn grenade_pos(&self, id: u32, a: f32) -> Option<Vec3> {
        let now = self.game.grenades.iter().find(|g| g.id == id)?.pos;
        Some(match self.prev_grenades.iter().find(|(i, _)| *i == id) {
            Some((_, p)) => p.lerp(now, a),
            None => now,
        })
    }

    pub fn me(&self) -> &zugriff_sim::game::Player {
        &self.game.players[self.me]
    }

    /// The player whose eyes the camera uses: the local player, or whom they spectate.
    pub fn viewed(&self, spectating: Option<PlayerId>) -> PlayerId {
        if self.me().alive {
            self.me
        } else {
            spectating.filter(|&s| self.game.players.get(s).is_some_and(|p| p.alive)).unwrap_or(self.me)
        }
    }
}

pub fn wrap(a: f32) -> f32 {
    let pi = std::f32::consts::PI;
    (a + pi).rem_euclid(2.0 * pi) - pi
}

/// This frame's events from the sim (all ticks run since the last frame).
#[derive(Resource, Default)]
pub struct Frame {
    pub events: Vec<Event>,
    /// A new match started this frame.
    pub started: bool,
}

fn clear_frame(mut frame: ResMut<Frame>) {
    frame.events.clear();
    frame.started = false;
}

/// Asks for a match: loads its map, and the session starts once the map (and its navigation) are in.
pub fn request(commands: &mut Commands, settings: MatchSettings, loads: &mut MessageWriter<LoadMap>) {
    loads.write(LoadMap(settings.map.clone()));
    commands.insert_resource(Pending(Some(settings)));
}

fn start_when_ready(mut commands: Commands, mut pending: ResMut<Pending>, current: Res<Current>, mut frame: ResMut<Frame>, mut held: ResMut<Held>) {
    let Some(settings) = pending.0.clone() else { return };
    if settings.online || current.name != settings.map {
        return;
    }
    let Some((level, nav)) = current.ready() else { return };
    pending.0 = None;
    let mode = if settings.mode == Mode::Deathmatch { Mode::Deathmatch } else { mode_for(&level) };
    let cfg = Config { mode, max_rounds: settings.max_rounds, warmup: settings.warmup, ..Default::default() };
    let seed = (js_now() * 1000.0) as u64;
    let mut game = Game::new(level, cfg, seed);
    game.nav = Some(nav.clone());
    let bots = Bots::new(nav, settings.difficulty, seed ^ 0x55);
    let mut rng = Rng::new(seed ^ 0x77);
    let team = settings.team.unwrap_or(if rng.chance(0.5) { Team::Ksk } else { Team::Opfor });
    // `autoplay`: a bot plays for the local player (for watching and testing).
    let autoplay = crate::args::get("autoplay").is_some() || crate::args::flag("autoplay");
    let me = game.add_player(&crate::options::player_name(), team, autoplay);
    // `--finish <key>`: every gun of ours in that finish (for screenshots).
    let finishes = match crate::args::get("finish").and_then(|k| crate::finish::FINISHES.iter().position(|f| f.key == k)) {
        Some(f) => vec![f as u8; zugriff_sim::weapons::WeaponId::ALL.len()],
        None => crate::options::load().finish_table(),
    };
    game.players[me].set_finishes(finishes);
    for side in [Team::Ksk, Team::Opfor] {
        let n = settings.per_side - usize::from(side == team);
        for _ in 0..n {
            let name = Bots::name_for(&game, side, &mut rng);
            let bot = game.add_player(&name, side, true);
            // Bots carry a finish on some of their guns (so there are painted guns to pick up).
            let table = zugriff_sim::weapons::WeaponId::ALL.iter().map(|_| if rng.chance(0.3) { 1 + rng.below(crate::finish::FINISHES.len() as u32 - 1) as u8 } else { 0 }).collect();
            game.players[bot].set_finishes(table);
        }
    }
    info!("match on {} ({:?}, {:?}), playing {:?}", settings.map, mode, settings.difficulty, team);
    let prev = (0..game.players.len()).map(|_| Snap::default()).collect();
    commands.insert_resource(Session { game, bots, me, settings, paused: false, prev, prev_hostages: Vec::new(), prev_grenades: Vec::new(), ticks: 0, online: false });
    frame.started = true;
    *held = Held::default();
}

fn js_now() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys_now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
    }
}

#[cfg(target_arch = "wasm32")]
fn js_sys_now() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now() + p.time_origin())
}

fn tick(session: Option<ResMut<Session>>, mut held: ResMut<Held>, look: Res<crate::view::Look>, mut frame: ResMut<Frame>, mut cost: ResMut<crate::perf::SimCost>) {
    let Some(mut s) = session else { return };
    if s.paused || s.online {
        return;
    }
    let s = &mut *s;
    s.prev = (0..s.game.players.len()).map(|i| s.snap(i)).collect();
    s.prev_hostages = s.game.hostages.iter().map(|h| h.mover.origin).collect();
    s.prev_grenades = s.game.grenades.iter().map(|g| (g.id, g.pos)).collect();
    let started = Instant::now();
    let mut cmds = if crate::args::flag("nobots") { Vec::new() } else { s.bots.think(&s.game) };
    let thought = Instant::now();
    let mine = UserCmd {
        forward: held.forward,
        side: held.side,
        yaw: look.yaw,
        pitch: look.pitch,
        buttons: zugriff_sim::game::Btn(held.buttons.0 | held.latched.0),
        select: held.select.take(),
        buy: std::mem::take(&mut held.buy),
        ..Default::default()
    };
    held.latched = zugriff_sim::game::Btn::empty();
    if !s.game.players[s.me].bot {
        cmds.push((s.me, mine));
    }
    s.game.tick(&cmds);
    cost.add(thought - started, thought.elapsed());
    s.ticks += 1;
    let radio = s.bots.drain_radio();
    frame.events.extend(s.game.events.drain(..));
    frame.events.extend(radio);
    // A finished match waits in its end screen; the menu offers another.
    if s.game.phase == Phase::MatchOver {
        s.game.phase_left = s.game.phase_left.max(0.0);
    }
}
