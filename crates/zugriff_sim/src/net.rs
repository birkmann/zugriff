//! Online play: what a client and a server say to each other, and how the match travels.
//!
//! - **Transport:** one WebSocket per client (binary messages, [`encode`]/[`decode`] with postcard), the same in the
//!   browser and natively. The server (`server`) hosts many rooms; a room is one match.
//! - **The server decides.** It ticks the [`Game`] and its bots at [`TICK_HZ`](crate::game::TICK_HZ); clients send one
//!   [`UserCmd`] per tick and get a [`Snapshot`] per tick: the match's state ([`Replica`]) as a delta against the
//!   previous one on that connection (the socket is ordered and reliable, so the previous one always arrived), plus
//!   that tick's events.
//! - **The client predicts itself**: it runs its own commands through `Game::predict_player` at once and corrects
//!   when the server's answer comes back (`Snapshot::ack`); everyone else is drawn from snapshots a few ticks old, and
//!   shots are judged against that picture on the server (`lagcomp.rs`).
//! - **Slots:** a room has a fixed number of players per side. Bots fill the slots no one plays (or they stay empty,
//!   [`Player::vacant`](crate::game::Player)); a joining player takes a slot over, a leaving one hands it back. So
//!   player ids never change within a match.

use crate::game::{Bomb, Config, Dropped, Event, Game, Grenade, Hostage, Mode, Phase, Player, PlayerId, Reason, Smoke, Team, UserCmd};
use serde::{Deserialize, Serialize};

/// Bumped whenever a message or the replica changes shape.
pub const PROTOCOL: u32 = 1;
/// The game server's default port.
pub const DEFAULT_PORT: u16 = 27015;
/// The WebSocket path on a server.
pub const PATH: &str = "/zg";

/// What a player asks for when creating a room.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoomSetup {
    pub name: String,
    pub map: String,
    /// Defuse means "the map's own objective" (defusal or rescue); Deathmatch is deathmatch anywhere.
    pub mode: Mode,
    /// 0 easy … 3 expert.
    pub difficulty: u8,
    /// Players per side (1–8).
    pub per_side: u8,
    pub max_rounds: u32,
    /// Bots fill free slots; without them, free slots stay empty.
    pub bots: bool,
    /// Not in the server list; joined by its code.
    pub private: bool,
    /// Asked for on joining (empty: none).
    pub password: String,
    pub friendly_fire: bool,
}

impl Default for RoomSetup {
    fn default() -> Self {
        RoomSetup { name: String::new(), map: "zg_hafen".into(), mode: Mode::Defuse, difficulty: 1, per_side: 5, max_rounds: 16, bots: true, private: false, password: String::new(), friendly_fire: false }
    }
}

/// A room as the server list shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoomInfo {
    pub id: u32,
    pub name: String,
    pub map: String,
    pub mode: Mode,
    pub humans: u8,
    pub bots: u8,
    /// Slots in all (both sides).
    pub slots: u8,
    pub password: bool,
    pub private: bool,
    pub phase: Phase,
    pub round: u32,
    pub score: [u32; 2],
    /// The server hosting it: its name, and its address when it isn't the one asked (a list from a master server).
    pub server: String,
    pub address: Option<String>,
}

/// Which room to join.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RoomRef {
    Id(u32),
    /// A private room's code (as shown to its creator).
    Code(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientMsg {
    /// `finishes`: the weapon finish for each weapon (by `WeaponId as usize`), as `Player::set_finishes` takes it.
    Hello { protocol: u32, name: String, finishes: Vec<u8> },
    /// The room list (this server's and, on a master server, those announced to it).
    List,
    Create(RoomSetup),
    Join { room: RoomRef, password: String, team: Option<Team> },
    /// The map is loaded: the client plays its slot from now on (bots keep it until then).
    Ready,
    Leave,
    Cmd(UserCmd),
    Say { team: bool, text: String },
    /// A radio line; `order` is the order for the bots (the radio menu's index), if it is one.
    Radio { order: Option<u8>, text: String },
    /// Change sides.
    Team(Team),
    Ping(f64),
    Pong(f64),
    /// A dedicated server telling a master server about itself (every few seconds).
    Announce { address: String, name: String, rooms: Vec<RoomInfo> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome { server: String, motd: String },
    Rooms(Vec<RoomInfo>),
    /// In a room (after Create or Join, and again when the room changes map): load `room.map`, then send Ready.
    Joined(Box<Joined>),
    Snap(Box<Snapshot>),
    Chat(Chat),
    /// Out of the room (kicked, the room closed).
    Left { reason: String },
    Error(String),
    Ping(f64),
    Pong(f64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Joined {
    pub room: RoomInfo,
    pub setup: RoomSetup,
    pub cfg: Config,
    /// The private room's code (to pass on), or the public room's id as text.
    pub code: String,
    /// The creator runs the room (chat commands: /map, /kick, /restart, /bots, /diff).
    pub owner: bool,
    /// `nav::geometry_hash` of the server's map, to spot a client with a different version of it.
    pub map_hash: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chat {
    pub from: Option<PlayerId>,
    pub name: String,
    /// Team chat: only that side sees it.
    pub team: Option<Team>,
    pub text: String,
    /// From the server (joins, leaves, admin), not a player.
    pub notice: bool,
}

/// One tick of the match for one client.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    /// The game's tick (`Game::ticks`).
    pub tick: u32,
    /// The client's slot (None: watching, e.g. before Ready).
    pub you: Option<PlayerId>,
    /// The last of the client's commands the server has run (`UserCmd::seq`).
    pub ack: u32,
    /// The [`Replica`], postcard-encoded, XORed with the previous snapshot's bytes on this connection and packed.
    pub delta: Vec<u8>,
    /// Packed against nothing (the first after joining or reloading): the receiver starts its chain over here.
    pub fresh: bool,
    pub events: Vec<Event>,
    /// Everyone's round trip (ms, by player id; bots 0), about once a second.
    pub pings: Option<Vec<u16>>,
}

/// The match's state as it travels: everything of the [`Game`] but the level, the navigation, the config and the
/// generator (fixed per match, or the server's alone).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Replica {
    pub ticks: u32,
    pub time: f32,
    pub phase: Phase,
    pub phase_left: f32,
    pub live_for: f32,
    pub round: u32,
    pub score: [u32; 2],
    pub loss_streak: [u32; 2],
    pub bomb: Bomb,
    pub next_id: u32,
    pub last_result: Option<(Option<Team>, Reason)>,
    pub planted_this_round: bool,
    pub halftime_done: bool,
    pub winner: Option<Team>,
    pub beep_left: f32,
    pub players: Vec<Player>,
    pub hostages: Vec<Hostage>,
    pub dropped: Vec<Dropped>,
    pub grenades: Vec<Grenade>,
    pub smokes: Vec<Smoke>,
}

/// The same fields borrowed, for encoding without a copy (postcard encodes a slice as it does a Vec).
#[derive(Serialize)]
struct ReplicaRef<'a> {
    ticks: u32,
    time: f32,
    phase: Phase,
    phase_left: f32,
    live_for: f32,
    round: u32,
    score: [u32; 2],
    loss_streak: [u32; 2],
    bomb: Bomb,
    next_id: u32,
    last_result: Option<(Option<Team>, Reason)>,
    planted_this_round: bool,
    halftime_done: bool,
    winner: Option<Team>,
    beep_left: f32,
    players: &'a [Player],
    hostages: &'a [Hostage],
    dropped: &'a [Dropped],
    grenades: &'a [Grenade],
    smokes: &'a [Smoke],
}

impl Replica {
    /// The game's state as bytes.
    pub fn encode(g: &Game) -> Vec<u8> {
        let r = ReplicaRef {
            ticks: g.ticks,
            time: g.time,
            phase: g.phase,
            phase_left: g.phase_left,
            live_for: g.live_for,
            round: g.round,
            score: g.score,
            loss_streak: g.loss_streak,
            bomb: g.bomb,
            next_id: g.next_id,
            last_result: g.last_result,
            planted_this_round: g.planted_this_round,
            halftime_done: g.halftime_done,
            winner: g.winner,
            beep_left: g.beep_left,
            players: &g.players,
            hostages: &g.hostages,
            dropped: &g.dropped,
            grenades: &g.grenades,
            smokes: &g.smokes,
        };
        postcard::to_allocvec(&r).unwrap_or_default()
    }

    pub fn decode(bytes: &[u8]) -> Option<Replica> {
        postcard::from_bytes(bytes).ok()
    }

    /// Puts this state into a game (the client's copy, which keeps its level and config).
    pub fn apply(self, g: &mut Game) {
        g.ticks = self.ticks;
        g.time = self.time;
        g.phase = self.phase;
        g.phase_left = self.phase_left;
        g.live_for = self.live_for;
        g.round = self.round;
        g.score = self.score;
        g.loss_streak = self.loss_streak;
        g.bomb = self.bomb;
        g.next_id = self.next_id;
        g.last_result = self.last_result;
        g.planted_this_round = self.planted_this_round;
        g.halftime_done = self.halftime_done;
        g.winner = self.winner;
        g.beep_left = self.beep_left;
        g.players = self.players;
        g.hostages = self.hostages;
        g.dropped = self.dropped;
        g.grenades = self.grenades;
        g.smokes = self.smokes;
    }
}

/// A message as bytes.
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_allocvec(msg).unwrap_or_default()
}

pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}

/// Delta packing between successive states on one connection: XOR with the previous bytes (most of a tick's state is
/// what it was a tick ago, so mostly zeros), then runs of zeros written as counts.
///
/// Packed form: the new length, then pairs of (zeros to skip, literal bytes count) each followed by the literals.
#[derive(Clone, Debug, Default)]
pub struct Delta {
    last: Vec<u8>,
}

impl Delta {
    /// Starts over from nothing (a new match or map: the next pack carries everything).
    pub fn reset(&mut self) {
        self.last.clear();
    }

    pub fn pack(&mut self, now: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 + now.len() / 8);
        put_varint(&mut out, now.len() as u64);
        let mut i = 0;
        let x = |i: usize| now[i] ^ self.last.get(i).copied().unwrap_or(0);
        while i < now.len() {
            let start = i;
            while i < now.len() && x(i) == 0 {
                i += 1;
            }
            let zeros = i - start;
            // Literals run until a stretch of 3+ zeros (shorter gaps are cheaper inline).
            let lit_start = i;
            while i < now.len() {
                if x(i) == 0 && (i + 2 >= now.len() || (x(i + 1) == 0 && x(i + 2) == 0)) {
                    break;
                }
                i += 1;
            }
            if i == lit_start && i >= now.len() {
                // Trailing zeros: implied by the length.
                break;
            }
            put_varint(&mut out, zeros as u64);
            put_varint(&mut out, (i - lit_start) as u64);
            for j in lit_start..i {
                out.push(x(j));
            }
        }
        self.last.clear();
        self.last.extend_from_slice(now);
        out
    }

    /// The state from a pack (None if it is malformed).
    pub fn unpack(&mut self, packed: &[u8]) -> Option<&[u8]> {
        let mut r = packed;
        let len = get_varint(&mut r)? as usize;
        if len > 64 << 20 {
            return None;
        }
        self.last.resize(len, 0);
        let mut i = 0;
        while !r.is_empty() {
            let zeros = get_varint(&mut r)? as usize;
            let lits = get_varint(&mut r)? as usize;
            i += zeros;
            if i + lits > len || lits > r.len() {
                return None;
            }
            for (j, b) in r[..lits].iter().enumerate() {
                self.last[i + j] ^= b;
            }
            r = &r[lits..];
            i += lits;
        }
        Some(&self.last)
    }
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn get_varint(r: &mut &[u8]) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let (&b, rest) = r.split_first()?;
        *r = rest;
        v |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// A private room's code: six letters and digits without the easily confused ones.
pub fn room_code(n: u64) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut n = n;
    (0..6)
        .map(|_| {
            let c = ALPHABET[(n % ALPHABET.len() as u64) as usize] as char;
            n /= ALPHABET.len() as u64;
            c
        })
        .collect()
}

/// A player name as the server accepts it: trimmed, printable, at most 24 characters.
pub fn clean_name(name: &str, fallback: &str) -> String {
    let n: String = name.chars().filter(|c| !c.is_control()).take(24).collect();
    let n = n.trim();
    if n.is_empty() { fallback.to_string() } else { n.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tests::game;

    #[test]
    fn delta_round_trips() {
        let mut a = Delta::default();
        let mut b = Delta::default();
        let states: [&[u8]; 5] = [b"hello world, this is a state", b"hello world, this is a state", b"hellO world, this is a staTe!!", b"short", b""];
        for s in states {
            let p = a.pack(s);
            assert_eq!(b.unpack(&p).unwrap(), s);
        }
    }

    #[test]
    fn replica_round_trips_and_packs_small() {
        let mut g = game(Mode::Defuse);
        let mut client = game(Mode::Defuse);
        let (mut tx, mut rx) = (Delta::default(), Delta::default());
        let mut sizes = Vec::new();
        for _ in 0..40 {
            g.tick(&[]);
            let bytes = Replica::encode(&g);
            let packed = tx.pack(&bytes);
            sizes.push(packed.len());
            let got = rx.unpack(&packed).unwrap();
            Replica::decode(got).unwrap().apply(&mut client);
        }
        assert_eq!(client.ticks, g.ticks);
        assert_eq!(client.players.len(), g.players.len());
        for (a, b) in client.players.iter().zip(&g.players) {
            assert_eq!(a.mover.origin, b.mover.origin);
            assert_eq!(a.health, b.health);
        }
        // After the first, a tick's delta is a small part of the full state.
        let full = Replica::encode(&g).len();
        assert!(sizes[20] * 4 < full, "delta {} vs full {}", sizes[20], full);
    }

    #[test]
    fn messages_round_trip() {
        let msg = ServerMsg::Snap(Box::new(Snapshot { tick: 7, you: Some(1), ack: 3, delta: vec![1, 2, 3], fresh: true, events: vec![Event::Money { player: 0, amount: 300, reason: "Rundensieg".into() }], pings: None }));
        let bytes = encode(&msg);
        let back: ServerMsg = decode(&bytes).unwrap();
        let ServerMsg::Snap(s) = back else { panic!() };
        assert_eq!(s.tick, 7);
        assert_eq!(s.events.len(), 1);
    }

    #[test]
    fn codes_are_six_clear_characters() {
        let c = room_code(123_456_789);
        assert_eq!(c.len(), 6);
        assert!(c.chars().all(|c| !"01IO".contains(c)));
    }
}
