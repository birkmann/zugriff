//! The buy menu (B): the side's whole catalogue at once, in five numbered columns as in the genre — pistols, heavy
//! weapons, submachine guns, rifles, equipment — each item a tile with its picture and price. A number picks a
//! column, a second number buys from it; the mouse buys with a click (the mouse stays captured; a cursor of our own
//! moves over the menu). What can't be afforded or is already carried is dimmed. The panel on the right shows what
//! the cursor is over: a weapon's damage, rate of fire, armour penetration, range and mobility — or, over nothing, what
//! is carried now.
//!
//! Tiles are only respawned when their content changes (column, money, side, inventory); hovering recolours them in
//! place and rebuilds only the details, so they keep their layout and the hit test always sees them.

use crate::actors::{prop_kind, PropKind};
use crate::hud::{text, Fonts, AMBER, KSK, OPFOR};
use crate::input::{Held, Ui};
use crate::session::Session;
use crate::preview::{Previews, Size};
use crate::ui_kit::{caps, glass, keycap, section, tag, AMBER_HI, DANGER, DIM, GLASS_HI, LINE, MONEY, SOFT};
use bevy::prelude::*;
use bevy::text::LetterSpacing;
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;
use zugriff_sim::game::{money, Buy, Phase, Player, Team};
use zugriff_sim::weapons::{Class, WeaponId};

pub struct BuyPlugin;

impl Plugin for BuyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuyState>().add_systems(Startup, spawn).add_systems(Update, (qa_open, close_when_unable, rebuild, interact, highlight, details, cursor).chain().after(crate::input::InputSet));
    }
}

#[derive(Resource, Default)]
struct BuyState {
    /// The column picked with a number key (the next number buys from it).
    category: Option<usize>,
    hover: Option<Item>,
    shown: Option<(Option<usize>, i32, Team, u64, bool)>,
    /// The hover the details panel was last built for.
    details_for: Option<Option<Item>>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Item {
    Category(usize),
    Buy(Buy),
}

#[derive(Component)]
struct BuyRoot;
#[derive(Component)]
struct Columns;
#[derive(Component)]
struct Details;
#[derive(Component)]
struct Cursor;
#[derive(Component)]
struct Clickable(Item);
/// Whether a tile can be bought now (only those light up under the cursor).
#[derive(Component)]
struct Affordable(bool);
#[derive(Component)]
struct Header;
/// The key hints under the columns (filled once the fonts are in).
#[derive(Component)]
struct Footer;
/// The buy-time line in the header, updated in place.
#[derive(Component)]
struct TimeLine;

const CATEGORIES: [&str; 5] = ["Pistolen", "Schwere Waffen", "Maschinenpistolen", "Gewehre", "Ausrüstung"];

fn items(cat: usize, team: Team) -> Vec<Buy> {
    let weapons: &[WeaponId] = match cat {
        0 => &[WeaponId::P8, WeaponId::Mp443, WeaponId::P30, WeaponId::Rev357],
        1 => &[WeaponId::M870, WeaponId::M1014, WeaponId::Mg5, WeaponId::Pkm],
        2 => &[WeaponId::Mp7, WeaponId::Pp19, WeaponId::Mp5, WeaponId::Ump],
        3 => &[WeaponId::G36, WeaponId::Ak74, WeaponId::G95, WeaponId::Akm, WeaponId::Ssg3000, WeaponId::G28, WeaponId::Svd, WeaponId::G22],
        _ => &[],
    };
    if cat == 4 {
        let mut v = vec![Buy::Vest, Buy::VestHelmet, Buy::Weapon(WeaponId::Flash), Buy::Weapon(WeaponId::He), Buy::Weapon(WeaponId::Smoke)];
        if team == Team::Ksk {
            v.push(Buy::Defuser);
        }
        return v;
    }
    weapons.iter().filter(|w| team.may_buy(w.def().side)).map(|&w| Buy::Weapon(w)).collect()
}

fn label(b: Buy) -> String {
    match b {
        Buy::Weapon(w) => w.name().to_string(),
        Buy::Vest => "Schutzweste".into(),
        Buy::VestHelmet => "Weste + Helm".into(),
        Buy::Defuser => "Entschärferset".into(),
    }
}

/// The item's picture (see tools/models/menu_art.py).
fn icon(b: Buy) -> &'static str {
    match b {
        Buy::Weapon(WeaponId::He) => "ui/icons/he.png",
        Buy::Weapon(WeaponId::Flash) => "ui/icons/flash.png",
        Buy::Weapon(WeaponId::Smoke) => "ui/icons/smoke.png",
        Buy::Weapon(w) => match prop_kind(w) {
            PropKind::Ak => "ui/icons/ak.png",
            PropKind::Sniper => "ui/icons/sniper.png",
            PropKind::Shotgun => "ui/icons/shotgun.png",
            PropKind::Smg => "ui/icons/smg.png",
            PropKind::Mg => "ui/icons/mg.png",
            PropKind::Pistol => "ui/icons/pistol.png",
            _ => "ui/icons/rifle.png",
        },
        Buy::Vest => "ui/icons/vest.png",
        Buy::VestHelmet => "ui/icons/vesthelm.png",
        Buy::Defuser => "ui/icons/kit.png",
    }
}

fn class_name(b: Buy) -> &'static str {
    match b {
        Buy::Weapon(w) => match w.def().class {
            Class::Pistol => "Pistole",
            Class::Smg => "Maschinenpistole",
            Class::Shotgun => "Flinte",
            Class::Rifle => "Sturmgewehr",
            Class::Sniper => "Scharfschützengewehr",
            Class::Mg => "Maschinengewehr",
            Class::Grenade => "Granate",
            _ => "",
        },
        Buy::Vest | Buy::VestHelmet => "Schutz",
        Buy::Defuser => "Werkzeug",
    }
}

fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundColor(Color::srgba(0.01, 0.015, 0.02, 0.55)),
            Visibility::Hidden,
            BuyRoot,
            GlobalZIndex(5),
        ))
        .with_children(|c| {
            c.spawn(glass(Node { width: Val::Percent(94.0), max_width: Val::Px(1760.0), max_height: Val::Percent(96.0), flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(26.0)), row_gap: Val::Px(18.0), ..default() })).with_children(|p| {
                p.spawn((Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(20.0), ..default() }, Header));
                p.spawn((Node { height: Val::Px(1.0), ..default() }, BackgroundColor(LINE)));
                p.spawn(Node { column_gap: Val::Px(22.0), flex_grow: 1.0, min_height: Val::Px(0.0), ..default() }).with_children(|row| {
                    row.spawn((Node { column_gap: Val::Px(12.0), flex_grow: 1.0, min_width: Val::Px(0.0), ..default() }, Columns));
                    row.spawn((
                        Node { width: Val::Px(380.0), flex_shrink: 0.0, flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), padding: UiRect::all(Val::Px(22.0)), border_radius: BorderRadius::all(Val::Px(12.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.035)),
                        BorderColor::all(LINE),
                        Details,
                    ));
                });
                p.spawn((Node { column_gap: Val::Px(22.0), align_items: AlignItems::Center, ..default() }, Footer));
            });
        });
    commands.spawn((
        Node { position_type: PositionType::Absolute, width: Val::Px(16.0), height: Val::Px(16.0), border_radius: BorderRadius::all(Val::Px(8.0)), border: UiRect::all(Val::Px(2.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        BorderColor::all(Color::WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.5), Val::Px(0.0), Val::Px(1.0), Val::Px(0.0), Val::Px(4.0)),
        Visibility::Hidden,
        Cursor,
        GlobalZIndex(20),
        children![(Node { width: Val::Px(4.0), height: Val::Px(4.0), border_radius: BorderRadius::all(Val::Px(2.0)), ..default() }, BackgroundColor(AMBER))],
    ));
}

/// `--menu buy` (`?menu=buy`) opens the menu once a match can buy, for screenshots; `--cursor x,y` puts the cursor
/// there (UI pixels).
fn qa_open(mut ui: ResMut<Ui>, session: Option<Res<Session>>, mut done: Local<bool>) {
    if *done || crate::args::get("menu").as_deref() != Some("buy") {
        return;
    }
    let Some(s) = session else { return };
    if s.game.can_buy(s.me()) {
        *done = true;
        ui.buy = true;
        if let Some((x, y)) = crate::args::get("cursor").and_then(|c| c.split_once(',').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)))) {
            ui.cursor = Vec2::new(x, y);
        }
    }
}

fn close_when_unable(mut ui: ResMut<Ui>, session: Option<Res<Session>>, mut state: ResMut<BuyState>) {
    let Some(s) = session else {
        ui.buy = false;
        return;
    };
    if ui.buy && !s.game.can_buy(s.me()) {
        ui.buy = false;
    }
    if !ui.buy {
        state.category = None;
        state.hover = None;
    }
}

fn owned(me: &Player, b: Buy) -> bool {
    match b {
        Buy::Weapon(w) if w.def().class != Class::Grenade => me.inv.has(w),
        Buy::Weapon(WeaponId::He) => me.inv.he >= 1,
        Buy::Weapon(WeaponId::Flash) => me.inv.flash >= 2,
        Buy::Weapon(_) => me.inv.smoke >= 1,
        Buy::Vest => me.armor >= 100,
        Buy::VestHelmet => me.armor >= 100 && me.helmet,
        Buy::Defuser => me.defuser,
    }
}

fn price_for(me: &Player, b: Buy) -> i32 {
    if b == Buy::VestHelmet && me.armor >= 100 && !me.helmet { money::HELMET } else { b.price() }
}

#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut commands: Commands,
    ui: Res<Ui>,
    session: Option<Res<Session>>,
    fonts: Res<Fonts>,
    server: Res<AssetServer>,
    mut state: ResMut<BuyState>,
    mut root: Single<&mut Visibility, With<BuyRoot>>,
    columns: Single<(Entity, Option<&Children>), With<Columns>>,
    header: Single<(Entity, Option<&Children>), With<Header>>,
    footer: Single<(Entity, Option<&Children>), With<Footer>>,
    mut time_line: Query<&mut Text, With<TimeLine>>,
    (mut previews, mut images): (ResMut<Previews>, ResMut<Assets<Image>>),
) {
    root.set_if_neq(if ui.buy { Visibility::Inherited } else { Visibility::Hidden });
    let Some(s) = session.filter(|_| ui.buy) else {
        state.shown = None;
        state.details_for = None;
        return;
    };
    let me = s.me();
    let free = s.game.phase == Phase::Warmup || s.game.cfg.mode == zugriff_sim::game::Mode::Deathmatch;
    let buy_left = if s.game.phase == Phase::Freeze { s.game.cfg.buy_time } else { s.game.cfg.buy_time - s.game.live_for };
    let line = if free { "Aufwärmen: alles frei".to_string() } else { format!("Kaufzeit {}", fmt(buy_left)) };
    let key = (state.category, me.money, me.team, inv_hash(me), free);
    if state.shown == Some(key) {
        for mut t in &mut time_line {
            if t.0 != line {
                t.0 = line.clone();
            }
        }
        return;
    }
    state.shown = Some(key);
    state.details_for = None;
    let f = &*fonts;
    let (foot, filled) = footer.into_inner();
    if filled.is_none_or(|c| c.is_empty()) {
        commands.entity(foot).with_children(|r| {
            for (k, what) in [("1 – 5", "Spalte wählen"), ("1 – 6", "darin kaufen"), ("Klick", "kaufen"), ("B", "schließen")] {
                r.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|h| {
                    keycap(h, f, k);
                    h.spawn(text(what, &f.label, 19.0, DIM));
                });
            }
        });
    }
    // Header: title and side, buy time, money.
    let (h, hc) = header.into_inner();
    if let Some(ch) = hc {
        for c in ch.iter() {
            commands.entity(c).despawn();
        }
    }
    let side = if me.team == Team::Ksk { KSK } else { OPFOR };
    commands.entity(h).with_children(|c| {
        c.spawn(Node { column_gap: Val::Px(16.0), align_items: AlignItems::Center, width: Val::Px(560.0), ..default() }).with_children(|l| {
            l.spawn((text("AUSRÜSTUNG", &f.bold, 42.0, Color::WHITE), LetterSpacing::Px(3.0)));
            tag(l, f, me.team.name(), side);
        });
        c.spawn((
            Node { padding: UiRect::axes(Val::Px(18.0), Val::Px(6.0)), border_radius: BorderRadius::all(Val::Px(8.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)),
            BorderColor::all(LINE),
            children![(text(&line, &f.label, 26.0, if free { MONEY } else { SOFT }), TimeLine)],
        ));
        c.spawn(Node { width: Val::Px(560.0), justify_content: JustifyContent::End, align_items: AlignItems::Baseline, column_gap: Val::Px(8.0), ..default() }).with_children(|r| {
            r.spawn(caps("Konto", &f.bold, 16.0, DIM));
            r.spawn(text(&format!("${}", me.money), &f.digits, 56.0, MONEY));
        });
    });
    // The columns.
    let (l, lc) = columns.into_inner();
    if let Some(ch) = lc {
        for c in ch.iter() {
            commands.entity(c).despawn();
        }
    }
    commands.entity(l).with_children(|cols| {
        for (ci, name) in CATEGORIES.iter().enumerate() {
            let picked = state.category == Some(ci);
            let quiet = state.category.is_some() && !picked;
            cols.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), ..default() }).with_children(|col| {
                // The column's head: its number and name; picked by number or click.
                let (bg, border) = if picked { (Color::srgba(1.0, 0.69, 0.0, 0.18), AMBER) } else { (Color::srgba(1.0, 1.0, 1.0, 0.0), Color::NONE) };
                col.spawn((
                    Node { column_gap: Val::Px(10.0), align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)), border: UiRect::bottom(Val::Px(2.0)), border_radius: BorderRadius::all(Val::Px(6.0)), ..default() },
                    BackgroundColor(bg),
                    BorderColor::all(border),
                    Clickable(Item::Category(ci)),
                    Affordable(false),
                ))
                .with_children(|h| {
                    h.spawn(text(&(ci + 1).to_string(), &f.digits, 26.0, AMBER));
                    h.spawn(caps(name, &f.bold, 18.0, if quiet { DIM } else { Color::WHITE }));
                });
                for (i, b) in items(ci, me.team).into_iter().enumerate() {
                    let price = price_for(me, b);
                    let has = owned(me, b);
                    let can = (free || me.money >= price) && !has;
                    let pic = picture(&server, &mut previews, &mut images, me, b, Size::Tile);
                    tile(col, f, pic, i + 1, b, if free { 0 } else { price }, can, has, quiet, state.hover == Some(Item::Buy(b)));
                }
            });
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn tile(c: &mut ChildSpawnerCommands, f: &Fonts, pic: (Handle<Image>, bool), key: usize, b: Buy, price: i32, can: bool, has: bool, quiet: bool, hover: bool) {
    let (bg, border) = tile_colors(hover, can);
    // Rendered pictures are in colour (dimmed when out of reach); the equipment's white ones are tinted.
    let ink = match (pic.1, !can, quiet) {
        (true, true, _) => Color::srgba(0.55, 0.55, 0.55, 0.35),
        (true, false, true) => Color::srgba(1.0, 1.0, 1.0, 0.55),
        (true, false, false) => Color::WHITE,
        (false, true, _) => Color::srgba(1.0, 1.0, 1.0, 0.22),
        (false, false, true) => Color::srgba(0.85, 0.9, 0.85, 0.5),
        (false, false, false) => Color::srgb(0.86, 0.93, 0.86),
    };
    c.spawn((
        Node { height: Val::Px(104.0), flex_direction: FlexDirection::Column, padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(10.0)), overflow: Overflow::clip(), ..default() },
        BackgroundColor(bg),
        BorderColor::all(border),
        Clickable(Item::Buy(b)),
        Affordable(can),
    ))
    .with_children(|t| {
        t.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(text(&key.to_string(), &f.digits, 18.0, if can { AMBER } else { DIM }));
            r.spawn(text(&label(b), &f.label, 19.0, if can { Color::WHITE } else { DIM }));
        });
        t.spawn(Node { flex_grow: 1.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, min_height: Val::Px(0.0), ..default() }).with_children(|m| {
            m.spawn((Node { height: Val::Px(64.0), aspect_ratio: Some(Size::ASPECT), ..default() }, ImageNode::new(pic.0).with_color(ink)));
        });
        t.spawn(Node { justify_content: JustifyContent::End, ..default() }).with_children(|r| {
            if has {
                r.spawn(caps("vorhanden", &f.bold, 14.0, MONEY));
            } else {
                r.spawn(text(&if price == 0 { "frei".to_string() } else { format!("${price}") }, &f.digits, 20.0, if can { MONEY } else { DANGER }));
            }
        });
    });
}

/// An item's picture: a gun's own model in the finish this player's comes in (rendered), else its icon; and whether
/// it is a rendered one.
fn picture(server: &AssetServer, previews: &mut Previews, images: &mut Assets<Image>, me: &Player, b: Buy, size: Size) -> (Handle<Image>, bool) {
    match b {
        Buy::Weapon(w) if w.def().is_gun() => (previews.get(images, w, me.issue(w).finish, size), true),
        _ => (server.load(icon(b)), false),
    }
}

fn tile_colors(hover: bool, can: bool) -> (Color, Color) {
    match (hover && can, can) {
        (true, _) => (Color::srgba(1.0, 0.69, 0.0, 0.16), AMBER),
        (false, true) => (GLASS_HI, LINE),
        (false, false) => (Color::srgba(0.05, 0.055, 0.065, 0.6), Color::srgba(1.0, 1.0, 1.0, 0.04)),
    }
}

/// Lights up the tile under the cursor, in place.
fn highlight(ui: Res<Ui>, state: Res<BuyState>, mut rows: Query<(&Clickable, &Affordable, &mut BackgroundColor, &mut BorderColor)>) {
    if !ui.buy {
        return;
    }
    for (c, a, mut bg, mut border) in &mut rows {
        if matches!(c.0, Item::Category(_)) {
            continue;
        }
        let (b, e) = tile_colors(state.hover == Some(c.0), a.0);
        bg.set_if_neq(BackgroundColor(b));
        border.set_if_neq(BorderColor::all(e));
    }
}

/// Details of what the cursor is over, rebuilt only when the hover changes.
#[allow(clippy::too_many_arguments)]
fn details(
    mut commands: Commands,
    ui: Res<Ui>,
    fonts: Res<Fonts>,
    server: Res<AssetServer>,
    session: Option<Res<Session>>,
    mut state: ResMut<BuyState>,
    details: Single<(Entity, Option<&Children>), With<Details>>,
    (mut previews, mut images): (ResMut<Previews>, ResMut<Assets<Image>>),
) {
    if !ui.buy || state.details_for == Some(state.hover) {
        return;
    }
    let Some(s) = session else { return };
    state.details_for = Some(state.hover);
    let (d, dc) = details.into_inner();
    if let Some(ch) = dc {
        for c in ch.iter() {
            commands.entity(c).despawn();
        }
    }
    let f = &*fonts;
    commands.entity(d).with_children(|c| match state.hover {
        Some(Item::Buy(b)) => {
            c.spawn(caps(class_name(b), &f.bold, 15.0, AMBER));
            c.spawn((text(&label(b), &f.bold, 46.0, Color::WHITE), LetterSpacing::Px(1.0)));
            c.spawn((
                Node { width: Val::Percent(100.0), height: Val::Px(130.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(Val::Px(10.0)), margin: UiRect::vertical(Val::Px(4.0)), ..default() },
                BackgroundGradient::from(RadialGradient::new(UiPosition::CENTER, RadialGradientShape::Ellipse(Val::Percent(50.0), Val::Percent(50.0)), vec![ColorStop::percent(Color::srgba(1.0, 0.69, 0.0, 0.12), 0.0), ColorStop::percent(Color::srgba(1.0, 0.69, 0.0, 0.0), 100.0)])),
            ))
            .with_children(|m| {
                let (pic, rendered) = picture(&server, &mut previews, &mut images, s.me(), b, Size::Large);
                m.spawn((Node { height: Val::Px(126.0), aspect_ratio: Some(Size::ASPECT), ..default() }, ImageNode::new(pic).with_color(if rendered { Color::WHITE } else { Color::srgb(0.86, 0.93, 0.86) })));
            });
            match b {
                Buy::Weapon(w) if w.def().is_gun() => {
                    let def = w.def();
                    let rpm = 60.0 / def.cycle;
                    c.spawn(Node { column_gap: Val::Px(8.0), flex_wrap: FlexWrap::Wrap, row_gap: Val::Px(6.0), ..default() }).with_children(|t| {
                        tag(t, f, &format!("${}", def.price), MONEY);
                        tag(t, f, &format!("{} / {}", def.clip, def.reserve), SOFT);
                        tag(t, f, if def.auto { "Automatik" } else { "Einzelfeuer" }, SOFT);
                        if def.scoped() {
                            tag(t, f, "Zielfernrohr", SOFT);
                        }
                    });
                    c.spawn(Node { height: Val::Px(4.0), ..default() });
                    bar(c, f, "Schaden", def.damage * def.pellets.max(1) as f32 / 120.0);
                    bar(c, f, "Feuerrate", rpm / 1000.0);
                    bar(c, f, "Panzerdurchschlag", (def.armor_ratio - 0.8) / 1.2);
                    bar(c, f, "Reichweite", (def.range_mod - 0.6) / 0.4);
                    bar(c, f, "Beweglichkeit", (def.speed / zugriff_sim::units::METRES_PER_UNIT as f32 - 200.0) / 60.0);
                    c.spawn(Node { height: Val::Px(4.0), ..default() });
                    c.spawn(text(&format!("{:.0} Schuss/min  ·  Abschussprämie ${}", rpm, def.kill_reward), &f.label, 20.0, DIM));
                }
                _ => {
                    let what = match b {
                        Buy::Vest => "Hält Treffer in Brust und Bauch ab.",
                        Buy::VestHelmet => "Weste und Helm: schützt auch den Kopf vor schwachen Waffen.",
                        Buy::Defuser => "Entschärft die Sprengladung in 5 statt 10 Sekunden.",
                        Buy::Weapon(WeaponId::He) => "Splittergranate: Schaden im Umkreis, hinter Deckung nicht.",
                        Buy::Weapon(WeaponId::Flash) => "Blendet, wer hinsieht. Höchstens zwei.",
                        Buy::Weapon(WeaponId::Smoke) => "Rauchwand für 18 Sekunden – nimmt Sicht und Schusslinie.",
                        _ => "",
                    };
                    tag(c, f, &format!("${}", price_for(s.me(), b)), MONEY);
                    c.spawn(text(what, &f.text, 20.0, SOFT));
                }
            }
        }
        _ => {
            // Over nothing: what is carried now.
            let me = s.me();
            section(c, f, "Mitgeführt");
            let line = |c: &mut ChildSpawnerCommands, k: &str, v: String, on: bool| {
                c.spawn(Node { justify_content: JustifyContent::SpaceBetween, padding: UiRect::vertical(Val::Px(7.0)), border: UiRect::bottom(Val::Px(1.0)), ..default() }).insert(BorderColor::all(LINE)).with_children(|r| {
                    r.spawn(caps(k, &f.bold, 15.0, DIM));
                    r.spawn(text(&v, &f.label, 22.0, if on { Color::WHITE } else { DIM }));
                });
            };
            line(c, "Hauptwaffe", me.inv.primary.map_or("—".into(), |g| g.id.name().to_string()), me.inv.primary.is_some());
            line(c, "Pistole", me.inv.secondary.map_or("—".into(), |g| g.id.name().to_string()), me.inv.secondary.is_some());
            let armor = match (me.armor > 0, me.helmet) {
                (true, true) => format!("Weste {} + Helm", me.armor),
                (true, false) => format!("Weste {}", me.armor),
                _ => "—".into(),
            };
            line(c, "Schutz", armor, me.armor > 0);
            let nades = [(me.inv.he, "Splitter"), (me.inv.flash, "Blend"), (me.inv.smoke, "Rauch")].iter().filter(|(n, _)| *n > 0).map(|(n, w)| format!("{n}× {w}")).collect::<Vec<_>>().join(", ");
            line(c, "Granaten", if nades.is_empty() { "—".into() } else { nades }, true);
            if me.team == Team::Ksk {
                line(c, "Entschärferset", if me.defuser { "ja".into() } else { "nein".into() }, me.defuser);
            }
            c.spawn(Node { flex_grow: 1.0, ..default() });
            c.spawn(text("Über ein Feld fahren zeigt die Werte.", &f.text, 18.0, DIM));
        }
    });
}

fn inv_hash(p: &Player) -> u64 {
    let mut h = p.armor as u64 * 7 + p.helmet as u64 * 3 + p.defuser as u64 * 5 + p.inv.he as u64 * 11 + p.inv.flash as u64 * 13 + p.inv.smoke as u64 * 17;
    h += p.inv.primary.map_or(0, |g| g.id as u64 * 19) + p.inv.secondary.map_or(0, |g| g.id as u64 * 23);
    h * 1_000_000
}

fn fmt(t: f32) -> String {
    let t = t.max(0.0).ceil() as i32;
    format!("{}:{:02}", t / 60, t % 60)
}

fn bar(c: &mut ChildSpawnerCommands, fonts: &Fonts, label: &str, k: f32) {
    c.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(5.0), ..default() }).with_children(|col| {
        col.spawn(caps(label, &fonts.bold, 14.0, DIM));
        col.spawn((Node { width: Val::Percent(100.0), height: Val::Px(6.0), border_radius: BorderRadius::all(Val::Px(3.0)), overflow: Overflow::clip(), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)))).with_children(|b| {
            b.spawn((Node { width: Val::Percent(k.clamp(0.04, 1.0) * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundGradient::from(LinearGradient::to_right(vec![ColorStop::percent(crate::ui_kit::AMBER_DEEP, 0.0), ColorStop::percent(AMBER_HI, 100.0)]))));
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn interact(
    mut ui: ResMut<Ui>,
    mut state: ResMut<BuyState>,
    mut held: ResMut<Held>,
    session: Option<Res<Session>>,
    window: Single<&Window, With<PrimaryWindow>>,
    scale: Res<UiScale>,
    clickables: Query<(&Clickable, &ComputedNode, &UiGlobalTransform)>,
    mut cues: MessageWriter<crate::audio::Cue>,
) {
    let Some(s) = session.filter(|_| ui.buy) else { return };
    let team = s.me().team;
    let k = scale.0 * window.scale_factor();
    let cursor = ui.cursor * k;
    // Tiles spawned this frame are not laid out yet (zero size): keep the hover as it was until they are.
    let laid_out = clickables.iter().any(|(_, n, _)| n.size != Vec2::ZERO);
    let over = clickables.iter().find(|(_, n, tf)| {
        let c = tf.translation;
        let half = n.size * 0.5;
        n.size != Vec2::ZERO && (cursor.x - c.x).abs() <= half.x && (cursor.y - c.y).abs() <= half.y
    });
    let hover = if laid_out { over.map(|(c, ..)| c.0) } else { state.hover };
    if hover != state.hover {
        if matches!(hover, Some(Item::Buy(_))) {
            cues.write(crate::audio::Cue::UiHover);
        }
        state.hover = hover;
    }
    let mut chosen = if ui.clicked { hover } else { None };
    // Number keys: the first picks a column, the second buys from it.
    if let Some(d) = ui.digit {
        let i = d as usize - 1;
        chosen = match state.category {
            None if i < CATEGORIES.len() => Some(Item::Category(i)),
            Some(cat) => items(cat, team).get(i).map(|b| Item::Buy(*b)),
            _ => None,
        };
    }
    match chosen {
        Some(Item::Category(i)) => {
            state.category = if state.category == Some(i) { None } else { Some(i) };
            cues.write(crate::audio::Cue::UiClick);
        }
        Some(Item::Buy(b)) => {
            let me = s.me();
            let free = s.game.phase == Phase::Warmup || s.game.cfg.mode == zugriff_sim::game::Mode::Deathmatch;
            if (free || me.money >= price_for(me, b)) && !owned(me, b) {
                held.buy.push(b);
                cues.write(crate::audio::Cue::UiClick);
                // Close after a weapon (like the classic menu); stay open for equipment.
                if matches!(b, Buy::Weapon(w) if w.def().class != Class::Grenade) {
                    ui.buy = false;
                }
            }
            state.category = None;
        }
        None => {}
    }
}

fn cursor(ui: Res<Ui>, mut q: Single<(&mut Node, &mut Visibility), With<Cursor>>) {
    let (n, vis) = &mut *q;
    vis.set_if_neq(if ui.buy { Visibility::Inherited } else { Visibility::Hidden });
    let (left, top) = (Val::Px(ui.cursor.x - 8.0), Val::Px(ui.cursor.y - 8.0));
    if n.left != left || n.top != top {
        n.left = left;
        n.top = top;
    }
}
