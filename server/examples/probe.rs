//! A headless check of a running server: two clients create and join a private room by its code, play a few seconds
//! (walking forward, firing), chat, switch sides and leave. Prints what arrived and the snapshots' sizes.
//!
//! `cargo run --release -p zugriff_server --example probe -- [ws://127.0.0.1:27015/zg] [map] [seconds]`

use futures_util::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;
use zugriff_sim::game::{Btn, Team, UserCmd};
use zugriff_sim::net::{self, ClientMsg, Delta, Replica, RoomRef, RoomSetup, ServerMsg, PROTOCOL};

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Client {
    ws: Ws,
    name: &'static str,
    delta: Delta,
    snaps: usize,
    bytes: usize,
    events: usize,
    last: Option<Replica>,
    you: Option<usize>,
    ack: u32,
    chat: Vec<String>,
    joined: Option<net::Joined>,
    errors: Vec<String>,
}

impl Client {
    async fn open(url: &str, name: &'static str) -> Client {
        let (ws, _) = tokio_tungstenite::connect_async(url).await.expect("connect");
        let mut c = Client { ws, name, delta: Delta::default(), snaps: 0, bytes: 0, events: 0, last: None, you: None, ack: 0, chat: Vec::new(), joined: None, errors: Vec::new() };
        c.send(&ClientMsg::Hello { protocol: PROTOCOL, name: name.into(), finishes: vec![0, 1, 2] }).await;
        c
    }

    async fn send(&mut self, m: &ClientMsg) {
        self.ws.send(Message::Binary(net::encode(m).into())).await.expect("send");
    }

    /// Reads what arrives within `d`.
    async fn pump(&mut self, d: Duration) {
        let until = Instant::now() + d;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            let Ok(Some(Ok(Message::Binary(b)))) = tokio::time::timeout(left, self.ws.next()).await else { return };
            let Some(m) = net::decode::<ServerMsg>(&b) else { panic!("undecodable message") };
            match m {
                ServerMsg::Joined(j) => {
                    println!("[{}] joined {:?} code {} owner {} map {}", self.name, j.room.name, j.code, j.owner, j.room.map);
                    self.delta.reset();
                    self.joined = Some(*j);
                    self.send(&ClientMsg::Ready).await;
                }
                ServerMsg::Snap(s) => {
                    self.snaps += 1;
                    self.bytes += b.len();
                    self.events += s.events.len();
                    if s.fresh {
                        self.delta.reset();
                    }
                    let state = self.delta.unpack(&s.delta).expect("delta").to_vec();
                    self.last = Some(Replica::decode(&state).expect("replica"));
                    self.you = s.you;
                    self.ack = s.ack;
                }
                ServerMsg::Chat(c) => self.chat.push(format!("{}{}: {}", if c.notice { "* " } else { "" }, c.name, c.text)),
                ServerMsg::Error(e) => self.errors.push(e),
                ServerMsg::Ping(t) => self.send(&ClientMsg::Pong(t)).await,
                _ => {}
            }
        }
    }

    fn me(&self) -> Option<&zugriff_sim::game::Player> {
        self.last.as_ref()?.players.get(self.you?)
    }
}

#[tokio::main]
async fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let url = argv.get(1).cloned().unwrap_or_else(|| "ws://127.0.0.1:27015/zg".into());
    let map = argv.get(2).cloned().unwrap_or_else(|| "dev_arena".into());
    let secs: f32 = argv.get(3).and_then(|s| s.parse().ok()).unwrap_or(4.0);

    let mut a = Client::open(&url, "Anna").await;
    a.send(&ClientMsg::Create(RoomSetup { name: "Probe".into(), map: map.clone(), private: true, password: "geheim".into(), per_side: 3, ..Default::default() })).await;
    a.pump(Duration::from_millis(1500)).await;
    let code = a.joined.as_ref().expect("room created").code.clone();
    assert_eq!(code.len(), 6, "a private room has a six-character code");

    let mut b = Client::open(&url, "Bert").await;
    // Wrong password first, then the right one.
    b.send(&ClientMsg::Join { room: RoomRef::Code(code.clone()), password: "falsch".into(), team: Some(Team::Opfor) }).await;
    b.pump(Duration::from_millis(300)).await;
    assert!(b.errors.iter().any(|e| e.contains("Passwort")), "wrong password refused: {:?}", b.errors);
    b.send(&ClientMsg::Join { room: RoomRef::Code(code.clone()), password: "geheim".into(), team: Some(Team::Opfor) }).await;
    b.pump(Duration::from_millis(800)).await;
    assert!(b.joined.is_some(), "joined by code");

    // The room is private: not in the list.
    let mut c = Client::open(&url, "Lister").await;
    c.send(&ClientMsg::List).await;
    c.pump(Duration::from_millis(300)).await;

    // Play: both walk forward and fire now and then.
    let start = Instant::now();
    let mut seq = 0u32;
    while start.elapsed().as_secs_f32() < secs {
        seq += 1;
        for cl in [&mut a, &mut b] {
            let fire = seq % 40 < 3;
            let cmd = UserCmd { forward: 1.0, yaw: seq as f32 * 0.01, buttons: if fire { Btn::ATTACK } else { Btn::empty() }, seq, view_tick: cl.last.as_ref().map_or(0, |r| r.ticks), ..Default::default() };
            cl.send(&ClientMsg::Cmd(cmd)).await;
        }
        a.pump(Duration::from_millis(7)).await;
        b.pump(Duration::from_millis(8)).await;
    }
    a.send(&ClientMsg::Say { team: false, text: "Hallo zusammen".into() }).await;
    b.send(&ClientMsg::Say { team: true, text: "nur OPFOR".into() }).await;
    a.send(&ClientMsg::Say { team: false, text: "/code".into() }).await;
    b.send(&ClientMsg::Team(Team::Ksk)).await;
    a.pump(Duration::from_millis(400)).await;
    b.pump(Duration::from_millis(400)).await;

    for cl in [&a, &b] {
        let Some(me) = cl.me() else { panic!("[{}] no slot: {} snapshots, you {:?}, errors {:?}, chat {:?}", cl.name, cl.snaps, cl.you, cl.errors, cl.chat) };
        let avg = cl.bytes / cl.snaps.max(1);
        println!(
            "[{}] {} snapshots ({:.0}/s), {} bytes each on average, {} events; slot {:?} ({}, {}), ack {} of {seq}, humans in replica: {}",
            cl.name,
            cl.snaps,
            cl.snaps as f32 / secs,
            avg,
            cl.events,
            cl.you,
            me.name,
            me.team.name(),
            cl.ack,
            cl.last.as_ref().unwrap().players.iter().filter(|p| !p.bot && !p.vacant).count()
        );
        for line in &cl.chat {
            println!("[{}]   chat {line}", cl.name);
        }
    }
    assert!(a.snaps as f32 > secs * 50.0, "about 66 snapshots a second");
    assert!(b.chat.iter().any(|l| l.contains("nur OPFOR")), "team chat reaches the sender's side");
    assert!(!a.chat.iter().any(|l| l.contains("nur OPFOR")), "team chat stays on its side");
    assert!(a.chat.iter().any(|l| l.contains(&code)), "/code answers with the code");
    assert_eq!(b.me().unwrap().team, Team::Ksk, "switched sides");
    assert!(a.ack + 10 >= seq, "commands are being run");
    // The owner changes the map: everyone gets Joined again, reloads (Ready) and snapshots go on from a fresh chain.
    let before = (a.snaps, b.snaps);
    b.send(&ClientMsg::Say { team: false, text: "/restart".into() }).await;
    a.send(&ClientMsg::Say { team: false, text: "/map zg_hafen".into() }).await;
    a.pump(Duration::from_millis(1500)).await;
    b.pump(Duration::from_millis(600)).await;
    assert!(b.chat.iter().any(|l| l.contains("Nur die Raumleitung")), "only the owner runs the room");
    assert_eq!(a.joined.as_ref().unwrap().room.map, "zg_hafen", "map changed");
    assert_eq!(b.joined.as_ref().unwrap().room.map, "zg_hafen", "map changed for everyone");
    assert!(a.snaps > before.0 + 20 && b.snaps > before.1 + 10, "snapshots go on after the reload");
    println!("map change ok: {} slot {:?}, {} slot {:?}", a.name, a.you, b.name, b.you);
    b.send(&ClientMsg::Leave).await;
    a.pump(Duration::from_millis(300)).await;
    assert!(a.chat.iter().any(|l| l.contains("verlassen")), "leaving is announced");
    println!("probe ok");
}
