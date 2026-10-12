//! The online menus (the lobby's "Mehrspieler" tab): the server list, creating a room, joining a private one by its
//! code, and servers added by address.
//!
//! The list asks every known server (the default one, those added, and those a server says were announced to it) for
//! its rooms and measures the ping to each, over short-lived sockets of their own; joining then opens the game's
//! socket (`net.rs`). Text fields take the keyboard while focused (click one; Enter confirms, Tab/Esc leaves it).

use crate::hud::{text, Fonts, AMBER, KSK, OPFOR};
use crate::link::{self, Link, LinkEvent};
use crate::menu::{MenuState, Screen, MAPS};
use crate::net::{Net, Stage};
use crate::options::Options;
use crate::ui_kit::{caps, ghost, glass, primary, rule, section, segmented, Hover, AMBER_HI, DANGER, DIM, GLASS_HI, INK, LINE, LINE_HI, MONEY, SOFT};
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use zugriff_sim::game::{Mode, Phase, Team};
use zugriff_sim::net::{ClientMsg, RoomInfo, RoomRef, RoomSetup, ServerMsg, PROTOCOL};

pub struct NetMenuPlugin;

impl Plugin for NetMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Browser>()
            .init_resource::<OnlineUi>()
            .add_systems(Startup, open_from_address)
            .add_systems(Update, (refresh, probe, buttons, type_fields).chain().after(crate::input::InputSet));
    }
}

/// The server everyone asks first: `?server=` (`--server`), else the page's own host (port 27015, or `/zg` behind
/// https), else this machine.
pub fn default_server() -> String {
    if let Some(s) = crate::args::get("server") {
        return link::normalize(&s);
    }
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(loc) = web_sys::window().map(|w| w.location()) {
            let host = loc.hostname().unwrap_or_default();
            if loc.protocol().unwrap_or_default() == "https:" {
                return link::normalize(&format!("wss://{}", loc.host().unwrap_or(host)));
            }
            if !host.is_empty() {
                return link::normalize(&host);
            }
        }
    }
    link::normalize("127.0.0.1")
}

/// The servers to ask: the default, then those added.
pub fn servers(options: &Options) -> Vec<String> {
    let mut out = vec![default_server()];
    for s in &options.servers {
        let s = link::normalize(s);
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// One server being asked for its rooms.
struct Probe {
    url: String,
    link: Option<Link>,
    sent: f64,
    /// Pongs so far: the ping is the best of three (the first can wait behind a slow frame).
    pongs: u8,
}

/// What the list knows about a server.
#[derive(Clone, Debug, Default)]
pub struct ServerState {
    pub url: String,
    pub name: String,
    pub ping: Option<f32>,
    pub rooms: usize,
    pub error: Option<String>,
    pub done: bool,
    pub announced: bool,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub url: String,
    pub info: RoomInfo,
}

#[derive(Resource, Default)]
pub struct Browser {
    probes: Vec<Probe>,
    pub servers: Vec<ServerState>,
    pub rows: Vec<Row>,
    /// When the list was last asked for (seconds, `net::now`); None: never.
    asked: Option<f64>,
    pub version: u64,
}

impl Browser {
    pub fn refreshing(&self) -> bool {
        !self.probes.is_empty()
    }

    fn start(&mut self, urls: Vec<String>) {
        self.probes.clear();
        self.servers.clear();
        self.rows.clear();
        for url in urls {
            self.ask(url, false);
        }
        self.asked = Some(crate::net::now());
        self.version += 1;
    }

    fn ask(&mut self, url: String, announced: bool) {
        if self.servers.iter().any(|s| s.url == url) {
            return;
        }
        let link = Link::open(&url);
        let now = crate::net::now();
        link.send(&ClientMsg::Hello { protocol: PROTOCOL, name: crate::options::player_name(), finishes: Vec::new() });
        link.send(&ClientMsg::List);
        link.send(&ClientMsg::Ping(now));
        self.servers.push(ServerState { url: url.clone(), name: link::short(&url), announced, ..Default::default() });
        self.probes.push(Probe { url, link: Some(link), sent: now, pongs: 0 });
    }

    fn server_mut(&mut self, url: &str) -> Option<&mut ServerState> {
        self.servers.iter_mut().find(|s| s.url == url)
    }

    /// The room's server's ping, if known.
    pub fn ping(&self, url: &str) -> Option<f32> {
        self.servers.iter().find(|s| s.url == url).and_then(|s| s.ping)
    }
}

/// A text field of the online menus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Name,
    Code,
    Address,
    Password,
    RoomName,
    RoomPassword,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    #[default]
    List,
    Create,
}

#[derive(Resource)]
pub struct OnlineUi {
    pub page: Page,
    pub focus: Option<Field>,
    pub code: String,
    pub address: String,
    pub password: String,
    pub team: Option<Team>,
    /// 0 all, 1 with people, 2 with free slots.
    pub filter: u8,
    /// The chosen room (server URL, room id).
    pub selected: Option<(String, u32)>,
    pub setup: RoomSetup,
    /// Which server a new room goes to (index into the known servers).
    pub host: usize,
    pub version: u64,
}

impl Default for OnlineUi {
    fn default() -> Self {
        OnlineUi { page: Page::List, focus: None, code: String::new(), address: String::new(), password: String::new(), team: None, filter: 0, selected: None, setup: RoomSetup::default(), host: 0, version: 0 }
    }
}

#[derive(Component, Clone, PartialEq, Debug)]
pub enum NetAct {
    Refresh,
    Filter(u8),
    Select(String, u32),
    Join,
    JoinCode,
    Team(u8),
    Focus(Field),
    AddServer,
    RemoveServer(String),
    OpenCreate,
    Back,
    Create,
    Map(usize),
    Mode(bool),
    PerSide(i8),
    Bots,
    Difficulty(u8),
    Rounds(u32),
    Private(bool),
    FriendlyFire,
    Host(usize),
    Cancel,
}

/// A hash of everything the page shows (the menu rebuilds when it changes).
pub fn version(net: &Net, browser: &Browser, ui: &OnlineUi, options: &Options) -> u64 {
    let mut h = net.version.wrapping_mul(1_000_003) ^ browser.version.wrapping_mul(7919) ^ ui.version.wrapping_mul(104_729);
    h ^= options.name.len() as u64 * 31 + options.name.bytes().map(|b| b as u64).sum::<u64>() * 131 + options.servers.len() as u64 * 17;
    h
}

/// `?menu=create` (`--menu create`) opens the create page, for screenshots.
fn open_from_address(mut ui: ResMut<OnlineUi>, mut menu: ResMut<MenuState>, options: Res<Options>) {
    if crate::args::get("menu").as_deref() == Some("create") {
        ui.page = Page::Create;
        ui.setup.map = options.last_map.clone();
        menu.screen = Screen::Online;
    }
}

// ---- asking the servers -----------------------------------------------------------------------------------------

/// Asks again on opening the page and every 15 s while it's open.
fn refresh(mut browser: ResMut<Browser>, menu: Res<MenuState>, options: Res<Options>, ui: Res<OnlineUi>) {
    if menu.screen != Screen::Online || ui.page != Page::List {
        return;
    }
    let due = browser.asked.is_none_or(|t| crate::net::now() - t > 15.0);
    if due && !browser.refreshing() {
        browser.start(servers(&options));
    }
}

fn probe(mut browser: ResMut<Browser>) {
    if browser.probes.is_empty() {
        return;
    }
    let now = crate::net::now();
    let mut changed = false;
    let mut found: Vec<String> = Vec::new();
    let probes = std::mem::take(&mut browser.probes);
    let mut keep = Vec::new();
    for mut p in probes {
        let events = p.link.as_ref().map(|l| l.poll()).unwrap_or_default();
        let mut finished = false;
        for e in events {
            changed = true;
            match e {
                LinkEvent::Open => {}
                LinkEvent::Closed(why) => {
                    if let Some(s) = browser.server_mut(&p.url)
                        && !s.done
                    {
                        let _ = why;
                        s.error = Some("nicht erreichbar".into());
                        s.done = true;
                    }
                    finished = true;
                }
                LinkEvent::Msg(m) => match m {
                    ServerMsg::Welcome { server, .. } => {
                        if let Some(s) = browser.server_mut(&p.url) {
                            s.name = server;
                        }
                    }
                    ServerMsg::Rooms(rooms) => {
                        let url = p.url.clone();
                        let mut own = 0;
                        for r in rooms {
                            match r.address.clone() {
                                // Announced by another server: listed under that server (asked for its ping).
                                Some(a) => {
                                    let a = link::normalize(&a);
                                    if a != url && !found.contains(&a) {
                                        found.push(a.clone());
                                    }
                                    if !browser.rows.iter().any(|x| x.url == a && x.info.id == r.id) {
                                        browser.rows.push(Row { url: a, info: r });
                                    }
                                }
                                None => {
                                    own += 1;
                                    browser.rows.retain(|x| !(x.url == url && x.info.id == r.id));
                                    browser.rows.push(Row { url: url.clone(), info: r });
                                }
                            }
                        }
                        if let Some(s) = browser.server_mut(&url) {
                            s.rooms = own;
                        }
                    }
                    ServerMsg::Pong(t) => {
                        let ms = ((now - t) * 1000.0) as f32;
                        if let Some(s) = browser.server_mut(&p.url) {
                            s.ping = Some(s.ping.map_or(ms, |p| p.min(ms)));
                        }
                        p.pongs += 1;
                        if p.pongs < 3
                            && let Some(l) = &p.link
                        {
                            l.send(&ClientMsg::Ping(now));
                        }
                    }
                    ServerMsg::Error(e) => {
                        if let Some(s) = browser.server_mut(&p.url) {
                            s.error = Some(e);
                        }
                    }
                    _ => {}
                },
            }
        }
        // The first pong answers the last request (hello, list, ping), so with the third everything is in.
        let answered = p.pongs >= 3;
        if answered && !finished {
            if let Some(s) = browser.server_mut(&p.url) {
                s.done = true;
            }
            finished = true;
            changed = true;
        }
        if !finished && now - p.sent > 5.0 {
            if let Some(s) = browser.server_mut(&p.url) {
                s.error = Some("keine Antwort".into());
                s.done = true;
            }
            finished = true;
            changed = true;
        }
        if finished {
            p.link = None;
        } else {
            keep.push(p);
        }
    }
    browser.probes = keep;
    for url in found {
        browser.ask(url, true);
    }
    if changed {
        browser.rows.sort_by(|a, b| (b.info.humans, a.info.name.to_lowercase()).cmp(&(a.info.humans, b.info.name.to_lowercase())));
        browser.version += 1;
    }
}

// ---- the page ---------------------------------------------------------------------------------------------------

pub struct View<'a> {
    pub net: &'a Net,
    pub browser: &'a Browser,
    pub ui: &'a OnlineUi,
    pub options: &'a Options,
}

pub fn page(body: &mut ChildSpawnerCommands, f: &Fonts, server: &AssetServer, v: &View) {
    match v.ui.page {
        Page::List => list_page(body, f, v),
        Page::Create => create_page(body, f, server, v),
    }
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Defuse => "Sprengladung",
        Mode::Rescue => "Geiselrettung",
        Mode::Deathmatch => "Deathmatch",
    }
}

fn map_title(name: &str) -> &str {
    MAPS.iter().find(|m| m.name == name).map_or(name, |m| m.title)
}

fn ping_color(ms: f32) -> Color {
    if ms < 60.0 {
        MONEY
    } else if ms < 130.0 {
        AMBER
    } else {
        DANGER
    }
}

/// A one-line text field: the text (or a hint), a caret while focused.
fn field(c: &mut ChildSpawnerCommands, f: &Fonts, which: Field, value: &str, hint: &str, focused: bool, secret: bool, width: Val) {
    let border = if focused { AMBER } else { LINE };
    let shown = if secret { "•".repeat(value.chars().count()) } else { value.to_string() };
    let (t, color) = if shown.is_empty() && !focused { (hint.to_string(), Color::srgba(1.0, 1.0, 1.0, 0.35)) } else { (format!("{shown}{}", if focused { "▌" } else { "" }), Color::WHITE) };
    c.spawn((
        Button,
        Node { width, height: Val::Px(46.0), padding: UiRect::horizontal(Val::Px(14.0)), align_items: AlignItems::Center, border: UiRect::all(Val::Px(if focused { 2.0 } else { 1.0 })), border_radius: BorderRadius::all(Val::Px(8.0)), overflow: Overflow::clip(), ..default() },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)),
        BorderColor::all(border),
        Hover::new(Color::srgba(0.0, 0.0, 0.0, 0.4), Color::srgba(0.0, 0.0, 0.0, 0.55), border, if focused { AMBER_HI } else { LINE_HI }),
        NetAct::Focus(which),
        children![(text(&t, &f.label, 23.0, color), Pickable::IGNORE)],
    ));
}

fn label(c: &mut ChildSpawnerCommands, f: &Fonts, t: &str) {
    c.spawn(caps(t, &f.bold, 15.0, DIM));
}

fn small_button(c: &mut ChildSpawnerCommands, f: &Fonts, t: &str, act: NetAct, on: bool) {
    let (bg, hot, fg) = if on { (AMBER, AMBER_HI, INK) } else { (Color::srgba(1.0, 1.0, 1.0, 0.05), Color::srgba(1.0, 1.0, 1.0, 0.13), Color::WHITE) };
    c.spawn((
        Button,
        Node { padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(8.0)), flex_shrink: 0.0, ..default() },
        BackgroundColor(bg),
        BorderColor::all(if on { AMBER_HI } else { LINE }),
        Hover::new(bg, hot, if on { AMBER_HI } else { LINE }, LINE_HI),
        act,
        children![(caps(t, &f.bold, 17.0, fg), Pickable::IGNORE)],
    ));
}

fn switch(c: &mut ChildSpawnerCommands, f: &Fonts, label: &str, on: bool, act: NetAct) {
    c.spawn(Node { align_items: AlignItems::Center, ..default() }).with_children(|r| {
        r.spawn((Node { flex_grow: 1.0, ..default() }, children![text(label, &f.label, 23.0, Color::WHITE)]));
        let (bg, hot) = if on { (AMBER, AMBER_HI) } else { (Color::srgba(1.0, 1.0, 1.0, 0.1), Color::srgba(1.0, 1.0, 1.0, 0.18)) };
        r.spawn((
            Button,
            Node { width: Val::Px(58.0), height: Val::Px(30.0), padding: UiRect::all(Val::Px(3.0)), justify_content: if on { JustifyContent::End } else { JustifyContent::Start }, border_radius: BorderRadius::all(Val::Px(15.0)), ..default() },
            BackgroundColor(bg),
            Hover::new(bg, hot, Color::NONE, Color::NONE),
            act,
            children![(Node { width: Val::Px(24.0), height: Val::Px(24.0), border_radius: BorderRadius::all(Val::Px(12.0)), ..default() }, BackgroundColor(if on { INK } else { Color::WHITE }), Pickable::IGNORE)],
        ));
    });
}

/// A labelled group of segmented choices.
fn choice(c: &mut ChildSpawnerCommands, f: &Fonts, title: &str, items: Vec<(&str, NetAct, bool)>) {
    c.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }).with_children(|g| {
        label(g, f, title);
        segmented(g, f, items);
    });
}

/// The status line under the list: connecting, errors.
fn status(c: &mut ChildSpawnerCommands, f: &Fonts, v: &View) {
    let (t, color) = if let Some(e) = &v.net.error {
        (e.clone(), DANGER)
    } else {
        match v.net.stage {
            Stage::Connecting => (format!("Verbinde mit {} …", link::short(&v.net.url)), SOFT),
            Stage::Joining => ("Trete bei …".to_string(), SOFT),
            Stage::Loading | Stage::Waiting => ("Lade den Einsatz …".to_string(), SOFT),
            _ => (String::new(), SOFT),
        }
    };
    if !t.is_empty() {
        c.spawn(text(&t, &f.label, 21.0, color));
    }
}

fn list_page(body: &mut ChildSpawnerCommands, f: &Fonts, v: &View) {
    let ui = v.ui;
    body.spawn(Node { width: Val::Percent(100.0), height: Val::Percent(100.0), column_gap: Val::Px(28.0), ..default() }).with_children(|row| {
        // The list.
        row.spawn(glass(Node { flex_grow: 1.0, min_width: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(14.0), padding: UiRect::all(Val::Px(26.0)), ..default() })).with_children(|p| {
            p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(12.0), ..default() }).with_children(|h| {
                section(h, f, "Serverliste");
                h.spawn(Node { flex_grow: 1.0, ..default() });
                for (i, t) in ["Alle", "Mit Spielern", "Freie Plätze"].iter().enumerate() {
                    small_button(h, f, t, NetAct::Filter(i as u8), ui.filter == i as u8);
                }
                small_button(h, f, if v.browser.refreshing() { "Suche …" } else { "Aktualisieren" }, NetAct::Refresh, false);
            });
            // The column heads.
            let cols: [(&str, f32); 5] = [("Karte", 170.0), ("Modus", 160.0), ("Spieler", 150.0), ("Stand", 130.0), ("Ping", 80.0)];
            p.spawn(Node { padding: UiRect::axes(Val::Px(16.0), Val::Px(2.0)), ..default() }).with_children(|r| {
                r.spawn((Node { flex_grow: 1.0, ..default() }, children![caps("Raum", &f.bold, 14.0, DIM)]));
                for (t, w) in cols {
                    r.spawn((Node { width: Val::Px(w), flex_shrink: 0.0, ..default() }, children![caps(t, &f.bold, 14.0, DIM)]));
                }
            });
            rule(p);
            let rows: Vec<&Row> = v
                .browser
                .rows
                .iter()
                .filter(|r| match ui.filter {
                    1 => r.info.humans > 0,
                    2 => (r.info.humans as u32) < r.info.slots as u32,
                    _ => true,
                })
                .collect();
            p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::scroll_y(), ..default() }).with_children(|list| {
                if rows.is_empty() {
                    let t = if v.browser.refreshing() { "Frage die Server …" } else { "Keine Räume gefunden. Erstelle einen – oder füge einen Server hinzu." };
                    list.spawn((Node { padding: UiRect::all(Val::Px(30.0)), justify_content: JustifyContent::Center, ..default() }, children![text(t, &f.label, 23.0, DIM)]));
                }
                for r in rows {
                    room_row(list, f, v, r, &cols);
                }
            });
            // What to do with the chosen room.
            rule(p);
            let chosen = ui.selected.as_ref().and_then(|(u, id)| v.browser.rows.iter().find(|r| r.url == *u && r.info.id == *id));
            p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(14.0), ..default() }).with_children(|b| {
                b.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), flex_grow: 1.0, min_width: Val::Px(0.0), ..default() }).with_children(|t| {
                    match chosen {
                        Some(r) => {
                            t.spawn(text(&r.info.name, &f.bold, 30.0, Color::WHITE));
                            t.spawn(text(&format!("{} · {} · {}", map_title(&r.info.map), mode_name(r.info.mode), r.info.server), &f.label, 20.0, DIM));
                        }
                        None => {
                            t.spawn(text("Wähle einen Raum.", &f.label, 22.0, DIM));
                        }
                    }
                    status(t, f, v);
                });
                if chosen.is_some_and(|r| r.info.password) {
                    field(b, f, Field::Password, &ui.password, "Passwort", ui.focus == Some(Field::Password), true, Val::Px(220.0));
                }
                b.spawn(Node { width: Val::Px(300.0), flex_shrink: 0.0, ..default() }).with_children(|s| {
                    segmented(s, f, vec![("Auto", NetAct::Team(2), ui.team.is_none()), ("KSK", NetAct::Team(0), ui.team == Some(Team::Ksk)), ("OPFOR", NetAct::Team(1), ui.team == Some(Team::Opfor))]);
                });
                if chosen.is_some() && !v.net.busy() {
                    primary(b, f, "Beitreten", NetAct::Join, Val::Px(250.0), 64.0, 28.0);
                } else if v.net.busy() {
                    ghost(b, f, "Abbrechen", NetAct::Cancel, Val::Px(250.0), true);
                }
            });
        });
        // The side column.
        row.spawn(Node { width: Val::Px(460.0), flex_shrink: 0.0, flex_direction: FlexDirection::Column, row_gap: Val::Px(18.0), ..default() }).with_children(|col| {
            col.spawn(glass(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(14.0), padding: UiRect::all(Val::Px(24.0)), ..default() })).with_children(|p| {
                section(p, f, "Eigener Raum");
                p.spawn(text("Wähle Karte und Regeln, lade Freunde mit dem Code ein – Bots füllen die freien Plätze.", &f.text, 17.0, SOFT));
                primary(p, f, "Raum erstellen", NetAct::OpenCreate, Val::Percent(100.0), 60.0, 26.0);
            });
            col.spawn(glass(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(12.0), padding: UiRect::all(Val::Px(24.0)), ..default() })).with_children(|p| {
                section(p, f, "Spieler");
                label(p, f, "Dein Name");
                field(p, f, Field::Name, &v.options.name, "Name", ui.focus == Some(Field::Name), false, Val::Percent(100.0));
                label(p, f, "Privater Raum");
                p.spawn(Node { column_gap: Val::Px(10.0), ..default() }).with_children(|r| {
                    field(r, f, Field::Code, &ui.code, "Code, z. B. K7Q2XM", ui.focus == Some(Field::Code), false, Val::Px(250.0));
                    small_button(r, f, "Beitreten", NetAct::JoinCode, false);
                });
            });
            col.spawn(glass(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), padding: UiRect::all(Val::Px(24.0)), flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::clip(), ..default() })).with_children(|p| {
                section(p, f, "Server");
                for (i, s) in v.browser.servers.iter().enumerate() {
                    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() }).with_children(|r| {
                        let dot = if s.error.is_some() { DANGER } else if s.done { MONEY } else { DIM };
                        r.spawn((Node { width: Val::Px(9.0), height: Val::Px(9.0), border_radius: BorderRadius::all(Val::Px(5.0)), flex_shrink: 0.0, ..default() }, BackgroundColor(dot)));
                        r.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_width: Val::Px(0.0), ..default() }).with_children(|t| {
                            t.spawn(text(&s.name, &f.label, 21.0, Color::WHITE));
                            let detail = match (&s.error, s.ping) {
                                (Some(e), _) => format!("{} · {e}", link::short(&s.url)),
                                (None, Some(ms)) => format!("{} · {} Räume · {:.0} ms", link::short(&s.url), s.rooms, ms),
                                _ => format!("{} · …", link::short(&s.url)),
                            };
                            t.spawn(text(&detail, &f.label, 16.0, DIM));
                        });
                        if i > 0 && !s.announced {
                            small_button(r, f, "✕", NetAct::RemoveServer(s.url.clone()), false);
                        }
                    });
                }
                p.spawn(Node { column_gap: Val::Px(10.0), margin: UiRect::top(Val::Px(6.0)), ..default() }).with_children(|r| {
                    field(r, f, Field::Address, &ui.address, "Adresse, z. B. 192.168.1.20", ui.focus == Some(Field::Address), false, Val::Px(250.0));
                    small_button(r, f, "Hinzufügen", NetAct::AddServer, false);
                });
                p.spawn(text("Eigenen Server starten: zugriff-server (siehe README).", &f.text, 15.0, DIM));
            });
        });
    });
}

fn room_row(c: &mut ChildSpawnerCommands, f: &Fonts, v: &View, r: &Row, cols: &[(&str, f32); 5]) {
    let on = v.ui.selected.as_ref().is_some_and(|(u, id)| *u == r.url && *id == r.info.id);
    let (bg, border) = if on { (Color::srgba(1.0, 0.69, 0.0, 0.14), AMBER) } else { (Color::srgba(1.0, 1.0, 1.0, 0.03), Color::NONE) };
    let full = r.info.humans >= r.info.slots;
    c.spawn((
        Button,
        Node { padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)), align_items: AlignItems::Center, border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(8.0)), flex_shrink: 0.0, ..default() },
        BackgroundColor(bg),
        BorderColor::all(border),
        Hover::new(bg, if on { bg } else { Color::srgba(1.0, 1.0, 1.0, 0.08) }, border, if on { AMBER_HI } else { LINE_HI }),
        NetAct::Select(r.url.clone(), r.info.id),
    ))
    .with_children(|row| {
        row.spawn((Node { flex_grow: 1.0, min_width: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() }, Pickable::IGNORE)).with_children(|t| {
            t.spawn((Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|n| {
                if r.info.password {
                    n.spawn((text("🔒", &f.text, 18.0, AMBER), Pickable::IGNORE));
                }
                n.spawn((text(&r.info.name, &f.label, 25.0, Color::WHITE), Pickable::IGNORE));
            });
            t.spawn((text(&r.info.server, &f.label, 16.0, DIM), Pickable::IGNORE));
        });
        let players = if r.info.bots > 0 { format!("{}/{}  +{} Bots", r.info.humans, r.info.slots, r.info.bots) } else { format!("{}/{}", r.info.humans, r.info.slots) };
        let standing = match r.info.phase {
            Phase::Warmup => "Aufwärmen".to_string(),
            Phase::MatchOver => "Spielende".to_string(),
            _ if r.info.mode == Mode::Deathmatch => "läuft".to_string(),
            _ => format!("{}:{} · R{}", r.info.score[0], r.info.score[1], r.info.round),
        };
        let ping = v.browser.ping(&r.url);
        let cells: [(String, Color); 5] = [
            (map_title(&r.info.map).to_string(), Color::WHITE),
            (mode_name(r.info.mode).to_string(), if r.info.mode == Mode::Rescue { KSK } else if r.info.mode == Mode::Deathmatch { OPFOR } else { AMBER }),
            (players, if full { DANGER } else { Color::WHITE }),
            (standing, SOFT),
            (ping.map_or("–".into(), |p| format!("{p:.0}")), ping.map_or(DIM, ping_color)),
        ];
        for ((t, color), (_, w)) in cells.into_iter().zip(cols.iter()) {
            row.spawn((Node { width: Val::Px(*w), flex_shrink: 0.0, ..default() }, Pickable::IGNORE, children![(text(&t, &f.label, 22.0, color), Pickable::IGNORE)]));
        }
    });
}

fn create_page(body: &mut ChildSpawnerCommands, f: &Fonts, server: &AssetServer, v: &View) {
    let ui = v.ui;
    let s = &ui.setup;
    body.spawn(Node { width: Val::Percent(100.0), height: Val::Percent(100.0), column_gap: Val::Px(28.0), ..default() }).with_children(|row| {
        // Maps.
        row.spawn(Node { width: Val::Px(430.0), flex_shrink: 0.0, flex_direction: FlexDirection::Column, row_gap: Val::Px(12.0), ..default() }).with_children(|col| {
            section(col, f, "Karte");
            for (i, m) in MAPS.iter().enumerate() {
                let on = s.map == m.name;
                let border = if on { AMBER } else { LINE };
                col.spawn((
                    Button,
                    Node { width: Val::Percent(100.0), height: Val::Px(112.0), border: UiRect::all(Val::Px(if on { 2.0 } else { 1.0 })), border_radius: BorderRadius::all(Val::Px(12.0)), overflow: Overflow::clip(), align_items: AlignItems::End, ..default() },
                    BackgroundColor(GLASS_HI),
                    BorderColor::all(border),
                    Hover::new(GLASS_HI, GLASS_HI, border, if on { AMBER_HI } else { LINE_HI }),
                    NetAct::Map(i),
                ))
                .with_children(|card| {
                    card.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(-70.0), width: Val::Percent(100.0), aspect_ratio: Some(5.0 / 3.0), ..default() }, ImageNode::new(server.load(format!("thumbs/{}.jpg", m.name))).with_color(if on { Color::srgb(0.8, 0.8, 0.8) } else { Color::srgb(0.42, 0.44, 0.46) }), Pickable::IGNORE));
                    card.spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundGradient::from(LinearGradient::to_right(vec![ColorStop::percent(Color::srgba(0.01, 0.015, 0.02, 0.92), 0.0), ColorStop::percent(Color::srgba(0.01, 0.015, 0.02, 0.0), 100.0)])), Pickable::IGNORE));
                    card.spawn((Node { flex_direction: FlexDirection::Column, padding: UiRect::new(Val::Px(20.0), Val::Px(16.0), Val::Px(0.0), Val::Px(12.0)), ..default() }, Pickable::IGNORE)).with_children(|t| {
                        t.spawn((text(m.title, &f.bold, 32.0, Color::WHITE), Pickable::IGNORE));
                        t.spawn((caps(m.objective, &f.bold, 13.0, if m.objective == "Geiselrettung" { KSK } else { AMBER }), Pickable::IGNORE));
                    });
                });
            }
        });
        // Rules.
        row.spawn(glass(Node { flex_grow: 1.0, min_width: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(16.0), padding: UiRect::all(Val::Px(28.0)), ..default() })).with_children(|p| {
            section(p, f, "Regeln");
            p.spawn(Node { column_gap: Val::Px(28.0), ..default() }).with_children(|cols| {
                cols.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(16.0), flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() }).with_children(|c| {
                    label(c, f, "Name des Raums");
                    field(c, f, Field::RoomName, &s.name, &format!("{}s Raum", v.options.name), ui.focus == Some(Field::RoomName), false, Val::Percent(100.0));
                    choice(c, f, "Modus", vec![("Ziel der Karte", NetAct::Mode(false), s.mode != Mode::Deathmatch), ("Deathmatch", NetAct::Mode(true), s.mode == Mode::Deathmatch)]);
                    if s.mode != Mode::Deathmatch {
                        choice(c, f, "Länge", vec![("Kurz · MR8", NetAct::Rounds(16), s.max_rounds == 16), ("Lang · MR15", NetAct::Rounds(30), s.max_rounds == 30)]);
                    }
                    c.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }).with_children(|g| {
                        label(g, f, "Spieler pro Seite");
                        g.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(12.0), ..default() }).with_children(|r| {
                            small_button(r, f, "–", NetAct::PerSide(-1), false);
                            r.spawn((Node { width: Val::Px(120.0), justify_content: JustifyContent::Center, ..default() }, children![text(&format!("{0} gegen {0}", s.per_side), &f.digits, 28.0, AMBER)]));
                            small_button(r, f, "+", NetAct::PerSide(1), false);
                        });
                    });
                });
                cols.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(16.0), flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() }).with_children(|c| {
                    choice(c, f, "Sichtbarkeit", vec![("Öffentlich", NetAct::Private(false), !s.private), ("Privat · mit Code", NetAct::Private(true), s.private)]);
                    label(c, f, "Passwort (optional)");
                    field(c, f, Field::RoomPassword, &s.password, "ohne Passwort", ui.focus == Some(Field::RoomPassword), false, Val::Percent(100.0));
                    switch(c, f, "Bots füllen freie Plätze", s.bots, NetAct::Bots);
                    if s.bots {
                        choice(c, f, "Bot-Stärke", vec![("Leicht", NetAct::Difficulty(0), s.difficulty == 0), ("Normal", NetAct::Difficulty(1), s.difficulty == 1), ("Schwer", NetAct::Difficulty(2), s.difficulty == 2), ("Experte", NetAct::Difficulty(3), s.difficulty == 3)]);
                    }
                    switch(c, f, "Eigenbeschuss", s.friendly_fire, NetAct::FriendlyFire);
                });
            });
            p.spawn(Node { flex_grow: 1.0, ..default() });
            rule(p);
            let hosts = servers(v.options);
            if hosts.len() > 1 {
                p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }).with_children(|g| {
                    label(g, f, "Auf Server");
                    g.spawn(Node { column_gap: Val::Px(8.0), flex_wrap: FlexWrap::Wrap, row_gap: Val::Px(8.0), ..default() }).with_children(|r| {
                        for (i, h) in hosts.iter().enumerate() {
                            let name = v.browser.servers.iter().find(|s| s.url == *h).map_or_else(|| link::short(h), |s| s.name.clone());
                            small_button(r, f, &name, NetAct::Host(i), ui.host.min(hosts.len() - 1) == i);
                        }
                    });
                });
            }
            p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(14.0), ..default() }).with_children(|b| {
                ghost(b, f, "‹ Zurück", NetAct::Back, Val::Auto, false);
                b.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, row_gap: Val::Px(4.0), ..default() }).with_children(|t| {
                    let who = if s.private { "Nur mit Code" } else { "In der Serverliste" };
                    let bots = if s.bots { "Bots füllen auf" } else { "ohne Bots" };
                    t.spawn(text(&format!("{} · {who} · {bots}", map_title(&s.map)), &f.label, 20.0, DIM));
                    status(t, f, v);
                });
                if v.net.busy() {
                    ghost(b, f, "Abbrechen", NetAct::Cancel, Val::Px(260.0), true);
                } else {
                    primary(b, f, "Raum erstellen", NetAct::Create, Val::Px(300.0), 70.0, 28.0);
                }
            });
        });
    });
}

// ---- input ------------------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn buttons(
    q: Query<(&Interaction, &NetAct), (Changed<Interaction>, With<Button>)>,
    mut ui: ResMut<OnlineUi>,
    mut net: ResMut<Net>,
    mut browser: ResMut<Browser>,
    mut options: ResMut<Options>,
    mut cues: MessageWriter<crate::audio::Cue>,
) {
    for (i, act) in &q {
        match i {
            Interaction::Hovered => {
                cues.write(crate::audio::Cue::UiHover);
                continue;
            }
            Interaction::None => continue,
            Interaction::Pressed => {}
        }
        cues.write(crate::audio::Cue::UiClick);
        ui.version += 1;
        if !matches!(act, NetAct::Focus(_)) {
            ui.focus = None;
        }
        match act.clone() {
            NetAct::Refresh => browser.start(servers(&options)),
            NetAct::Filter(x) => ui.filter = x,
            NetAct::Select(url, id) => {
                if ui.selected == Some((url.clone(), id)) {
                    join_selected(&mut ui, &mut net, &browser);
                } else {
                    ui.selected = Some((url, id));
                    ui.password.clear();
                    net.error = None;
                }
            }
            NetAct::Join => join_selected(&mut ui, &mut net, &browser),
            NetAct::JoinCode => join_code(&mut ui, &mut net),
            NetAct::Team(t) => ui.team = [Some(Team::Ksk), Some(Team::Opfor), None][t as usize],
            NetAct::Focus(which) => ui.focus = if ui.focus == Some(which) { None } else { Some(which) },
            NetAct::AddServer => add_server(&mut ui, &mut options, &mut browser),
            NetAct::RemoveServer(url) => {
                options.servers.retain(|s| link::normalize(s) != url);
                browser.start(servers(&options));
            }
            NetAct::OpenCreate => {
                ui.page = Page::Create;
                net.error = None;
                if ui.setup.map.is_empty() || !MAPS.iter().any(|m| m.name == ui.setup.map) {
                    ui.setup.map = options.last_map.clone();
                }
            }
            NetAct::Back => {
                ui.page = Page::List;
                net.error = None;
            }
            NetAct::Create => {
                let hosts = servers(&options);
                let url = hosts[ui.host.min(hosts.len() - 1)].clone();
                let mut setup = ui.setup.clone();
                if setup.name.trim().is_empty() {
                    setup.name = format!("{}s Raum", options.name);
                }
                net.create(&url, setup);
            }
            NetAct::Map(i) => ui.setup.map = MAPS[i].name.to_string(),
            NetAct::Mode(dm) => ui.setup.mode = if dm { Mode::Deathmatch } else { Mode::Defuse },
            NetAct::PerSide(d) => ui.setup.per_side = (ui.setup.per_side as i8 + d).clamp(1, 8) as u8,
            NetAct::Bots => ui.setup.bots = !ui.setup.bots,
            NetAct::Difficulty(d) => ui.setup.difficulty = d,
            NetAct::Rounds(r) => ui.setup.max_rounds = r,
            NetAct::Private(p) => ui.setup.private = p,
            NetAct::FriendlyFire => ui.setup.friendly_fire = !ui.setup.friendly_fire,
            NetAct::Host(i) => ui.host = i,
            NetAct::Cancel => net.disconnect(),
        }
    }
}

fn join_selected(ui: &mut OnlineUi, net: &mut Net, browser: &Browser) {
    let Some((url, id)) = ui.selected.clone() else { return };
    if !browser.rows.iter().any(|r| r.url == url && r.info.id == id) {
        return;
    }
    net.join(&url, RoomRef::Id(id), ui.password.clone(), ui.team);
}

fn join_code(ui: &mut OnlineUi, net: &mut Net) {
    let code: String = ui.code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase();
    if code.is_empty() {
        ui.focus = Some(Field::Code);
        return;
    }
    // A code is for a room on the default server (or on the server the code names: "host:port/CODE").
    let (url, code) = match ui.code.trim().rsplit_once('/') {
        Some((host, c)) if !host.is_empty() => (link::normalize(host), c.trim().to_uppercase()),
        _ => (default_server(), code),
    };
    net.join(&url, RoomRef::Code(code), ui.password.clone(), ui.team);
}

fn add_server(ui: &mut OnlineUi, options: &mut Options, browser: &mut Browser) {
    let a = ui.address.trim().to_string();
    if a.is_empty() {
        ui.focus = Some(Field::Address);
        return;
    }
    let url = link::normalize(&a);
    if !servers(options).contains(&url) {
        options.servers.push(a);
    }
    ui.address.clear();
    browser.start(servers(options));
}

/// Keys into the focused field.
#[allow(clippy::too_many_arguments)]
fn type_fields(mut typed: MessageReader<KeyboardInput>, mut ui: ResMut<OnlineUi>, mut options: ResMut<Options>, mut net: ResMut<Net>, mut browser: ResMut<Browser>, menu: Res<MenuState>) {
    let Some(which) = ui.focus.filter(|_| menu.screen == Screen::Online) else {
        typed.clear();
        return;
    };
    let mut changed = false;
    for k in typed.read() {
        if k.state != ButtonState::Pressed {
            continue;
        }
        let mut value = match which {
            Field::Name => options.name.clone(),
            Field::Code => ui.code.clone(),
            Field::Address => ui.address.clone(),
            Field::Password => ui.password.clone(),
            Field::RoomName => ui.setup.name.clone(),
            Field::RoomPassword => ui.setup.password.clone(),
        };
        let max = match which {
            Field::Name => 24,
            Field::Code => 64,
            Field::Address => 120,
            _ => 32,
        };
        match &k.logical_key {
            Key::Enter => {
                ui.focus = None;
                match which {
                    Field::Code => join_code(&mut ui, &mut net),
                    Field::Address => add_server(&mut ui, &mut options, &mut browser),
                    Field::Password => join_selected(&mut ui, &mut net, &browser),
                    _ => {}
                }
                ui.version += 1;
                return;
            }
            Key::Escape | Key::Tab => {
                ui.focus = None;
                ui.version += 1;
                return;
            }
            Key::Backspace => {
                value.pop();
            }
            _ => {
                if let Some(t) = &k.text {
                    for c in t.chars().filter(|c| !c.is_control()) {
                        if value.chars().count() < max {
                            value.push(c);
                        }
                    }
                }
            }
        }
        changed = true;
        match which {
            Field::Name => options.name = value,
            Field::Code => ui.code = value,
            Field::Address => ui.address = value,
            Field::Password => ui.password = value,
            Field::RoomName => ui.setup.name = value,
            Field::RoomPassword => ui.setup.password = value,
        }
    }
    if changed {
        ui.version += 1;
    }
}
