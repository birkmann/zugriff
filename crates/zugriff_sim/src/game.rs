//! The match: players, the round cycle, the economy, the bomb and the hostages. Everything moves at the fixed tick
//! ([`TICK`]); each tick takes one [`UserCmd`] per player (from the keyboard, a bot or, later, the network) and leaves a
//! list of [`Event`]s for the client to draw and play.
//!
//! The firing itself is in `combat.rs`, grenades in `grenades.rs`; this file ties them together.

use crate::compile::{Level, Volume};
use crate::hitbox::Hitboxes;
use crate::material::Material;
use crate::movement::{self, Buttons, Ladders, MoveCmd, MoveParams, Mover};
use crate::rng::Rng;
use crate::units::METRES_PER_UNIT;
use crate::weapons::{Action, Class, Gun, Inventory, Side, Slot, WeaponId, MAX_FLASH, MAX_HE, MAX_SMOKE};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const TICK_HZ: f32 = 66.0;
pub const TICK: f32 = 1.0 / TICK_HZ;
const U: f32 = METRES_PER_UNIT as f32;

pub type PlayerId = usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum Team {
    Ksk,
    Opfor,
}

impl Team {
    pub fn other(self) -> Team {
        match self {
            Team::Ksk => Team::Opfor,
            Team::Opfor => Team::Ksk,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Team::Ksk => "KSK",
            Team::Opfor => "OPFOR",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Team::Ksk => 0,
            Team::Opfor => 1,
        }
    }

    pub fn pistol(self) -> WeaponId {
        match self {
            Team::Ksk => WeaponId::P8,
            Team::Opfor => WeaponId::Mp443,
        }
    }

    pub fn rifle(self) -> WeaponId {
        match self {
            Team::Ksk => WeaponId::G95,
            Team::Opfor => WeaponId::Akm,
        }
    }

    fn spawn_class(self) -> &'static str {
        match self {
            Team::Ksk => "info_player_ksk",
            Team::Opfor => "info_player_opfor",
        }
    }

    pub fn may_buy(self, side: Side) -> bool {
        match side {
            Side::Both => true,
            Side::Ksk => self == Team::Ksk,
            Side::Opfor => self == Team::Opfor,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Mode {
    /// Bomb defusal: OPFOR plants at A or B, KSK defends or defuses.
    Defuse,
    /// Hostage rescue: KSK bring the hostages to the rescue zone, OPFOR hold them.
    Rescue,
    /// Everyone respawns, no rounds; most kills when the time runs out.
    Deathmatch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub mode: Mode,
    /// Rounds in a match (sides switch at half; first to more than half wins).
    pub max_rounds: u32,
    pub warmup: f32,
    pub freeze: f32,
    pub buy_time: f32,
    pub round_time: f32,
    pub round_end: f32,
    pub bomb_timer: f32,
    pub plant_time: f32,
    pub defuse_time: f32,
    pub defuse_kit_time: f32,
    pub start_money: i32,
    pub max_money: i32,
    pub friendly_fire: bool,
    /// Deathmatch length (s).
    pub dm_time: f32,
    pub respawn: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: Mode::Defuse,
            max_rounds: 30,
            warmup: 60.0,
            freeze: 6.0,
            buy_time: 45.0,
            round_time: 115.0,
            round_end: 6.0,
            bomb_timer: 40.0,
            plant_time: 3.0,
            defuse_time: 10.0,
            defuse_kit_time: 5.0,
            start_money: 800,
            max_money: 16_000,
            friendly_fire: false,
            dm_time: 600.0,
            respawn: 2.0,
        }
    }
}

/// Round money.
pub mod money {
    pub const WIN_ELIMINATION: i32 = 3250;
    pub const WIN_BOMB: i32 = 3500;
    pub const WIN_DEFUSE: i32 = 3500;
    pub const WIN_TIME: i32 = 3250;
    pub const WIN_RESCUE: i32 = 3500;
    pub const LOSS: i32 = 1400;
    pub const LOSS_STEP: i32 = 500;
    pub const LOSS_MAX: i32 = 3400;
    pub const PLANT_TEAM: i32 = 800;
    pub const PLANT_PLAYER: i32 = 300;
    pub const DEFUSE_PLAYER: i32 = 300;
    pub const RESCUE_TEAM: i32 = 1000;
    pub const RESCUE_PLAYER: i32 = 300;
    pub const TEAM_KILL: i32 = -3300;
    pub const HOSTAGE_KILL: i32 = -1000;
    pub const VEST: i32 = 650;
    pub const VEST_HELMET: i32 = 1000;
    pub const HELMET: i32 = 350;
    pub const DEFUSER: i32 = 400;
}

crate::bitflags_lite! {
    /// Buttons held this tick.
    pub struct Btn: u16 {
        const ATTACK = 1;
        const ATTACK2 = 2;
        const RELOAD = 4;
        const USE = 8;
        const JUMP = 16;
        const DUCK = 32;
        const WALK = 64;
        const DROP = 128;
        const INSPECT = 256;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Select {
    Slot(Slot),
    Weapon(WeaponId),
    Last,
    Next,
    Prev,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Buy {
    Weapon(WeaponId),
    Vest,
    VestHelmet,
    Defuser,
}

impl Buy {
    pub fn price(self) -> i32 {
        match self {
            Buy::Weapon(w) => w.def().price,
            Buy::Vest => money::VEST,
            Buy::VestHelmet => money::VEST_HELMET,
            Buy::Defuser => money::DEFUSER,
        }
    }
}

/// One tick of a player's intent.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserCmd {
    pub forward: f32,
    pub side: f32,
    /// View angles (radians): yaw about +Y (0 looks down −Z), pitch up positive.
    pub yaw: f32,
    pub pitch: f32,
    pub buttons: Btn,
    pub select: Option<Select>,
    pub buy: Vec<Buy>,
    /// Online: the command's number from the client (0 offline). The shot's spread and recoil draw from a generator
    /// seeded by it, so the client's prediction of its own shots matches the server.
    #[serde(default)]
    pub seq: u32,
    /// Online: the server tick the client was showing when it made this command (0: now). Shots are traced against
    /// everyone where they stood then (lag compensation, see `lagcomp.rs`).
    #[serde(default)]
    pub view_tick: u32,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Stats {
    pub kills: i32,
    pub deaths: i32,
    pub assists: i32,
    pub headshots: i32,
    pub damage: f32,
    pub mvps: i32,
    pub score: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub team: Team,
    pub bot: bool,
    pub alive: bool,
    pub health: i32,
    pub armor: i32,
    pub helmet: bool,
    pub defuser: bool,
    pub money: i32,
    pub mover: Mover,
    pub yaw: f32,
    pub pitch: f32,
    /// The recoil's push on the aim (degrees: up, right), recovering each tick.
    pub punch: Vec2,
    pub inv: Inventory,
    pub prev_buttons: Btn,
    /// Being flashed: seconds left, and the flash's full length.
    pub flash_left: f32,
    pub flash_total: f32,
    /// Slowed by hits (1 = full speed).
    pub tag: f32,
    pub stats: Stats,
    /// Damage taken from each player this life (for assists).
    pub damage_from: Vec<f32>,
    /// Seconds since last death (deathmatch respawn), and since the last round started.
    pub dead_for: f32,
    /// Defusing: seconds done.
    pub defusing: Option<f32>,
    /// Distance walked since the last footstep (m).
    pub stride: f32,
    /// Seconds since this player last made a noise others hear (a shot, a footstep, a landing).
    pub noise: f32,
    pub last_noise_pos: Vec3,
    /// The last hit taken from someone: who, from where (their eye then) and when (game time). Bots turn on it.
    #[serde(default)]
    pub hurt: Option<(PlayerId, Vec3, f32)>,
    /// The last shot fired: from where and when (game time). A shot carries much further than a footstep.
    #[serde(default)]
    pub gunshot: Option<(Vec3, f32)>,
    pub kills_this_round: i32,
    /// Buy menu use: spent this round (for the refund-free economy bookkeeping and bots).
    pub spent: i32,
    /// Online: an empty slot (no one plays it and it never spawns; see `set_vacant`).
    #[serde(default)]
    pub vacant: bool,
    /// The finish each weapon this player buys or is issued comes in (by `WeaponId as usize`; missing = factory).
    #[serde(default)]
    pub finishes: Vec<u8>,
}

impl Player {
    /// A new gun of this player's, in the finish they chose for it.
    pub fn issue(&self, w: WeaponId) -> Gun {
        Gun::new(w).with_finish(self.finishes.get(w as usize).copied().unwrap_or(0))
    }

    /// Sets the finishes this player's guns come in (by `WeaponId as usize`); the guns they carry now, issued when
    /// they joined, take them too.
    pub fn set_finishes(&mut self, finishes: Vec<u8>) {
        self.finishes = finishes;
        let of = |w: WeaponId, f: &[u8]| f.get(w as usize).copied().unwrap_or(0);
        if let Some(g) = self.inv.primary.as_mut() {
            g.finish = of(g.id, &self.finishes);
        }
        if let Some(g) = self.inv.secondary.as_mut() {
            g.finish = of(g.id, &self.finishes);
        }
    }

    /// A fresh loadout (knife and the side's pistol), the pistol in its finish.
    pub fn fresh_inventory(&self) -> Inventory {
        let mut inv = Inventory::spawn(self.team.pistol());
        if let Some(g) = inv.secondary.as_mut() {
            g.finish = self.issue(g.id).finish;
        }
        inv
    }

    pub fn eye(&self, p: &MoveParams) -> Vec3 {
        self.mover.eye(p)
    }

    pub fn hitboxes(&self) -> Hitboxes {
        Hitboxes::of(self.mover.origin, self.mover.duck, self.yaw)
    }

    /// The aim with the recoil's punch (radians).
    pub fn aim_angles(&self) -> (f32, f32) {
        (self.yaw - self.punch.y.to_radians(), (self.pitch + self.punch.x.to_radians()).clamp(-1.55, 1.55))
    }

    pub fn aim_dir(&self) -> Vec3 {
        let (yaw, pitch) = self.aim_angles();
        dir_from(yaw, pitch)
    }

    pub fn place_max_speed(&self) -> f32 {
        let d = self.inv.def();
        let base = if self.inv.zoom > 0 { d.scoped_speed } else { d.speed };
        base * self.tag * (1.0 - crate::weapons::ADS_SLOW * self.inv.ads)
    }
}

/// A unit direction from yaw (0 looks down −Z) and pitch (up positive).
pub fn dir_from(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(-yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos())
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Bomb {
    /// Not in play (other modes, between rounds).
    None,
    Carried(PlayerId),
    Dropped { pos: Vec3, vel: Vec3 },
    Planted { pos: Vec3, site: char, left: f32, defuser: Option<PlayerId>, progress: f32 },
    Exploded { pos: Vec3 },
    Defused { pos: Vec3 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hostage {
    pub mover: Mover,
    pub yaw: f32,
    pub health: i32,
    pub alive: bool,
    pub rescued: bool,
    pub leader: Option<PlayerId>,
    pub start: Vec3,
    pub start_yaw: f32,
    /// A route to the leader from the navigation, when bots' nav is available (waypoints, next first).
    pub route: Vec<Vec3>,
    pub repath: f32,
}

impl Hostage {
    pub fn hitboxes(&self) -> Hitboxes {
        Hitboxes::of(self.mover.origin, self.mover.duck, self.yaw)
    }
}

/// A weapon lying on the floor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dropped {
    pub id: u32,
    pub gun: Gun,
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub rest: bool,
    /// Who dropped it and when (they can't pick it straight back up).
    pub by: Option<PlayerId>,
    pub age: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grenade {
    pub id: u32,
    pub kind: WeaponId,
    pub pos: Vec3,
    pub vel: Vec3,
    pub thrower: PlayerId,
    pub team: Team,
    pub age: f32,
    pub rest: bool,
    pub spin: Vec3,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Smoke {
    pub id: u32,
    pub pos: Vec3,
    pub age: f32,
}

/// How long a smoke lasts and how big it is (m).
pub const SMOKE_TIME: f32 = 18.0;
pub const SMOKE_RADIUS: f32 = 144.0 * U;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Phase {
    Warmup,
    Freeze,
    Live,
    /// The round is decided; the next starts when the timer runs out.
    Over,
    MatchOver,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Reason {
    Elimination,
    BombExploded,
    BombDefused,
    TimeRanOut,
    HostagesRescued,
    Draw,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Spawn { player: PlayerId, yaw: f32 },
    Shot { player: PlayerId, weapon: WeaponId, from: Vec3, dir: Vec3, end: Vec3, tracer: bool },
    Impact { pos: Vec3, normal: Vec3, material: Material, exit: bool },
    Hit { victim: PlayerId, attacker: Option<PlayerId>, pos: Vec3, dir: Vec3, damage: i32, zone: crate::hitbox::Zone, armor: bool },
    HostageHit { hostage: usize, pos: Vec3, dir: Vec3 },
    Kill { killer: Option<PlayerId>, victim: PlayerId, weapon: WeaponId, headshot: bool, wallbang: bool, assist: Option<PlayerId> },
    KnifeSwing { player: PlayerId, stab: bool },
    KnifeHit { player: PlayerId, pos: Vec3, material: Option<Material> },
    DryFire { player: PlayerId },
    Reload { player: PlayerId, weapon: WeaponId },
    Shell { player: PlayerId },
    Draw { player: PlayerId, weapon: WeaponId },
    Zoom { player: PlayerId, level: u8 },
    Footstep { player: PlayerId, pos: Vec3, material: Material, loud: bool },
    Jump { player: PlayerId },
    Land { player: PlayerId, pos: Vec3, speed: f32 },
    FallDamage { player: PlayerId, damage: i32 },
    Throw { player: PlayerId, grenade: u32, kind: WeaponId },
    GrenadeBounce { grenade: u32, pos: Vec3 },
    Detonate { grenade: u32, kind: WeaponId, pos: Vec3 },
    Flashed { player: PlayerId, seconds: f32 },
    SmokeGone { smoke: u32 },
    PlantStart { player: PlayerId },
    Planted { player: PlayerId, pos: Vec3, site: char },
    Beep { pos: Vec3, left: f32 },
    DefuseStart { player: PlayerId, kit: bool },
    DefuseAbort { player: PlayerId },
    Defused { player: PlayerId },
    Exploded { pos: Vec3 },
    BombDropped { pos: Vec3 },
    BombPickup { player: PlayerId },
    WeaponDropped { player: PlayerId, item: u32 },
    Pickup { player: PlayerId, weapon: WeaponId },
    HostageFollow { hostage: usize, player: Option<PlayerId> },
    HostageRescued { hostage: usize, player: Option<PlayerId> },
    HostageKilled { hostage: usize, by: Option<PlayerId> },
    Bought { player: PlayerId, item: Buy },
    Money { player: PlayerId, amount: i32, reason: std::borrow::Cow<'static, str> },
    RoundStart { round: u32 },
    FreezeEnd,
    RoundEnd { winner: Option<Team>, reason: Reason, mvp: Option<PlayerId> },
    Halftime,
    MatchEnd { winner: Option<Team> },
    WarmupEnd,
    Radio { player: PlayerId, text: String },
}

pub(crate) struct LevelLadders<'a>(pub &'a Level);

impl Ladders for LevelLadders<'_> {
    fn ladder_at(&self, feet: Vec3, height: f32) -> bool {
        self.0.volumes_of("func_ladder").any(|v| v.contains(feet + Vec3::Y * 0.1) || v.contains(feet + Vec3::Y * height * 0.5))
    }
}

/// The match.
#[derive(Clone)]
pub struct Game {
    pub cfg: Config,
    pub level: Arc<Level>,
    pub params: MoveParams,
    pub players: Vec<Player>,
    pub phase: Phase,
    /// Seconds left in the phase (freeze, round, round end, warmup).
    pub phase_left: f32,
    /// Seconds since the round went live (buy time).
    pub live_for: f32,
    pub time: f32,
    pub round: u32,
    pub score: [u32; 2],
    pub loss_streak: [u32; 2],
    pub bomb: Bomb,
    pub hostages: Vec<Hostage>,
    pub dropped: Vec<Dropped>,
    pub grenades: Vec<Grenade>,
    pub smokes: Vec<Smoke>,
    pub events: Vec<Event>,
    pub rng: Rng,
    pub next_id: u32,
    pub last_result: Option<(Option<Team>, Reason)>,
    /// Whether the bomb was planted this round (the attackers' plant bonus).
    pub planted_this_round: bool,
    pub halftime_done: bool,
    pub winner: Option<Team>,
    pub beep_left: f32,
    /// The navigation graph, if loaded: led hostages walk it.
    pub nav: Option<Arc<crate::nav::Nav>>,
    /// Ticks run (the network's clock).
    pub ticks: u32,
    /// Online: where everyone stood over the last second, for lag-compensated shots (None offline).
    pub lag: Option<crate::lagcomp::History>,
}

impl Game {
    pub fn new(level: Arc<Level>, cfg: Config, seed: u64) -> Game {
        let mut g = Game {
            cfg,
            level,
            params: MoveParams::default(),
            players: Vec::new(),
            phase: Phase::Warmup,
            phase_left: 0.0,
            live_for: 0.0,
            time: 0.0,
            round: 0,
            score: [0, 0],
            loss_streak: [0, 0],
            bomb: Bomb::None,
            hostages: Vec::new(),
            dropped: Vec::new(),
            grenades: Vec::new(),
            smokes: Vec::new(),
            events: Vec::new(),
            rng: Rng::new(seed),
            next_id: 1,
            last_result: None,
            planted_this_round: false,
            halftime_done: false,
            winner: None,
            beep_left: 0.0,
            nav: None,
            ticks: 0,
            lag: None,
        };
        g.phase_left = g.cfg.warmup;
        if g.cfg.mode == Mode::Deathmatch {
            g.phase = Phase::Live;
            g.phase_left = g.cfg.dm_time;
        } else if g.cfg.warmup <= 0.0 {
            g.phase_left = 0.0;
        }
        g
    }

    /// Adds a player (joins dead in rounds; spawns at once in warmup/deathmatch). Returns its id.
    pub fn add_player(&mut self, name: &str, team: Team, bot: bool) -> PlayerId {
        let id = self.players.len();
        self.players.push(Player {
            id,
            name: name.to_string(),
            team,
            bot,
            alive: false,
            health: 0,
            armor: 0,
            helmet: false,
            defuser: false,
            money: self.cfg.start_money,
            mover: Mover::default(),
            yaw: 0.0,
            pitch: 0.0,
            punch: Vec2::ZERO,
            inv: Inventory::spawn(team.pistol()),
            prev_buttons: Btn::empty(),
            flash_left: 0.0,
            flash_total: 0.0,
            tag: 1.0,
            stats: Stats::default(),
            damage_from: Vec::new(),
            dead_for: 0.0,
            defusing: None,
            stride: 0.0,
            noise: 99.0,
            last_noise_pos: Vec3::ZERO,
            hurt: None,
            gunshot: None,
            kills_this_round: 0,
            spent: 0,
            finishes: Vec::new(),
            vacant: false,
        });
        for p in &mut self.players {
            p.damage_from.resize(id + 1, 0.0);
        }
        if matches!(self.phase, Phase::Warmup) || self.cfg.mode == Mode::Deathmatch {
            self.respawn_dm(id);
        }
        id
    }

    /// Online: empties a slot (the player left and no bot takes over) or fills it again. An emptied slot's body is
    /// gone at once (the bomb it carried falls where it stood); a filled one joins dead and spawns as players do.
    pub fn set_vacant(&mut self, id: PlayerId, vacant: bool) {
        let p = &mut self.players[id];
        p.vacant = vacant;
        if !vacant {
            p.dead_for = 0.0;
            if matches!(self.phase, Phase::Warmup) || self.cfg.mode == Mode::Deathmatch {
                self.respawn_dm(id);
            }
            return;
        }
        p.bot = false;
        p.defusing = None;
        if p.alive {
            p.alive = false;
            p.health = 0;
            if p.inv.bomb {
                p.inv.remove(WeaponId::Bomb);
                let pos = p.mover.origin + Vec3::Y * 0.3;
                self.bomb = Bomb::Dropped { pos, vel: Vec3::ZERO };
                self.events.push(Event::BombDropped { pos });
            }
        }
    }

    pub fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    // ---- level queries ------------------------------------------------------------------------------------------

    pub fn volumes<'a>(&'a self, class: &'a str) -> Vec<&'a Volume> {
        self.level.volumes_of(class).collect()
    }

    pub fn in_buyzone(&self, p: &Player) -> bool {
        if self.phase == Phase::Warmup || self.cfg.mode == Mode::Deathmatch {
            return true;
        }
        let team = match p.team {
            Team::Ksk => "ksk",
            Team::Opfor => "opfor",
        };
        let at = p.mover.origin + Vec3::Y * 0.5;
        self.level.volumes_of("func_buyzone").any(|v| v.get("team").is_none_or(|t| t.eq_ignore_ascii_case(team)) && v.contains(at))
    }

    pub fn can_buy(&self, p: &Player) -> bool {
        p.alive
            && match self.phase {
                Phase::Warmup => true,
                Phase::Freeze => true,
                Phase::Live => self.cfg.mode == Mode::Deathmatch || self.live_for < self.cfg.buy_time,
                _ => false,
            }
            && self.in_buyzone(p)
    }

    /// The bomb site `p` is in, if any.
    pub fn site_at(&self, p: Vec3) -> Option<char> {
        self.level
            .volumes_of("func_bomb_target")
            .find(|v| v.contains(p + Vec3::Y * 0.3))
            .map(|v| v.get("site").and_then(|s| s.chars().next()).unwrap_or('A'))
    }

    /// The callout for a position (the last matching place volume wins, so small places inside big ones take over).
    pub fn place_at(&self, p: Vec3) -> Option<&str> {
        self.level.volumes_of("zg_place").filter(|v| v.contains(p + Vec3::Y * 0.5)).last().and_then(|v| v.get("name"))
    }

    pub fn alive(&self, team: Team) -> usize {
        self.players.iter().filter(|p| p.team == team && p.alive).count()
    }

    pub fn eye(&self, id: PlayerId) -> Vec3 {
        self.players[id].eye(&self.params)
    }

    /// Whether the straight line between two points is clear of the world (and of smoke if `smoke`).
    pub fn clear_line(&self, a: Vec3, b: Vec3, smoke: bool) -> bool {
        let t = self.level.world.trace(a, b, Vec3::ZERO, crate::world::Contents::MASK_SHOT);
        if t.hit() {
            return false;
        }
        !(smoke && self.smoke_blocks(a, b))
    }

    /// Whether a smoke cloud hides the line from `a` to `b`.
    pub fn smoke_blocks(&self, a: Vec3, b: Vec3) -> bool {
        let ab = b - a;
        let len = ab.length();
        if len < 1e-4 {
            return false;
        }
        let d = ab / len;
        self.smokes.iter().filter(|s| s.age > 0.8 && s.age < SMOKE_TIME - 1.0).any(|s| {
            let c = s.pos + Vec3::Y * 1.0;
            let t = (c - a).dot(d).clamp(0.0, len);
            (a + d * t).distance(c) < SMOKE_RADIUS * 0.9
        })
    }

    // ---- spawning -----------------------------------------------------------------------------------------------

    fn spawn_points(&self, class: &str) -> Vec<(Vec3, f32)> {
        self.level.entities_of(class).map(|e| (e.origin, e.yaw)).collect()
    }

    fn place_player(&mut self, id: PlayerId, at: Vec3, yaw: f32) {
        let p = &mut self.players[id];
        p.mover = Mover { origin: at, on_ground: false, ..Default::default() };
        p.yaw = yaw;
        p.pitch = 0.0;
        p.punch = Vec2::ZERO;
        p.alive = true;
        p.health = 100;
        p.flash_left = 0.0;
        p.tag = 1.0;
        p.defusing = None;
        p.dead_for = 0.0;
        p.damage_from.iter_mut().for_each(|d| *d = 0.0);
        // Settle onto the floor below the spawn point.
        let world = &self.level.world;
        let ext = p.mover.half_extents(&self.params);
        let from = at + Vec3::Y * (ext.y + 0.05);
        let t = world.trace(from, from - Vec3::Y * 2.0, ext, crate::world::Contents::MASK_PLAYER);
        if !t.start_solid && t.fraction < 1.0 {
            p.mover.origin = t.end - Vec3::Y * ext.y;
            p.mover.on_ground = true;
        }
        self.events.push(Event::Spawn { player: id, yaw });
    }

    /// Spawns a player anywhere (warmup, deathmatch), away from enemies, with a rifle.
    pub fn respawn_dm(&mut self, id: PlayerId) {
        if self.players[id].vacant {
            return;
        }
        let team = self.players[id].team;
        let mut points = self.spawn_points("info_player_dm");
        if points.is_empty() {
            points = self.spawn_points(team.spawn_class());
        }
        if points.is_empty() {
            return;
        }
        let enemies: Vec<Vec3> = self.players.iter().filter(|p| p.alive && p.team != team).map(|p| p.mover.origin).collect();
        let friends: Vec<Vec3> = self.players.iter().filter(|p| p.alive && p.id != id).map(|p| p.mover.origin).collect();
        // The point farthest from the nearest enemy, with some randomness among the best few.
        let mut scored: Vec<(f32, (Vec3, f32))> = points
            .iter()
            .map(|&(pos, yaw)| {
                let near_enemy = enemies.iter().map(|e| e.distance(pos)).fold(60.0, f32::min);
                let crowded = friends.iter().any(|f| f.distance(pos) < 1.0);
                (near_enemy - if crowded { 100.0 } else { 0.0 }, (pos, yaw))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let pick = self.rng.below(scored.len().min(4) as u32) as usize;
        let (pos, yaw) = scored[pick].1;
        self.place_player(id, pos, yaw);
        let p = &mut self.players[id];
        p.inv = p.fresh_inventory();
        p.inv.primary = Some(p.issue(team.rifle()));
        p.inv.select(team.rifle());
        p.armor = 100;
        p.helmet = true;
    }

    /// Everyone to their side's spawn for a new round: survivors keep what they carry, the dead start over.
    fn spawn_round(&mut self) {
        for team in [Team::Ksk, Team::Opfor] {
            let mut points = self.spawn_points(team.spawn_class());
            self.rng.shuffle(&mut points);
            let ids: Vec<PlayerId> = self.players.iter().filter(|p| p.team == team && !p.vacant).map(|p| p.id).collect();
            for (i, id) in ids.into_iter().enumerate() {
                let was_alive = self.players[id].alive;
                let (pos, yaw) = points.get(i % points.len().max(1)).copied().unwrap_or((Vec3::ZERO, 0.0));
                // Extra players beyond the spawn points stand a little behind the others.
                let extra = (i / points.len().max(1)) as f32;
                let back = Vec3::new(yaw.sin(), 0.0, yaw.cos()) * extra * 1.2;
                self.place_player(id, pos + back, yaw);
                let p = &mut self.players[id];
                if !was_alive {
                    p.inv = p.fresh_inventory();
                    p.armor = 0;
                    p.helmet = false;
                    p.defuser = false;
                } else {
                    // Keep the kit, but put the best gun in hand, reloaded state reset.
                    p.inv.bomb = false;
                    p.inv.action = Action::Idle;
                    p.inv.zoom = 0;
                    let best = p.inv.best();
                    if !p.inv.select(best) {
                        p.inv.active = best;
                    }
                }
                p.kills_this_round = 0;
                p.spent = 0;
            }
        }
    }

    // ---- the round cycle ----------------------------------------------------------------------------------------

    fn start_match(&mut self) {
        self.score = [0, 0];
        self.loss_streak = [0, 0];
        self.round = 0;
        self.halftime_done = false;
        self.winner = None;
        for p in &mut self.players {
            p.money = self.cfg.start_money;
            p.stats = Stats::default();
            p.alive = false;
            p.armor = 0;
            p.helmet = false;
            p.defuser = false;
        }
        self.events.push(Event::WarmupEnd);
        self.start_round();
    }

    fn start_round(&mut self) {
        self.round += 1;
        self.dropped.clear();
        self.grenades.clear();
        self.smokes.clear();
        self.planted_this_round = false;
        self.last_result = None;
        self.spawn_round();
        self.phase = Phase::Freeze;
        self.phase_left = self.cfg.freeze;
        self.live_for = 0.0;
        self.bomb = Bomb::None;
        if self.cfg.mode == Mode::Defuse {
            let attackers: Vec<PlayerId> = self.players.iter().filter(|p| p.team == Team::Opfor && p.alive).map(|p| p.id).collect();
            if let Some(&c) = self.rng.pick(&attackers) {
                self.players[c].inv.bomb = true;
                self.bomb = Bomb::Carried(c);
            }
        }
        // Hostages back to where they are held, set down on the floor (from above, so furniture they overlap
        // doesn't trap them).
        let params = self.params;
        let world = self.level.clone();
        let settle = |at: Vec3| -> Vec3 {
            let ext = Vec3::new(params.half_width, params.stand_height * 0.5, params.half_width);
            let from = at + Vec3::Y * (ext.y + 0.5);
            let t = world.world.trace(from, from - Vec3::Y * 2.5, ext, crate::world::Contents::MASK_PLAYER);
            if !t.start_solid && t.fraction < 1.0 { t.end - Vec3::Y * ext.y } else { at }
        };
        self.hostages = self
            .level
            .entities_of("info_hostage")
            .map(|e| Hostage {
                mover: Mover { origin: settle(e.origin), on_ground: false, ..Default::default() },
                yaw: e.yaw,
                health: 100,
                alive: true,
                rescued: false,
                leader: None,
                start: e.origin,
                start_yaw: e.yaw,
                route: Vec::new(),
                repath: 0.0,
            })
            .collect();
        self.events.push(Event::RoundStart { round: self.round });
    }

    /// Ends the round: the winner's reward, the losers' bonus, the score, the MVP.
    pub fn end_round(&mut self, winner: Option<Team>, reason: Reason) {
        if self.phase != Phase::Live && self.phase != Phase::Freeze {
            return;
        }
        self.phase = Phase::Over;
        self.phase_left = self.cfg.round_end;
        self.last_result = Some((winner, reason));
        let mvp = winner.and_then(|w| {
            let special = match reason {
                Reason::BombDefused | Reason::BombExploded | Reason::HostagesRescued => None,
                _ => None,
            };
            special.or_else(|| self.players.iter().filter(|p| p.team == w).max_by_key(|p| (p.kills_this_round, p.alive as i32)).filter(|p| p.kills_this_round > 0).map(|p| p.id))
        });
        if let Some(m) = mvp {
            self.players[m].stats.mvps += 1;
            self.players[m].stats.score += 2;
        }
        if let Some(w) = winner {
            self.score[w.index()] += 1;
            let l = w.other();
            self.loss_streak[w.index()] = 0;
            let reward = match reason {
                Reason::BombExploded => money::WIN_BOMB,
                Reason::BombDefused => money::WIN_DEFUSE,
                Reason::TimeRanOut => money::WIN_TIME,
                Reason::HostagesRescued => money::WIN_RESCUE,
                _ => money::WIN_ELIMINATION,
            };
            let bonus = (money::LOSS + money::LOSS_STEP * self.loss_streak[l.index()] as i32).min(money::LOSS_MAX);
            self.loss_streak[l.index()] += 1;
            let plant = if l == Team::Opfor && self.planted_this_round { money::PLANT_TEAM } else { 0 };
            for i in 0..self.players.len() {
                let team = self.players[i].team;
                if team == w {
                    self.give_money(i, reward, "Rundensieg");
                } else {
                    self.give_money(i, bonus + plant, "Verlustbonus");
                }
            }
        }
        self.events.push(Event::RoundEnd { winner, reason, mvp });
    }

    pub fn give_money(&mut self, id: PlayerId, amount: i32, reason: &'static str) {
        let max = self.cfg.max_money;
        let p = &mut self.players[id];
        p.money = (p.money + amount).clamp(0, max);
        self.events.push(Event::Money { player: id, amount, reason: reason.into() });
    }

    fn next_after_round(&mut self) {
        let half = self.cfg.max_rounds / 2;
        let need = half + 1;
        let played = self.score[0] + self.score[1];
        let done = self.score[0] >= need || self.score[1] >= need || played >= self.cfg.max_rounds;
        if done {
            self.phase = Phase::MatchOver;
            self.phase_left = 15.0;
            self.winner = if self.score[0] > self.score[1] {
                Some(Team::Ksk)
            } else if self.score[1] > self.score[0] {
                Some(Team::Opfor)
            } else {
                None
            };
            self.events.push(Event::MatchEnd { winner: self.winner });
            return;
        }
        if !self.halftime_done && played == half {
            self.halftime_done = true;
            self.score.swap(0, 1);
            self.loss_streak = [0, 0];
            for p in &mut self.players {
                p.team = p.team.other();
                p.money = self.cfg.start_money;
                p.alive = false;
                p.armor = 0;
                p.helmet = false;
                p.defuser = false;
            }
            self.events.push(Event::Halftime);
        }
        self.start_round();
    }

    fn check_round_end(&mut self) {
        if self.phase != Phase::Live || self.cfg.mode == Mode::Deathmatch {
            return;
        }
        let (ksk, opfor) = (self.alive(Team::Ksk), self.alive(Team::Opfor));
        let has = |t: Team| self.players.iter().any(|p| p.team == t && !p.vacant);
        let planted = matches!(self.bomb, Bomb::Planted { .. });
        if has(Team::Ksk) && ksk == 0 {
            self.end_round(Some(Team::Opfor), Reason::Elimination);
        } else if has(Team::Opfor) && opfor == 0 && !planted {
            self.end_round(Some(Team::Ksk), Reason::Elimination);
        } else if self.cfg.mode == Mode::Rescue && !self.hostages.is_empty() {
            let left = self.hostages.iter().filter(|h| h.alive && !h.rescued).count();
            let rescued = self.hostages.iter().filter(|h| h.rescued).count();
            if left == 0 && rescued > 0 {
                self.end_round(Some(Team::Ksk), Reason::HostagesRescued);
            }
        }
    }

    // ---- the tick -----------------------------------------------------------------------------------------------

    /// One tick: every player's command, then the world (grenades, the bomb, hostages, dropped weapons), then the
    /// round's rules. Commands for players who aren't listed count as empty.
    pub fn tick(&mut self, cmds: &[(PlayerId, UserCmd)]) {
        self.time += TICK;
        self.ticks += 1;
        self.phase_tick();
        let empty = UserCmd::default();
        for id in 0..self.players.len() {
            let cmd = cmds.iter().rev().find(|(i, _)| *i == id).map(|(_, c)| c).unwrap_or(&empty);
            self.player_tick(id, cmd);
        }
        self.separate_players();
        crate::grenades::tick(self);
        self.dropped_tick();
        self.bomb_tick();
        self.hostages_tick();
        self.check_round_end();
        crate::lagcomp::record(self);
    }

    /// Online prediction: one player's part of a tick (moving, buying, the weapon), without the world's.
    pub fn predict_player(&mut self, id: PlayerId, cmd: &UserCmd) {
        self.player_tick(id, cmd);
    }

    fn phase_tick(&mut self) {
        self.phase_left -= TICK;
        match self.phase {
            Phase::Warmup => {
                // Respawns during warmup.
                self.dm_respawns();
                if self.phase_left <= 0.0 {
                    self.start_match();
                }
            }
            Phase::Freeze => {
                if self.phase_left <= 0.0 {
                    self.phase = Phase::Live;
                    self.phase_left = self.cfg.round_time;
                    self.events.push(Event::FreezeEnd);
                }
            }
            Phase::Live => {
                self.live_for += TICK;
                if self.cfg.mode == Mode::Deathmatch {
                    self.dm_respawns();
                    if self.phase_left <= 0.0 {
                        self.phase = Phase::MatchOver;
                        self.phase_left = 15.0;
                        let best = self.players.iter().max_by_key(|p| p.stats.kills).map(|p| p.team);
                        self.winner = best;
                        self.events.push(Event::MatchEnd { winner: best });
                    }
                } else if self.phase_left <= 0.0 && !matches!(self.bomb, Bomb::Planted { .. }) {
                    let winner = match self.cfg.mode {
                        Mode::Rescue => Team::Opfor,
                        _ => Team::Ksk,
                    };
                    self.end_round(Some(winner), Reason::TimeRanOut);
                }
            }
            Phase::Over => {
                if self.phase_left <= 0.0 {
                    self.next_after_round();
                }
            }
            Phase::MatchOver => {}
        }
    }

    fn dm_respawns(&mut self) {
        for id in 0..self.players.len() {
            let p = &mut self.players[id];
            if !p.alive {
                p.dead_for += TICK;
                if p.dead_for >= self.cfg.respawn {
                    self.respawn_dm(id);
                }
            }
        }
    }

    fn player_tick(&mut self, id: PlayerId, cmd: &UserCmd) {
        if !self.players[id].alive {
            self.players[id].dead_for += TICK;
            return;
        }
        let frozen = matches!(self.phase, Phase::Freeze) || matches!(self.phase, Phase::MatchOver);
        {
            let p = &mut self.players[id];
            p.yaw = cmd.yaw;
            p.pitch = cmd.pitch.clamp(-1.55, 1.55);
            p.noise += TICK;
            p.flash_left = (p.flash_left - TICK).max(0.0);
            p.tag = (p.tag + TICK * 0.9).min(1.0);
            // The recoil's punch recovers (the classic decay: 10°/s plus half of it per second).
            let len = p.punch.length();
            if len > 0.0 {
                let new = (len - (10.0 + len * 0.5) * TICK).max(0.0);
                p.punch *= new / len;
            }
        }
        // Buying.
        for &b in &cmd.buy {
            self.buy(id, b);
        }
        let pressed = cmd.buttons.0 & !self.players[id].prev_buttons.0;
        let pressed = Btn(pressed);
        // Switching weapons.
        if let Some(sel) = cmd.select {
            let p = &mut self.players[id];
            let before = p.inv.active;
            let changed = match sel {
                Select::Slot(s) => p.inv.select_slot(s),
                Select::Weapon(w) => p.inv.select(w),
                Select::Last => {
                    let last = p.inv.last;
                    p.inv.select(last)
                }
                Select::Next => p.inv.cycle(1),
                Select::Prev => p.inv.cycle(-1),
            };
            if changed && before != p.inv.active {
                let w = p.inv.active;
                self.events.push(Event::Draw { player: id, weapon: w });
            }
        }
        if pressed.contains(Btn::DROP) {
            self.drop_active(id);
        }
        // Using: defuse, hostages, swapping for a weapon on the floor.
        self.use_tick(id, cmd.buttons.contains(Btn::USE), pressed.contains(Btn::USE));
        // Moving.
        let busy_still = matches!(self.players[id].inv.action, Action::Planting(_)) || self.players[id].defusing.is_some();
        let (fwd, side) = if frozen || busy_still { (0.0, 0.0) } else { (cmd.forward, cmd.side) };
        let mut buttons = Buttons::empty();
        buttons.set(Buttons::JUMP, cmd.buttons.contains(Btn::JUMP) && !frozen && !busy_still);
        buttons.set(Buttons::DUCK, cmd.buttons.contains(Btn::DUCK));
        buttons.set(Buttons::WALK, cmd.buttons.contains(Btn::WALK));
        let max_speed = self.players[id].place_max_speed();
        let mc = MoveCmd { forward: fwd, side, yaw: cmd.yaw, pitch: cmd.pitch, buttons, max_speed };
        let level = self.level.clone();
        let params = self.params;
        let ev = {
            let p = &mut self.players[id];
            movement::step(&level.world, &LevelLadders(&level), &params, &mut p.mover, &mc, TICK)
        };
        self.movement_events(id, &ev, cmd);
        // Out of the world (a hole in a map): gone.
        if self.players[id].alive && self.players[id].mover.origin.y < self.level.world.bounds().min.y - 8.0 {
            let pos = self.players[id].mover.origin;
            crate::combat::damage(self, id, None, 1000.0, None, WeaponId::Knife, Vec3::NEG_Y, pos, 1.0, false);
        }
        // The weapon (online: with its own random draws, against everyone where the shooter saw them).
        let (rng, rewound) = crate::lagcomp::begin(self, id, cmd);
        crate::combat::weapon_tick(self, id, cmd.buttons, pressed);
        crate::lagcomp::end(self, rng, rewound);
        self.players[id].prev_buttons = cmd.buttons;
    }

    fn movement_events(&mut self, id: PlayerId, ev: &movement::MoveEvents, cmd: &UserCmd) {
        let pos = self.players[id].mover.origin;
        if ev.jumped {
            self.events.push(Event::Jump { player: id });
            self.noise(id, pos);
        }
        if let Some(speed) = ev.landed {
            if speed > 3.0 {
                self.events.push(Event::Land { player: id, pos, speed });
                self.noise(id, pos);
            }
        }
        if ev.fall_damage > 0.0 {
            let dmg = ev.fall_damage.round() as i32;
            self.events.push(Event::FallDamage { player: id, damage: dmg });
            crate::combat::damage(self, id, None, dmg as f32, None, WeaponId::Knife, Vec3::NEG_Y, pos, 1.0, false);
        }
        // Footsteps: running on the ground makes noise; walking and crouching are silent.
        let p = &mut self.players[id];
        let speed = p.mover.horizontal_speed();
        if p.mover.on_ground && !p.mover.noclip {
            p.stride += speed * TICK;
            let quiet = cmd.buttons.contains(Btn::WALK) || p.mover.ducked || speed < 150.0 * U;
            if p.stride > 1.9 {
                p.stride = 0.0;
                let material = p.mover.ground_surface.map_or(Material::Concrete, |s| Material::of(level_surface(&self.level, s)));
                self.events.push(Event::Footstep { player: id, pos, material, loud: !quiet });
                if !quiet {
                    self.noise(id, pos);
                }
            }
        }
    }

    /// Marks a noise enemies (bots) can hear.
    pub fn noise(&mut self, id: PlayerId, pos: Vec3) {
        let p = &mut self.players[id];
        p.noise = 0.0;
        p.last_noise_pos = pos;
    }

    /// Players don't walk through each other: overlapping bodies are pushed apart where there is room.
    fn separate_players(&mut self) {
        let w = self.params.half_width * 2.0;
        let n = self.players.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let (a, b) = (&self.players[i], &self.players[j]);
                if !a.alive || !b.alive {
                    continue;
                }
                let (pa, pb) = (a.mover.origin, b.mover.origin);
                if (pa.y - pb.y).abs() > 1.6 {
                    continue;
                }
                let d = Vec2::new(pb.x - pa.x, pb.z - pa.z);
                let (dx, dz) = (d.x.abs(), d.y.abs());
                if dx >= w || dz >= w {
                    continue;
                }
                // Push along the axis of least overlap.
                let push = if w - dx < w - dz { Vec3::new((w - dx) * 0.5 * d.x.signum().max(-1.0), 0.0, 0.0) } else { Vec3::new(0.0, 0.0, (w - dz) * 0.5 * d.y.signum()) };
                let push = if push.length_squared() < 1e-12 { Vec3::X * w * 0.5 } else { push };
                for (k, s) in [(i, -1.0f32), (j, 1.0f32)] {
                    let m = self.players[k].mover;
                    let ext = m.half_extents(&self.params);
                    let to = m.origin + push * s;
                    if !self.level.world.solid_at(to + Vec3::Y * ext.y, ext, crate::world::Contents::MASK_PLAYER) {
                        self.players[k].mover.origin = to;
                    }
                }
            }
        }
    }

    // ---- buying -------------------------------------------------------------------------------------------------

    /// Buys an item if the player may: in the buy zone and time, able to afford it, allowed for the side, with room.
    pub fn buy(&mut self, id: PlayerId, item: Buy) -> bool {
        if !self.can_buy(&self.players[id]) {
            return false;
        }
        let free = self.phase == Phase::Warmup || self.cfg.mode == Mode::Deathmatch;
        let p = &self.players[id];
        let mut price = item.price();
        let ok = match item {
            Buy::Weapon(w) => {
                let d = w.def();
                if !p.team.may_buy(d.side) || matches!(d.class, Class::Knife | Class::Bomb) {
                    false
                } else {
                    match w {
                        WeaponId::He => p.inv.he < MAX_HE,
                        WeaponId::Flash => p.inv.flash < MAX_FLASH,
                        WeaponId::Smoke => p.inv.smoke < MAX_SMOKE,
                        _ => !p.inv.has(w),
                    }
                }
            }
            Buy::Vest => p.armor < 100,
            Buy::VestHelmet => {
                if p.armor >= 100 && !p.helmet {
                    price = money::HELMET;
                }
                p.armor < 100 || !p.helmet
            }
            Buy::Defuser => p.team == Team::Ksk && !p.defuser && self.cfg.mode == Mode::Defuse,
        };
        if !ok || (!free && p.money < price) {
            return false;
        }
        if !free {
            let p = &mut self.players[id];
            p.money -= price;
            p.spent += price;
        }
        match item {
            Buy::Weapon(w) => {
                let slot = w.def().slot();
                match slot {
                    Slot::Primary | Slot::Secondary => {
                        // The one it replaces goes on the floor.
                        let old = if slot == Slot::Primary { self.players[id].inv.primary } else { self.players[id].inv.secondary };
                        if let Some(old) = old {
                            self.players[id].inv.remove(old.id);
                            self.spawn_dropped(id, old, 1.5);
                        }
                        let p = &mut self.players[id];
                        if slot == Slot::Primary {
                            p.inv.primary = Some(p.issue(w));
                        } else {
                            p.inv.secondary = Some(p.issue(w));
                        }
                        if p.inv.select(w) {
                            self.events.push(Event::Draw { player: id, weapon: w });
                        }
                    }
                    Slot::Grenade => {
                        let p = &mut self.players[id];
                        match w {
                            WeaponId::He => p.inv.he += 1,
                            WeaponId::Flash => p.inv.flash += 1,
                            _ => p.inv.smoke += 1,
                        }
                    }
                    _ => {}
                }
            }
            Buy::Vest => {
                self.players[id].armor = 100;
            }
            Buy::VestHelmet => {
                let p = &mut self.players[id];
                p.armor = 100;
                p.helmet = true;
            }
            Buy::Defuser => self.players[id].defuser = true,
        }
        self.events.push(Event::Bought { player: id, item });
        true
    }

    // ---- items on the floor -------------------------------------------------------------------------------------

    fn spawn_dropped(&mut self, id: PlayerId, gun: Gun, speed: f32) {
        let p = &self.players[id];
        let eye = p.eye(&self.params);
        let dir = dir_from(p.yaw, p.pitch.clamp(-0.5, 0.6));
        let vel = dir * speed * 2.5 + p.mover.velocity + Vec3::Y * 1.0;
        let yaw = p.yaw;
        let item = self.id();
        self.dropped.push(Dropped { id: item, gun, pos: eye - Vec3::Y * 0.3, vel, yaw, rest: false, by: Some(id), age: 0.0 });
        self.events.push(Event::WeaponDropped { player: id, item });
    }

    /// Drops the weapon in hand (guns and the bomb; not the knife or grenades).
    pub fn drop_active(&mut self, id: PlayerId) {
        let active = self.players[id].inv.active;
        let d = active.def();
        if active == WeaponId::Bomb {
            self.players[id].inv.remove(WeaponId::Bomb);
            let p = &self.players[id];
            let pos = p.eye(&self.params) - Vec3::Y * 0.3;
            let vel = dir_from(p.yaw, 0.2) * 3.0 + p.mover.velocity;
            self.bomb = Bomb::Dropped { pos, vel };
            self.events.push(Event::BombDropped { pos });
            return;
        }
        if !d.is_gun() || matches!(self.players[id].inv.action, Action::PinPulled { .. }) {
            return;
        }
        if let Some(g) = self.players[id].inv.remove(active) {
            self.spawn_dropped(id, g, 1.0);
        }
    }

    /// On death: the best gun and the bomb fall.
    fn drop_on_death(&mut self, id: PlayerId) {
        let inv = self.players[id].inv.clone();
        let pos = self.players[id].mover.origin + Vec3::Y * 0.9;
        if let Some(g) = inv.primary.or(inv.secondary) {
            let item = self.id();
            let vel = self.players[id].mover.velocity * 0.5 + Vec3::Y * 1.0;
            let yaw = self.players[id].yaw;
            self.dropped.push(Dropped { id: item, gun: g, pos, vel, yaw, rest: false, by: None, age: 0.0 });
            self.events.push(Event::WeaponDropped { player: id, item });
        }
        if inv.bomb {
            self.bomb = Bomb::Dropped { pos, vel: Vec3::Y * 0.5 };
            self.events.push(Event::BombDropped { pos });
        }
        let p = &mut self.players[id];
        p.inv = Inventory::default();
    }

    fn dropped_tick(&mut self) {
        let world = &self.level.world;
        let g = self.params.gravity;
        for d in &mut self.dropped {
            d.age += TICK;
            if d.rest {
                continue;
            }
            d.vel.y -= g * TICK;
            let to = d.pos + d.vel * TICK;
            let t = world.trace(d.pos, to, Vec3::splat(0.04), crate::world::Contents::MASK_GRENADE);
            d.pos = t.end;
            if t.hit() {
                if t.normal.y > 0.7 {
                    d.vel *= 0.3;
                    d.vel.y = 0.0;
                    if d.vel.length() < 0.3 {
                        d.rest = true;
                    }
                } else {
                    d.vel -= t.normal * d.vel.dot(t.normal) * 1.4;
                    d.vel *= 0.5;
                }
            }
            if d.pos.y < -200.0 {
                d.rest = true;
            }
        }
        // Walking over a gun picks it up if that slot is empty.
        let mut picked: Vec<(usize, PlayerId)> = Vec::new();
        for (i, d) in self.dropped.iter().enumerate() {
            if d.age < 0.4 {
                continue;
            }
            for p in &self.players {
                if !p.alive || (d.by == Some(p.id) && d.age < 1.5) {
                    continue;
                }
                let feet = p.mover.origin;
                let near = Vec2::new(d.pos.x - feet.x, d.pos.z - feet.z).length() < 0.7 && (d.pos.y - feet.y) > -0.5 && (d.pos.y - feet.y) < 1.2;
                if !near {
                    continue;
                }
                let slot = d.gun.id.def().slot();
                let empty = match slot {
                    Slot::Primary => p.inv.primary.is_none(),
                    Slot::Secondary => p.inv.secondary.is_none(),
                    _ => false,
                };
                if empty && p.team.may_buy(Side::Both) {
                    picked.push((i, p.id));
                    break;
                }
            }
        }
        picked.sort_by(|a, b| b.0.cmp(&a.0));
        let mut taken = Vec::new();
        for (i, pid) in picked {
            if taken.contains(&pid) {
                continue;
            }
            taken.push(pid);
            let d = self.dropped.remove(i);
            self.give_gun(pid, d.gun);
        }
        // The bomb on the floor.
        if let Bomb::Dropped { pos, vel } = self.bomb {
            let mut vel = vel;
            let mut pos = pos;
            vel.y -= g * TICK;
            let t = self.level.world.trace(pos, pos + vel * TICK, Vec3::splat(0.08), crate::world::Contents::MASK_GRENADE);
            pos = t.end;
            if t.hit() {
                vel = if t.normal.y > 0.7 { Vec3::ZERO } else { (vel - t.normal * vel.dot(t.normal) * 1.5) * 0.4 };
            }
            self.bomb = Bomb::Dropped { pos, vel };
            let carrier = self.players.iter().find(|p| {
                p.alive && p.team == Team::Opfor && Vec2::new(pos.x - p.mover.origin.x, pos.z - p.mover.origin.z).length() < 0.8 && (pos.y - p.mover.origin.y).abs() < 1.3
            });
            if let Some(c) = carrier.map(|p| p.id) {
                self.players[c].inv.bomb = true;
                self.bomb = Bomb::Carried(c);
                self.events.push(Event::BombPickup { player: c });
            }
        }
    }

    fn give_gun(&mut self, id: PlayerId, gun: Gun) {
        let slot = gun.id.def().slot();
        let p = &mut self.players[id];
        match slot {
            Slot::Primary => p.inv.primary = Some(gun),
            Slot::Secondary => p.inv.secondary = Some(gun),
            _ => return,
        }
        // A better gun than the one in hand goes straight into the hands.
        let rank = |w: WeaponId| match w.def().slot() {
            Slot::Primary => 3,
            Slot::Secondary => 2,
            _ => 1,
        };
        if rank(gun.id) > rank(p.inv.active) && !p.inv.busy() {
            p.inv.select(gun.id);
        }
        self.events.push(Event::Pickup { player: id, weapon: gun.id });
    }

    // ---- using: defusing, hostages, swapping weapons --------------------------------------------------------------

    fn use_tick(&mut self, id: PlayerId, held: bool, pressed: bool) {
        let team = self.players[id].team;
        let eye = self.eye(id);
        let look = dir_from(self.players[id].yaw, self.players[id].pitch);
        // Defusing the planted bomb.
        if let Bomb::Planted { pos, left, site, defuser, progress } = self.bomb {
            let close = eye.distance(pos) < 2.0 && self.players[id].mover.on_ground;
            if team == Team::Ksk {
                match defuser {
                    Some(d) if d == id => {
                        if !held || !close || !self.players[id].alive {
                            self.bomb = Bomb::Planted { pos, left, site, defuser: None, progress: 0.0 };
                            self.players[id].defusing = None;
                            self.events.push(Event::DefuseAbort { player: id });
                        } else {
                            let total = if self.players[id].defuser { self.cfg.defuse_kit_time } else { self.cfg.defuse_time };
                            let progress = progress + TICK;
                            self.players[id].defusing = Some(progress);
                            if progress >= total {
                                self.bomb = Bomb::Defused { pos };
                                self.players[id].defusing = None;
                                self.players[id].stats.score += 3;
                                self.give_money(id, money::DEFUSE_PLAYER, "Entschärft");
                                self.events.push(Event::Defused { player: id });
                                self.end_round(Some(Team::Ksk), Reason::BombDefused);
                            } else {
                                self.bomb = Bomb::Planted { pos, left, site, defuser, progress };
                            }
                        }
                        return;
                    }
                    None if held && close && self.phase == Phase::Live => {
                        self.bomb = Bomb::Planted { pos, left, site, defuser: Some(id), progress: 0.0 };
                        self.players[id].defusing = Some(0.0);
                        let kit = self.players[id].defuser;
                        self.events.push(Event::DefuseStart { player: id, kit });
                        return;
                    }
                    _ => {}
                }
            }
        }
        if !pressed {
            return;
        }
        // Hostages: KSK press E on one to lead it (again to let it wait).
        if team == Team::Ksk {
            let best = self
                .hostages
                .iter()
                .enumerate()
                .filter(|(_, h)| h.alive && !h.rescued)
                .map(|(i, h)| (i, h.mover.origin + Vec3::Y * 1.0))
                .filter(|(_, c)| c.distance(eye) < 2.2 && (*c - eye).normalize_or_zero().dot(look) > 0.6)
                .min_by(|a, b| a.1.distance(eye).total_cmp(&b.1.distance(eye)));
            if let Some((i, _)) = best {
                let h = &mut self.hostages[i];
                h.leader = if h.leader == Some(id) { None } else { Some(id) };
                h.route.clear();
                let leader = h.leader;
                self.events.push(Event::HostageFollow { hostage: i, player: leader });
                return;
            }
        }
        // A weapon on the floor in front: swap it for the one of its slot.
        let best = self
            .dropped
            .iter()
            .enumerate()
            .filter(|(_, d)| d.pos.distance(eye) < 2.0 && (d.pos - eye).normalize_or_zero().dot(look) > 0.8)
            .min_by(|a, b| a.1.pos.distance(eye).total_cmp(&b.1.pos.distance(eye)))
            .map(|(i, _)| i);
        if let Some(i) = best {
            let gun = self.dropped[i].gun;
            let slot = gun.id.def().slot();
            let old = match slot {
                Slot::Primary => self.players[id].inv.primary,
                Slot::Secondary => self.players[id].inv.secondary,
                _ => None,
            };
            self.dropped.remove(i);
            if let Some(o) = old {
                self.players[id].inv.remove(o.id);
                self.spawn_dropped(id, o, 1.0);
            }
            self.give_gun(id, gun);
            let p = &mut self.players[id];
            if p.inv.active != gun.id {
                p.inv.select(gun.id);
            }
        }
    }

    // ---- the bomb -------------------------------------------------------------------------------------------------

    /// Plants the bomb where the planter stands (called by `combat` when the plant finishes).
    pub(crate) fn plant(&mut self, id: PlayerId) {
        let pos = self.players[id].mover.origin;
        let Some(site) = self.site_at(pos) else { return };
        self.players[id].inv.remove(WeaponId::Bomb);
        self.bomb = Bomb::Planted { pos: pos + Vec3::Y * 0.05, site, left: self.cfg.bomb_timer, defuser: None, progress: 0.0 };
        self.planted_this_round = true;
        self.beep_left = 0.0;
        self.players[id].stats.score += 2;
        self.give_money(id, money::PLANT_PLAYER, "Gelegt");
        self.phase_left = self.phase_left.max(0.0);
        self.events.push(Event::Planted { player: id, pos, site });
    }

    fn bomb_tick(&mut self) {
        if let Bomb::Carried(c) = self.bomb {
            if !self.players[c].alive || !self.players[c].inv.bomb {
                // (Death and dropping set the bomb themselves; this is a safety net.)
                if !matches!(self.bomb, Bomb::Dropped { .. }) && !self.players[c].inv.bomb && !self.players[c].alive {
                    let pos = self.players[c].mover.origin + Vec3::Y * 0.3;
                    self.bomb = Bomb::Dropped { pos, vel: Vec3::ZERO };
                }
            }
        }
        let Bomb::Planted { pos, site, left, defuser, progress } = self.bomb else { return };
        if self.phase == Phase::MatchOver {
            return;
        }
        let left = left - TICK;
        // Beeps quicken as the timer runs down.
        self.beep_left -= TICK;
        if self.beep_left <= 0.0 {
            self.events.push(Event::Beep { pos, left });
            self.beep_left = (0.1 + 0.9 * (left / self.cfg.bomb_timer)).clamp(0.12, 1.0);
        }
        if left <= 0.0 {
            self.bomb = Bomb::Exploded { pos };
            self.events.push(Event::Exploded { pos });
            // Everyone near is caught in the blast.
            let radius = 1750.0 * U;
            for i in 0..self.players.len() {
                if !self.players[i].alive {
                    continue;
                }
                let d = self.players[i].hitboxes().distance(pos + Vec3::Y * 0.3);
                if d < radius {
                    let dmg = 500.0 * (1.0 - d / radius).powf(1.4);
                    crate::combat::damage(self, i, None, dmg, None, WeaponId::Bomb, (self.players[i].mover.origin - pos).normalize_or_zero(), pos, 1.0, false);
                }
            }
            if self.phase == Phase::Live || self.phase == Phase::Over {
                if self.phase == Phase::Over {
                    // Already decided (e.g. all KSK dead): the blast changes nothing.
                } else {
                    self.end_round(Some(Team::Opfor), Reason::BombExploded);
                }
            }
            return;
        }
        self.bomb = Bomb::Planted { pos, site, left, defuser, progress };
        // All defenders dead after the plant: the attackers have it.
        if self.phase == Phase::Live && self.alive(Team::Ksk) == 0 && self.players.iter().any(|p| p.team == Team::Ksk) {
            self.end_round(Some(Team::Opfor), Reason::Elimination);
        }
    }

    // ---- hostages -----------------------------------------------------------------------------------------------

    fn hostages_tick(&mut self) {
        let level = self.level.clone();
        let params = self.params;
        for i in 0..self.hostages.len() {
            let (alive, rescued, leader) = { let h = &self.hostages[i]; (h.alive, h.rescued, h.leader) };
            if !alive || rescued {
                continue;
            }
            let mut cmd = MoveCmd { max_speed: 230.0 * U, ..Default::default() };
            if let Some(l) = leader {
                if !self.players[l].alive || self.players[l].team != Team::Ksk {
                    self.hostages[i].leader = None;
                    self.events.push(Event::HostageFollow { hostage: i, player: None });
                } else {
                    let target_pos = self.players[l].mover.origin;
                    if self.hostages[i].repath <= 0.0
                        && let Some(nav) = self.nav.clone()
                    {
                        let from = self.hostages[i].mover.origin;
                        let route = match (nav.nearest(from), nav.nearest(target_pos)) {
                            (Some(a), Some(b)) => nav.route(a, b, &|_| 0.0),
                            _ => None,
                        };
                        let h = &mut self.hostages[i];
                        h.route = route.map(|r| r.iter().skip(1).map(|&k| nav.pos(k)).collect()).unwrap_or_default();
                        h.repath = 1.0;
                    }
                    let h = &mut self.hostages[i];
                    let next = h.route.first().copied().unwrap_or(target_pos);
                    if h.route.first().is_some_and(|w| Vec2::new(w.x - h.mover.origin.x, w.z - h.mover.origin.z).length() < 0.6) {
                        h.route.remove(0);
                    }
                    let to = next - h.mover.origin;
                    let dist = h.mover.origin.distance(target_pos);
                    if dist > 1.6 {
                        h.yaw = (-to.x).atan2(-to.z);
                        cmd.forward = 1.0;
                        cmd.yaw = h.yaw;
                        if dist > 4.0 {
                            cmd.max_speed = 250.0 * U;
                        }
                        // Jump small obstacles when stuck.
                        if h.mover.horizontal_speed() < 0.3 && h.mover.on_ground {
                            cmd.buttons = Buttons::JUMP;
                        }
                    }
                    h.repath -= TICK;
                }
            }
            let h = &mut self.hostages[i];
            movement::step(&level.world, &LevelLadders(&level), &params, &mut h.mover, &cmd, TICK);
            // Rescue.
            let at = h.mover.origin + Vec3::Y * 0.5;
            if level.volumes_of("func_hostage_rescue").any(|v| v.contains(at)) {
                h.rescued = true;
                let by = h.leader;
                h.leader = None;
                self.events.push(Event::HostageRescued { hostage: i, player: by });
                for p in 0..self.players.len() {
                    if self.players[p].team == Team::Ksk {
                        self.give_money(p, money::RESCUE_TEAM / 4, "Geisel gerettet");
                    }
                }
                if let Some(b) = by {
                    self.players[b].stats.score += 2;
                    self.give_money(b, money::RESCUE_PLAYER, "Geisel gerettet");
                }
            }
        }
    }

    /// Kills a player: the kill feed, the rewards, the dropped weapons.
    pub(crate) fn kill(&mut self, victim: PlayerId, killer: Option<PlayerId>, weapon: WeaponId, headshot: bool, wallbang: bool) {
        if !self.players[victim].alive {
            return;
        }
        self.players[victim].alive = false;
        self.players[victim].health = 0;
        self.players[victim].dead_for = 0.0;
        self.players[victim].stats.deaths += 1;
        self.players[victim].defusing = None;
        if let Bomb::Planted { pos, site, left, defuser: Some(d), .. } = self.bomb
            && d == victim
        {
            self.bomb = Bomb::Planted { pos, site, left, defuser: None, progress: 0.0 };
        }
        self.drop_on_death(victim);
        // Hostages following the victim wait.
        for (i, h) in self.hostages.iter_mut().enumerate() {
            if h.leader == Some(victim) {
                h.leader = None;
                self.events.push(Event::HostageFollow { hostage: i, player: None });
            }
        }
        // An assist: the most damage from someone else on the killer's side, at least 40.
        let assist = self.players[victim]
            .damage_from
            .iter()
            .enumerate()
            .filter(|(i, d)| Some(*i) != killer && **d >= 40.0 && *i != victim && self.players[*i].team != self.players[victim].team)
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i);
        if let Some(a) = assist {
            self.players[a].stats.assists += 1;
            self.players[a].stats.score += 1;
        }
        if let Some(k) = killer {
            if k == victim {
                self.players[k].stats.kills -= 1;
                self.players[k].stats.score -= 1;
            } else if self.players[k].team == self.players[victim].team {
                self.players[k].stats.kills -= 1;
                self.players[k].stats.score -= 1;
                self.give_money(k, money::TEAM_KILL, "Teamkill");
            } else {
                let p = &mut self.players[k];
                p.stats.kills += 1;
                p.stats.score += 2;
                p.kills_this_round += 1;
                if headshot {
                    p.stats.headshots += 1;
                }
                let reward = if self.phase == Phase::Warmup || self.cfg.mode == Mode::Deathmatch { 0 } else { weapon.def().kill_reward };
                if reward > 0 {
                    self.give_money(k, reward, "Abschuss");
                }
            }
        }
        self.events.push(Event::Kill { killer, victim, weapon, headshot, wallbang, assist });
    }
}

fn level_surface(level: &Level, s: u16) -> &str {
    level.world.surface_name(s)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{compile, map};

    /// A flat arena 64 m square with two spawn groups, a buy zone each, a bomb site and a wall.
    pub fn arena() -> Arc<Level> {
        let block = |lo: [f64; 3], hi: [f64; 3], tex: &str| crate::compile::tests_support::block(lo, hi, tex);
        let mut world = vec![block([-1280.0, -1280.0, -32.0], [1280.0, 1280.0, 0.0], "dev/floor")];
        // A 16-unit wooden wall between x = 300 and 316 (for penetration), y −200..200.
        world.push(block([300.0, -200.0, 0.0], [316.0, 200.0, 128.0], "mat/wood_crate"));
        // A thick concrete wall at x = −400..−272.
        world.push(block([-400.0, -200.0, 0.0], [-272.0, 200.0, 128.0], "mat/concrete_wall"));
        let mut ents = format!("{{\n\"classname\" \"worldspawn\"\n{}}}\n", world.concat());
        for i in 0..5 {
            ents += &format!("{{\n\"classname\" \"info_player_ksk\"\n\"origin\" \"{} 900 4\"\n\"angle\" \"270\"\n}}\n", -256 + i * 128);
            ents += &format!("{{\n\"classname\" \"info_player_opfor\"\n\"origin\" \"{} -900 4\"\n\"angle\" \"90\"\n}}\n", -256 + i * 128);
        }
        let vol = |class: &str, key: &str, val: &str, lo: [f64; 3], hi: [f64; 3]| format!("{{\n\"classname\" \"{class}\"\n\"{key}\" \"{val}\"\n{}}}\n", block(lo, hi, "tools/trigger"));
        ents += &vol("func_buyzone", "team", "ksk", [-600.0, 700.0, 0.0], [600.0, 1100.0, 128.0]);
        ents += &vol("func_buyzone", "team", "opfor", [-600.0, -1100.0, 0.0], [600.0, -700.0, 128.0]);
        ents += &vol("func_bomb_target", "site", "A", [-200.0, -200.0, 0.0], [200.0, 200.0, 128.0]);
        ents += "{\n\"classname\" \"info_hostage\"\n\"origin\" \"0 -300 4\"\n}\n";
        ents += &vol("func_hostage_rescue", "name", "x", [-600.0, 700.0, 0.0], [600.0, 1100.0, 128.0]);
        Arc::new(compile::compile(&map::parse(&ents).unwrap()))
    }

    pub fn game(mode: Mode) -> Game {
        let cfg = Config { mode, warmup: 0.0, ..Default::default() };
        let mut g = Game::new(arena(), cfg, 7);
        for i in 0..5 {
            g.add_player(&format!("K{i}"), Team::Ksk, true);
            g.add_player(&format!("O{i}"), Team::Opfor, true);
        }
        g
    }

    pub fn run(g: &mut Game, seconds: f32, cmds: &[(PlayerId, UserCmd)]) {
        for _ in 0..(seconds * TICK_HZ).round() as usize {
            g.tick(cmds);
        }
    }

    #[test]
    fn the_match_starts_with_a_freeze_and_everyone_spawned() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 0.1, &[]);
        assert_eq!(g.phase, Phase::Freeze);
        assert_eq!(g.round, 1);
        assert!(g.players.iter().all(|p| p.alive && p.health == 100 && p.money == 800));
        assert!(matches!(g.bomb, Bomb::Carried(c) if g.players[c].team == Team::Opfor));
        // On the floor, not falling.
        run(&mut g, 1.0, &[]);
        assert!(g.players.iter().all(|p| p.mover.on_ground), "{:?}", g.players.iter().map(|p| p.mover.origin).collect::<Vec<_>>());
        run(&mut g, 6.0, &[]);
        assert_eq!(g.phase, Phase::Live);
    }

    #[test]
    fn buying_in_the_zone_with_money() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 0.1, &[]);
        let k = 0;
        assert!(!g.buy(k, Buy::Weapon(WeaponId::Akm)), "not for KSK");
        assert!(!g.buy(k, Buy::Weapon(WeaponId::G95)), "too dear at $800");
        assert!(g.buy(k, Buy::Vest));
        assert_eq!(g.players[k].money, 150);
        assert!(!g.buy(k, Buy::Weapon(WeaponId::Flash)), "can't afford a flash with $150");
        assert!(!g.buy(k, Buy::Weapon(WeaponId::He)), "can't afford");
        g.players[k].money = 5000;
        assert!(g.buy(k, Buy::Weapon(WeaponId::G95)));
        assert_eq!(g.players[k].inv.active, WeaponId::G95);
        assert!(g.buy(k, Buy::Weapon(WeaponId::P30)));
        assert!(!g.dropped.is_empty(), "the old pistol lies on the floor");
    }

    #[test]
    fn elimination_ends_the_round_and_pays() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 7.0, &[]);
        for i in 0..g.players.len() {
            if g.players[i].team == Team::Opfor {
                g.kill(i, Some(0), WeaponId::P8, false, false);
            }
        }
        g.tick(&[]);
        assert_eq!(g.phase, Phase::Over);
        assert_eq!(g.score, [1, 0]);
        assert_eq!(g.players[0].stats.kills, 5);
        // 800 + 5 kills × 300 + 3250 win.
        assert_eq!(g.players[0].money, 800 + 1500 + 3250);
        let opfor = g.players.iter().find(|p| p.team == Team::Opfor).unwrap();
        assert_eq!(opfor.money, 800 + 1400);
        run(&mut g, 6.5, &[]);
        assert_eq!(g.round, 2);
        assert_eq!(g.phase, Phase::Freeze);
        assert!(g.players.iter().all(|p| p.alive));
    }

    #[test]
    fn the_bomb_is_planted_and_explodes() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 7.0, &[]);
        let Bomb::Carried(c) = g.bomb else { panic!() };
        // Teleport the carrier onto site A and hold the trigger with the bomb out.
        g.players[c].mover.origin = Vec3::new(0.0, 0.0, 0.0);
        let sel = UserCmd { select: Some(Select::Weapon(WeaponId::Bomb)), ..Default::default() };
        g.tick(&[(c, sel)]);
        run(&mut g, 1.2, &[]);
        let plant = UserCmd { buttons: Btn::ATTACK, ..Default::default() };
        run(&mut g, 3.3, &[(c, plant)]);
        assert!(matches!(g.bomb, Bomb::Planted { site: 'A', .. }), "{:?}", g.bomb);
        assert!(g.events.iter().any(|e| matches!(e, Event::Planted { .. })));
        run(&mut g, 41.0, &[]);
        assert!(matches!(g.bomb, Bomb::Exploded { .. }));
        assert_eq!(g.score, [0, 1]);
        // The losers get the plant bonus... the winners are the attackers here; KSK lost with no plant bonus.
    }

    #[test]
    fn defusing_with_a_kit_takes_five_seconds() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 7.0, &[]);
        let Bomb::Carried(c) = g.bomb else { panic!() };
        g.players[c].mover.origin = Vec3::ZERO;
        g.plant_for_test(c);
        let k = 0;
        g.players[k].defuser = true;
        g.players[k].mover.origin = Vec3::new(0.8, 0.0, 0.0);
        let hold = UserCmd { buttons: Btn::USE, pitch: -1.0, ..Default::default() };
        run(&mut g, 4.5, &[(k, hold.clone())]);
        assert!(matches!(g.bomb, Bomb::Planted { .. }));
        run(&mut g, 0.7, &[(k, hold)]);
        assert!(matches!(g.bomb, Bomb::Defused { .. }), "{:?}", g.bomb);
        assert_eq!(g.score, [1, 0]);
    }

    #[test]
    fn loss_bonus_grows() {
        let mut g = game(Mode::Defuse);
        for round in 0..3 {
            run(&mut g, 7.0, &[]);
            let before = g.players.iter().find(|p| p.team == Team::Opfor).unwrap().money;
            g.end_round(Some(Team::Ksk), Reason::TimeRanOut);
            let after = g.players.iter().find(|p| p.team == Team::Opfor).unwrap().money;
            assert_eq!(after - before, 1400 + 500 * round);
            run(&mut g, 6.5, &[]);
        }
    }

    #[test]
    fn halftime_switches_sides() {
        let mut g = game(Mode::Defuse);
        g.cfg.max_rounds = 4;
        for _ in 0..2 {
            run(&mut g, 7.0, &[]);
            g.end_round(Some(Team::Ksk), Reason::TimeRanOut);
            run(&mut g, 6.5, &[]);
        }
        assert_eq!(g.players[0].team, Team::Opfor, "player 0 switched");
        assert_eq!(g.score, [0, 2], "the score follows the sides");
        assert_eq!(g.players[0].money, 800);
        run(&mut g, 7.0, &[]);
        g.end_round(Some(Team::Opfor), Reason::Elimination);
        run(&mut g, 6.5, &[]);
        assert_eq!(g.phase, Phase::MatchOver);
        assert_eq!(g.winner, Some(Team::Opfor));
    }

    #[test]
    fn hostages_follow_and_are_rescued() {
        let mut g = game(Mode::Rescue);
        run(&mut g, 7.0, &[]);
        assert_eq!(g.hostages.len(), 1);
        let k = 0;
        // Stand next to the hostage and look at it.
        let h = g.hostages[0].mover.origin;
        g.players[k].mover.origin = h + Vec3::new(0.0, 0.0, 1.2);
        let press = UserCmd { buttons: Btn::USE, pitch: -0.3, yaw: 0.0, ..Default::default() };
        g.tick(&[(k, press)]);
        assert_eq!(g.hostages[0].leader, Some(k));
        // Walk to the rescue zone (towards map +y = sim −z) and the hostage comes along.
        for _ in 0..(20.0 * TICK_HZ) as usize {
            let p = g.players[k].mover.origin;
            let yaw = 0.0; // looking down −Z
            let cmd = UserCmd { forward: if p.z > -22.0 { 1.0 } else { 0.0 }, yaw, ..Default::default() };
            g.tick(&[(k, cmd)]);
            if g.phase == Phase::Over {
                break;
            }
        }
        assert!(g.hostages[0].rescued, "hostage at {:?}", g.hostages[0].mover.origin);
        assert_eq!(g.last_result, Some((Some(Team::Ksk), Reason::HostagesRescued)));
    }

    #[test]
    fn deathmatch_respawns() {
        let cfg = Config { mode: Mode::Deathmatch, ..Default::default() };
        let mut g = Game::new(arena(), cfg, 3);
        let a = g.add_player("a", Team::Ksk, false);
        let b = g.add_player("b", Team::Opfor, true);
        assert!(g.players[a].alive && g.players[b].alive);
        g.kill(b, Some(a), WeaponId::G95, true, false);
        run(&mut g, 2.5, &[]);
        assert!(g.players[b].alive);
        assert_eq!(g.players[a].stats.kills, 1);
    }

    impl Game {
        pub fn plant_for_test(&mut self, id: PlayerId) {
            self.plant(id);
        }
    }
}
