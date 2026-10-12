//! Online play, the client's side. The server runs the match (`server`, protocol `zugriff_sim::net`);
//! this module connects, joins rooms and keeps the [`Session`] in step with it, so everything that draws the match
//! (actors, HUD, effects, sound) works online exactly as offline.
//!
//! - **Joining:** `Joined` names the map → it loads behind the loading screen → `Ready` → the first snapshot makes
//!   the session.
//! - **Everyone else** comes from snapshots, played back through a small buffer (a couple of ticks, more when the
//!   connection is uneven) so they move smoothly although packets arrive in clumps.
//! - **You** are predicted: each tick's command runs at once on the newest state the server confirmed, replaying the
//!   commands it hasn't answered yet (`Game::predict_player`). Your own shots, steps, reloads and switches play at
//!   once; the server's copies of them are dropped. Hits and kills come from the server, which judges shots against
//!   what you saw (lag compensation).
//! - **Chat:** Y to everyone, U to your side; `/hilfe` lists the room's commands.

use crate::hud::{team_color, HudState, AMBER};
use crate::input::{Held, Ui};
use crate::level::{Current, LoadMap};
use crate::link::{Link, LinkEvent};
use crate::session::{Frame, MatchSettings, Pending, Session, Snap};
use crate::view::Look;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use std::collections::VecDeque;
use zugriff_sim::bots::{Bots, Difficulty};
use zugriff_sim::game::{Event, Game, PlayerId, Team, UserCmd};
use zugriff_sim::net::{ClientMsg, Delta, Joined, Replica, RoomRef, RoomSetup, ServerMsg, PROTOCOL};

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Net>()
            .add_systems(PostStartup, (spawn_chat_box, autojoin))
            .add_systems(Update, (poll, start_when_loaded, left_match, keep_alive).chain().before(crate::session::SessionSet))
            .add_systems(FixedUpdate, tick)
            .add_systems(PostUpdate, (qa_say, chat_input, chat_box).chain());
    }
}

/// Where the online side stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Stage {
    #[default]
    Offline,
    /// Opening the socket and saying hello.
    Connecting,
    /// Connected, in no room.
    Lobby,
    /// Asked to create or join a room.
    Joining,
    /// In a room: loading its map.
    Loading,
    /// Map loaded, `Ready` sent, waiting for the first snapshot.
    Waiting,
    Playing,
}

/// A snapshot waiting for its tick.
struct Incoming {
    replica: Replica,
    events: Vec<Event>,
    you: Option<PlayerId>,
}

#[derive(Resource, Default)]
pub struct Net {
    link: Option<Link>,
    /// The server's WebSocket URL.
    pub url: String,
    pub stage: Stage,
    pub server: String,
    pub motd: String,
    pub joined: Option<Joined>,
    /// The last thing that went wrong (shown in the online menu until the next action).
    pub error: Option<String>,
    /// What to ask once the server said hello.
    after_hello: Option<ClientMsg>,
    delta: Delta,
    /// A fresh snapshot came since the last reset (the delta chain is ours).
    synced: bool,
    /// The loaded map's geometry isn't the server's.
    map_differs: bool,
    buffer: VecDeque<Incoming>,
    /// The newest confirmed state of your own player, and the last command it includes.
    newest: Option<(u32, PlayerId, zugriff_sim::game::Player)>,
    pending: VecDeque<UserCmd>,
    seq: u32,
    /// How many snapshots to keep in hand (grows when the buffer runs dry, shrinks when it stays full).
    depth: f32,
    calm: f32,
    /// Round trip to the server (ms), and everyone's as the server measured them (by player id).
    pub rtt: f32,
    pub pings: Vec<u16>,
    ping_clock: f32,
    /// A line being typed: to your side only, and the text.
    pub typing: Option<(bool, String)>,
    /// Bumped whenever something the online menu shows changes.
    pub version: u64,
    /// Where each command left you in prediction, to compare with the server's answer (the misprediction).
    predicted: VecDeque<(u32, Vec3)>,
    /// Over the last report: mispredictions (sum, worst, count, over 5 cm), snapshots taken, ticks starved.
    pub stats: NetStats,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct NetStats {
    pub err_sum: f32,
    pub err_max: f32,
    pub checked: u32,
    pub corrections: u32,
    pub snaps: u32,
    pub starved: u32,
    pub ticks: u32,
}

impl Net {
    /// Whether a room's match is (or is about to be) the session.
    pub fn in_room(&self) -> bool {
        matches!(self.stage, Stage::Loading | Stage::Waiting | Stage::Playing)
    }

    pub fn busy(&self) -> bool {
        matches!(self.stage, Stage::Connecting | Stage::Joining)
    }

    fn send(&self, msg: &ClientMsg) {
        if let Some(l) = &self.link {
            l.send(msg);
        }
    }

    /// Connects to a server (unless already there) and then asks `then`.
    pub fn go(&mut self, url: &str, then: ClientMsg) {
        self.error = None;
        self.version += 1;
        if self.link.is_some() && self.url == url && !matches!(self.stage, Stage::Offline | Stage::Connecting) {
            self.send(&then);
            self.stage = Stage::Joining;
            return;
        }
        self.link = Some(Link::open(url));
        self.url = url.to_string();
        self.stage = Stage::Connecting;
        let name = crate::options::player_name();
        self.send(&ClientMsg::Hello { protocol: PROTOCOL, name, finishes: crate::options::load().finish_table() });
        self.after_hello = Some(then);
    }

    pub fn create(&mut self, url: &str, setup: RoomSetup) {
        self.go(url, ClientMsg::Create(setup));
    }

    pub fn join(&mut self, url: &str, room: RoomRef, password: String, team: Option<Team>) {
        self.go(url, ClientMsg::Join { room, password, team });
    }

    /// Out of the room (the socket stays open for the next).
    pub fn leave(&mut self) {
        if self.in_room() || self.stage == Stage::Joining {
            self.send(&ClientMsg::Leave);
            self.stage = Stage::Lobby;
        }
        self.joined = None;
        self.buffer.clear();
        self.newest = None;
        self.pending.clear();
        self.typing = None;
        self.version += 1;
    }

    /// Off the server altogether.
    pub fn disconnect(&mut self) {
        self.leave();
        self.link = None;
        self.stage = Stage::Offline;
        self.version += 1;
    }

    pub fn say(&self, team: bool, text: String) {
        self.send(&ClientMsg::Say { team, text });
    }

    pub fn radio(&self, order: Option<u8>, text: String) {
        self.send(&ClientMsg::Radio { order, text });
    }

    pub fn switch_team(&self, team: Team) {
        self.send(&ClientMsg::Team(team));
    }

    fn reset_match(&mut self) {
        self.delta.reset();
        self.synced = false;
        self.buffer.clear();
        self.newest = None;
        self.pending.clear();
        self.depth = 2.0;
        self.calm = 0.0;
    }
}

fn settings_of(j: &Joined) -> MatchSettings {
    let difficulty = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard, Difficulty::Expert][j.setup.difficulty.min(3) as usize];
    MatchSettings { map: j.room.map.clone(), mode: j.cfg.mode, difficulty, team: None, per_side: j.setup.per_side as usize, max_rounds: j.cfg.max_rounds, warmup: j.cfg.warmup, online: true }
}

#[allow(clippy::too_many_arguments)]
fn poll(
    mut commands: Commands,
    mut net: ResMut<Net>,
    mut hud: ResMut<HudState>,
    mut loads: MessageWriter<LoadMap>,
    mut menu: ResMut<crate::menu::MenuState>,
    mut ui: ResMut<Ui>,
    session: Option<Res<Session>>,
    mut pending: ResMut<Pending>,
) {
    let Some(link) = &net.link else { return };
    let events = link.poll();
    for e in events {
        match e {
            LinkEvent::Open => {}
            LinkEvent::Closed(why) => {
                let was_playing = net.in_room();
                net.link = None;
                net.stage = Stage::Offline;
                net.joined = None;
                net.error = Some(if why.is_empty() { "Die Verbindung zum Server wurde beendet.".into() } else { format!("Keine Verbindung zu {}: {why}", crate::link::short(&net.url)) });
                net.version += 1;
                if was_playing || session.as_ref().is_some_and(|s| s.online) {
                    commands.remove_resource::<Session>();
                    pending.0 = None;
                    menu.screen = crate::menu::Screen::Online;
                }
            }
            LinkEvent::Msg(m) => match m {
                ServerMsg::Welcome { server, motd } => {
                    net.server = server;
                    net.motd = motd;
                    net.stage = Stage::Lobby;
                    if let Some(then) = net.after_hello.take() {
                        net.send(&then);
                        net.stage = Stage::Joining;
                    }
                    net.version += 1;
                }
                ServerMsg::Joined(j) => {
                    info!("joined room {} ({}) on {}", j.room.name, j.room.map, net.url);
                    net.reset_match();
                    let settings = settings_of(&j);
                    loads.write(LoadMap(settings.map.clone()));
                    commands.remove_resource::<Session>();
                    pending.0 = Some(settings);
                    net.joined = Some(*j);
                    net.stage = Stage::Loading;
                    net.version += 1;
                    menu.screen = crate::menu::Screen::None;
                    ui.menu = false;
                }
                ServerMsg::Snap(s) => {
                    if !matches!(net.stage, Stage::Waiting | Stage::Playing) {
                        continue;
                    }
                    let s = *s;
                    // Until the server's fresh start after our Ready, what arrives belongs to an older chain.
                    if s.fresh {
                        net.delta.reset();
                        net.synced = true;
                    } else if !net.synced {
                        continue;
                    }
                    let Some(bytes) = net.delta.unpack(&s.delta).map(|b| b.to_vec()) else {
                        warn!("bad snapshot delta");
                        continue;
                    };
                    let Some(replica) = Replica::decode(&bytes) else {
                        warn!("bad snapshot");
                        continue;
                    };
                    if let Some(p) = s.pings {
                        net.pings = p;
                    }
                    if let Some(you) = s.you
                        && let Some(me) = replica.players.get(you)
                    {
                        net.newest = Some((s.ack, you, me.clone()));
                    }
                    net.buffer.push_back(Incoming { replica, events: s.events, you: s.you });
                    // A long stall (loading, a hitch): don't pile up history.
                    while net.buffer.len() > 60 {
                        let old = net.buffer.pop_front().unwrap();
                        if let Some(next) = net.buffer.front_mut() {
                            let mut ev = old.events;
                            ev.append(&mut next.events);
                            next.events = ev;
                        }
                    }
                }
                ServerMsg::Chat(c) => {
                    let (line, color) = if c.notice {
                        (c.text, AMBER)
                    } else {
                        let team = c.from.and_then(|id| session.as_ref().and_then(|s| s.game.players.get(id)).map(|p| p.team));
                        let tag = if c.team.is_some() { "[Team] " } else { "" };
                        (format!("{tag}{}: {}", c.name, c.text), team.map_or(Color::WHITE, team_color))
                    };
                    hud.say(line, color);
                }
                ServerMsg::Left { reason } => {
                    net.leave();
                    net.error = Some(reason);
                    commands.remove_resource::<Session>();
                    pending.0 = None;
                    menu.screen = crate::menu::Screen::Online;
                }
                ServerMsg::Error(e) => {
                    warn!("server: {e}");
                    if net.stage == Stage::Joining {
                        net.stage = Stage::Lobby;
                    }
                    if net.in_room() {
                        hud.say(e.clone(), crate::ui_kit::DANGER);
                    }
                    net.error = Some(e);
                    net.version += 1;
                }
                ServerMsg::Ping(t) => net.send(&ClientMsg::Pong(t)),
                ServerMsg::Pong(t) => {
                    let ms = ((now() - t) * 1000.0) as f32;
                    net.rtt = if net.rtt <= 0.0 { ms } else { net.rtt * 0.75 + ms * 0.25 };
                }
                ServerMsg::Rooms(_) => {}
            },
        }
    }
}

/// Seconds on a steady clock (the ping's).
pub fn now() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now() / 1000.0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
    }
}

/// Once the room's map is in: Ready; with the first snapshot, the session.
#[allow(clippy::too_many_arguments)]
fn start_when_loaded(mut commands: Commands, mut net: ResMut<Net>, current: Res<Current>, mut frame: ResMut<Frame>, mut held: ResMut<Held>) {
    let Some(j) = net.joined.clone() else { return };
    if net.stage == Stage::Loading {
        if current.name != j.room.map {
            return;
        }
        let Some((level, _)) = current.ready() else { return };
        net.map_differs = zugriff_sim::nav::geometry_hash(&level.world) != j.map_hash;
        net.send(&ClientMsg::Ready);
        net.stage = Stage::Waiting;
        return;
    }
    if net.stage != Stage::Waiting || net.buffer.is_empty() {
        return;
    }
    let Some((level, nav)) = current.ready() else { return };
    let first = net.buffer.pop_back().unwrap();
    net.buffer.clear();
    let mut game = Game::new(level, j.cfg.clone(), 0);
    game.nav = Some(nav.clone());
    first.replica.apply(&mut game);
    let Some(me) = first.you.filter(|&y| y < game.players.len()) else { return };
    let settings = settings_of(&j);
    let bots = Bots::new(nav, settings.difficulty, 1);
    let prev = (0..game.players.len()).map(|_| Snap::default()).collect();
    commands.insert_resource(Session { game, bots, me, settings, paused: false, prev, prev_hostages: Vec::new(), prev_grenades: Vec::new(), ticks: 0, online: true });
    // (`pending` stays until the session is in, next frame, so the menu never sees neither.)
    frame.started = true;
    *held = Held::default();
    net.stage = Stage::Playing;
    net.version += 1;
}

/// The menu took the session away (leaving the match): out of the room, back to the server list.
fn left_match(mut net: ResMut<Net>, session: Option<Res<Session>>, mut pending: ResMut<Pending>, mut menu: ResMut<crate::menu::MenuState>, mut hud: ResMut<HudState>) {
    if net.stage == Stage::Playing && session.as_ref().is_some_and(|s| s.online) && pending.0.as_ref().is_some_and(|p| p.online) {
        pending.0 = None;
        // The session is in (the HUD cleared its chat for it): the welcome.
        if let Some(j) = &net.joined {
            let code = if j.setup.private { format!(" · Code {}", j.code) } else { String::new() };
            hud.say(format!("{} – {}{code}. Chat: T oder Y (alle), U (Team), /hilfe.", net.server, j.room.name), AMBER);
        }
        if !net.motd.is_empty() {
            hud.say(net.motd.clone(), Color::WHITE);
        }
        if let Some(j) = net.joined.as_ref().filter(|_| net.map_differs) {
            hud.say(format!("Deine Version von {} weicht vom Server ab – Bewegungen können ruckeln.", j.room.map), crate::ui_kit::DANGER);
        }
        return;
    }
    if net.stage == Stage::Playing && session.is_none() && pending.0.is_none() {
        net.leave();
        menu.screen = crate::menu::Screen::Online;
    }
}

fn keep_alive(mut net: ResMut<Net>, time: Res<Time<Real>>) {
    if net.link.is_none() || matches!(net.stage, Stage::Offline | Stage::Connecting) {
        return;
    }
    net.ping_clock -= time.delta_secs();
    if net.ping_clock <= 0.0 {
        net.ping_clock = 1.0;
        net.send(&ClientMsg::Ping(now()));
    }
}

/// Whether an event is one of those your prediction makes itself.
fn predicted(e: &Event, me: PlayerId) -> bool {
    match e {
        Event::Shot { player, .. }
        | Event::DryFire { player }
        | Event::Reload { player, .. }
        | Event::Shell { player }
        | Event::Draw { player, .. }
        | Event::Zoom { player, .. }
        | Event::KnifeSwing { player, .. }
        | Event::KnifeHit { player, .. }
        | Event::Footstep { player, .. }
        | Event::Jump { player }
        | Event::Land { player, .. }
        | Event::Bought { player, .. } => *player == me,
        _ => false,
    }
}

/// The server's events without those you already played (your shots' impacts included).
fn server_events(events: Vec<Event>, me: PlayerId, out: &mut Vec<Event>) {
    let mut after_my_shot = false;
    for e in events {
        match &e {
            Event::Impact { .. } if after_my_shot => continue,
            Event::Impact { .. } | Event::Hit { .. } | Event::HostageHit { .. } | Event::Kill { .. } => {}
            _ => after_my_shot = matches!(e, Event::Shot { player, .. } if player == me),
        }
        if !predicted(&e, me) {
            out.push(e);
        }
    }
}

/// One tick online: play back snapshots, send and predict your command.
#[allow(clippy::too_many_arguments)]
fn tick(session: Option<ResMut<Session>>, mut net: ResMut<Net>, mut held: ResMut<Held>, mut look: ResMut<Look>, mut frame: ResMut<Frame>, mut cost: ResMut<crate::perf::SimCost>) {
    let Some(mut s) = session else { return };
    if !s.online || net.stage != Stage::Playing {
        return;
    }
    let started = bevy::platform::time::Instant::now();
    let s = &mut *s;
    s.prev = (0..s.game.players.len()).map(|i| s.snap(i)).collect();
    s.prev_hostages = s.game.hostages.iter().map(|h| h.mover.origin).collect();
    s.prev_grenades = s.game.grenades.iter().map(|g| (g.id, g.pos)).collect();

    // Snapshots: one a tick, more to catch up, none when the buffer ran dry.
    let n = net.buffer.len();
    let depth = net.depth.round() as usize;
    net.stats.ticks += 1;
    // The aim grows a little with each dry tick and eases back after a calm second, between 1.5 and 6.
    let take = if n == 0 {
        net.stats.starved += 1;
        net.depth = (net.depth + 0.35).min(6.0);
        net.calm = 0.0;
        0
    } else if n > depth + 2 {
        n - depth
    } else {
        1
    };
    net.calm += 1.0;
    if net.calm > 66.0 {
        net.calm = 0.0;
        net.depth = (net.depth - 0.25).max(1.5);
    }
    let mut events = Vec::new();
    let mut last = None;
    for _ in 0..take {
        let Some(inc) = net.buffer.pop_front() else { break };
        net.stats.snaps += 1;
        events.extend(inc.events);
        if let Some(y) = inc.you {
            if y != s.me {
                net.pending.clear();
            }
            s.me = y;
        }
        last = Some(inc.replica);
    }
    if let Some(r) = last {
        r.apply(&mut s.game);
    }
    let me = s.me;
    if me >= s.game.players.len() {
        return;
    }

    // Your command.
    net.seq += 1;
    let mut cmd = UserCmd {
        forward: held.forward,
        side: held.side,
        yaw: look.yaw,
        pitch: look.pitch,
        buttons: zugriff_sim::game::Btn(held.buttons.0 | held.latched.0),
        select: held.select.take(),
        buy: std::mem::take(&mut held.buy),
        seq: net.seq,
        view_tick: s.game.ticks,
    };
    held.latched = zugriff_sim::game::Btn::empty();
    if crate::args::get("autoplay").is_some() || crate::args::flag("autoplay") {
        // A bot plays your slot (testing): it thinks on a copy where only you are a bot.
        let mut g = s.game.clone();
        for p in &mut g.players {
            p.bot = p.id == me;
        }
        if let Some((_, c)) = s.bots.think(&g).into_iter().find(|(id, _)| *id == me) {
            cmd = UserCmd { seq: cmd.seq, view_tick: cmd.view_tick, ..c };
            look.yaw = cmd.yaw;
            look.pitch = cmd.pitch;
        }
    }
    net.send(&ClientMsg::Cmd(cmd.clone()));
    net.pending.push_back(cmd);
    while net.pending.len() > 200 {
        net.pending.pop_front();
    }

    // Prediction: from the newest state the server confirmed, replay what it hasn't answered yet.
    let mut mine = Vec::new();
    if let Some((ack, you, base)) = net.newest.clone()
        && you == me
    {
        net.pending.retain(|c| c.seq > ack);
        // How far off the prediction for the command the server just confirmed was.
        while net.predicted.front().is_some_and(|(q, _)| *q < ack) {
            net.predicted.pop_front();
        }
        if let Some(&(q, at)) = net.predicted.front()
            && q == ack
            && base.alive
        {
            net.predicted.pop_front();
            let err = at.distance(base.mover.origin);
            let st = &mut net.stats;
            // (A spawn moves you further than any misprediction: not counted.)
            let err = if err > 3.0 { 0.0 } else { err };
            st.err_sum += err;
            st.err_max = st.err_max.max(err);
            st.checked += 1;
            st.corrections += u32::from(err > 0.05);
        }
        let mut scratch = s.game.clone();
        scratch.players[me] = base;
        let n = net.pending.len();
        for (i, c) in net.pending.iter().enumerate() {
            if i + 1 == n {
                scratch.events.clear();
            }
            scratch.predict_player(me, c);
        }
        mine = std::mem::take(&mut scratch.events).into_iter().filter(|e| predicted(e, me) || matches!(e, Event::Impact { .. })).collect();
        s.game.players[me] = scratch.players.swap_remove(me);
        let seq = net.seq;
        let at = s.game.players[me].mover.origin;
        net.predicted.push_back((seq, at));
        while net.predicted.len() > 200 {
            net.predicted.pop_front();
        }
    }
    if net.stats.ticks >= 330 {
        let st = std::mem::take(&mut net.stats);
        info!(
            "net: rtt {:.0} ms, buffer {} (aim {:.0}), {} snapshots / {} ticks, {} starved; prediction off by {:.1} cm on average, {:.1} cm at most, {} of {} over 5 cm",
            net.rtt,
            net.buffer.len(),
            net.depth,
            st.snaps,
            st.ticks,
            st.starved,
            st.err_sum / st.checked.max(1) as f32 * 100.0,
            st.err_max * 100.0,
            st.corrections,
            st.checked
        );
    }
    server_events(events, me, &mut frame.events);
    frame.events.extend(mine);
    s.ticks += 1;
    cost.add(std::time::Duration::ZERO, started.elapsed());
}

/// `--join <room id | code>` or `--create <map>` (`?join=`, `?create=`; the server from `--server`): straight into a
/// room, for testing. `--private` makes the created room private, `--nobots` leaves its free slots empty.
fn autojoin(mut net: ResMut<Net>, mut menu: ResMut<crate::menu::MenuState>) {
    let url = crate::netmenu::default_server();
    if let Some(j) = crate::args::get("join") {
        let room = match j.parse::<u32>() {
            Ok(id) => RoomRef::Id(id),
            Err(_) => RoomRef::Code(j),
        };
        let team = match crate::args::get("team").as_deref() {
            Some("ksk") => Some(Team::Ksk),
            Some("opfor") => Some(Team::Opfor),
            _ => None,
        };
        net.join(&url, room, crate::args::get("password").unwrap_or_default(), team);
        menu.screen = crate::menu::Screen::Online;
    } else if let Some(map) = crate::args::get("create") {
        let setup = RoomSetup { name: format!("{}s Raum", crate::options::player_name()), map, private: crate::args::flag("private"), bots: !crate::args::flag("nobots"), ..Default::default() };
        net.create(&url, setup);
        menu.screen = crate::menu::Screen::Online;
    }
}

// ---- chat -------------------------------------------------------------------------------------------------------

#[derive(Component)]
struct ChatBox;

fn spawn_chat_box(mut commands: Commands, fonts: Res<crate::hud::Fonts>) {
    commands.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(26.0), bottom: Val::Px(78.0), padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)), border_radius: BorderRadius::all(Val::Px(7.0)), border: UiRect::all(Val::Px(1.0)), min_width: Val::Px(520.0), ..default() },
        BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.85)),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.2)),
        Visibility::Hidden,
        GlobalZIndex(6),
        ChatBox,
        children![crate::hud::text("", &fonts.label, 22.0, Color::WHITE)],
    ));
}

/// `--say <text>` (`?say=`): 4 s into the match the chat box opens with the text, 2 s later it is sent (testing).
fn qa_say(mut net: ResMut<Net>, session: Option<Res<Session>>, time: Res<Time>, mut since: Local<f32>, mut done: Local<u8>) {
    let Some(text) = crate::args::get("say") else { return };
    if !session.is_some_and(|s| s.online) || *done >= 2 {
        return;
    }
    *since += time.delta_secs();
    if *done == 0 && *since > 4.0 {
        net.typing = Some((false, text));
        *done = 1;
    } else if *done == 1 && *since > 6.0 {
        let t = net.typing.take().map_or(text, |(_, t)| t);
        net.say(false, t);
        *done = 2;
    }
}

/// Y (everyone) or U (your side) opens a line; Enter sends it, Esc drops it.
#[allow(clippy::too_many_arguments)]
fn chat_input(mut net: ResMut<Net>, mut ui: ResMut<Ui>, keys: Res<ButtonInput<KeyCode>>, mut typed: MessageReader<KeyboardInput>, look: Res<Look>, session: Option<Res<Session>>) {
    let online = session.as_ref().is_some_and(|s| s.online);
    if !online || ui.menu {
        net.typing = None;
        ui.typing = false;
        typed.clear();
        return;
    }
    let Some((team, mut line)) = net.typing.take() else {
        ui.typing = false;
        if look.locked && ui.playing() {
            if keys.just_pressed(KeyCode::KeyY) || keys.just_pressed(KeyCode::KeyT) {
                net.typing = Some((false, String::new()));
            } else if keys.just_pressed(KeyCode::KeyU) {
                net.typing = Some((true, String::new()));
            }
            ui.typing = net.typing.is_some();
        }
        typed.clear();
        return;
    };
    for k in typed.read() {
        if k.state != ButtonState::Pressed {
            continue;
        }
        match &k.logical_key {
            Key::Enter => {
                if !line.trim().is_empty() {
                    net.say(team, line.clone());
                }
                ui.typing = false;
                return;
            }
            Key::Escape => {
                ui.typing = false;
                return;
            }
            Key::Backspace => {
                line.pop();
            }
            _ => {
                if let Some(t) = &k.text
                    && line.chars().count() < 120
                {
                    line.extend(t.chars().filter(|c| !c.is_control()));
                }
            }
        }
    }
    net.typing = Some((team, line));
    ui.typing = true;
}

fn chat_box(net: Res<Net>, root: Single<(&mut Visibility, &Children), With<ChatBox>>, mut texts: Query<&mut Text>, time: Res<Time>) {
    let (mut vis, children) = root.into_inner();
    let Some((team, line)) = &net.typing else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    vis.set_if_neq(Visibility::Inherited);
    let caret = if (time.elapsed_secs() * 2.0) as i32 % 2 == 0 { "▌" } else { " " };
    let s = format!("{}  {line}{caret}", if *team { "Team:" } else { "Alle:" });
    if let Some(mut t) = children.first().and_then(|c| texts.get_mut(*c).ok())
        && t.0 != s
    {
        t.0 = s;
    }
}
