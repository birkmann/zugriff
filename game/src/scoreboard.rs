//! The scoreboard (held Tab): both sides with kills, deaths, assists, MVPs, score and — for your own side — money;
//! the dead dimmed, the local player highlighted. And the radio menus: Z gives the bots orders (follow me, hold,
//! regroup, go to a site, storm), X sends reports.

use crate::hud::{team_color, text, Fonts, AMBER};
use crate::input::Ui;
use crate::session::{Frame, Session};
use bevy::prelude::*;
use zugriff_sim::bots::Order;
use zugriff_sim::game::{Event, Mode, Team};

pub struct ScoreboardPlugin;

impl Plugin for ScoreboardPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn).add_systems(Update, (board, radio_menu, radio_choose).chain().after(crate::input::InputSet));
    }
}

#[derive(Component)]
struct Board;
#[derive(Component)]
struct RadioBox;

fn spawn(mut commands: Commands) {
    commands.spawn((
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        Visibility::Hidden,
        Board,
        GlobalZIndex(4),
    ));
    commands.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(24.0), top: Val::Percent(38.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), padding: UiRect::all(Val::Px(14.0)), border_radius: BorderRadius::all(Val::Px(8.0)), ..default() },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Visibility::Hidden,
        RadioBox,
        GlobalZIndex(4),
    ));
}

#[allow(clippy::too_many_arguments)]
fn board(mut commands: Commands, ui: Res<Ui>, session: Option<Res<Session>>, fonts: Res<Fonts>, root: Single<(Entity, &mut Visibility, Option<&Children>), With<Board>>, time: Res<Time>, mut last: Local<f32>, net: Res<crate::net::Net>) {
    let (e, mut vis, children) = root.into_inner();
    let show = session.is_some() && (ui.scoreboard || session.as_ref().is_some_and(|s| s.game.phase == zugriff_sim::game::Phase::MatchOver && !ui.menu && false));
    *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
    let Some(s) = session.filter(|_| show) else { return };
    // Rebuilt a few times a second while shown.
    *last += time.delta_secs();
    if *last < 0.25 && children.is_some_and(|c| !c.is_empty()) {
        return;
    }
    *last = 0.0;
    if let Some(ch) = children {
        for c in ch.iter() {
            commands.entity(c).despawn();
        }
    }
    let g = &s.game;
    let my_team = s.me().team;
    commands.entity(e).with_children(|c| {
        c.spawn((
            Node { width: Val::Px(1100.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), padding: UiRect::all(Val::Px(22.0)), border_radius: BorderRadius::all(Val::Px(12.0)), ..default() },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.9)),
        ))
        .with_children(|p| {
            let mode = match g.cfg.mode {
                Mode::Defuse => "Sprengladung",
                Mode::Rescue => "Geiselrettung",
                Mode::Deathmatch => "Deathmatch",
            };
            p.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|h| {
                h.spawn(text(&format!("{} · {}", s.settings.map, mode), &fonts.label, 26.0, Color::srgba(1.0, 1.0, 1.0, 0.7)));
                h.spawn(text(&format!("Runde {} von {}", g.round.max(1), g.cfg.max_rounds), &fonts.label, 26.0, Color::srgba(1.0, 1.0, 1.0, 0.7)));
            });
            for team in [Team::Ksk, Team::Opfor] {
                let score = if g.cfg.mode == Mode::Deathmatch { g.players.iter().filter(|q| q.team == team).map(|q| q.stats.kills).sum::<i32>() as u32 } else { g.score[team.index()] };
                p.spawn((Node { justify_content: JustifyContent::SpaceBetween, padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)), border_radius: BorderRadius::all(Val::Px(6.0)), ..default() }, BackgroundColor(team_color(team).with_alpha(0.18)))).with_children(|h| {
                    h.spawn(text(team.name(), &fonts.bold, 34.0, team_color(team)));
                    h.spawn(text(&score.to_string(), &fonts.digits, 40.0, team_color(team)));
                });
                let head = |r: &mut ChildSpawnerCommands| {
                    for (label, w) in [("Name", 380.0), ("Geld", 110.0), ("K", 70.0), ("T", 70.0), ("A", 70.0), ("MVP", 70.0), ("Punkte", 110.0), ("Ping", 80.0)] {
                        r.spawn((Node { width: Val::Px(w), ..default() }, children![text(label, &fonts.label, 20.0, Color::srgba(1.0, 1.0, 1.0, 0.45))]));
                    }
                };
                p.spawn(Node { padding: UiRect::horizontal(Val::Px(12.0)), ..default() }).with_children(head);
                let mut list: Vec<&zugriff_sim::game::Player> = g.players.iter().filter(|q| q.team == team && !q.vacant).collect();
                list.sort_by_key(|q| (-q.stats.score, -q.stats.kills));
                for q in list {
                    let alpha = if q.alive { 1.0 } else { 0.4 };
                    let me = q.id == s.me;
                    p.spawn((Node { padding: UiRect::axes(Val::Px(12.0), Val::Px(3.0)), border_radius: BorderRadius::all(Val::Px(4.0)), ..default() }, BackgroundColor(if me { Color::srgba(1.0, 0.69, 0.0, 0.15) } else { Color::NONE }))).with_children(|r| {
                        let name = format!("{}{}", q.name, if me { "  (du)" } else if q.bot && s.online { "  · Bot" } else { "" });
                        let money = if team == my_team { format!("${}", q.money) } else { String::new() };
                        let cells = [(name, 380.0), (money, 110.0), (q.stats.kills.to_string(), 70.0), (q.stats.deaths.to_string(), 70.0), (q.stats.assists.to_string(), 70.0), (q.stats.mvps.to_string(), 70.0), (q.stats.score.to_string(), 110.0), (ping_of(&s, &net, q), 80.0)];
                        for (i, (t, w)) in cells.into_iter().enumerate() {
                            let color = if i == 0 { team_color(team).with_alpha(alpha) } else { Color::srgba(1.0, 1.0, 1.0, alpha) };
                            r.spawn((Node { width: Val::Px(w), ..default() }, children![text(&t, &fonts.label, 24.0, color)]));
                        }
                    });
                }
            }
        });
    });
}

const COMMANDS: [(&str, u8); 6] = [("Folgt mir!", 0), ("Position halten!", 1), ("Sammeln bei mir!", 2), ("Auf Zielpunkt A!", 3), ("Auf Zielpunkt B!", 4), ("Stürmt!", 5)];
const REPORTS: [&str; 6] = ["Verstanden!", "Feind gesichtet!", "Brauche Unterstützung!", "Bereich gesichert.", "Ich bin in Position.", "Negativ."];

fn radio_menu(mut commands: Commands, ui: Res<Ui>, fonts: Res<Fonts>, root: Single<(Entity, &mut Visibility, Option<&Children>), With<RadioBox>>, mut shown: Local<Option<u8>>) {
    let (e, mut vis, children) = root.into_inner();
    *vis = if ui.radio.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    if *shown == ui.radio {
        return;
    }
    *shown = ui.radio;
    if let Some(ch) = children {
        for c in ch.iter() {
            commands.entity(c).despawn();
        }
    }
    let Some(menu) = ui.radio else { return };
    commands.entity(e).with_children(|c| {
        c.spawn(text(if menu == 1 { "FUNK – BEFEHLE" } else { "FUNK – MELDUNGEN" }, &fonts.bold, 26.0, AMBER));
        let lines: Vec<&str> = if menu == 1 { COMMANDS.iter().map(|c| c.0).collect() } else { REPORTS.to_vec() };
        for (i, l) in lines.iter().enumerate() {
            c.spawn(text(&format!("{}  {}", i + 1, l), &fonts.label, 24.0, Color::WHITE));
        }
        c.spawn(text("0  Abbrechen", &fonts.label, 22.0, Color::srgba(1.0, 1.0, 1.0, 0.5)));
    });
}

/// A player's ping online: the server's measure (yours: your own, fresher); bots show BOT.
fn ping_of(s: &Session, net: &crate::net::Net, q: &zugriff_sim::game::Player) -> String {
    if !s.online {
        return String::new();
    }
    if q.bot {
        return "BOT".into();
    }
    let ms = if q.id == s.me && net.rtt > 0.0 { net.rtt as u16 } else { net.pings.get(q.id).copied().unwrap_or(0) };
    if ms == 0 { "–".into() } else { ms.to_string() }
}

fn radio_choose(mut ui: ResMut<Ui>, keys: Res<ButtonInput<KeyCode>>, session: Option<ResMut<Session>>, mut frame: ResMut<Frame>, net: Res<crate::net::Net>) {
    let Some(menu) = ui.radio else { return };
    if keys.just_pressed(KeyCode::Digit0) || keys.just_pressed(KeyCode::Escape) {
        ui.radio = None;
        return;
    }
    let Some(d) = ui.digit else { return };
    let Some(mut s) = session else { return };
    let i = d as usize - 1;
    let me = s.me;
    let team = s.me().team;
    let mut order_index = None;
    let text = if menu == 1 {
        let Some(&(line, cmd)) = COMMANDS.get(i) else { return };
        order_index = Some(cmd);
        let order = match cmd {
            0 => Order::FollowMe(me),
            1 => Order::Hold(me),
            2 => Order::Regroup(me),
            3 => Order::GoSite('A'),
            4 => Order::GoSite('B'),
            _ => Order::Storm,
        };
        if !s.online {
            s.bots.order(team, order);
        }
        line.to_string()
    } else {
        let Some(line) = REPORTS.get(i) else { return };
        let place = s.game.place_at(s.me().mover.origin).map(|p| format!(" – {p}")).unwrap_or_default();
        if i == 1 || i == 2 { format!("{line}{place}") } else { line.to_string() }
    };
    if s.online {
        // The server gives the order to its bots and sends the line to everyone.
        net.radio(order_index, text);
    } else {
        frame.events.push(Event::Radio { player: me, text });
    }
    ui.radio = None;
}
