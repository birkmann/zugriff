//! Bots: each tick they look at the match the way a player would and answer with a [`UserCmd`] — the same input a
//! human gives, so they obey the same movement, weapons and rules.
//!
//! - **Team plans** each round: the attackers pick a site (and maybe send a lurker to the other), the defenders split
//!   over the sites with a rotator; after the plant everyone converges on the bomb; in hostage rescue the defenders
//!   go for the hostages and the attackers guard them. Warmup and deathmatch: hunt.
//! - **Perception**: a field of view, lines of sight to head and chest (smoke and walls block, flashes blind), hearing
//!   footsteps and shots; what one bot sees, its team hears about (and it says so on the radio).
//! - **Aim**: a reaction time before the first shot, an aim error that shrinks while tracking, a sway that never
//!   settles, how far off the aim may be when pulling the trigger, a turn rate, recoil control, a preference for head
//!   or chest — all by difficulty, so that Easy misses a lot and reacts late, and Expert rarely does either
//!   (`examples/aim_duel` measures it).
//! - **Fire discipline**: taps and bursts at range, sprays up close, stopping to shoot rifles, strafing between
//!   bursts, scoping snipers, closing in with shotguns and knives, reloading behind cover.
//! - **Buying** by the economy (pistol round, eco, force, full buy), grenades thrown onto sites and at heard enemies.

use crate::game::{dir_from, Bomb, Btn, Buy, Event, Game, Mode, Phase, PlayerId, Select, Team, UserCmd};
use crate::nav::{flags, LinkKind, Nav};
use crate::rng::Rng;
use crate::weapons::{Action, Class, Slot, WeaponId};
use glam::{Vec2, Vec3};
use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
    Expert,
}

impl Difficulty {
    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Easy => "Leicht",
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Schwer",
            Difficulty::Expert => "Experte",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Skill {
    /// Seconds from seeing someone to the first shot (min, max), never less than `react_floor` whatever the bonuses.
    reaction: (f32, f32),
    react_floor: f32,
    /// Reaction factor when standing and watching (the better, the more it helps).
    alert: f32,
    /// Aim sway that never settles (rad), and how far off the aim may be when firing (× the target's size).
    sway: f32,
    trigger: f32,
    /// Fastest turn (rad/s) and how quickly the aim settles (1/s).
    turn: f32,
    settle: f32,
    /// Initial aim error (rad) and how fast it shrinks while tracking (1/s).
    error: f32,
    focus: f32,
    head: f32,
    /// How much of the recoil's punch is pulled down (0..1).
    recoil: f32,
    /// Strafes between bursts, crouches in long sprays.
    strafe: bool,
    crouch: f32,
    /// Half-angle of the field of view (rad).
    fov: f32,
    grenades: f32,
}

fn skill(d: Difficulty, rng: &mut Rng) -> Skill {
    let j = |rng: &mut Rng, v: f32| v * rng.range(0.85, 1.15);
    match d {
        Difficulty::Easy => Skill { reaction: (0.8, 1.2), react_floor: 0.65, alert: 0.95, sway: j(rng, 0.05), trigger: 2.4, turn: j(rng, 2.6), settle: 4.5, error: j(rng, 0.16), focus: 0.8, head: 0.03, recoil: 0.1, strafe: false, crouch: 0.0, fov: 0.8, grenades: 0.05 },
        Difficulty::Normal => Skill { reaction: (0.5, 0.75), react_floor: 0.4, alert: 0.85, sway: j(rng, 0.024), trigger: 1.5, turn: j(rng, 5.0), settle: 8.0, error: j(rng, 0.09), focus: 1.6, head: 0.15, recoil: 0.45, strafe: true, crouch: 0.3, fov: 1.0, grenades: 0.3 },
        Difficulty::Hard => Skill { reaction: (0.3, 0.46), react_floor: 0.24, alert: 0.74, sway: j(rng, 0.011), trigger: 1.15, turn: j(rng, 8.5), settle: 12.0, error: j(rng, 0.05), focus: 2.8, head: 0.38, recoil: 0.72, strafe: true, crouch: 0.5, fov: 1.1, grenades: 0.6 },
        Difficulty::Expert => Skill { reaction: (0.17, 0.3), react_floor: 0.15, alert: 0.64, sway: j(rng, 0.004), trigger: 1.0, turn: j(rng, 14.0), settle: 18.0, error: j(rng, 0.025), focus: 4.5, head: 0.62, recoil: 0.9, strafe: true, crouch: 0.6, fov: 1.2, grenades: 0.85 },
    }
}

/// Names for bots, by side.
pub const NAMES_KSK: &[&str] = &["Jonas", "Lukas", "Felix", "Maximilian", "Tobias", "Florian", "Sebastian", "Niklas", "Moritz", "Tim"];
pub const NAMES_OPFOR: &[&str] = &["Dmitri", "Kirill", "Artjom", "Wadim", "Ruslan", "Timur", "Gleb", "Oleg", "Pawel", "Stas"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Goal {
    Idle,
    Hunt,
    /// Go to a site (attack), carrying or escorting the bomb.
    Attack(char),
    /// Hold a spot at a site.
    Defend(char),
    /// Hold near the planted bomb.
    GuardBomb,
    /// Go to the planted bomb and defuse it.
    Retake,
    /// Pick up the dropped bomb.
    GetBomb,
    /// Go and lead a hostage out.
    Rescue(usize),
    /// Lead the hostages to the rescue zone.
    Escort,
    /// Stay near the hostages.
    GuardHostages,
    /// Follow a player (radio: follow me).
    Follow(PlayerId),
    /// Hold a position (radio).
    Hold,
}

#[derive(Clone, Debug)]
struct Brain {
    skill: Skill,
    goal: Goal,
    dest: Option<Vec3>,
    /// Where to look while holding.
    watch: Option<Vec3>,
    path: Vec<u32>,
    path_i: usize,
    repath: f32,
    target: Option<PlayerId>,
    visible: bool,
    react: f32,
    lost: f32,
    last_seen: Option<Vec3>,
    aim_point_head: bool,
    err: Vec2,
    yaw: f32,
    pitch: f32,
    think: f32,
    burst_left: i32,
    pause: f32,
    strafe: f32,
    strafe_t: f32,
    stuck: f32,
    last_pos: Vec3,
    heard: Option<(Vec3, f32)>,
    /// Something to turn to at once: where, when (game time), and whether it was a hit taken (else a shot close by
    /// or a team-mate hit). Stronger than `heard`.
    alarm: Option<(Vec3, f32, bool)>,
    /// The game time of the last hit already reacted to.
    hurt_seen: f32,
    bought: u32,
    buy_at: f32,
    radio: f32,
    press: bool,
    scope_wait: f32,
    nade: Option<(WeaponId, Vec3, f32)>,
    nade_done: u32,
    hold_t: f32,
    scan: f32,
    unstuck: f32,
    /// Standing where it is for a moment (waiting for led hostages), route kept.
    wait: f32,
    /// Seconds until a hunter may pick a new destination from a sighting.
    retarget: f32,
    /// Seconds until a goal whose route failed tries again.
    retry: f32,
    /// The sway's phase (differs per bot).
    sway_ph: f32,
    /// In a fight: a covered spot to fight from (hidden crouched, able to shoot standing) and seconds left to reach it;
    /// seconds until cover is looked for again; whether holding crouched, re-decided when `duck_t` runs out; a single
    /// side-step under way (seconds, direction) and seconds until another may start.
    cover: Option<Vec3>,
    cover_t: f32,
    plan_t: f32,
    duck: bool,
    duck_t: f32,
    duck_target: Option<PlayerId>,
    step_t: f32,
    step_dir: f32,
    step_wait: f32,
}

impl Brain {
    fn new(skill: Skill, rng: &mut Rng) -> Brain {
        Brain {
            skill,
            goal: Goal::Idle,
            dest: None,
            watch: None,
            path: Vec::new(),
            path_i: 0,
            repath: 0.0,
            target: None,
            visible: false,
            react: 0.0,
            lost: 0.0,
            last_seen: None,
            aim_point_head: false,
            err: Vec2::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            think: rng.range(0.0, 0.1),
            burst_left: 0,
            pause: 0.0,
            strafe: 1.0,
            strafe_t: 0.0,
            stuck: 0.0,
            last_pos: Vec3::ZERO,
            heard: None,
            alarm: None,
            hurt_seen: -1.0,
            bought: 0,
            buy_at: rng.range(0.3, 2.0),
            radio: 0.0,
            press: false,
            scope_wait: 0.0,
            nade: None,
            nade_done: 0,
            hold_t: 0.0,
            scan: rng.range(0.0, 6.0),
            unstuck: 0.0,
            wait: 0.0,
            retarget: 0.0,
            retry: 0.0,
            sway_ph: rng.range(0.0, 6.28),
            cover: None,
            cover_t: 0.0,
            plan_t: 0.0,
            duck: false,
            duck_t: 0.0,
            duck_target: None,
            step_t: 0.0,
            step_dir: 1.0,
            step_wait: 0.0,
        }
    }
}

/// An order from the player's radio to their bot team-mates.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Order {
    FollowMe(PlayerId),
    Hold(PlayerId),
    Regroup(PlayerId),
    GoSite(char),
    Storm,
}

/// Every bot of the match.
pub struct Bots {
    pub nav: Arc<Nav>,
    pub difficulty: Difficulty,
    brains: HashMap<PlayerId, Brain>,
    rng: Rng,
    round: u32,
    /// Attackers' site this round.
    plan_site: char,
    /// Where each side last saw enemies (pos, game time).
    /// Where each side saw enemies: place, last time, and how often (routes steer round well-watched places).
    sightings: [Vec<(Vec3, f32, f32)>; 2],
    orders: Vec<(Team, Order)>,
    /// Hand-off: radio lines to emit.
    pub radio: Vec<(PlayerId, String)>,
    was_planted: bool,
    /// The sites' centres (A, B), and which sites the defenders already rotated to this round.
    sites: [Option<Vec3>; 2],
    rotated: [u8; 2],
    /// A side's radio is quiet until then (game time), so sightings aren't called five times over.
    team_radio: [f32; 2],
    /// Attackers the defenders have seen near each site: who, when.
    contacts: [Vec<(PlayerId, f32)>; 2],
}

const DT: f32 = crate::game::TICK;

/// Other bodies on the way (players push each other apart, and on stairs or in a door that can shove the one in front
/// backwards): queue behind a team-mate going the same way, steer round one standing or coming the other way — head-on,
/// both keep to their own side. Returns the direction to move (shorter than 1 to slow down) and whether someone is close
/// in front.
fn make_way(g: &Game, id: PlayerId, dir: Vec2) -> (Vec2, bool) {
    let p = &g.players[id];
    let here = Vec2::new(p.mover.origin.x, p.mover.origin.z);
    let left = Vec2::new(dir.y, -dir.x);
    let mut out = dir;
    let mut slow: f32 = 1.0;
    let mut busy = false;
    for q in g.players.iter().filter(|q| q.alive && q.id != id && (q.mover.origin.y - p.mover.origin.y).abs() < 1.6) {
        let to = Vec2::new(q.mover.origin.x, q.mover.origin.z) - here;
        let dist = to.length();
        if !(1e-3..1.7).contains(&dist) {
            continue;
        }
        let ahead = to.dot(dir) / dist;
        if ahead < 0.25 {
            continue;
        }
        busy |= dist < 1.1;
        let v = Vec2::new(q.mover.velocity.x, q.mover.velocity.z);
        let same_way = v.length() > 0.8 && v.normalize().dot(dir) > 0.5;
        if same_way && q.team == p.team {
            // Queue: keep a gap rather than pushing.
            slow = slow.min(((dist - 0.8) / 0.6).clamp(0.0, 1.0));
            continue;
        }
        // Round it, away from the side it is on (dead ahead: both keep to their right).
        let lateral = to.dot(left);
        let away = if lateral.abs() < 0.12 { -1.0 } else { -lateral.signum() };
        let weight = ((1.7 - dist) / 0.9).clamp(0.0, 1.0) * ahead;
        out += left * away * weight * 1.6;
    }
    let out = out.normalize_or_zero() * slow;
    (out, busy)
}

fn wrap(a: f32) -> f32 {
    let mut a = (a + PI).rem_euclid(2.0 * PI) - PI;
    if a <= -PI {
        a += 2.0 * PI;
    }
    a
}

fn angles_to(from: Vec3, to: Vec3) -> (f32, f32) {
    let d = to - from;
    let yaw = (-d.x).atan2(-d.z);
    let pitch = (d.y / d.length().max(1e-4)).clamp(-1.0, 1.0).asin();
    (yaw, pitch)
}

impl Bots {
    pub fn new(nav: Arc<Nav>, difficulty: Difficulty, seed: u64) -> Bots {
        Bots {
            nav,
            difficulty,
            brains: HashMap::new(),
            rng: Rng::new(seed ^ 0xb075),
            round: 0,
            plan_site: 'A',
            sightings: [Vec::new(), Vec::new()],
            orders: Vec::new(),
            radio: Vec::new(),
            was_planted: false,
            sites: [None, None],
            rotated: [0, 0],
            team_radio: [0.0, 0.0],
            contacts: [Vec::new(), Vec::new()],
        }
    }

    /// Forgets a player's mind (online: a person took the slot over, or handed it back).
    pub fn forget(&mut self, id: PlayerId) {
        self.brains.remove(&id);
    }

    pub fn order(&mut self, team: Team, order: Order) {
        self.orders.push((team, order));
    }

    /// A name for a new bot on `team` that isn't taken.
    pub fn name_for(g: &Game, team: Team, rng: &mut Rng) -> String {
        let list = if team == Team::Ksk { NAMES_KSK } else { NAMES_OPFOR };
        let free: Vec<&&str> = list.iter().filter(|n| !g.players.iter().any(|p| p.name == **n)).collect();
        rng.pick(&free).map_or_else(|| format!("Bot {}", g.players.len() + 1), |n| n.to_string())
    }

    /// The bots' commands for this tick.
    pub fn think(&mut self, g: &Game) -> Vec<(PlayerId, UserCmd)> {
        // New round: plans, and fresh minds.
        if g.round != self.round && matches!(g.phase, Phase::Freeze) {
            self.round = g.round;
            self.new_round(g);
        }
        let planted = matches!(g.bomb, Bomb::Planted { .. });
        if planted && !self.was_planted {
            self.after_plant(g);
        }
        self.was_planted = planted;
        self.apply_orders(g);
        self.rotate(g);
        let now = g.time;
        for s in &mut self.sightings {
            s.retain(|(_, t, _)| now - *t < 8.0);
        }
        let ids: Vec<PlayerId> = g.players.iter().filter(|p| p.bot).map(|p| p.id).collect();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let mut brain = match self.brains.remove(&id) {
                Some(b) => b,
                None => {
                    let s = skill(self.difficulty, &mut self.rng);
                    let mut b = Brain::new(s, &mut self.rng);
                    b.yaw = g.players[id].yaw;
                    b.pitch = g.players[id].pitch;
                    b
                }
            };
            let cmd = self.bot(g, id, &mut brain);
            self.brains.insert(id, brain);
            out.push((id, cmd));
        }
        out
    }

    /// Defenders rotate in two stages: the first attacker seen near a site sends the nearest defender of the other
    /// site; once two are seen (the push is real), another follows. One anchor always stays on the other site.
    fn rotate(&mut self, g: &Game) {
        if g.cfg.mode != Mode::Defuse || g.phase != Phase::Live || matches!(g.bomb, Bomb::Planted { .. }) {
            return;
        }
        let now = g.time;
        for (si, site) in ['A', 'B'].into_iter().enumerate() {
            let Some(centre) = self.sites[si] else { continue };
            let seen = self.contacts[si].iter().filter(|(_, t)| now - t < 3.0).count() as u8;
            let stage = seen.min(2);
            if stage <= self.rotated[si] {
                continue;
            }
            let other = if site == 'A' { 'B' } else { 'A' };
            let mut helpers: Vec<(f32, PlayerId)> = g
                .players
                .iter()
                .filter(|p| p.bot && p.alive && p.team == Team::Ksk && self.brains.get(&p.id).is_some_and(|b| b.goal == Goal::Defend(other)))
                .map(|p| (p.mover.origin.distance(centre), p.id))
                .collect();
            if helpers.len() <= 1 {
                continue;
            }
            helpers.sort_by(|a, b| a.0.total_cmp(&b.0));
            let send = (stage - self.rotated[si]) as usize;
            self.rotated[si] = stage;
            for (i, (_, id)) in helpers.into_iter().take(send).enumerate() {
                self.set_goal(id, Goal::Defend(site));
                if i == 0 {
                    self.radio.push((id, format!("Rotiere nach {site}!")));
                }
            }
        }
    }

    fn new_round(&mut self, g: &Game) {
        self.rotated = [0, 0];
        self.contacts = [Vec::new(), Vec::new()];
        for (i, mask) in [flags::SITE_A, flags::SITE_B].into_iter().enumerate() {
            let nodes: Vec<Vec3> = self.nav.with_flags(mask).map(|n| self.nav.pos(n)).collect();
            self.sites[i] = (!nodes.is_empty()).then(|| nodes.iter().sum::<Vec3>() / nodes.len() as f32);
        }
        self.plan_site = if self.rng.chance(0.5) { 'A' } else { 'B' };
        self.sightings = [Vec::new(), Vec::new()];
        self.was_planted = false;
        let mut defenders: Vec<PlayerId> = g.players.iter().filter(|p| p.bot && p.team == Team::Ksk).map(|p| p.id).collect();
        let mut attackers: Vec<PlayerId> = g.players.iter().filter(|p| p.bot && p.team == Team::Opfor).map(|p| p.id).collect();
        self.rng.shuffle(&mut defenders);
        self.rng.shuffle(&mut attackers);
        for b in self.brains.values_mut() {
            b.path.clear();
            b.target = None;
            b.last_seen = None;
            b.heard = None;
            b.alarm = None;
            b.nade = None;
            b.dest = None;
            b.watch = None;
            b.buy_at = self.rng.range(0.3, 2.5);
        }
        let lurk = attackers.len() >= 4 && self.rng.chance(0.4);
        let site = self.plan_site;
        let other = if site == 'A' { 'B' } else { 'A' };
        let mode = g.cfg.mode;
        for (i, id) in defenders.iter().enumerate() {
            let goal = match mode {
                Mode::Defuse => Goal::Defend(if i % 2 == 0 { 'A' } else { 'B' }),
                Mode::Rescue => {
                    if i < 3 {
                        Goal::Rescue(i)
                    } else {
                        Goal::Hunt
                    }
                }
                Mode::Deathmatch => Goal::Hunt,
            };
            self.set_goal(*id, goal);
        }
        for (i, id) in attackers.iter().enumerate() {
            let goal = match mode {
                Mode::Defuse => Goal::Attack(if lurk && i == attackers.len() - 1 { other } else { site }),
                // Holding the hostages together (one roams to catch rescuers on the way).
                Mode::Rescue => {
                    if i < 4 {
                        Goal::GuardHostages
                    } else {
                        Goal::Hunt
                    }
                }
                Mode::Deathmatch => Goal::Hunt,
            };
            self.set_goal(*id, goal);
        }
    }

    fn after_plant(&mut self, g: &Game) {
        for p in g.players.iter().filter(|p| p.bot && p.alive) {
            let goal = if p.team == Team::Ksk { Goal::Retake } else { Goal::GuardBomb };
            self.set_goal(p.id, goal);
        }
    }

    fn set_goal(&mut self, id: PlayerId, goal: Goal) {
        let b = self.brains.entry(id).or_insert_with(|| {
            let s = skill(self.difficulty, &mut self.rng);
            Brain::new(s, &mut self.rng)
        });
        if b.goal != goal {
            b.goal = goal;
            b.dest = None;
            b.watch = None;
            b.path.clear();
        }
    }

    fn apply_orders(&mut self, g: &Game) {
        let orders = std::mem::take(&mut self.orders);
        for (team, order) in orders {
            let ids: Vec<PlayerId> = g.players.iter().filter(|p| p.bot && p.alive && p.team == team).map(|p| p.id).collect();
            for id in ids {
                let goal = match order {
                    Order::FollowMe(p) | Order::Regroup(p) => Goal::Follow(p),
                    Order::Hold(_) => Goal::Hold,
                    Order::GoSite(s) => {
                        if team == Team::Opfor {
                            Goal::Attack(s)
                        } else {
                            Goal::Defend(s)
                        }
                    }
                    Order::Storm => Goal::Hunt,
                };
                self.set_goal(id, goal);
                if let (Order::Hold(_), Some(b)) = (order, self.brains.get_mut(&id)) {
                    b.dest = Some(g.players[id].mover.origin);
                }
                if self.rng.chance(0.5) {
                    self.radio.push((id, "Verstanden!".into()));
                }
            }
        }
    }

    // ---- one bot ------------------------------------------------------------------------------------------------

    fn bot(&mut self, g: &Game, id: PlayerId, b: &mut Brain) -> UserCmd {
        let p = &g.players[id];
        let mut cmd = UserCmd { yaw: b.yaw, pitch: b.pitch, ..Default::default() };
        if !p.alive {
            b.target = None;
            b.path.clear();
            return cmd;
        }
        if matches!(g.phase, Phase::Freeze | Phase::Warmup) || g.cfg.mode == Mode::Deathmatch {
            self.buy(g, id, b, &mut cmd);
        }
        if matches!(g.phase, Phase::MatchOver) {
            return cmd;
        }
        let eye = g.eye(id);
        b.think -= DT;
        b.react -= DT;
        b.pause -= DT;
        b.strafe_t -= DT;
        b.repath -= DT;
        b.radio -= DT;
        b.scope_wait -= DT;
        b.unstuck -= DT;
        b.retarget -= DT;
        b.retry -= DT;
        b.scan += DT;
        b.cover_t -= DT;
        b.plan_t -= DT;
        b.duck_t -= DT;
        b.step_t -= DT;
        b.step_wait -= DT;
        let flashed = p.flash_left > 0.6;

        // Perception, ten times a second.
        if b.think <= 0.0 {
            b.think = 0.1;
            self.perceive(g, id, b, eye, flashed);
        }
        // The goal decides where to go.
        self.update_goal(g, id, b);

        // Aim and fire at a target, else move and look where it matters.
        let engaged = b.target.is_some_and(|t| g.players[t].alive) && b.visible && !flashed;
        let mut want_move: Option<Vec3> = None;
        let mut look_at: Option<Vec3> = None;
        let mut walk = false;
        if engaged {
            let t = b.target.unwrap();
            self.fight(g, id, b, t, eye, &mut cmd, &mut want_move);
        } else {
            b.burst_left = 0;
            // Out of the fight: the fight's cover and crouch are done with.
            b.cover = None;
            b.duck = false;
            b.duck_t = 0.0;
            b.duck_target = None;
            // Reload in a quiet moment.
            let inv = &p.inv;
            if let Some(gun) = inv.gun()
                && gun.clip < (inv.def().clip as f32 * 0.4) as u16
                && gun.reserve > 0
                && !inv.busy()
            {
                cmd.buttons |= Btn::RELOAD;
            }
            // Out of rounds entirely: take the pistol, or the knife.
            if inv.gun().is_some_and(|g| g.clip == 0 && g.reserve == 0) {
                let sel = if inv.active.def().slot() == Slot::Primary && inv.secondary.is_some_and(|g| g.clip + g.reserve > 0) { Slot::Secondary } else { Slot::Knife };
                cmd.select = Some(Select::Slot(sel));
            }
            // A grenade plan in progress.
            if self.grenade(g, id, b, eye, &mut cmd) {
                return cmd;
            }
            // Special actions at the goal: plant, defuse, lead a hostage.
            if self.goal_action(g, id, b, &mut cmd) {
                return cmd;
            }
            // Walking the path.
            if let Some(next) = self.follow_path(g, id, b, &mut cmd) {
                want_move = Some(next);
                look_at = Some(next + Vec3::Y * 1.5);
            }
            // Where to look: where a shot at us came from, a remembered enemy, a noise, the watch point, or ahead.
            let alarm = b.alarm.filter(|(_, t, hit)| g.time - t < if *hit { 3.0 } else { 2.0 });
            if let Some((pos, t, hit)) = alarm {
                look_at = Some(pos);
                // Hit from somewhere unseen: stop and turn on it rather than walk on with a back to the shooter.
                if hit && g.time - t < 1.2 && !matches!(b.goal, Goal::Retake | Goal::Escort) {
                    want_move = None;
                }
            } else if let Some(ls) = b.last_seen.filter(|_| b.lost < 3.0) {
                look_at = Some(ls + Vec3::Y * 1.4);
                walk = true;
            } else if let Some((pos, _)) = b.heard {
                look_at = Some(pos + Vec3::Y * 1.2);
            } else if want_move.is_none() {
                if let Some(w) = b.watch {
                    // Scan a little around the watch point.
                    let side = (b.scan * 0.6).sin() * 0.35;
                    let (yaw, _) = angles_to(eye, w);
                    let d = dir_from(yaw + side, 0.0);
                    look_at = Some(eye + d * 10.0 + Vec3::Y * ((w.y - eye.y).clamp(-3.0, 3.0)));
                }
            }
            // The knife out while running long distances is faster; keep the gun near enemies.
            let _ = walk;
            // Holding a weapon other than the best one without reason: switch back.
            if !inv.busy() && inv.active != inv.best() && matches!(inv.active.def().class, Class::Knife | Class::Grenade) && b.nade.is_none() && !matches!(b.goal, Goal::Attack(_) if inv.bomb && inv.active == WeaponId::Bomb) {
                cmd.select = Some(Select::Weapon(inv.best()));
            }
        }
        // Turn toward what matters: fast onto a shot at us, quickly onto a shot close by, calmly otherwise.
        if !engaged {
            if let Some(l) = look_at {
                let (yaw, pitch) = angles_to(eye, l);
                let urgency = match b.alarm {
                    Some((_, t, true)) if g.time - t < 3.0 => 1.8,
                    Some((_, t, false)) if g.time - t < 2.0 => 1.1,
                    _ => 0.5,
                };
                self.turn(b, yaw, pitch.clamp(-0.6, 0.6), urgency);
            }
            // An enemy heard close by: those holding or hunting go quietly (walking makes no footsteps).
            if b.heard.is_some_and(|(pos, t)| g.time - t < 2.0 && pos.distance(p.mover.origin) < 18.0)
                && matches!(b.goal, Goal::Hunt | Goal::Defend(_) | Goal::Hold | Goal::GuardBomb | Goal::GuardHostages)
            {
                cmd.buttons |= Btn::WALK;
            }
        }
        // Move (making way for other bodies).
        let mut crowded = false;
        if let Some(to) = want_move {
            let d = Vec2::new(to.x - p.mover.origin.x, to.z - p.mover.origin.z);
            if d.length() > 0.05 {
                let (d, busy) = make_way(g, id, d.normalize());
                crowded = busy;
                let fwd = Vec2::new(-b.yaw.sin(), -b.yaw.cos());
                let right = Vec2::new(b.yaw.cos(), -b.yaw.sin());
                cmd.forward = d.dot(fwd);
                cmd.side = d.dot(right);
                // Full speed, unless queueing behind someone (then as slow as `make_way` says).
                let len = Vec2::new(cmd.forward, cmd.side).length();
                let scale = if len > 1e-4 { d.length().min(1.0) / len } else { 0.0 };
                cmd.forward *= scale;
                cmd.side *= scale;
            }
        }
        cmd.yaw = b.yaw;
        cmd.pitch = b.pitch;
        // Stuck: jump, then pick another way.
        let moved = p.mover.origin.distance(b.last_pos);
        b.last_pos = p.mover.origin;
        if (cmd.forward.abs() + cmd.side.abs()) > 0.3 && moved < 0.5 * DT && g.phase != Phase::Freeze {
            b.stuck += DT;
        } else {
            b.stuck = (b.stuck - DT * 2.0).max(0.0);
        }
        // (Pressed against another body, jumping doesn't help: the side-step below does. Nor in a fight: a bot hopping
        // about while it shoots looked silly.)
        if b.stuck > 0.5 && !crowded && !engaged {
            cmd.buttons |= Btn::JUMP;
            if b.stuck > 0.55 {
                cmd.buttons |= Btn::DUCK;
            }
        }
        if b.stuck > 1.4 {
            b.stuck = 0.0;
            b.path.clear();
            b.repath = 0.0;
            b.unstuck = 0.6;
            b.strafe = -b.strafe;
        }
        if b.unstuck > 0.0 {
            cmd.side = b.strafe;
            cmd.forward = -0.3;
        }
        cmd
    }

    fn turn(&self, b: &mut Brain, yaw: f32, pitch: f32, speed_scale: f32) {
        let k = 1.0 - (-b.skill.settle * DT).exp();
        let max = b.skill.turn * DT * speed_scale.max(0.2) * 2.0;
        let dy = (wrap(yaw - b.yaw) * k).clamp(-max, max);
        let dp = ((pitch - b.pitch) * k).clamp(-max, max);
        b.yaw = wrap(b.yaw + dy);
        b.pitch = (b.pitch + dp).clamp(-1.4, 1.4);
    }

    fn perceive(&mut self, g: &Game, id: PlayerId, b: &mut Brain, eye: Vec3, flashed: bool) {
        let p = &g.players[id];
        let team = p.team;
        let look = dir_from(b.yaw, 0.0);
        let mut best: Option<(f32, PlayerId, Vec3)> = None;
        if !flashed {
            for e in g.players.iter().filter(|e| e.alive && e.team != team) {
                let hb = e.hitboxes();
                let chest = hb.chest();
                let to = chest - eye;
                let d = to.length();
                if d > 90.0 {
                    continue;
                }
                let flat = Vec3::new(to.x, 0.0, to.z).normalize_or_zero();
                let angle = look.dot(flat).clamp(-1.0, 1.0).acos();
                let tracking = b.target == Some(e.id) && b.lost < 1.0;
                let fov = if tracking { b.skill.fov + 0.6 } else { b.skill.fov };
                if angle > fov && d > 2.5 {
                    continue;
                }
                let head = hb.head();
                let seen = if g.clear_line(eye, head, true) {
                    Some(head)
                } else if g.clear_line(eye, chest, true) {
                    Some(chest)
                } else if g.clear_line(eye, hb.stomach(), true) {
                    Some(hb.stomach())
                } else {
                    None
                };
                let Some(point) = seen else { continue };
                // Prefer the current target, then the nearest.
                let score = d - if tracking { 8.0 } else { 0.0 };
                if best.is_none_or(|bb| score < bb.0) {
                    best = Some((score, e.id, point));
                }
            }
        }
        match best {
            Some((_, t, _)) => {
                let d = g.players[t].mover.origin.distance(p.mover.origin);
                if b.target != Some(t) || b.lost > 1.5 {
                    let (lo, hi) = b.skill.reaction;
                    b.react = self.rng.range(lo, hi) + (d / 120.0).min(0.3);
                    // Holding still and watching, one sees first; running, one is slower to react.
                    let speed = p.mover.horizontal_speed();
                    b.react *= if speed < 0.5 { b.skill.alert } else if speed > 3.0 { 1.25 } else { 1.0 };
                    b.err = Vec2::new(self.rng.range(-1.0, 1.0), self.rng.range(-0.6, 0.6)) * b.skill.error * (1.0 + d / 40.0);
                    // Already looking that way (holding an angle): on target sooner and closer.
                    let to = (g.players[t].hitboxes().chest() - eye).normalize_or_zero();
                    let off = dir_from(b.yaw, b.pitch).angle_between(to);
                    if off < 0.3 {
                        b.err *= 0.35;
                        b.react *= 0.5 + 0.4 * b.skill.alert;
                    } else if off > 1.2 {
                        b.react += 0.12;
                    }
                    // Alerted from that side (shot at, or heard shooting there): readier, but never under the floor.
                    if let Some((apos, at, _)) = b.alarm
                        && g.time - at < 4.0
                        && apos.distance(g.players[t].mover.origin) < 12.0
                    {
                        b.react *= 0.75;
                        b.err *= 0.8;
                    }
                    b.react = b.react.max(b.skill.react_floor);
                    b.aim_point_head = self.rng.chance(b.skill.head);
                    b.burst_left = 0;
                    if b.radio <= 0.0 && self.team_radio[p.team.index()] <= g.time && self.rng.chance(0.2) {
                        self.team_radio[p.team.index()] = g.time + 5.0;
                        let place = g.place_at(g.players[t].mover.origin).unwrap_or("");
                        let text = if place.is_empty() { "Feind gesichtet!".to_string() } else { format!("Feind gesichtet – {place}") };
                        self.radio.push((id, text));
                        b.radio = 8.0;
                    }
                }
                b.target = Some(t);
                b.visible = true;
                b.lost = 0.0;
                let pos = g.players[t].mover.origin;
                b.last_seen = Some(pos);
                // Defenders note attackers near a site (for rotations), one entry per attacker.
                if team == Team::Ksk {
                    for si in 0..2 {
                        if self.sites[si].is_some_and(|c| c.distance(pos) < 24.0) {
                            let list = &mut self.contacts[si];
                            match list.iter_mut().find(|(who, _)| *who == t) {
                                Some(e) => e.1 = g.time,
                                None => list.push((t, g.time)),
                            }
                        }
                    }
                }
                // One entry per spot: a sighting near an earlier one refreshes it (the list stays short, and route
                // costs look through it at every step).
                let list = &mut self.sightings[team.index()];
                match list.iter_mut().find(|(q, _, _)| q.distance(pos) < 4.0) {
                    Some(e) => *e = (pos, g.time, e.2 + 1.0),
                    None => {
                        list.push((pos, g.time, 1.0));
                        if list.len() > 16 {
                            list.remove(0);
                        }
                    }
                }
            }
            None => {
                b.visible = false;
                b.lost += 0.1;
                if b.lost > 4.0 {
                    b.target = None;
                    b.last_seen = None;
                }
            }
        }
        // Hit: from where, even from behind. The shooter's spot becomes the thing to turn to.
        if let Some((a, from, t)) = p.hurt
            && t > b.hurt_seen
        {
            b.hurt_seen = t;
            if g.players[a].alive && g.players[a].team != team {
                b.alarm = Some((from, g.time, true));
                if b.radio <= 0.0 && !b.visible && self.team_radio[team.index()] <= g.time && self.rng.chance(0.25) {
                    self.team_radio[team.index()] = g.time + 5.0;
                    let place = g.place_at(p.mover.origin).unwrap_or("");
                    let text = if place.is_empty() { "Werde beschossen!".to_string() } else { format!("Werde beschossen – {place}") };
                    self.radio.push((id, text));
                    b.radio = 8.0;
                }
            }
        }
        // Hearing: shots carry far (less through walls), running footsteps and landings close by. A team-mate hit
        // nearby is a warning too: towards whoever shot them.
        let mut heard: Option<(f32, Vec3, bool)> = None;
        for e in g.players.iter().filter(|e| e.alive) {
            if e.team == team {
                if e.id != id
                    && let Some((a, from, t)) = e.hurt
                    && g.time - t < 0.3
                    && g.players[a].team != team
                    && e.mover.origin.distance(eye) < 25.0
                {
                    let score = from.distance(eye) * 0.6;
                    if heard.is_none_or(|h| score < h.0) {
                        heard = Some((score, from, true));
                    }
                }
                continue;
            }
            if let Some((pos, t)) = e.gunshot
                && g.time - t < 0.35
            {
                let d = pos.distance(eye);
                let range = if g.clear_line(eye, pos, false) { 90.0 } else { 55.0 };
                if d < range && heard.is_none_or(|h| d * 0.5 < h.0) {
                    heard = Some((d * 0.5, pos, d < 40.0));
                }
            }
            if e.noise < 0.35 {
                let d = e.last_noise_pos.distance(eye);
                if d < 26.0 && heard.is_none_or(|h| d < h.0) {
                    heard = Some((d, e.last_noise_pos, false));
                }
            }
        }
        if let Some((_, pos, urgent)) = heard {
            b.heard = Some((pos, g.time));
            if urgent && b.alarm.is_none_or(|(_, t, hit)| !hit || g.time - t > 1.5) {
                b.alarm = Some((pos, g.time, false));
            }
        } else if b.heard.is_some_and(|(_, t)| g.time - t > 3.0) {
            b.heard = None;
        }
        if b.alarm.is_some_and(|(_, t, _)| g.time - t > 4.0) {
            b.alarm = None;
        }
    }

    /// Shooting at a seen target.
    fn fight(&mut self, g: &Game, id: PlayerId, b: &mut Brain, t: PlayerId, eye: Vec3, cmd: &mut UserCmd, want_move: &mut Option<Vec3>) {
        let p = &g.players[id];
        let target = &g.players[t];
        let hb = target.hitboxes();
        let point = if b.aim_point_head { hb.head() } else { hb.chest() };
        let d = eye.distance(point);
        // The aim error shrinks while tracking; moving targets are harder.
        let lateral = target.mover.velocity.length();
        b.err *= (-b.skill.focus * DT).exp();
        b.err += Vec2::new(self.rng.range(-1.0, 1.0), self.rng.range(-1.0, 1.0)) * lateral * 0.0004;
        let (yaw, pitch) = angles_to(eye, point);
        // A sway that never settles (more on a moving target).
        let t = b.scan + b.sway_ph;
        let sway = Vec2::new((t * 1.9).sin() + 0.5 * (t * 4.3).sin(), 0.6 * (t * 1.3 + 1.7).cos()) * b.skill.sway * (1.0 + lateral * 0.12);
        // Recoil control: pull against the punch.
        let comp = b.skill.recoil;
        let want_yaw = yaw + b.err.x + sway.x + p.punch.y.to_radians() * comp;
        let want_pitch = pitch + b.err.y + sway.y - p.punch.x.to_radians() * comp;
        self.turn(b, want_yaw, want_pitch, 1.0);
        // How far off the aim is (with the punch).
        let aim = dir_from(b.yaw - p.punch.y.to_radians(), b.pitch + p.punch.x.to_radians());
        let off = aim.angle_between((point - eye).normalize_or_zero());
        // How far off the aim may be when firing: the target's size, stretched by the difficulty's trigger.
        let size = (0.22 / d.max(0.5)).atan() * b.skill.trigger;
        let def = p.inv.def();
        let inv = &p.inv;
        let mut fire = false;
        let mut stand_still = false;
        if b.react <= 0.0 {
            match def.class {
                Class::Knife => {
                    *want_move = Some(target.mover.origin);
                    fire = d < 1.9 && off < 0.4;
                    // A better weapon to hand?
                    if inv.best() != WeaponId::Knife && inv.gun().is_none() && (inv.primary.is_some_and(|g| g.clip + g.reserve > 0) || inv.secondary.is_some_and(|g| g.clip + g.reserve > 0)) {
                        cmd.select = Some(Select::Weapon(inv.best()));
                    }
                }
                Class::Grenade | Class::Bomb => {
                    cmd.select = Some(Select::Weapon(inv.best()));
                }
                Class::Sniper => {
                    if inv.zoom == 0 && !inv.busy() && inv.cooldown <= 0.0 && d > 6.0 {
                        if b.scope_wait <= -0.3 {
                            cmd.buttons |= Btn::ATTACK2;
                            b.scope_wait = 0.3;
                        }
                    } else {
                        stand_still = true;
                        fire = off < size * 0.8 && b.scope_wait <= 0.0 && p.mover.horizontal_speed() < 1.0;
                    }
                }
                Class::Shotgun => {
                    if d > 10.0 {
                        *want_move = Some(target.mover.origin);
                    }
                    fire = d < 18.0 && off < size * 3.0;
                }
                Class::Pistol => {
                    fire = off < size * 1.6;
                    if b.pause > 0.0 {
                        fire = false;
                    }
                    if fire {
                        b.pause = if d > 15.0 { 0.35 } else { 0.18 };
                    }
                }
                _ => {
                    // Rifles, SMGs, MGs: bursts by range.
                    if b.burst_left <= 0 && b.pause <= 0.0 {
                        b.burst_left = if d > 28.0 {
                            1 + self.rng.below(2) as i32
                        } else if d > 14.0 {
                            3 + self.rng.below(4) as i32
                        } else {
                            8 + self.rng.below(12) as i32
                        };
                    }
                    stand_still = true;
                    fire = b.burst_left > 0 && off < size * if d < 10.0 { 2.5 } else { 1.5 };
                }
            }
        }
        // Semi-automatic weapons need a fresh press: alternate.
        if fire {
            let semi = !def.auto;
            if semi && b.press {
                fire = false;
            }
        }
        if fire {
            cmd.buttons |= Btn::ATTACK;
            b.press = true;
            if matches!(def.class, Class::Rifle | Class::Smg | Class::Mg) && inv.cooldown <= DT {
                b.burst_left -= 1;
                if b.burst_left <= 0 {
                    b.pause = if d > 28.0 { self.rng.range(0.3, 0.5) } else if d > 14.0 { self.rng.range(0.25, 0.4) } else { 0.1 };
                }
            }
            if def.class == Class::Sniper {
                b.scope_wait = 0.4;
            }
        } else {
            b.press = false;
        }
        // Reload when empty.
        if inv.gun().is_some_and(|g| g.clip == 0) {
            cmd.buttons |= Btn::RELOAD;
            if inv.gun().is_some_and(|g| g.reserve == 0) {
                cmd.select = Some(Select::Slot(if inv.active.def().slot() == Slot::Primary { Slot::Secondary } else { Slot::Knife }));
            }
        }
        // Moving while fighting, with a gun: as a player does, not dancing left and right in the open.
        let _ = stand_still;
        if !matches!(def.class, Class::Knife | Class::Shotgun | Class::Grenade | Class::Bomb) && want_move.is_none() {
            self.fight_move(g, id, b, target.id, d, fire, cmd, want_move);
        }
    }

    /// A gunfight's footwork:
    /// - A covered spot chosen: go there, then hold it, ducking behind it to reload and between bursts, up to shoot.
    /// - Exposed (little cover where one stands) and pressed (hit, hurt, reloading, or out-ranged with a pistol or SMG):
    ///   look for such a spot within eight metres (not attackers on the clock: they push on).
    /// - Now and then a single side-step (when hit, or close in with an SMG or pistol), seconds apart.
    /// - Otherwise hold the spot: crouched at range when the target stays in sight from crouch height.
    #[allow(clippy::too_many_arguments)]
    fn fight_move(&mut self, g: &Game, id: PlayerId, b: &mut Brain, enemy: PlayerId, d: f32, fire: bool, cmd: &mut UserCmd, want_move: &mut Option<Vec3>) {
        let p = &g.players[id];
        let e = &g.players[enemy];
        let at = p.mover.origin;
        let threat = e.eye(&g.params);
        let class = p.inv.def().class;
        let reloading = matches!(p.inv.action, Action::Reloading { .. });
        let hurt = g.time - b.hurt_seen < 1.5;
        if let Some(c) = b.cover {
            let off = Vec2::new(c.x - at.x, c.z - at.z).length();
            if off > 0.45 && b.cover_t > 0.0 {
                *want_move = Some(c);
                return;
            }
            if off > 2.5 {
                // (Never got there, or pushed off it.)
                b.cover = None;
            } else {
                if reloading || (!fire && b.pause > 0.05) {
                    cmd.buttons |= Btn::DUCK;
                }
                return;
            }
        }
        let exposed = self.nav.nearest(at).is_none_or(|n| self.nav.nodes[n as usize].cover < 3);
        let out_ranged = d > 14.0 && matches!(class, Class::Pistol | Class::Smg) && matches!(e.inv.def().class, Class::Rifle | Class::Sniper | Class::Mg);
        let pressed = hurt || reloading || p.health < 50 || out_ranged;
        // (Attackers on the clock push on: with the bomb, or under 45 s left, there's no time to dig in.)
        let pushing = g.cfg.mode == Mode::Defuse && matches!(b.goal, Goal::Attack(_) | Goal::GetBomb) && (p.inv.bomb || g.phase_left < 45.0);
        if exposed && pressed && !pushing && b.plan_t <= 0.0 {
            b.plan_t = 1.2;
            if let Some(c) = self.find_cover(g, id, at, threat) {
                b.cover = Some(c);
                b.cover_t = 2.5;
                b.duck = false;
                *want_move = Some(c);
                return;
            }
        }
        if b.step_t > 0.0 {
            let right = Vec3::new(b.yaw.cos(), 0.0, -b.yaw.sin());
            *want_move = Some(at + right * b.step_dir * 2.0);
            return;
        }
        let close_quarters = d < 10.0 && matches!(class, Class::Smg | Class::Pistol);
        if b.skill.strafe && b.step_wait <= 0.0 && (hurt || (close_quarters && self.rng.chance(0.03))) {
            // To the side with room (a wall a metre and a half away turns it round).
            let right = Vec3::new(b.yaw.cos(), 0.0, -b.yaw.sin());
            let knee = at + Vec3::Y * 0.5;
            let mut dir = if self.rng.chance(0.5) { 1.0 } else { -1.0 };
            if !g.clear_line(knee, knee + right * dir * 1.5, false) {
                dir = -dir;
            }
            if g.clear_line(knee, knee + right * dir * 1.5, false) {
                b.step_dir = dir;
                b.step_t = self.rng.range(0.3, 0.55);
            }
            b.step_wait = self.rng.range(2.0, 4.0);
            return;
        }
        // (Decided once per target and kept: re-rolled every few seconds, bots bobbed up and down. Up again at once when
        // the target drops out of sight from crouch height, or comes close.)
        let low = at + Vec3::Y * g.params.crouch_eye;
        if b.duck && (d < 7.0 || (b.duck_t <= 0.0 && !g.clear_line(low, e.hitboxes().head(), true))) {
            b.duck = false;
        }
        if b.duck_t <= 0.0 {
            b.duck_t = 0.5;
        }
        if b.duck_target != Some(enemy) {
            b.duck_target = Some(enemy);
            let steady = matches!(class, Class::Rifle | Class::Mg | Class::Sniper | Class::Pistol);
            b.duck = steady && d > 9.0 && self.rng.chance(b.skill.crouch + 0.25) && g.clear_line(low, e.hitboxes().head(), true);
        }
        if b.duck {
            cmd.buttons |= Btn::DUCK;
        }
    }

    /// A spot within eight metres, reached in a straight line, where one is hidden from `threat` crouched but can shoot
    /// at it standing (else one simply hidden), not towards it, and free of other bodies.
    fn find_cover(&self, g: &Game, id: PlayerId, at: Vec3, threat: Vec3) -> Option<Vec3> {
        let (stand, crouch) = (g.params.stand_eye, g.params.crouch_eye);
        let mut best: Option<(f32, Vec3)> = None;
        for n in &self.nav.nodes {
            if n.flags & flags::CORE == 0 || n.flags & flags::CROUCH != 0 || n.cover < 2 {
                continue;
            }
            let dist = n.pos.distance(at);
            if !(0.8..=8.0).contains(&dist) || (n.pos.y - at.y).abs() > 1.0 {
                continue;
            }
            if g.players.iter().any(|q| q.alive && q.id != id && q.mover.origin.distance(n.pos) < 1.0) {
                continue;
            }
            if g.clear_line(n.pos + Vec3::Y * crouch, threat, false) || !g.clear_line(at + Vec3::Y * 0.6, n.pos + Vec3::Y * 0.6, false) {
                continue;
            }
            let peek = g.clear_line(n.pos + Vec3::Y * stand, threat, false);
            let closer = threat.distance(n.pos) < threat.distance(at) - 2.0;
            let score = dist + if peek { 0.0 } else { 3.0 } + if closer { 5.0 } else { 0.0 };
            if best.is_none_or(|bb| score < bb.0) {
                best = Some((score, n.pos));
            }
        }
        best.map(|bb| bb.1)
    }

    /// Picks and walks towards the destination for the goal.
    fn update_goal(&mut self, g: &Game, id: PlayerId, b: &mut Brain) {
        let p = &g.players[id];
        let team = p.team;
        let mode = g.cfg.mode;
        // Reassess the goal when things change.
        match g.bomb {
            Bomb::Dropped { pos, .. } if team == Team::Opfor && mode == Mode::Defuse => {
                // The nearest attacker bot fetches the bomb.
                let nearest = g
                    .players
                    .iter()
                    .filter(|q| q.bot && q.alive && q.team == Team::Opfor)
                    .min_by(|a, c| a.mover.origin.distance(pos).total_cmp(&c.mover.origin.distance(pos)))
                    .map(|q| q.id);
                if nearest == Some(id) && b.goal != Goal::GetBomb {
                    b.goal = Goal::GetBomb;
                    b.dest = Some(pos);
                    b.path.clear();
                }
            }
            Bomb::Carried(c) if b.goal == Goal::GetBomb => {
                b.goal = Goal::Attack(self.plan_site);
                b.dest = None;
                b.path.clear();
                let _ = c;
            }
            _ => {}
        }
        if matches!(g.phase, Phase::Warmup) || mode == Mode::Deathmatch {
            if !matches!(b.goal, Goal::Hunt | Goal::Follow(_) | Goal::Hold) {
                b.goal = Goal::Hunt;
                b.dest = None;
            }
        }
        if b.goal == Goal::Idle {
            b.goal = Goal::Hunt;
        }
        // Goals that the world can overtake, checked every tick.
        let change = |b: &mut Brain, goal: Goal| {
            b.goal = goal;
            b.dest = None;
            b.path.clear();
        };
        match b.goal {
            Goal::Rescue(i) => {
                let h = g.hostages.get(i);
                if h.is_some_and(|h| h.leader == Some(id)) {
                    change(b, Goal::Escort);
                    if b.radio <= 0.0 {
                        self.radio.push((id, "Habe eine Geisel, bringe sie raus!".into()));
                        b.radio = 10.0;
                    }
                } else if h.is_none_or(|h| !h.alive || h.rescued || h.leader.is_some()) {
                    match g.hostages.iter().position(|h| h.alive && !h.rescued && h.leader.is_none()) {
                        Some(j) => change(b, Goal::Rescue(j)),
                        None if g.hostages.iter().any(|h| h.leader == Some(id)) => change(b, Goal::Escort),
                        None => change(b, Goal::Hunt),
                    }
                }
            }
            Goal::Escort if !g.hostages.iter().any(|h| h.leader == Some(id) && !h.rescued) => {
                // Delivered (or lost them): another hostage, else hunt.
                match g.hostages.iter().position(|h| h.alive && !h.rescued && h.leader.is_none()) {
                    Some(j) => change(b, Goal::Rescue(j)),
                    None => change(b, Goal::Hunt),
                }
            }
            Goal::Escort => {
                // Wait for the hostages to catch up.
                let behind = g.hostages.iter().filter(|h| h.leader == Some(id)).map(|h| h.mover.origin.distance(p.mover.origin)).fold(0.0f32, f32::max);
                if behind > 6.0 {
                    // Pause on the way (keeping the route) and look back for them.
                    b.wait = 0.4;
                    b.watch = g.hostages.iter().find(|h| h.leader == Some(id)).map(|h| h.mover.origin + Vec3::Y * 1.2);
                }
            }
            Goal::GuardBomb | Goal::Retake if !matches!(g.bomb, Bomb::Planted { .. }) => change(b, Goal::Hunt),
            _ => {}
        }
        if b.dest.is_some() && !matches!(b.goal, Goal::Hunt | Goal::Follow(_)) {
            return;
        }
        if b.dest.is_none() && b.retry > 0.0 {
            return;
        }
        let nav = self.nav.clone();
        let here = p.mover.origin;
        let dest = match b.goal {
            Goal::Hunt => {
                // Towards what the team knows about the enemy, else somewhere far.
                // (What the team saw, else what this bot heard or was shot from.)
                let known = self.sightings[team.index()].last().map(|s| s.0).or(b.alarm.map(|a| a.0)).or(b.heard.map(|h| h.0));
                match (b.dest, known) {
                    // (Re-aimed at the latest sighting at most every couple of seconds: routes cost.)
                    (_, Some(k)) if b.dest.is_none_or(|d| d.distance(k) > 6.0) && (b.dest.is_none() || b.retarget <= 0.0) => {
                        b.retarget = 2.0;
                        Some(k)
                    }
                    (Some(d), _) if d.distance(here) > 2.0 => return,
                    _ => {
                        if nav.nodes.is_empty() {
                            None
                        } else {
                            // A random node, not too close.
                            let mut pick = None;
                            for _ in 0..12 {
                                let i = self.rng.below(nav.nodes.len() as u32);
                                let q = nav.pos(i);
                                if q.distance(here) > 15.0 && nav.nodes[i as usize].flags & flags::CORE != 0 {
                                    pick = Some(q);
                                    break;
                                }
                            }
                            pick
                        }
                    }
                }
            }
            Goal::Follow(leader) => {
                let lp = g.players[leader].mover.origin;
                if !g.players[leader].alive {
                    b.goal = Goal::Hunt;
                    return;
                }
                if lp.distance(here) < 3.0 {
                    b.dest = None;
                    b.path.clear();
                    return;
                }
                if b.dest.is_some_and(|d| d.distance(lp) < 2.5) {
                    return;
                }
                Some(lp)
            }
            Goal::Hold => Some(here),
            Goal::Attack(site) => {
                let mask = if site == 'B' { flags::SITE_B } else { flags::SITE_A };
                let nodes: Vec<u32> = nav.with_flags(mask | flags::CORE).collect();
                // The carrier heads for a covered spot inside; the others spread over the site.
                let pick = if nodes.is_empty() { None } else { Some(nodes[self.rng.below(nodes.len() as u32) as usize]) };
                pick.map(|i| nav.pos(i))
            }
            Goal::Defend(site) => {
                let mask = if site == 'B' { flags::SITE_B } else { flags::SITE_A };
                let site_nodes: Vec<u32> = nav.with_flags(mask | flags::CORE).collect();
                if site_nodes.is_empty() {
                    None
                } else {
                    let centre = site_nodes.iter().map(|&i| nav.pos(i)).sum::<Vec3>() / site_nodes.len() as f32;
                    // Where the attackers come from: the route from their spawn, where it reaches 12 m from the site.
                    let enemy_spawn = g.level.entities_of("info_player_opfor").next().map(|e| e.origin).unwrap_or(centre);
                    let route = match (nav.nearest(enemy_spawn), nav.nearest(centre)) {
                        (Some(a), Some(c)) => nav.route(a, c, &|_| 0.0),
                        _ => None,
                    };
                    let approach: Vec<Vec3> = route.map(|r| r.iter().rev().map(|&k| nav.pos(k)).filter(|q| q.distance(centre) > 8.0).take(40).collect()).unwrap_or_default();
                    let watch_at = approach.iter().find(|q| q.distance(centre) > 12.0).copied().unwrap_or(enemy_spawn);
                    // A covered spot within 14 m of the site's centre that sees the way in.
                    let eye = Vec3::Y * 1.55;
                    let mut spots: Vec<u32> = nav
                        .nodes
                        .iter()
                        .enumerate()
                        .filter(|(_, n)| n.pos.distance(centre) < 14.0 && n.cover >= 2 && n.flags & flags::CROUCH == 0 && n.flags & flags::CORE != 0)
                        .map(|(i, _)| i as u32)
                        .collect();
                    self.rng.shuffle(&mut spots);
                    let sees = |i: u32| approach.iter().step_by(4).any(|q| g.clear_line(nav.pos(i) + eye, *q + eye, false));
                    let pick = spots.iter().take(40).copied().find(|&i| sees(i)).or_else(|| site_nodes.iter().copied().find(|&i| sees(i))).unwrap_or(site_nodes[self.rng.below(site_nodes.len() as u32) as usize]);
                    let i = pick;
                    // Watch the nearest visible point of the way in.
                    let watch = approach.iter().copied().filter(|q| g.clear_line(nav.pos(i) + eye, *q + eye, false)).min_by(|a, c| a.distance(nav.pos(i)).total_cmp(&c.distance(nav.pos(i))));
                    b.watch = Some(watch.unwrap_or(watch_at) + Vec3::Y * 1.5);
                    Some(nav.pos(i))
                }
            }
            Goal::GuardBomb => {
                let Bomb::Planted { pos, .. } = g.bomb else {
                    b.goal = Goal::Hunt;
                    return;
                };
                let spots: Vec<u32> = nav
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| {
                        let d = n.pos.distance(pos);
                        d > 4.0 && d < 16.0 && n.cover >= 2 && n.flags & flags::CORE != 0
                    })
                    .map(|(i, _)| i as u32)
                    .collect();
                b.watch = Some(pos + Vec3::Y * 0.5);
                let pick = self.rng.pick(&spots).copied();
                Some(pick.map_or(pos, |i| nav.pos(i)))
            }
            Goal::Retake => {
                let Bomb::Planted { pos, .. } = g.bomb else {
                    b.goal = Goal::Hunt;
                    return;
                };
                Some(pos)
            }
            Goal::GetBomb => match g.bomb {
                Bomb::Dropped { pos, .. } => Some(pos),
                _ => {
                    b.goal = Goal::Attack(self.plan_site);
                    return;
                }
            },
            Goal::Rescue(i) => {
                let h = g.hostages.get(i).filter(|h| h.alive && !h.rescued && h.leader.is_none());
                match h {
                    Some(h) => Some(h.mover.origin),
                    None => {
                        // Another hostage free to take, else escort what follows us, else hunt.
                        if let Some(j) = g.hostages.iter().position(|h| h.alive && !h.rescued && h.leader.is_none()) {
                            b.goal = Goal::Rescue(j);
                        } else if g.hostages.iter().any(|h| h.leader == Some(id)) {
                            b.goal = Goal::Escort;
                        } else {
                            b.goal = Goal::Hunt;
                        }
                        return;
                    }
                }
            }
            Goal::Escort => {
                if !g.hostages.iter().any(|h| h.leader == Some(id)) {
                    b.goal = Goal::Hunt;
                    return;
                }
                let rescue: Vec<u32> = nav.with_flags(flags::RESCUE | flags::CORE).collect();
                self.rng.pick(&rescue).map(|&i| nav.pos(i))
            }
            Goal::GuardHostages => {
                let hs: Vec<Vec3> = g.hostages.iter().filter(|h| h.alive && !h.rescued).map(|h| h.mover.origin).collect();
                let Some(&h) = self.rng.pick(&hs) else {
                    b.goal = Goal::Hunt;
                    return;
                };
                let spots: Vec<u32> = nav.nodes.iter().enumerate().filter(|(_, n)| n.pos.distance(h) < 12.0 && n.cover >= 3 && n.flags & flags::CORE != 0).map(|(i, _)| i as u32).collect();
                b.watch = None;
                Some(self.rng.pick(&spots).map_or(h, |&i| nav.pos(i)))
            }
            Goal::Idle => None,
        };
        if let Some(d) = dest {
            b.dest = Some(d);
            b.path.clear();
            b.repath = 0.0;
        }
    }

    /// Follows the route to `dest`, repathing as needed; returns the point to move towards.
    fn follow_path(&mut self, g: &Game, id: PlayerId, b: &mut Brain, cmd: &mut UserCmd) -> Option<Vec3> {
        let p = &g.players[id];
        let here = p.mover.origin;
        if b.wait > 0.0 {
            b.wait -= DT;
            return None;
        }
        let dest = b.dest?;
        let arrive: f32 = match b.goal {
            Goal::Follow(_) => 2.5,
            Goal::Rescue(_) => 1.2,
            _ => 0.8,
        };
        // A team-mate already standing on the spot: hold next to it instead of jostling for it.
        let taken = g.players.iter().any(|q| q.alive && q.id != id && q.team == p.team && q.mover.origin.distance(dest) < 1.0 && q.mover.horizontal_speed() < 0.5);
        let arrive = if taken { arrive.max(2.2) } else { arrive };
        if Vec2::new(dest.x - here.x, dest.z - here.z).length() < arrive && (dest.y - here.y).abs() < 1.5 {
            b.path.clear();
            // Holding here: face the longest open line of sight (leaning towards the enemy's side), not a wall.
            if b.watch.is_none() && !matches!(b.goal, Goal::Hunt | Goal::Follow(_)) {
                b.watch = Some(best_view(g, g.eye(id), p.team));
            }
            b.hold_t += DT;
            if matches!(b.goal, Goal::Hunt) {
                b.dest = None;
            }
            return None;
        }
        b.hold_t = 0.0;
        let nav = &self.nav;
        if b.path.is_empty() || b.repath <= 0.0 && b.path_i >= b.path.len() {
            b.repath = 3.0 + self.rng.range(0.0, 1.0);
            let (Some(a), Some(z)) = (nav.nearest(here), nav.nearest_core(dest)) else { return Some(dest) };
            // Avoid places where the team recently saw enemies (for the second route onwards, flanks happen).
            let sightings = self.sightings[p.team.index()].clone();
            let extra = move |i: u32| {
                let q = nav.pos(i);
                sightings.iter().filter(|(s, _, _)| s.distance(q) < 6.0).map(|(_, _, w)| w.min(200.0)).sum::<f32>() * 2.0
            };
            match nav.route(a, z, &extra) {
                Some(r) => {
                    b.path = r;
                    b.path_i = 0;
                }
                None => {
                    // No way there: think again in a moment (and pick another spot), not every tick.
                    b.dest = None;
                    b.retry = 1.5;
                    return None;
                }
            }
        }
        // Advance past waypoints we're at or have passed: the furthest of the next few that is close, or any whose
        // successor is nearer to us than to it.
        let flat = |a: Vec3, c: Vec3| Vec2::new(a.x - c.x, a.z - c.z).length();
        let mut k = b.path_i;
        while k < b.path.len() && k < b.path_i + 8 {
            let w = nav.pos(b.path[k]);
            // (On the waypoint's own floor: a stair step above or a ledge beside doesn't count.)
            let level = (w.y - here.y).abs() < 0.4;
            let close = flat(w, here) < 0.7 && level;
            let passed = b.path.get(k + 1).is_some_and(|&n| {
                let next = nav.pos(n);
                flat(next, here) < flat(next, w) && level && (next.y - here.y).abs() < 0.8 && flat(w, here) < 2.0
            });
            if close || passed {
                b.path_i = k + 1;
            }
            k += 1;
        }
        if b.path_i >= b.path.len() {
            return Some(dest);
        }
        // Look ahead: steer at a waypoint a couple along when the next is close, for smooth corners.
        let i = b.path_i;
        let w = nav.pos(b.path[i]);
        let prev = if i > 0 { Some(b.path[i - 1]) } else { None };
        if let Some(pr) = prev {
            match nav.link(pr, b.path[i]) {
                Some(LinkKind::Jump) => {
                    if Vec2::new(w.x - here.x, w.z - here.z).length() < 1.2 && p.mover.on_ground {
                        cmd.buttons |= Btn::JUMP;
                    }
                    if !p.mover.on_ground {
                        cmd.buttons |= Btn::DUCK;
                    }
                }
                Some(LinkKind::Crouch) => cmd.buttons |= Btn::DUCK,
                _ => {}
            }
        }
        if nav.nodes[b.path[i] as usize].flags & flags::CROUCH != 0 {
            cmd.buttons |= Btn::DUCK;
        }
        let ahead = b.path.get(i + 2).map(|&k| nav.pos(k));
        let steer = match ahead {
            Some(a) if Vec2::new(w.x - here.x, w.z - here.z).length() < 0.9 && (a.y - here.y).abs() < 0.4 => a,
            _ => w,
        };
        Some(steer)
    }

    /// Planting, defusing and taking hostages when at the goal. Returns true if it took over the command.
    fn goal_action(&mut self, g: &Game, id: PlayerId, b: &mut Brain, cmd: &mut UserCmd) -> bool {
        let p = &g.players[id];
        let here = p.mover.origin;
        // The carrier plants once inside the site.
        if p.inv.bomb && g.phase == Phase::Live && g.site_at(here).is_some() && b.target.is_none() {
            // Walk a little further in when just at the edge.
            if p.inv.active != WeaponId::Bomb {
                cmd.select = Some(Select::Weapon(WeaponId::Bomb));
            } else if !p.inv.busy() || matches!(p.inv.action, Action::Planting(_)) {
                cmd.buttons |= Btn::ATTACK;
                if matches!(p.inv.action, Action::Idle) && b.radio <= 0.0 {
                    self.radio.push((id, "Lege die Ladung!".into()));
                    b.radio = 10.0;
                }
            }
            self.turn(b, b.yaw, -0.9, 0.6);
            cmd.yaw = b.yaw;
            cmd.pitch = b.pitch;
            return true;
        }
        // Defusing.
        if let Bomb::Planted { pos, left, defuser, .. } = g.bomb
            && p.team == Team::Ksk
            && b.goal == Goal::Retake
        {
            let close = g.eye(id).distance(pos) < 1.8;
            let kit = p.defuser;
            let need = if kit { g.cfg.defuse_kit_time } else { g.cfg.defuse_time };
            let someone_else = defuser.is_some_and(|d| d != id);
            if close && !someone_else && (b.target.is_none() || left < need + 1.0) {
                cmd.buttons |= Btn::USE;
                let (yaw, pitch) = angles_to(g.eye(id), pos);
                self.turn(b, yaw, pitch, 1.0);
                cmd.yaw = b.yaw;
                cmd.pitch = b.pitch;
                if defuser.is_none() && b.radio <= 0.0 {
                    self.radio.push((id, "Entschärfe!".into()));
                    b.radio = 10.0;
                }
                return true;
            }
            if someone_else {
                // Cover the defuser: hold near.
                b.watch = None;
            }
        }
        // Taking a hostage along.
        if let Goal::Rescue(i) = b.goal
            && let Some(h) = g.hostages.get(i)
            && h.alive
            && !h.rescued
            && h.leader.is_none()
        {
            let c = h.mover.origin + Vec3::Y * 1.0;
            let eye = g.eye(id);
            if eye.distance(c) < 2.0 {
                let (yaw, pitch) = angles_to(eye, c);
                self.turn(b, yaw, pitch, 1.5);
                cmd.yaw = b.yaw;
                cmd.pitch = b.pitch;
                let facing = dir_from(b.yaw, b.pitch).dot((c - eye).normalize_or_zero()) > 0.8;
                if facing {
                    b.press = !b.press;
                    if b.press {
                        cmd.buttons |= Btn::USE;
                    }
                }
                return true;
            }
        }
        if b.goal == Goal::Rescue(usize::MAX) {
            return false;
        }
        false
    }

    /// Grenades: onto the site before going in (attackers), at a heard enemy close by (anyone). Returns true while
    /// throwing.
    fn grenade(&mut self, g: &Game, id: PlayerId, b: &mut Brain, eye: Vec3, cmd: &mut UserCmd) -> bool {
        let p = &g.players[id];
        if let Some((kind, at, since)) = b.nade {
            let since = since + DT;
            b.nade = Some((kind, at, since));
            if since > 3.0 || !p.inv.has(kind) && p.inv.active != kind {
                b.nade = None;
                return false;
            }
            if p.inv.active != kind {
                cmd.select = Some(Select::Weapon(kind));
                return true;
            }
            let (yaw, pitch) = throw_angles(eye, at);
            self.turn(b, yaw, pitch, 1.5);
            cmd.yaw = b.yaw;
            cmd.pitch = b.pitch;
            let aimed = wrap(b.yaw - yaw).abs() < 0.05 && (b.pitch - pitch).abs() < 0.05;
            match p.inv.action {
                Action::PinPulled { .. } => {
                    if !aimed {
                        cmd.buttons |= Btn::ATTACK;
                    } else {
                        b.nade = None;
                        b.nade_done = g.round;
                    }
                }
                _ if !p.inv.busy() && p.inv.cooldown <= 0.0 => cmd.buttons |= Btn::ATTACK,
                _ => {}
            }
            return true;
        }
        if g.phase != Phase::Live || b.nade_done == g.round && g.cfg.mode != Mode::Deathmatch {
            return false;
        }
        if !self.rng.chance(b.skill.grenades * 0.02) {
            return false;
        }
        // A heard enemy 8–22 m away, behind cover: an HE.
        if p.inv.he > 0
            && let Some((pos, _)) = b.heard
        {
            let d = pos.distance(eye);
            if (8.0..22.0).contains(&d) && !g.clear_line(eye, pos + Vec3::Y * 1.2, false) {
                b.nade = Some((WeaponId::He, pos, 0.0));
                return true;
            }
        }
        // Attackers approaching their site: a smoke or flash onto it from 12–28 m.
        if let Goal::Attack(site) = b.goal {
            let mask = if site == 'B' { flags::SITE_B } else { flags::SITE_A };
            let nodes: Vec<u32> = self.nav.with_flags(mask).collect();
            if !nodes.is_empty() {
                let centre = nodes.iter().map(|&i| self.nav.pos(i)).sum::<Vec3>() / nodes.len() as f32;
                let d = centre.distance(eye);
                if (12.0..28.0).contains(&d) {
                    let kind = if p.inv.smoke > 0 {
                        Some(WeaponId::Smoke)
                    } else if p.inv.flash > 0 {
                        Some(WeaponId::Flash)
                    } else {
                        None
                    };
                    if let Some(k) = kind {
                        b.nade = Some((k, centre, 0.0));
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Buys at the start of the round (a moment into the freeze), by the economy.
    fn buy(&mut self, g: &Game, id: PlayerId, b: &mut Brain, cmd: &mut UserCmd) {
        let p = &g.players[id];
        let key = if g.phase == Phase::Warmup || g.cfg.mode == Mode::Deathmatch { 10_000 + (g.time / 5.0) as u32 } else { g.round };
        if b.bought == key || !g.can_buy(p) {
            return;
        }
        let into = if matches!(g.phase, Phase::Freeze) { g.cfg.freeze - g.phase_left } else { 99.0 };
        if into < b.buy_at && matches!(g.phase, Phase::Freeze) {
            return;
        }
        b.bought = key;
        let free = g.phase == Phase::Warmup || g.cfg.mode == Mode::Deathmatch;
        let team = p.team;
        let mut money = if free { 16_000 } else { p.money };
        let mut list = Vec::new();
        let take = |item: Buy, money: &mut i32, list: &mut Vec<Buy>| {
            if *money >= item.price() {
                *money -= item.price();
                list.push(item);
                true
            } else {
                false
            }
        };
        let half = g.cfg.max_rounds / 2;
        let pistol_round = !free && (g.round == 1 || g.round == half + 1);
        let team_money: i32 = g.players.iter().filter(|q| q.team == team).map(|q| q.money).sum::<i32>() / g.players.iter().filter(|q| q.team == team).count().max(1) as i32;
        let loss_streak = g.loss_streak[team.index()];
        let has_primary = p.inv.primary.is_some();
        if pistol_round {
            match self.rng.below(3) {
                0 => {
                    take(Buy::Vest, &mut money, &mut list);
                }
                1 => {
                    take(Buy::Weapon(WeaponId::P30), &mut money, &mut list);
                    take(Buy::Weapon(WeaponId::Flash), &mut money, &mut list);
                }
                _ => {
                    take(Buy::Weapon(WeaponId::Flash), &mut money, &mut list);
                    take(Buy::Weapon(WeaponId::Smoke), &mut money, &mut list);
                }
            }
        } else {
            let full = money >= 3900 || (money >= 3100 && team_money >= 3000) || free;
            let force = !full && money >= 2100 && (loss_streak >= 2 || self.rng.chance(0.25) || has_primary);
            if full && !has_primary {
                let roll = self.rng.f32();
                let sniper = if free { roll < 0.12 } else { roll < 0.1 && money >= 6000 };
                let mg = !sniper && roll < 0.13 && money >= 6500;
                let gun = if sniper {
                    if self.rng.chance(0.7) { WeaponId::G22 } else { WeaponId::Ssg3000 }
                } else if mg {
                    if team == Team::Ksk { WeaponId::Mg5 } else { WeaponId::Pkm }
                } else if money >= team.rifle().def().price + 1000 || free {
                    team.rifle()
                } else if team == Team::Ksk {
                    WeaponId::G36
                } else {
                    WeaponId::Ak74
                };
                take(Buy::Weapon(gun), &mut money, &mut list);
            } else if force && !has_primary {
                let options: &[WeaponId] = if team == Team::Ksk { &[WeaponId::Mp7, WeaponId::Ump, WeaponId::Mp5, WeaponId::G36, WeaponId::M870] } else { &[WeaponId::Pp19, WeaponId::Ump, WeaponId::Mp5, WeaponId::Ak74, WeaponId::M870] };
                let pick = options[self.rng.below(options.len() as u32) as usize];
                if !take(Buy::Weapon(pick), &mut money, &mut list) {
                    take(Buy::Weapon(WeaponId::Ump), &mut money, &mut list);
                }
            } else if !has_primary && money >= 700 && self.rng.chance(0.3) {
                take(Buy::Weapon(if self.rng.chance(0.5) { WeaponId::P30 } else { WeaponId::Rev357 }), &mut money, &mut list);
            }
            if full || force || has_primary {
                if p.armor < 50 || !p.helmet {
                    if !take(Buy::VestHelmet, &mut money, &mut list) {
                        take(Buy::Vest, &mut money, &mut list);
                    }
                }
                if team == Team::Ksk && !p.defuser && g.cfg.mode == Mode::Defuse && self.rng.chance(0.6) {
                    take(Buy::Defuser, &mut money, &mut list);
                }
                let nades = [WeaponId::Flash, WeaponId::Smoke, WeaponId::He, WeaponId::Flash];
                for n in nades {
                    if money > 300 && self.rng.chance(0.75) {
                        take(Buy::Weapon(n), &mut money, &mut list);
                    }
                }
            }
        }
        cmd.buy = list;
    }

    /// What a bot is up to (for debugging and the balance tool).
    pub fn describe(&self, id: PlayerId) -> String {
        match self.brains.get(&id) {
            Some(b) => {
                let next: Vec<String> = (b.path_i.saturating_sub(1)..(b.path_i + 3).min(b.path.len()))
                    .map(|k| {
                        let kind = if k > 0 { self.nav.link(b.path[k - 1], b.path[k]).map_or("?".to_string(), |l| format!("{l:?}")) } else { "-".into() };
                        format!("{}:{:?}{}", k, (self.nav.pos(b.path[k]) * 100.0).round() / 100.0, kind)
                    })
                    .collect();
                format!("{:?} dest={:?} path={}/{} target={:?} visible={} stuck={:.1} next=[{}]", b.goal, b.dest.map(|d| (d * 10.0).round() / 10.0), b.path_i, b.path.len(), b.target, b.visible, b.stuck, next.join(" "))
            }
            None => "no brain".into(),
        }
    }

    /// Radio lines produced since the last call, as game events (for the client's chat).
    pub fn drain_radio(&mut self) -> Vec<Event> {
        self.radio.drain(..).map(|(player, text)| Event::Radio { player, text }).collect()
    }
}

/// Where to look while holding a spot: of sixteen directions at eye height, the one with the longest free line of
/// sight, favouring the way towards the enemy's spawn.
fn best_view(g: &Game, eye: Vec3, team: Team) -> Vec3 {
    let enemy = g.level.entities_of(if team == Team::Ksk { "info_player_opfor" } else { "info_player_ksk" }).next().map(|e| e.origin);
    let towards = enemy.map(|e| (e - eye).with_y(0.0).normalize_or_zero()).unwrap_or(Vec3::ZERO);
    let mut best = (f32::MIN, eye + Vec3::X);
    for k in 0..16 {
        let a = k as f32 * std::f32::consts::TAU / 16.0;
        let d = Vec3::new(a.cos(), 0.0, a.sin());
        let t = g.level.world.trace(eye, eye + d * 40.0, Vec3::ZERO, crate::world::Contents::MASK_SHOT);
        let free = t.fraction * 40.0;
        let score = free.min(30.0) + 8.0 * d.dot(towards);
        if score > best.0 {
            best = (score, eye + d * free.min(15.0));
        }
    }
    best.1
}

/// View angles to lob a grenade onto `at`: the lower of the two ballistic arcs at the throw speed, converted back
/// through the throw's upward bias.
fn throw_angles(eye: Vec3, at: Vec3) -> (f32, f32) {
    let (yaw, _) = angles_to(eye, at);
    let d = Vec2::new(at.x - eye.x, at.z - eye.z).length();
    let h = at.y - eye.y;
    let v = 675.0 * crate::units::METRES_PER_UNIT as f32;
    let gr = 0.4 * 800.0 * crate::units::METRES_PER_UNIT as f32;
    let v2 = v * v;
    let disc = v2 * v2 - gr * (gr * d * d + 2.0 * h * v2);
    let theta = if disc >= 0.0 { ((v2 - disc.sqrt()) / (gr * d.max(0.1))).atan() } else { PI / 4.0 };
    // The throw adds (90 − |pitch|)/9 degrees upwards: invert it.
    let t = theta.to_degrees();
    let p = if t >= 10.0 { (9.0 * t - 90.0) / 8.0 } else { (9.0 * t - 90.0) / 10.0 };
    (yaw, p.clamp(-80.0, 80.0).to_radians())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tests::arena;
    use crate::game::{Config, Game, TICK_HZ};

    fn bot_match(mode: Mode, seconds: f32, difficulty: Difficulty, seed: u64) -> (Game, Bots) {
        let level = arena();
        let nav = Arc::new(Nav::build(&level, &crate::movement::MoveParams::default()));
        let cfg = Config { mode, warmup: 0.0, ..Default::default() };
        let mut g = Game::new(level, cfg, seed);
        let mut bots = Bots::new(nav, difficulty, seed);
        for i in 0..5 {
            g.add_player(&format!("K{i}"), Team::Ksk, true);
            g.add_player(&format!("O{i}"), Team::Opfor, true);
        }
        for _ in 0..(seconds * TICK_HZ) as usize {
            let cmds = bots.think(&g);
            g.tick(&cmds);
            g.events.clear();
        }
        (g, bots)
    }

    /// A bot shot from behind turns on the shooter and fires back (it can't see him: he's behind it).
    #[test]
    fn a_bot_shot_from_behind_turns_and_fights() {
        for difficulty in [Difficulty::Easy, Difficulty::Normal, Difficulty::Expert] {
            let level = arena();
            let nav = Arc::new(Nav::build(&level, &crate::movement::MoveParams::default()));
            let mut g = Game::new(level, Config { mode: Mode::Deathmatch, warmup: 0.0, ..Default::default() }, 3);
            let mut bots = Bots::new(nav.clone(), difficulty, 3);
            let a = g.add_player("bot", Team::Ksk, true);
            let s = g.add_player("shooter", Team::Opfor, false);
            for _ in 0..(TICK_HZ as usize) {
                let cmds = bots.think(&g);
                g.tick(&cmds);
                g.events.clear();
            }
            // Somewhere open: the bot looks down −Z, the shooter stands 7 m behind it (+Z), both on the floor.
            let spot = (0..nav.nodes.len() as u32)
                .map(|i| nav.pos(i))
                .find(|&q| {
                    let behind = q + Vec3::Z * 7.0;
                    g.clear_line(q + Vec3::Y * 1.6, behind + Vec3::Y * 1.6, false) && nav.nodes.iter().enumerate().any(|(j, _)| nav.pos(j as u32).distance(behind) < 0.8)
                })
                .expect("an open spot");
            g.players[a].mover.origin = spot;
            g.players[a].mover.velocity = Vec3::ZERO;
            g.players[a].yaw = 0.0;
            g.players[s].mover.origin = spot + Vec3::Z * 7.0;
            g.players[s].yaw = std::f32::consts::PI;
            if let Some(b) = bots.brains.get_mut(&a) {
                b.yaw = 0.0;
                b.goal = Goal::Hold;
                b.target = None;
                b.visible = false;
            }
            // One round into its back.
            crate::combat::damage(&mut g, a, Some(s), 12.0, None, WeaponId::P8, Vec3::NEG_Z, spot + Vec3::Y * 1.2, 1.0, false);
            let mut fired_at = None;
            for i in 0..(3 * TICK_HZ as usize) {
                let cmds = bots.think(&g);
                g.tick(&cmds);
                if g.events.iter().any(|e| matches!(e, Event::Shot { player, .. } if *player == a)) {
                    fired_at = Some(i as f32 / TICK_HZ);
                    break;
                }
                g.events.clear();
            }
            let t = fired_at.unwrap_or_else(|| panic!("{difficulty:?}: never fired back (yaw {:.2})", g.players[a].yaw));
            eprintln!("{difficulty:?}: fired back after {t:.2} s");
            assert!(t < 2.5, "{difficulty:?}: fired back only after {t:.2} s");
            // It has to turn round and react: not in the same instant (the floor keeps Easy slow).
            let floor = skill(difficulty, &mut Rng::new(1)).react_floor;
            assert!(t >= floor, "{difficulty:?}: fired back after {t:.2} s, under the reaction floor {floor}");
        }
    }

    #[test]
    fn bots_buy_fight_and_finish_rounds() {
        // (Normal bots miss enough that the first kills take a while: give them four minutes.)
        let (g, _) = bot_match(Mode::Defuse, 240.0, Difficulty::Normal, 11);
        assert!(g.round >= 2, "rounds played: {} (phase {:?})", g.round, g.phase);
        let kills: i32 = g.players.iter().map(|p| p.stats.kills).sum();
        assert!(kills >= 5, "{kills} kills");
    }

    #[test]
    fn deathmatch_bots_hunt_each_other() {
        let (g, _) = bot_match(Mode::Deathmatch, 60.0, Difficulty::Hard, 5);
        let kills: i32 = g.players.iter().map(|p| p.stats.kills).sum();
        assert!(kills >= 5, "{kills} kills in a minute");
    }

    #[test]
    fn throw_angles_reach_the_spot() {
        // Simulate the throw with the grenade physics' numbers and check it lands near the target.
        let eye = Vec3::new(0.0, 1.63, 0.0);
        for &(dx, dy) in &[(8.0f32, 0.0f32), (15.0, 0.0), (22.0, -1.0), (12.0, 1.0)] {
            let at = Vec3::new(0.0, dy, -dx);
            let (yaw, pitch) = throw_angles(eye, at);
            let pd = pitch.to_degrees();
            let actual = (pd + (90.0 - pd.abs()) * 10.0 / 90.0).to_radians();
            let v = dir_from(yaw, actual) * 675.0 * 0.0254;
            let gr = 0.4 * 800.0 * 0.0254;
            // Time to reach the target's horizontal distance.
            let t = dx / Vec2::new(v.x, v.z).length();
            let y = eye.y + v.y * t - 0.5 * gr * t * t;
            assert!((y - dy).abs() < 0.3, "at {dx} m: lands at y {y}, want {dy}");
        }
    }
}
