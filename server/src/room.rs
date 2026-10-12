//! A room: one match ticking at the sim's rate, its bots, and the people in it. Each tick takes one command per
//! player, runs the game and sends every player a snapshot (the state as a delta, the tick's events).
//!
//! Slots: the match has `per_side` players a side from the start. Bots play the slots no one plays (or, without
//! bots, those stay empty); a person joining takes a slot over — the bot's body mid-round, like stepping in — and
//! hands it back on leaving. Before the client has loaded the map (`Ready`) a bot keeps playing its slot.

use crate::conn::{self, Out};
use crate::hub::{Hub, Map, RoomHandle};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::sync::oneshot;
use zugriff_sim::bots::{Bots, Difficulty, Order};
use zugriff_sim::game::{Config, Event, Game, Mode, Phase, PlayerId, Team, UserCmd, TICK_HZ};
use zugriff_sim::net::{Chat, ClientMsg, Delta, Joined, Replica, RoomInfo, RoomSetup, ServerMsg, Snapshot};
use zugriff_sim::rng::Rng;

pub struct JoinReq {
    pub client: u64,
    pub name: String,
    pub finishes: Vec<u8>,
    pub team: Option<Team>,
    pub owner: bool,
    pub out: Out,
    pub reply: oneshot::Sender<Result<(), String>>,
}

pub enum RoomIn {
    Join(Box<JoinReq>),
    Msg(u64, ClientMsg),
    Gone(u64),
}

/// Commands queued beyond this are dropped (the client's clock runs ahead, or it sent a burst).
const MAX_QUEUE: usize = 6;
/// Commands kept in hand before the server starts using them (again): browsers send them in clumps with their
/// frames, and a tick without one repeats the last, which the client didn't predict.
const CMD_CUSHION: usize = 2;
/// A room created by a player closes this long after the last one left.
const EMPTY_CLOSE: Duration = Duration::from_secs(45);
/// The end screen shows this long before the next match.
const END_SCREEN: f32 = 12.0;

struct Client {
    out: Out,
    name: String,
    /// The weapon finishes the player chose (their guns show them).
    finishes: Vec<u8>,
    slot: PlayerId,
    ready: bool,
    cmds: VecDeque<UserCmd>,
    /// The cushion is filled: commands are being used.
    primed: bool,
    last: UserCmd,
    ack: u32,
    delta: Delta,
    /// The next snapshot starts the delta chain over.
    fresh: bool,
    ping: u16,
    /// Chat: messages in the last few seconds (flood guard).
    said: VecDeque<Instant>,
}

struct Room {
    hub: Arc<Hub>,
    id: u32,
    code: String,
    setup: RoomSetup,
    map: Map,
    rotation: Vec<String>,
    persistent: bool,
    game: Game,
    bots: Bots,
    rng: Rng,
    clients: HashMap<u64, Client>,
    owner: Option<u64>,
    info: Arc<Mutex<RoomInfo>>,
    /// Events that happened between ticks (joins, radio), sent with the next snapshot.
    extra: Vec<Event>,
    empty_since: Option<Instant>,
    ticks: u64,
}

pub fn spawn(hub: Arc<Hub>, id: u32, code: String, setup: RoomSetup, map: Map, rotation: Vec<String>, persistent: bool) -> RoomHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let seed = hub.now().to_bits() ^ (id as u64) << 40;
    let (game, bots) = new_game(&setup, &map, None, seed);
    let info = Arc::new(Mutex::new(RoomInfo {
        id,
        name: setup.name.clone(),
        map: setup.map.clone(),
        mode: game.cfg.mode,
        humans: 0,
        bots: 0,
        slots: game.players.len() as u8,
        password: !setup.password.is_empty(),
        private: setup.private,
        phase: game.phase,
        round: 0,
        score: [0, 0],
        server: hub.name.clone(),
        address: None,
    }));
    let handle = RoomHandle { tx, info: info.clone(), code: code.clone(), password: setup.password.clone() };
    let room = Room { hub, id, code, setup, map, rotation, persistent, game, bots, rng: Rng::new(seed ^ 0x1234), clients: HashMap::new(), owner: None, info, extra: Vec::new(), empty_since: Some(Instant::now()), ticks: 0 };
    tokio::spawn(room.run(rx));
    handle
}

/// The mode a map is made for.
fn mode_for(level: &zugriff_sim::compile::Level) -> Mode {
    if level.volumes_of("func_bomb_target").next().is_some() {
        Mode::Defuse
    } else if level.entities_of("info_hostage").next().is_some() {
        Mode::Rescue
    } else {
        Mode::Deathmatch
    }
}

fn difficulty(d: u8) -> Difficulty {
    [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard, Difficulty::Expert][d.min(3) as usize]
}

/// A new match. With `slots` (the last match's players), everyone keeps their slot, side and name.
fn new_game(setup: &RoomSetup, map: &Map, slots: Option<&Game>, seed: u64) -> (Game, Bots) {
    let (level, nav) = map.clone();
    let mode = if setup.mode == Mode::Deathmatch { Mode::Deathmatch } else { mode_for(&level) };
    let cfg = Config { mode, max_rounds: setup.max_rounds, warmup: 25.0, friendly_fire: setup.friendly_fire, ..Default::default() };
    let mut game = Game::new(level, cfg, seed);
    game.nav = Some(nav.clone());
    zugriff_sim::lagcomp::enable(&mut game);
    let bots = Bots::new(nav, difficulty(setup.difficulty), seed ^ 0x55);
    let mut rng = Rng::new(seed ^ 0x77);
    match slots {
        Some(old) => {
            for p in &old.players {
                let id = game.add_player(&p.name, p.team, p.bot);
                game.players[id].set_finishes(p.finishes.clone());
                if p.vacant {
                    game.set_vacant(id, true);
                }
            }
        }
        None => {
            for i in 0..setup.per_side as usize * 2 {
                let team = if i % 2 == 0 { Team::Ksk } else { Team::Opfor };
                let name = Bots::name_for(&game, team, &mut rng);
                let id = game.add_player(&name, team, true);
                if !setup.bots {
                    game.set_vacant(id, true);
                    game.players[id].name = "—".into();
                }
            }
        }
    }
    (game, bots)
}

impl Room {
    async fn run(mut self, mut rx: UnboundedReceiver<RoomIn>) {
        let mut interval = tokio::time::interval(Duration::from_secs_f64(1.0 / TICK_HZ as f64));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
        let mut last_info = Instant::now() - Duration::from_secs(5);
        loop {
            interval.tick().await;
            loop {
                match rx.try_recv() {
                    Ok(m) => self.handle(m).await,
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => return,
                }
            }
            self.drop_dead();
            if self.clients.is_empty() {
                let since = *self.empty_since.get_or_insert_with(Instant::now);
                if !self.persistent && since.elapsed() > EMPTY_CLOSE {
                    self.hub.remove(self.id);
                    println!("room {} closed (empty)", self.id);
                    return;
                }
            } else {
                self.empty_since = None;
                self.tick();
                if self.game.phase == Phase::MatchOver && self.game.phase_left < 15.0 - END_SCREEN {
                    self.next_match().await;
                }
            }
            if last_info.elapsed() > Duration::from_secs(1) {
                last_info = Instant::now();
                self.update_info();
                self.ping();
            }
        }
    }

    // ---- people -------------------------------------------------------------------------------------------------

    async fn handle(&mut self, m: RoomIn) {
        match m {
            RoomIn::Join(req) => self.join(*req),
            RoomIn::Gone(id) => self.leave(id, None),
            RoomIn::Msg(id, msg) => self.message(id, msg).await,
        }
    }

    /// A free slot on a side: one no person plays (a bot's, or an empty one).
    fn free_slot(&self, team: Team) -> Option<PlayerId> {
        let taken: Vec<PlayerId> = self.clients.values().map(|c| c.slot).collect();
        let free = |p: &&zugriff_sim::game::Player| p.team == team && !taken.contains(&p.id);
        // A bot's slot first (a body to step into), then an empty one.
        self.game.players.iter().filter(free).find(|p| !p.vacant).or_else(|| self.game.players.iter().find(|p| free(p))).map(|p| p.id)
    }

    fn humans(&self, team: Team) -> usize {
        self.clients.values().filter(|c| self.game.players[c.slot].team == team).count()
    }

    fn join(&mut self, req: JoinReq) {
        // The side asked for, else the one with fewer people.
        let fewer = if self.humans(Team::Ksk) <= self.humans(Team::Opfor) { Team::Ksk } else { Team::Opfor };
        let first = req.team.unwrap_or(fewer);
        let Some(slot) = self.free_slot(first).or_else(|| self.free_slot(first.other())) else {
            let _ = req.reply.send(Err("Der Raum ist voll.".into()));
            return;
        };
        let mut name = req.name.clone();
        let mut n = 2;
        while self.clients.values().any(|c| c.name == name) || self.game.players.iter().any(|p| p.name == name && p.bot) {
            name = format!("{} ({n})", req.name);
            n += 1;
        }
        self.game.players[slot].name = name.clone();
        self.game.players[slot].stats = Default::default();
        if req.owner {
            self.owner = Some(req.client);
        }
        let client = Client { out: req.out, name: name.clone(), finishes: req.finishes, slot, ready: false, cmds: VecDeque::new(), primed: false, last: UserCmd::default(), ack: 0, delta: Delta::default(), fresh: true, ping: 0, said: VecDeque::new() };
        self.clients.insert(req.client, client);
        let _ = req.reply.send(Ok(()));
        self.send_joined(req.client);
        self.notice(&format!("{name} ist beigetreten ({}).", self.game.players[slot].team.name()));
        println!("room {}: {name} joined slot {slot}", self.id);
        self.update_info();
    }

    fn send_joined(&mut self, client: u64) {
        let info = self.info.lock().unwrap().clone();
        let owner = self.owner == Some(client);
        let joined = Joined { room: info, setup: RoomSetup { password: String::new(), ..self.setup.clone() }, cfg: self.game.cfg.clone(), code: self.code.clone(), owner, map_hash: zugriff_sim::nav::geometry_hash(&self.map.0.world) };
        if let Some(c) = self.clients.get_mut(&client) {
            c.ready = false;
            c.delta.reset();
            c.fresh = true;
            c.cmds.clear();
            conn::send(&c.out, &ServerMsg::Joined(Box::new(joined)));
        }
    }

    /// Someone left (or was dropped): their slot goes back to a bot, or stays empty.
    fn leave(&mut self, client: u64, reason: Option<&str>) {
        let Some(c) = self.clients.remove(&client) else { return };
        if let Some(r) = reason {
            conn::send(&c.out, &ServerMsg::Left { reason: r.into() });
        }
        self.release(c.slot);
        self.notice(&format!("{} hat das Spiel verlassen.", c.name));
        println!("room {}: {} left", self.id, c.name);
        if self.owner == Some(client) {
            self.owner = self.clients.keys().next().copied();
            if let Some(o) = self.owner {
                let name = self.clients[&o].name.clone();
                self.notice(&format!("{name} leitet jetzt den Raum."));
            }
        }
        self.update_info();
    }

    fn release(&mut self, slot: PlayerId) {
        self.bots.forget(slot);
        self.game.players[slot].set_finishes(Vec::new());
        if self.setup.bots {
            let team = self.game.players[slot].team;
            let was_vacant = self.game.players[slot].vacant;
            self.game.players[slot].name = String::new();
            let name = Bots::name_for(&self.game, team, &mut self.rng);
            let p = &mut self.game.players[slot];
            p.name = name;
            p.bot = true;
            if was_vacant {
                self.game.set_vacant(slot, false);
            }
        } else {
            self.game.set_vacant(slot, true);
            self.game.players[slot].name = "—".into();
        }
    }

    /// The client now plays its slot. `fresh`: it just (re)loaded the map, so its snapshots start over from nothing
    /// (both ends reset their delta then; a side switch keeps the chain going).
    fn take(&mut self, client: u64, fresh: bool) {
        let Some(c) = self.clients.get_mut(&client) else { return };
        c.ready = true;
        if fresh {
            c.delta.reset();
            c.fresh = true;
        }
        let slot = c.slot;
        let finishes = c.finishes.clone();
        self.game.players[slot].set_finishes(finishes);
        self.bots.forget(slot);
        let vacant = self.game.players[slot].vacant;
        self.game.players[slot].bot = false;
        if vacant {
            self.game.set_vacant(slot, false);
        }
    }

    async fn message(&mut self, client: u64, msg: ClientMsg) {
        if !self.clients.contains_key(&client) {
            return;
        }
        match msg {
            ClientMsg::Ready => self.take(client, true),
            ClientMsg::Cmd(cmd) => {
                let c = self.clients.get_mut(&client).unwrap();
                if c.ready {
                    c.cmds.push_back(cmd);
                    while c.cmds.len() > MAX_QUEUE {
                        c.cmds.pop_front();
                    }
                }
            }
            ClientMsg::Pong(t) => {
                let ms = ((self.hub.now() - t).max(0.0) * 1000.0).min(9999.0) as u16;
                let c = self.clients.get_mut(&client).unwrap();
                c.ping = if c.ping == 0 { ms } else { ((c.ping as u32 * 3 + ms as u32) / 4) as u16 };
            }
            ClientMsg::Say { team, text } => self.say(client, team, text).await,
            ClientMsg::Radio { order, text } => {
                let slot = self.clients[&client].slot;
                let p = &self.game.players[slot];
                if !p.alive {
                    return;
                }
                let team = p.team;
                if let Some(o) = order {
                    let order = match o {
                        0 => Order::FollowMe(slot),
                        1 => Order::Hold(slot),
                        2 => Order::Regroup(slot),
                        3 => Order::GoSite('A'),
                        4 => Order::GoSite('B'),
                        _ => Order::Storm,
                    };
                    self.bots.order(team, order);
                }
                let text: String = text.chars().take(80).collect();
                self.extra.push(Event::Radio { player: slot, text });
            }
            ClientMsg::Team(team) => self.switch_team(client, team),
            _ => {}
        }
    }

    fn switch_team(&mut self, client: u64, team: Team) {
        let old = self.clients[&client].slot;
        if self.game.players[old].team == team {
            return;
        }
        let Some(new) = self.free_slot(team) else {
            self.tell(client, "Auf dieser Seite ist kein Platz frei.");
            return;
        };
        let name = self.clients[&client].name.clone();
        let (kills, deaths) = (self.game.players[old].stats.kills, self.game.players[old].stats.deaths);
        self.release(old);
        // Changing sides mid-round: the new slot's body is out until the next spawn.
        let rounds = self.game.cfg.mode != Mode::Deathmatch && self.game.phase != Phase::Warmup;
        if rounds && self.game.players[new].alive {
            self.game.set_vacant(new, true);
            self.game.set_vacant(new, false);
        }
        let p = &mut self.game.players[new];
        p.name = name.clone();
        p.stats = Default::default();
        p.stats.kills = kills;
        p.stats.deaths = deaths;
        self.clients.get_mut(&client).unwrap().slot = new;
        self.take(client, false);
        self.notice(&format!("{name} wechselt zu {}.", team.name()));
    }

    fn tell(&self, client: u64, text: &str) {
        if let Some(c) = self.clients.get(&client) {
            conn::send(&c.out, &ServerMsg::Chat(Chat { from: None, name: String::new(), team: None, text: text.into(), notice: true }));
        }
    }

    fn notice(&self, text: &str) {
        let msg = ServerMsg::Chat(Chat { from: None, name: String::new(), team: None, text: text.into(), notice: true });
        for c in self.clients.values() {
            conn::send(&c.out, &msg);
        }
    }

    async fn say(&mut self, client: u64, team: bool, text: String) {
        let text: String = text.chars().filter(|c| !c.is_control()).take(160).collect();
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(cmd) = text.strip_prefix('/') {
            self.command(client, cmd).await;
            return;
        }
        let c = self.clients.get_mut(&client).unwrap();
        let now = Instant::now();
        c.said.retain(|t| now.duration_since(*t) < Duration::from_secs(5));
        if c.said.len() >= 5 {
            let out = c.out.clone();
            conn::send(&out, &ServerMsg::Chat(Chat { from: None, name: String::new(), team: None, text: "Langsamer – zu viele Nachrichten.".into(), notice: true }));
            return;
        }
        c.said.push_back(now);
        let slot = c.slot;
        let p = &self.game.players[slot];
        let side = team.then_some(p.team);
        let msg = ServerMsg::Chat(Chat { from: Some(slot), name: c.name.clone(), team: side, text, notice: false });
        for other in self.clients.values() {
            if side.is_none_or(|t| self.game.players[other.slot].team == t) {
                conn::send(&other.out, &msg);
            }
        }
    }

    /// The room's commands (in chat, starting with /): anyone may ask for help; the rest are the owner's.
    async fn command(&mut self, client: u64, line: &str) {
        let mut parts = line.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_lowercase();
        let arg = parts.collect::<Vec<_>>().join(" ");
        let owner = self.owner == Some(client);
        match cmd.as_str() {
            "help" | "hilfe" => {
                let maps = self.hub.maps_available().join(", ");
                self.tell(client, "/code · /map <karte> · /restart · /kick <name> · /bots an|aus · /stärke 0–3 · /runden <n> (Leitung)");
                self.tell(client, &format!("Karten: {maps}"));
            }
            "code" => {
                let text = if self.setup.private { format!("Code dieses Raums: {}", self.code) } else { format!("Öffentlicher Raum Nr. {}", self.id) };
                self.tell(client, &text);
            }
            _ if !owner => self.tell(client, "Nur die Raumleitung kann das."),
            "map" | "karte" => {
                let hub = self.hub.clone();
                let name = arg.clone();
                match tokio::task::spawn_blocking(move || hub.load_map(&name)).await {
                    Ok(Ok(map)) => {
                        self.setup.map = arg.clone();
                        self.map = map;
                        self.notice(&format!("Kartenwechsel: {arg}"));
                        self.restart(true);
                    }
                    Ok(Err(e)) => self.tell(client, &e),
                    Err(_) => {}
                }
            }
            "restart" | "neustart" => {
                self.notice("Das Spiel beginnt neu.");
                self.restart(true);
            }
            "kick" => {
                let target = self.clients.iter().find(|(id, c)| **id != client && c.name.eq_ignore_ascii_case(arg.trim())).map(|(id, _)| *id);
                match target {
                    Some(t) => self.leave(t, Some("Aus dem Raum entfernt.")),
                    None => self.tell(client, "Niemand mit diesem Namen."),
                }
            }
            "bots" => {
                let on = matches!(arg.as_str(), "an" | "on" | "1" | "ein");
                if on != self.setup.bots {
                    self.setup.bots = on;
                    for id in 0..self.game.players.len() {
                        if self.clients.values().any(|c| c.slot == id) {
                            continue;
                        }
                        self.release(id);
                    }
                    self.notice(if on { "Bots füllen die freien Plätze." } else { "Keine Bots mehr: freie Plätze bleiben leer." });
                }
            }
            "stärke" | "staerke" | "diff" => {
                if let Ok(d) = arg.parse::<u8>() {
                    self.setup.difficulty = d.min(3);
                    self.bots.difficulty = difficulty(d);
                    self.notice(&format!("Bot-Stärke: {}", ["Leicht", "Normal", "Schwer", "Experte"][d.min(3) as usize]));
                }
            }
            "runden" | "rounds" => {
                if let Ok(r) = arg.parse::<u32>() {
                    self.setup.max_rounds = r.clamp(2, 30);
                    self.notice(&format!("Ab dem nächsten Spiel: {} Runden.", self.setup.max_rounds));
                }
            }
            _ => self.tell(client, "Unbekannter Befehl – /hilfe"),
        }
    }

    // ---- the match ----------------------------------------------------------------------------------------------

    fn drop_dead(&mut self) {
        let dead: Vec<u64> = self.clients.iter().filter(|(_, c)| c.out.is_closed()).map(|(id, _)| *id).collect();
        for id in dead {
            self.leave(id, None);
        }
    }

    fn tick(&mut self) {
        let mut cmds: Vec<(PlayerId, UserCmd)> = Vec::with_capacity(self.game.players.len());
        cmds.extend(self.bots.think(&self.game));
        for c in self.clients.values_mut() {
            if !c.ready {
                continue;
            }
            if c.cmds.len() >= CMD_CUSHION {
                c.primed = true;
            }
            let next = if c.primed { c.cmds.pop_front() } else { None };
            if next.is_none() {
                c.primed = false;
            }
            let cmd = match next {
                Some(cmd) => {
                    c.ack = cmd.seq;
                    c.last = cmd.clone();
                    cmd
                }
                // Nothing arrived in time: the last one again, without its one-off parts.
                None => UserCmd { select: None, buy: Vec::new(), seq: 0, ..c.last.clone() },
            };
            cmds.push((c.slot, cmd));
        }
        self.game.tick(&cmds);
        self.ticks += 1;
        let mut events: Vec<Event> = std::mem::take(&mut self.extra);
        events.extend(self.game.events.drain(..));
        events.extend(self.bots.drain_radio());
        let state = Replica::encode(&self.game);
        let pings = (self.ticks % 66 == 0).then(|| {
            let mut v = vec![0u16; self.game.players.len()];
            for c in self.clients.values() {
                v[c.slot] = c.ping.max(1);
            }
            v
        });
        let mut slow = Vec::new();
        for (id, c) in self.clients.iter_mut() {
            if !c.ready {
                continue;
            }
            let snap = Snapshot { tick: self.game.ticks, you: Some(c.slot), ack: c.ack, delta: c.delta.pack(&state), fresh: std::mem::take(&mut c.fresh), events: events.clone(), pings: pings.clone() };
            if !conn::send(&c.out, &ServerMsg::Snap(Box::new(snap))) {
                slow.push(*id);
            }
        }
        for id in slow {
            self.leave(id, None);
        }
    }

    /// After the end screen: the next match (the next map of a rotation), everyone staying in their slots.
    async fn next_match(&mut self) {
        if self.rotation.len() > 1 {
            let i = self.rotation.iter().position(|m| *m == self.setup.map).map_or(0, |i| (i + 1) % self.rotation.len());
            let next = self.rotation[i].clone();
            let hub = self.hub.clone();
            let name = next.clone();
            if let Ok(Ok(map)) = tokio::task::spawn_blocking(move || hub.load_map(&name)).await {
                self.setup.map = next;
                self.map = map;
            }
        }
        self.restart(true);
    }

    fn restart(&mut self, keep_slots: bool) {
        let seed = self.hub.now().to_bits() ^ self.ticks;
        let (game, bots) = new_game(&self.setup, &self.map, keep_slots.then_some(&self.game), seed);
        self.game = game;
        self.bots = bots;
        {
            let mut info = self.info.lock().unwrap();
            info.map = self.setup.map.clone();
            info.mode = self.game.cfg.mode;
        }
        let ids: Vec<u64> = self.clients.keys().copied().collect();
        for id in ids {
            self.send_joined(id);
        }
        // Until they've loaded again, bots keep the slots (or, without bots, they wait empty); see `take`.
        let slots: Vec<PlayerId> = self.clients.values().map(|c| c.slot).collect();
        for slot in slots {
            if self.setup.bots {
                self.game.players[slot].bot = true;
            } else {
                self.game.set_vacant(slot, true);
            }
        }
    }

    fn update_info(&mut self) {
        let mut info = self.info.lock().unwrap();
        info.humans = self.clients.len() as u8;
        info.bots = self.game.players.iter().filter(|p| p.bot && !p.vacant).count() as u8 - self.clients.values().filter(|c| !c.ready && self.game.players[c.slot].bot).count() as u8;
        info.slots = self.game.players.len() as u8;
        info.phase = self.game.phase;
        info.round = self.game.round;
        info.score = self.game.score;
        info.name = self.setup.name.clone();
    }

    fn ping(&self) {
        let msg = ServerMsg::Ping(self.hub.now());
        for c in self.clients.values() {
            conn::send(&c.out, &msg);
        }
    }
}
