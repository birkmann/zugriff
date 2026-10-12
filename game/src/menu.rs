//! The menus: the lobby over a slow flight across a map (maps on the left, the chosen side's operator in the middle,
//! the match settings and the start on the right), options (mouse, view, sound, crosshair with a live preview), the
//! controls as key caps, credits, the pause menu (Esc in a match: the match waits), the loading screen and the end of
//! a match. The look comes from `ui_kit`.

use crate::hud::{text, Fonts, AMBER, KSK, OPFOR};
use crate::input::Ui;
use crate::level::{Current, LoadMap, MapAsset};
use crate::options::{Options, Resolutions, CROSSHAIR_COLORS, DISPLAY_NAMES};
use crate::session::{self, MatchSettings, Pending, Session};
use crate::ui_kit::{self, caps, ghost, glass, keycap, primary, rule, section, segmented, tag, Hover, AMBER_HI, DIM, GLASS_HI, INK, LINE, LINE_HI, SOFT};
use bevy::prelude::*;
use bevy::text::LetterSpacing;
use zugriff_sim::bots::Difficulty;
use zugriff_sim::game::{Mode, Phase, Team};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuState>()
            .add_plugins(ui_kit::UiKitPlugin)
            .add_systems(Startup, (spawn, first_map))
            .add_systems(Update, (fallback_map, open_on_escape, build, buttons, loading, loading_sweep, match_end).chain().after(crate::input::InputSet));
    }
}

pub struct MapInfo {
    pub name: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
    /// What the rounds are about (Standard mode).
    pub objective: &'static str,
}

pub const MAPS: [MapInfo; 4] = [
    MapInfo { name: "zg_hafen", title: "Hafen", blurb: "Marinehafen an der Kieler Förde: Containerlager, Lagerhalle, Kran.", objective: "Sprengladung" },
    MapInfo { name: "zg_altstadt", title: "Altstadt", blurb: "Süddeutsche Altstadt: Kirchplatz, Fachwerkgassen, Brauereihof.", objective: "Sprengladung" },
    MapInfo { name: "zg_fliegerhorst", title: "Fliegerhorst", blurb: "Luftwaffenstützpunkt: Flugzeugschutzbau, Stabsgebäude, Tanklager.", objective: "Geiselrettung" },
    MapInfo { name: "dev_arena", title: "Übungsgelände", blurb: "Ein Hof zum Üben von Bewegung und Schießen.", objective: "Sprengladung" },
];

/// Tips for the loading screen.
const TIPS: [&str; 8] = [
    "Gehen (Umschalt) ist leise – der Gegner hört Schritte.",
    "Ein Entschärferset halbiert die Zeit an der Sprengladung.",
    "Im Stand und geduckt treffen die ersten Schüsse am genauesten.",
    "Z und X geben den Bots Befehle und melden Lagen.",
    "Weste und Helm lohnen sich fast immer vor einem Gewehr ohne Schutz.",
    "Blendgranaten wirken nur auf den, der hinsieht – dreh dich weg.",
    "Rauch nimmt Sicht und Schusslinie: gut zum Legen oder Entschärfen.",
    "Die Kaufzeit läuft ab – kaufen, bevor die Runde richtig beginnt.",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Screen {
    /// The lobby: map, settings, start.
    #[default]
    Title,
    Options,
    /// The graphics settings and the quick performance test (`graphics.rs`, `bench.rs`).
    Graphics,
    Controls,
    Credits,
    Pause,
    /// Online: the server list and creating rooms (`netmenu.rs`).
    Online,
    /// The guns' finishes (`arsenal.rs`).
    Arsenal,
    None,
}

#[derive(Resource)]
pub struct MenuState {
    pub screen: Screen,
    built: Option<(Screen, u64)>,
    pub setup: MatchSettings,
    /// Where Options/Controls go back to.
    back: Screen,
    tried_fallback: bool,
    /// A match asked for by the address, not yet started.
    autostart: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        MenuState { screen: Screen::Title, built: None, setup: MatchSettings::default(), back: Screen::Title, tried_fallback: false, autostart: false }
    }
}

#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct LoadingBox;
/// The loading screen's moving bar.
#[derive(Component)]
struct Sweep;
#[derive(Component)]
struct EndBox;

#[derive(Component, Clone, Copy, PartialEq, Debug)]
enum Act {
    Tab(Screen),
    Quit,
    Back,
    Map(usize),
    Mode(bool),
    Difficulty(u8),
    Team(u8),
    Rounds(u32),
    Start,
    Resume,
    Leave,
    SwitchSide,
    Again,
    MainMenu,
    Sens(f32),
    Fov(f32),
    Volume(f32),
    CrossSize(f32),
    CrossGap(f32),
    CrossColor,
    CrossDynamic,
    Perf,
    Invert,
    Render(i8),
    Display,
    Resolution(i8),
    Vsync,
    Preset,
    Shadows,
    Aa,
    Sharpen(f32),
    Bloom,
    Ground,
    Reflections,
    Grass(f32),
    Litter(f32),
    Fauna,
    ViewDistance(f32),
    FpsLimit,
    Bench,
}

fn first_map(mut loads: MessageWriter<LoadMap>, mut state: ResMut<MenuState>, options: Res<Options>) {
    // A match straight from the address (?play=<map>), for testing; else the title screen over the last map.
    if let Some(map) = crate::args::get("play") {
        state.setup.map = map;
        state.setup.warmup = crate::args::get("warmup").and_then(|w| w.parse().ok()).unwrap_or(0.0);
        if let Some(t) = crate::args::get("team") {
            state.setup.team = match t.as_str() {
                "opfor" => Some(Team::Opfor),
                "ksk" => Some(Team::Ksk),
                _ => None,
            };
        }
        if crate::args::get("mode").as_deref() == Some("dm") {
            state.setup.mode = Mode::Deathmatch;
        }
        if let Some(r) = crate::args::get("rounds").and_then(|r| r.parse().ok()) {
            state.setup.max_rounds = r;
        }
        state.screen = Screen::None;
        state.autostart = true;
        loads.write(LoadMap(state.setup.map.clone()));
        return;
    }
    let map = crate::args::get("map").unwrap_or_else(|| if MAPS.iter().any(|m| m.name == options.last_map) { options.last_map.clone() } else { "zg_hafen".into() });
    // A fixed view for screenshots: no menu over it.
    if crate::args::pose().is_some() {
        state.screen = Screen::None;
    }
    // ?menu=options|graphics|controls|credits opens that page (for screenshots).
    state.screen = match crate::args::get("menu").as_deref() {
        Some("options") => Screen::Options,
        Some("graphics") => Screen::Graphics,
        Some("controls") => Screen::Controls,
        Some("credits") => Screen::Credits,
        Some("online") => Screen::Online,
        Some("arsenal") => Screen::Arsenal,
        _ => state.screen,
    };
    state.setup.map = map.clone();
    state.setup.difficulty = match options.difficulty {
        0 => Difficulty::Easy,
        2 => Difficulty::Hard,
        3 => Difficulty::Expert,
        _ => Difficulty::Normal,
    };
    loads.write(LoadMap(map));
}

/// Starts the match asked for by the address once its map is in.
fn fallback_map(mut commands: Commands, mut state: ResMut<MenuState>, current: Res<Current>, assets: Res<AssetServer>, mut loads: MessageWriter<LoadMap>, session: Option<Res<Session>>, pending: Res<Pending>, maps: Res<Assets<MapAsset>>) {
    if matches!(assets.get_load_state(&current.handle), Some(bevy::asset::LoadState::Failed(_))) && !state.tried_fallback {
        state.tried_fallback = true;
        warn!("map {} failed to load; falling back to dev_arena", current.name);
        loads.write(LoadMap("dev_arena".into()));
        if state.setup.map == current.name {
            state.setup.map = "dev_arena".into();
        }
        return;
    }
    // ?play=: start once loaded.
    if state.autostart && session.is_none() && pending.0.is_none() && current.name == state.setup.map && maps.get(&current.handle).is_some() {
        state.autostart = false;
        commands.insert_resource(Pending(Some(state.setup.clone())));
    }
}

fn spawn(mut commands: Commands) {
    commands.spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, MenuRoot, GlobalZIndex(10)));
    commands.spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(INK), Visibility::Hidden, LoadingBox, GlobalZIndex(30)));
    commands.spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, Visibility::Hidden, EndBox, GlobalZIndex(12)));
}

/// Escape (the pointer lock ending, on the web) in a match opens the pause menu.
fn open_on_escape(mut state: ResMut<MenuState>, mut ui: ResMut<Ui>, look: Res<crate::view::Look>, keys: Res<ButtonInput<KeyCode>>, session: Option<ResMut<Session>>, pending: Res<Pending>, mut was_locked: Local<bool>) {
    let Some(mut s) = session else {
        let waiting = pending.0.is_some() || state.autostart || crate::args::pose().is_some();
        if (state.screen == Screen::None && !waiting) || state.screen == Screen::Pause {
            state.screen = Screen::Title;
        }
        ui.menu = state.screen != Screen::None;
        return;
    };
    state.autostart = false;
    // `--menu pause` (`?menu=pause`) pauses once the match is live, for screenshots.
    if crate::args::get("menu").as_deref() == Some("pause") && s.game.phase == Phase::Live && state.screen == Screen::None && !*was_locked {
        *was_locked = true;
        state.screen = Screen::Pause;
    }
    let lost_lock = *was_locked && !look.locked && !ui.menu;
    *was_locked = look.locked;
    let esc = keys.just_pressed(KeyCode::Escape);
    if (lost_lock || (esc && !cfg!(target_arch = "wasm32"))) && state.screen == Screen::None && ui.radio.is_none() && !ui.typing {
        if ui.buy {
            ui.buy = false;
        } else {
            state.screen = Screen::Pause;
        }
    } else if esc && state.screen == Screen::Pause && !cfg!(target_arch = "wasm32") {
        state.screen = Screen::None;
    }
    ui.menu = state.screen != Screen::None;
    // The match waits while its menu is open (not at its end).
    s.paused = ui.menu && s.game.phase != Phase::MatchOver;
}

fn hash_setup(m: &MatchSettings, o: &Options) -> u64 {
    let mut h = m.map.len() as u64 * 131 + m.map.bytes().map(|b| b as u64).sum::<u64>();
    h = h * 31 + m.mode as u64;
    h = h * 31 + m.difficulty as u64;
    h = h * 31 + m.team.map_or(9, |t| t as u64);
    h = h * 31 + m.max_rounds as u64;
    h = h * 31 + (o.sensitivity * 100.0) as u64;
    h = h * 31 + o.fov as u64;
    h = h * 31 + (o.volume * 100.0) as u64 + (o.render_scale * 100.0) as u64 * 1000;
    h = h * 31 + (o.crosshair_size * 10.0) as u64 + (o.crosshair_gap * 10.0) as u64 * 7 + o.crosshair_color as u64 * 3 + o.crosshair_dynamic as u64 + o.perf as u64 * 11 + o.invert as u64 * 5;
    h = h * 31 + o.display as u64 * 3 + o.vsync as u64 + o.resolution.map_or(0, |(w, hh)| w as u64 * 7919 + hh as u64);
    h.wrapping_mul(31).wrapping_add(o.gfx.hash())
}

fn full() -> Node {
    Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }
}

fn map_info(name: &str) -> Option<&'static MapInfo> {
    MAPS.iter().find(|m| m.name == name)
}

fn objective_color(objective: &str) -> Color {
    if objective == "Geiselrettung" { KSK } else { AMBER }
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    mut state: ResMut<MenuState>,
    fonts: Res<Fonts>,
    options: Res<Options>,
    resolutions: Res<Resolutions>,
    server: Res<AssetServer>,
    session: Option<Res<Session>>,
    root: Single<(Entity, Option<&Children>), With<MenuRoot>>,
    online: (Res<crate::net::Net>, Res<crate::netmenu::Browser>, Res<crate::netmenu::OnlineUi>),
    bench: Res<crate::bench::Bench>,
) {
    let in_match = session.is_some();
    let mut key = (state.screen, (hash_setup(&state.setup, &options).wrapping_mul(64) + resolutions.0.len() as u64) * 2 + in_match as u64);
    if state.screen == Screen::Graphics {
        key.1 ^= (bench.version as u64).wrapping_mul(0x2545_f491_4f6c_dd1d);
    }
    let view = crate::netmenu::View { net: &online.0, browser: &online.1, ui: &online.2, options: &options };
    if state.screen == Screen::Online || state.screen == Screen::Pause {
        key.1 ^= crate::netmenu::version(view.net, view.browser, view.ui, view.options).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    if state.built == Some(key) {
        return;
    }
    state.built = Some(key);
    let (e, children) = root.into_inner();
    if let Some(ch) = children {
        for c in ch.iter() {
            commands.entity(c).despawn();
        }
    }
    let f = &*fonts;
    let setup = state.setup.clone();
    let screen = state.screen;
    if screen == Screen::None {
        return;
    }
    commands.entity(e).with_children(|root| {
        // Shade over the world behind: the lobby keeps its middle lighter for the flight and the operator; the edges
        // go darker where the panels are.
        let shade = Color::srgba(0.012, 0.018, 0.026, if screen == Screen::Title { 0.58 } else { 0.78 });
        root.spawn((full(), BackgroundColor(shade)));
        root.spawn((full(), BackgroundGradient::from(RadialGradient::new(UiPosition::CENTER, RadialGradientShape::FarthestCorner, vec![ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, 0.0), 0.0), ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, 0.75), 100.0)]))));
        root.spawn(Node { width: Val::Percent(100.0), height: Val::Percent(100.0), flex_direction: FlexDirection::Column, ..default() }).with_children(|page| {
            match screen {
                Screen::Pause => pause(page, f, session.as_deref(), view.net),
                _ => {
                    if in_match {
                        sub_header(page, f, match screen {
                            Screen::Options => "Optionen",
                            Screen::Graphics => "Grafik",
                            Screen::Controls => "Steuerung",
                            _ => "Mitwirkende",
                        });
                    } else {
                        top_bar(page, f, screen);
                    }
                    page.spawn(Node { flex_grow: 1.0, width: Val::Percent(100.0), padding: UiRect::new(Val::Px(56.0), Val::Px(56.0), Val::Px(34.0), Val::Px(40.0)), min_height: Val::Px(0.0), ..default() }).with_children(|body| match screen {
                        Screen::Title => lobby(body, f, &server, &setup),
                        Screen::Online => crate::netmenu::page(body, f, &server, &view),
                        Screen::Arsenal => {
                            body.spawn((Node { flex_grow: 1.0, width: Val::Percent(100.0), column_gap: Val::Px(24.0), min_height: Val::Px(0.0), ..default() }, crate::arsenal::ArsenalRoot));
                        }
                        Screen::Options => options_page(body, f, &options, &resolutions),
                        Screen::Graphics => graphics_page(body, f, &options, &bench, in_match),
                        Screen::Controls => controls_page(body, f),
                        _ => credits_page(body, f),
                    });
                }
            }
        });
        root.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(56.0), bottom: Val::Px(14.0), ..default() }, children![caps("ZUGRIFF · Bevy/WebGPU", &f.label, 14.0, Color::srgba(1.0, 1.0, 1.0, 0.32))]));
    });
}

/// The brand, the tabs, and quit.
fn top_bar(page: &mut ChildSpawnerCommands, f: &Fonts, screen: Screen) {
    page.spawn((
        Node { width: Val::Percent(100.0), height: Val::Px(88.0), padding: UiRect::horizontal(Val::Px(56.0)), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, border: UiRect::bottom(Val::Px(1.0)), flex_shrink: 0.0, ..default() },
        BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, 0.75), 0.0), ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, 0.35), 100.0)])),
        BorderColor::all(LINE),
    ))
    .with_children(|bar| {
        bar.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(16.0), width: Val::Px(360.0), ..default() }).with_children(|brand| {
            emblem(brand, 34.0);
            brand.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|t| {
                t.spawn((text("ZUGRIFF", &f.bold, 40.0, Color::WHITE), LetterSpacing::Px(4.0)));
                t.spawn(caps("KSK gegen OPFOR", &f.label, 13.0, AMBER));
            });
        });
        bar.spawn(Node { column_gap: Val::Px(6.0), height: Val::Percent(100.0), ..default() }).with_children(|tabs| {
            for (label, target) in [("Spielen", Screen::Title), ("Mehrspieler", Screen::Online), ("Arsenal", Screen::Arsenal), ("Optionen", Screen::Options), ("Grafik", Screen::Graphics), ("Steuerung", Screen::Controls), ("Mitwirkende", Screen::Credits)] {
                let on = screen == target;
                tabs.spawn((
                    Button,
                    Node { height: Val::Percent(100.0), padding: UiRect::horizontal(Val::Px(26.0)), align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() },
                    BackgroundColor(Color::NONE),
                    Hover::new(Color::NONE, Color::srgba(1.0, 1.0, 1.0, 0.06), Color::NONE, Color::NONE),
                    Act::Tab(target),
                ))
                .with_children(|t| {
                    t.spawn((caps(label, &f.bold, 24.0, if on { Color::WHITE } else { DIM }), Pickable::IGNORE));
                    if on {
                        t.spawn((Node { position_type: PositionType::Absolute, bottom: Val::Px(0.0), left: Val::Px(18.0), right: Val::Px(18.0), height: Val::Px(3.0), border_radius: BorderRadius::all(Val::Px(2.0)), ..default() }, BackgroundColor(AMBER), BoxShadow::new(Color::srgba(1.0, 0.6, 0.0, 0.6), Val::Px(0.0), Val::Px(0.0), Val::Px(0.0), Val::Px(10.0))));
                    }
                });
            }
        });
        bar.spawn(Node { width: Val::Px(360.0), justify_content: JustifyContent::End, ..default() }).with_children(|r| {
            if !cfg!(target_arch = "wasm32") {
                ghost(r, f, "Beenden", Act::Quit, Val::Auto, false);
            }
        });
    });
}

/// In a match: the page's title and the way back to the pause menu.
fn sub_header(page: &mut ChildSpawnerCommands, f: &Fonts, title: &str) {
    page.spawn((
        Node { width: Val::Percent(100.0), height: Val::Px(88.0), padding: UiRect::horizontal(Val::Px(56.0)), align_items: AlignItems::Center, column_gap: Val::Px(24.0), border: UiRect::bottom(Val::Px(1.0)), flex_shrink: 0.0, ..default() },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        BorderColor::all(LINE),
    ))
    .with_children(|bar| {
        ghost(bar, f, "‹ Zurück", Act::Back, Val::Auto, false);
        bar.spawn(caps(title, &f.bold, 30.0, Color::WHITE));
    });
}

/// The mark: an amber diamond with a dark core.
fn emblem(c: &mut ChildSpawnerCommands, size: f32) {
    c.spawn(Node { width: Val::Px(size * 1.3), height: Val::Px(size * 1.3), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|m| {
        m.spawn((
            Node { width: Val::Px(size), height: Val::Px(size), border: UiRect::all(Val::Px(3.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(Val::Px(3.0)), ..default() },
            BorderColor::all(AMBER),
            UiTransform::from_rotation(Rot2::degrees(45.0)),
            BoxShadow::new(Color::srgba(1.0, 0.6, 0.0, 0.35), Val::Px(0.0), Val::Px(0.0), Val::Px(0.0), Val::Px(12.0)),
            children![(Node { width: Val::Px(size * 0.34), height: Val::Px(size * 0.34), border_radius: BorderRadius::all(Val::Px(1.0)), ..default() }, BackgroundColor(AMBER))],
        ));
    });
}

/// Maps on the left, the operator in the middle, the settings and the start on the right.
fn lobby(body: &mut ChildSpawnerCommands, f: &Fonts, server: &AssetServer, setup: &MatchSettings) {
    body.spawn(Node { width: Val::Percent(100.0), height: Val::Percent(100.0), column_gap: Val::Px(32.0), ..default() }).with_children(|row| {
        // Maps.
        row.spawn(Node { width: Val::Px(430.0), flex_shrink: 0.0, flex_direction: FlexDirection::Column, row_gap: Val::Px(12.0), ..default() }).with_children(|col| {
            section(col, f, "Einsatzgebiet");
            col.spawn(Node { height: Val::Px(2.0), ..default() });
            for (i, m) in MAPS.iter().enumerate() {
                map_card(col, f, server, i, m, setup.map == m.name);
            }
        });
        // The operator.
        row.spawn(Node { flex_grow: 1.0, height: Val::Percent(100.0), flex_direction: FlexDirection::Column, justify_content: JustifyContent::End, align_items: AlignItems::Center, min_width: Val::Px(0.0), ..default() }).with_children(|mid| {
            let sides: Vec<Team> = setup.team.map_or(vec![Team::Ksk, Team::Opfor], |t| vec![t]);
            mid.spawn(Node { height: Val::Percent(92.0), align_items: AlignItems::End, justify_content: JustifyContent::Center, column_gap: Val::Px(0.0), ..default() }).with_children(|figs| {
                for &t in &sides {
                    let file = if t == Team::Ksk { "ui/operator-ksk.png" } else { "ui/operator-opfor.png" };
                    figs.spawn(Node { height: Val::Percent(if sides.len() == 1 { 100.0 } else { 92.0 }), aspect_ratio: Some(900.0 / 1400.0), justify_content: JustifyContent::Center, margin: UiRect::horizontal(Val::Px(if sides.len() == 1 { 0.0 } else { -60.0 })), ..default() }).with_children(|fig| {
                        // A pool of light on the floor under the feet.
                        fig.spawn((
                            Node { position_type: PositionType::Absolute, bottom: Val::Percent(-1.5), width: Val::Percent(90.0), height: Val::Percent(9.0), ..default() },
                            BackgroundGradient::from(RadialGradient::new(UiPosition::CENTER, RadialGradientShape::Ellipse(Val::Percent(50.0), Val::Percent(50.0)), vec![ColorStop::percent(team_tint(t, 0.45), 0.0), ColorStop::percent(team_tint(t, 0.0), 100.0)])),
                        ));
                        fig.spawn((Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, ImageNode::new(server.load(file))));
                    });
                }
            });
            mid.spawn(Node { column_gap: Val::Px(14.0), margin: UiRect::top(Val::Px(10.0)), ..default() }).with_children(|plates| {
                for &t in &sides {
                    nameplate(plates, f, t);
                }
            });
        });
        // The mission.
        row.spawn(glass(Node { width: Val::Px(500.0), flex_shrink: 0.0, flex_direction: FlexDirection::Column, row_gap: Val::Px(16.0), padding: UiRect::all(Val::Px(30.0)), ..default() })).with_children(|p| {
            let info = map_info(&setup.map);
            section(p, f, "Einsatz");
            p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }).with_children(|t| {
                t.spawn((text(info.map_or(setup.map.as_str(), |m| m.title), &f.bold, 64.0, Color::WHITE), LetterSpacing::Px(1.0)));
                t.spawn(Node { column_gap: Val::Px(8.0), ..default() }).with_children(|tags| {
                    if setup.mode == Mode::Deathmatch {
                        tag(tags, f, "Deathmatch", OPFOR);
                    } else if let Some(m) = info {
                        tag(tags, f, m.objective, objective_color(m.objective));
                    }
                    tag(tags, f, "5 gegen 5", SOFT);
                });
                t.spawn(text(info.map_or("", |m| m.blurb), &f.text, 18.0, SOFT));
            });
            rule(p);
            let field = |p: &mut ChildSpawnerCommands, label: &str, items: Vec<(&str, Act, bool)>| {
                p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }).with_children(|g| {
                    g.spawn(caps(label, &f.bold, 15.0, DIM));
                    segmented(g, f, items);
                });
            };
            field(p, "Modus", vec![("Standard", Act::Mode(false), setup.mode != Mode::Deathmatch), ("Deathmatch", Act::Mode(true), setup.mode == Mode::Deathmatch)]);
            field(p, "Seite", vec![("KSK", Act::Team(0), setup.team == Some(Team::Ksk)), ("OPFOR", Act::Team(1), setup.team == Some(Team::Opfor)), ("Zufall", Act::Team(2), setup.team.is_none())]);
            field(
                p,
                "Bots",
                vec![
                    ("Leicht", Act::Difficulty(0), setup.difficulty == Difficulty::Easy),
                    ("Normal", Act::Difficulty(1), setup.difficulty == Difficulty::Normal),
                    ("Schwer", Act::Difficulty(2), setup.difficulty == Difficulty::Hard),
                    ("Experte", Act::Difficulty(3), setup.difficulty == Difficulty::Expert),
                ],
            );
            if setup.mode != Mode::Deathmatch {
                field(p, "Länge", vec![("Kurz · MR8", Act::Rounds(16), setup.max_rounds == 16), ("Lang · MR15", Act::Rounds(30), setup.max_rounds == 30)]);
            }
            p.spawn(Node { flex_grow: 1.0, ..default() });
            let summary = if setup.mode == Mode::Deathmatch { "Jeder gegen jeden · 10 Minuten".to_string() } else { format!("Bis zu {} Runden · Kaufzeit und Wirtschaft", setup.max_rounds) };
            p.spawn(text(&summary, &f.label, 19.0, DIM));
            primary(p, f, "Einsatz starten", Act::Start, Val::Percent(100.0), 86.0, 36.0);
        });
    });
}

fn team_tint(t: Team, a: f32) -> Color {
    let c = if t == Team::Ksk { KSK } else { OPFOR }.to_srgba();
    Color::srgba(c.red, c.green, c.blue, a)
}

fn nameplate(c: &mut ChildSpawnerCommands, f: &Fonts, t: Team) {
    let (name, sub) = if t == Team::Ksk { ("KSK", "Kommando Spezialkräfte") } else { ("OPFOR", "Gegnerkräfte") };
    c.spawn(glass(Node { padding: UiRect::new(Val::Px(0.0), Val::Px(20.0), Val::Px(8.0), Val::Px(8.0)), column_gap: Val::Px(14.0), align_items: AlignItems::Center, overflow: Overflow::clip(), ..default() })).with_children(|p| {
        p.spawn((Node { width: Val::Px(4.0), height: Val::Px(40.0), ..default() }, BackgroundColor(if t == Team::Ksk { KSK } else { OPFOR })));
        p.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|t2| {
            t2.spawn((text(name, &f.bold, 28.0, Color::WHITE), LetterSpacing::Px(2.0)));
            t2.spawn(caps(sub, &f.label, 13.0, DIM));
        });
    });
}

fn map_card(c: &mut ChildSpawnerCommands, f: &Fonts, server: &AssetServer, i: usize, m: &MapInfo, on: bool) {
    let border = if on { AMBER } else { LINE };
    c.spawn((
        Button,
        Node { width: Val::Percent(100.0), height: Val::Px(128.0), border: UiRect::all(Val::Px(if on { 2.0 } else { 1.0 })), border_radius: BorderRadius::all(Val::Px(12.0)), overflow: Overflow::clip(), align_items: AlignItems::End, ..default() },
        BackgroundColor(GLASS_HI),
        BorderColor::all(border),
        Hover::new(GLASS_HI, GLASS_HI, border, if on { AMBER_HI } else { LINE_HI }),
        BoxShadow::new(if on { Color::srgba(1.0, 0.6, 0.0, 0.25) } else { Color::srgba(0.0, 0.0, 0.0, 0.4) }, Val::Px(0.0), Val::Px(6.0), Val::Px(0.0), Val::Px(20.0)),
        Act::Map(i),
    ))
    .with_children(|card| {
        // The picture (5:3), centred in the strip.
        card.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(-65.0), width: Val::Percent(100.0), aspect_ratio: Some(5.0 / 3.0), ..default() }, ImageNode::new(server.load(format!("thumbs/{}.jpg", m.name))).with_color(if on { Color::srgb(0.8, 0.8, 0.8) } else { Color::srgb(0.42, 0.44, 0.46) }), Pickable::IGNORE));
        card.spawn((full(), BackgroundGradient::from(LinearGradient::to_right(vec![ColorStop::percent(Color::srgba(0.01, 0.015, 0.02, 0.92), 0.0), ColorStop::percent(Color::srgba(0.01, 0.015, 0.02, 0.0), 100.0)])), Pickable::IGNORE));
        if on {
            card.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), width: Val::Px(5.0), ..default() }, BackgroundColor(AMBER), Pickable::IGNORE));
        }
        card.spawn((Node { flex_direction: FlexDirection::Column, padding: UiRect::new(Val::Px(22.0), Val::Px(16.0), Val::Px(0.0), Val::Px(14.0)), row_gap: Val::Px(4.0), ..default() }, Pickable::IGNORE)).with_children(|t| {
            t.spawn((text(m.title, &f.bold, 36.0, Color::WHITE), TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.6) }, Pickable::IGNORE));
            t.spawn(Node { column_gap: Val::Px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                r.spawn((caps(m.objective, &f.bold, 14.0, objective_color(m.objective)), Pickable::IGNORE));
                r.spawn((caps(m.name, &f.label, 13.0, DIM), Pickable::IGNORE));
            });
        });
    });
}

/// Pause: where the match stands, and the ways on.
fn pause(page: &mut ChildSpawnerCommands, f: &Fonts, session: Option<&Session>, net: &crate::net::Net) {
    let online = session.is_some_and(|s| s.online);
    page.spawn(Node { width: Val::Percent(100.0), height: Val::Percent(100.0), padding: UiRect::left(Val::Px(120.0)), flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, row_gap: Val::Px(14.0), ..default() }).with_children(|col| {
        col.spawn(Node { column_gap: Val::Px(16.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
            emblem(r, 26.0);
            r.spawn(caps(if online { "Online – das Spiel läuft weiter" } else { "Spiel pausiert" }, &f.bold, 20.0, AMBER));
        });
        if let Some(s) = session {
            let g = &s.game;
            let title = map_info(&s.settings.map).map_or(s.settings.map.as_str(), |m| m.title);
            col.spawn((text(title, &f.bold, 110.0, Color::WHITE), LetterSpacing::Px(2.0)));
            if g.cfg.mode != Mode::Deathmatch {
                col.spawn(Node { column_gap: Val::Px(18.0), align_items: AlignItems::Center, margin: UiRect::bottom(Val::Px(18.0)), ..default() }).with_children(|r| {
                    r.spawn(caps("KSK", &f.bold, 24.0, KSK));
                    r.spawn(text(&format!("{}  :  {}", g.score[0], g.score[1]), &f.digits, 44.0, Color::WHITE));
                    r.spawn(caps("OPFOR", &f.bold, 24.0, OPFOR));
                    r.spawn(Node { width: Val::Px(1.0), height: Val::Px(28.0), margin: UiRect::horizontal(Val::Px(8.0)), ..default() }).insert(BackgroundColor(LINE_HI));
                    r.spawn(caps(&format!("Runde {}", g.round), &f.label, 20.0, DIM));
                });
            }
            if let Some(j) = net.joined.as_ref().filter(|_| online) {
                let code = if j.setup.private { format!("  ·  Code {}", j.code) } else { String::new() };
                col.spawn(text(&format!("{} · {}{code}", net.server, j.room.name), &f.label, 24.0, SOFT));
                col.spawn(text(&format!("Ping {:.0} ms", net.rtt), &f.label, 20.0, DIM));
            }
        }
        col.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), width: Val::Px(420.0), ..default() }).with_children(|b| {
            primary(b, f, "Weiterspielen", Act::Resume, Val::Percent(100.0), 70.0, 28.0);
            ghost(b, f, "Optionen", Act::Tab(Screen::Options), Val::Percent(100.0), false);
            ghost(b, f, "Grafik", Act::Tab(Screen::Graphics), Val::Percent(100.0), false);
            ghost(b, f, "Steuerung", Act::Tab(Screen::Controls), Val::Percent(100.0), false);
            if online {
                ghost(b, f, "Seite wechseln", Act::SwitchSide, Val::Percent(100.0), false);
            }
            ghost(b, f, if online { "Raum verlassen" } else { "Spiel verlassen" }, Act::Leave, Val::Percent(100.0), true);
        });
        col.spawn(text("Nach „Weiterspielen“ ins Bild klicken, um die Maus zu fangen.", &f.text, 17.0, DIM));
    });
}

fn card(c: &mut ChildSpawnerCommands, f: &Fonts, title: &str, width: f32, inner: impl FnOnce(&mut ChildSpawnerCommands)) {
    c.spawn(glass(Node { width: Val::Px(width), flex_direction: FlexDirection::Column, row_gap: Val::Px(18.0), padding: UiRect::all(Val::Px(28.0)), ..default() })).with_children(|p| {
        section(p, f, title);
        inner(p);
    });
}

/// A setting with a value, − and +, and how far along its range it is.
fn stepper(c: &mut ChildSpawnerCommands, f: &Fonts, label: &str, value: String, k: f32, minus: Act, plus: Act) {
    c.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }).with_children(|col| {
        col.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() }).with_children(|r| {
            r.spawn((Node { flex_grow: 1.0, ..default() }, children![text(label, &f.label, 23.0, Color::WHITE)]));
            square(r, f, "–", minus);
            r.spawn((Node { width: Val::Px(92.0), justify_content: JustifyContent::Center, ..default() }, children![text(&value, &f.digits, 26.0, AMBER)]));
            square(r, f, "+", plus);
        });
        col.spawn((Node { width: Val::Percent(100.0), height: Val::Px(4.0), border_radius: BorderRadius::all(Val::Px(2.0)), overflow: Overflow::clip(), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08))))
            .with_children(|t| {
                t.spawn((Node { width: Val::Percent(k.clamp(0.02, 1.0) * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundGradient::from(LinearGradient::to_right(vec![ColorStop::percent(ui_kit::AMBER_DEEP, 0.0), ColorStop::percent(AMBER_HI, 100.0)]))));
            });
    });
}

fn square(c: &mut ChildSpawnerCommands, f: &Fonts, label: &str, act: Act) {
    c.spawn((
        Button,
        Node { width: Val::Px(40.0), height: Val::Px(40.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(8.0)), ..default() },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.05)),
        BorderColor::all(LINE),
        Hover::new(Color::srgba(1.0, 1.0, 1.0, 0.05), Color::srgba(1.0, 0.69, 0.0, 0.2), LINE, AMBER),
        act,
        children![(text(label, &f.bold, 26.0, Color::WHITE), Pickable::IGNORE)],
    ));
}

/// An on/off switch.
fn switch(c: &mut ChildSpawnerCommands, f: &Fonts, label: &str, on: bool, act: Act) {
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

/// A setting that steps through named values.
fn cycle(c: &mut ChildSpawnerCommands, f: &Fonts, label: &str, value: &str, swatch: Option<Color>, act: Act) {
    c.spawn(Node { align_items: AlignItems::Center, ..default() }).with_children(|r| {
        r.spawn((Node { flex_grow: 1.0, ..default() }, children![text(label, &f.label, 23.0, Color::WHITE)]));
        r.spawn((
            Button,
            Node { min_width: Val::Px(170.0), padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)), column_gap: Val::Px(10.0), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(8.0)), ..default() },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.05)),
            BorderColor::all(LINE),
            Hover::new(Color::srgba(1.0, 1.0, 1.0, 0.05), Color::srgba(1.0, 1.0, 1.0, 0.12), LINE, LINE_HI),
            act,
        ))
        .with_children(|b| {
            if let Some(s) = swatch {
                b.spawn((Node { width: Val::Px(14.0), height: Val::Px(14.0), border_radius: BorderRadius::all(Val::Px(7.0)), ..default() }, BackgroundColor(s), Pickable::IGNORE));
            }
            b.spawn((text(value, &f.label, 22.0, Color::WHITE), Pickable::IGNORE));
            b.spawn((text("›", &f.bold, 24.0, AMBER), Pickable::IGNORE));
        });
    });
}

/// The resolutions to step through: the desktop's first, then what the monitor offers.
fn resolution_steps(res: &Resolutions) -> Vec<Option<(u32, u32)>> {
    std::iter::once(None).chain(res.0.iter().map(|s| Some((s.x, s.y)))).collect()
}

fn options_page(body: &mut ChildSpawnerCommands, f: &Fonts, o: &Options, res: &Resolutions) {
    body.spawn(Node { width: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Stretch, align_content: AlignContent::Start, column_gap: Val::Px(24.0), flex_wrap: FlexWrap::Wrap, row_gap: Val::Px(24.0), ..default() }).with_children(|row| {
        card(row, f, "Maus, Sicht und Klang", 480.0, |p| {
            stepper(p, f, "Mausempfindlichkeit", format!("{:.2}", o.sensitivity), (o.sensitivity - 0.1) / 4.9, Act::Sens(-0.1), Act::Sens(0.1));
            stepper(p, f, "Sichtfeld (4:3)", format!("{:.0}°", o.fov), (o.fov - 70.0) / 40.0, Act::Fov(-5.0), Act::Fov(5.0));
            switch(p, f, "Maus invertieren", o.invert, Act::Invert);
            stepper(p, f, "Lautstärke", format!("{:.0} %", o.volume * 100.0), o.volume, Act::Volume(-0.1), Act::Volume(0.1));
        });
        card(row, f, "Anzeige", 480.0, |p| {
            let web = cfg!(target_arch = "wasm32");
            // (On the web: the page, or the browser's fullscreen.)
            let mode = if web { ["Fenster", "Vollbild"][(o.display as usize).min(1)] } else { DISPLAY_NAMES[(o.display as usize).min(2)] };
            cycle(p, f, "Anzeigemodus", mode, None, Act::Display);
            if web {
                let res = if o.render_scale <= 0.0 { "Gerät".to_string() } else { format!("{:.0} %", o.render_scale * 100.0) };
                let k = if o.render_scale <= 0.0 { 1.0 } else { o.render_scale };
                stepper(p, f, "Renderauflösung", res, k, Act::Render(-1), Act::Render(1));
            } else {
                // Borderless always covers the desktop at its own resolution.
                let label = match (o.display, o.resolution) {
                    (1, _) | (_, None) => "Desktop".to_string(),
                    (_, Some((w, h))) => format!("{w} × {h}"),
                };
                let steps = resolution_steps(res);
                if o.display == 1 || steps.len() < 2 {
                    p.spawn(Node { align_items: AlignItems::Center, ..default() }).with_children(|r| {
                        r.spawn((Node { flex_grow: 1.0, ..default() }, children![text("Auflösung", &f.label, 23.0, Color::WHITE)]));
                        r.spawn(text(&label, &f.label, 22.0, DIM));
                    });
                } else {
                    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() }).with_children(|r| {
                        r.spawn((Node { flex_grow: 1.0, ..default() }, children![text("Auflösung", &f.label, 23.0, Color::WHITE)]));
                        square(r, f, "‹", Act::Resolution(-1));
                        r.spawn((Node { min_width: Val::Px(150.0), justify_content: JustifyContent::Center, ..default() }, children![text(&label, &f.digits, 24.0, AMBER)]));
                        square(r, f, "›", Act::Resolution(1));
                    });
                }
                switch(p, f, "V-Sync (kein Tearing, etwas mehr Verzögerung)", o.vsync, Act::Vsync);
            }
            cycle(p, f, "Leistungsanzeige (F3)", crate::perf::SIZE_NAMES[(o.perf as usize).min(3)], None, Act::Perf);
        });
        card(row, f, "Fadenkreuz", 520.0, |p| {
            crosshair_preview(p, f, o);
            stepper(p, f, "Länge", format!("{:.0}", o.crosshair_size), (o.crosshair_size - 2.0) / 18.0, Act::CrossSize(-1.0), Act::CrossSize(1.0));
            stepper(p, f, "Lücke", format!("{:.0}", o.crosshair_gap), o.crosshair_gap / 15.0, Act::CrossGap(-1.0), Act::CrossGap(1.0));
            let (name, color) = CROSSHAIR_COLORS[(o.crosshair_color as usize).min(CROSSHAIR_COLORS.len() - 1)];
            cycle(p, f, "Farbe", name, Some(color), Act::CrossColor);
            switch(p, f, "Dynamisch (öffnet beim Schießen)", o.crosshair_dynamic, Act::CrossDynamic);
        });
    });
}

/// The graphics settings: a preset and the quick test that picks one, then each setting on its own (changing one makes
/// the preset "custom"), and the frame rate.
fn graphics_page(body: &mut ChildSpawnerCommands, f: &Fonts, o: &Options, bench: &crate::bench::Bench, in_match: bool) {
    use crate::graphics::{AA_NAMES, GROUND_NAMES, PRESET_NAMES, SHADOW_NAMES};
    let g = &o.gfx;
    let web = cfg!(target_arch = "wasm32");
    let pct = |v: f32| if v <= 0.0 { "Aus".to_string() } else { format!("{:.0} %", v * 100.0) };
    body.spawn(Node { width: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Start, column_gap: Val::Px(24.0), flex_wrap: FlexWrap::Wrap, row_gap: Val::Px(24.0), ..default() }).with_children(|row| {
        row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(24.0), ..default() }).with_children(|col| {
            card(col, f, "Voreinstellung", 500.0, |p| {
                cycle(p, f, "Grafikstufe", PRESET_NAMES[(g.preset as usize).min(4)], None, Act::Preset);
                if in_match {
                    p.spawn(text("Der Leistungstest läuft im Hauptmenü (über der Karte im Hintergrund).", &f.text, 18.0, DIM));
                } else {
                    primary(p, f, "Leistungstest starten", Act::Bench, Val::Percent(100.0), 56.0, 22.0);
                }
                match &bench.summary {
                    Some(sum) => {
                        p.spawn(text(&format!("Ziel: {:.0} Bilder/s gleichmäßig · gewählt: {}", sum.target, PRESET_NAMES[sum.chosen as usize]), &f.label, 20.0, AMBER));
                        for (l, st) in &sum.tried {
                            let line = format!("{:<8}  Ø {:.0} fps   1 % {:.0} fps   {}", PRESET_NAMES[*l as usize], st.avg_fps, st.low_fps, if st.holds { "✓ hält" } else { "✗ zu langsam" });
                            p.spawn(text(&line, &f.label, 19.0, if st.holds { Color::WHITE } else { DIM }));
                        }
                    }
                    None => {
                        p.spawn(text("Der Test läuft rund 20 Sekunden über die Karte im Hintergrund und wählt die höchste Stufe, die dieser Rechner mit der Bildwiederholrate des Monitors gleichmäßig hält.", &f.text, 18.0, DIM));
                    }
                }
            });
            card(col, f, "Bildrate", 500.0, |p| {
                if !web {
                    let limit = if g.fps_limit == 0 { "Aus".to_string() } else { format!("{} fps", g.fps_limit) };
                    cycle(p, f, "Bildratenbegrenzung", &limit, None, Act::FpsLimit);
                    switch(p, f, "V-Sync", o.vsync, Act::Vsync);
                } else {
                    let res = if o.render_scale <= 0.0 { "Gerät".to_string() } else { format!("{:.0} %", o.render_scale * 100.0) };
                    let k = if o.render_scale <= 0.0 { 1.0 } else { o.render_scale };
                    stepper(p, f, "Renderauflösung", res, k, Act::Render(-1), Act::Render(1));
                }
                cycle(p, f, "Leistungsanzeige (F3)", crate::perf::SIZE_NAMES[(o.perf as usize).min(3)], None, Act::Perf);
            });
        });
        card(row, f, "Bild", 500.0, |p| {
            cycle(p, f, "Schatten", SHADOW_NAMES[(g.shadows as usize).min(4)], None, Act::Shadows);
            cycle(p, f, "Kantenglättung", AA_NAMES[(g.aa as usize).min(2)], None, Act::Aa);
            stepper(p, f, "Schärfe", pct(g.sharpen), g.sharpen, Act::Sharpen(-0.1), Act::Sharpen(0.1));
            switch(p, f, "Leuchten (Bloom)", g.bloom, Act::Bloom);
            cycle(p, f, "Boden", GROUND_NAMES[(g.ground as usize).min(2)], None, Act::Ground);
            switch(p, f, "Spiegelungen des Himmels", g.reflections, Act::Reflections);
        });
        card(row, f, "Welt", 500.0, |p| {
            stepper(p, f, "Gras und Unkraut", pct(g.grass), g.grass / 1.5, Act::Grass(-0.1), Act::Grass(0.1));
            stepper(p, f, "Laub und Unrat", pct(g.litter), g.litter / 1.5, Act::Litter(-0.1), Act::Litter(0.1));
            switch(p, f, "Tiere (Tauben, Möwen, Ratten)", g.fauna, Act::Fauna);
            stepper(p, f, "Sichtweite der Details", format!("{:.0} %", g.view * 100.0), (g.view - 0.5) / 1.0, Act::ViewDistance(-0.1), Act::ViewDistance(0.1));
        });
    });
}

/// The crosshair as it will look, over a patch of wall.
fn crosshair_preview(c: &mut ChildSpawnerCommands, f: &Fonts, o: &Options) {
    let color = CROSSHAIR_COLORS[(o.crosshair_color as usize).min(CROSSHAIR_COLORS.len() - 1)].1;
    let (len, gap, th) = (o.crosshair_size.round(), o.crosshair_gap.round(), o.crosshair_thickness.round().max(1.0));
    c.spawn((
        Node { width: Val::Percent(100.0), height: Val::Px(150.0), border_radius: BorderRadius::all(Val::Px(10.0)), border: UiRect::all(Val::Px(1.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::percent(Color::srgb(0.36, 0.38, 0.4), 0.0), ColorStop::percent(Color::srgb(0.22, 0.23, 0.24), 100.0)])),
        BorderColor::all(LINE),
    ))
    .with_children(|b| {
        b.spawn(Node { width: Val::Px(0.0), height: Val::Px(0.0), ..default() }).with_children(|o| {
            for (x, y, w, h) in [(gap, -th / 2.0, len, th), (-gap - len, -th / 2.0, len, th), (-th / 2.0, gap, th, len), (-th / 2.0, -gap - len, th, len)] {
                o.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(x), top: Val::Px(y), width: Val::Px(w), height: Val::Px(h), ..default() }, BackgroundColor(color)));
            }
        });
        b.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(12.0), bottom: Val::Px(8.0), ..default() }, children![caps("Vorschau", &f.bold, 12.0, Color::srgba(1.0, 1.0, 1.0, 0.5))]));
    });
}

fn controls_page(body: &mut ChildSpawnerCommands, f: &Fonts) {
    let groups: [(&str, &[(&[&str], &str)]); 4] = [
        ("Bewegung", &[(&["W", "A", "S", "D"], "Laufen"), (&["Umschalt"], "Gehen (leise)"), (&["Strg", "C"], "Ducken (im Browser: C, oder Vollbild für Strg)"), (&["Leertaste"], "Springen")]),
        (
            "Kampf",
            &[
                (&["Linke Maustaste"], "Feuer · Granate ziehen und werfen · Ladung legen (halten)"),
                (&["Rechte Maustaste"], "Zielfernrohr · Messerstich · Granate unterhand"),
                (&["R"], "Nachladen"),
                (&["1 – 5", "Q", "Mausrad"], "Waffen wählen · letzte Waffe · durchblättern"),
                (&["G"], "Waffe fallen lassen"),
            ],
        ),
        ("Einsatz", &[(&["E"], "Benutzen: entschärfen (halten), Geisel führen, Waffe aufnehmen"), (&["B"], "Kaufmenü"), (&["Z", "X"], "Funk: Befehle an die Bots · Meldungen")]),
        ("Oberfläche", &[(&["Tab"], "Punktetafel"), (&["T", "Y", "U"], "Online-Chat: an alle (T oder Y) · an die eigene Seite (U)"), (&["F3"], "Leistungsanzeige: klein · mittel · groß · aus"), (&["Esc"], "Pause")]),
    ];
    body.spawn(Node { width: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Stretch, align_content: AlignContent::Start, column_gap: Val::Px(24.0), row_gap: Val::Px(24.0), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|row| {
        for (title, rows) in groups {
            card(row, f, title, 760.0, |p| {
                for (keys, what) in rows {
                    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(18.0), ..default() }).with_children(|r| {
                        r.spawn(Node { width: Val::Px(270.0), flex_shrink: 0.0, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).with_children(|k| {
                            for key in keys.iter() {
                                keycap(k, f, key);
                            }
                        });
                        r.spawn(text(what, &f.label, 22.0, SOFT));
                    });
                }
            });
        }
    });
}

fn credits_page(body: &mut ChildSpawnerCommands, f: &Fonts) {
    body.spawn(Node { width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() }).with_children(|row| {
        card(row, f, "Mitwirkende", 1100.0, |p| {
            for (head, line) in [
                ("Spiel", "ZUGRIFF – Spiel, Karten, Simulation und Bots: eigene Arbeit (Bevy, Rust)."),
                ("Modelle", "Sketchfab-Modelle unter CC BY 4.0 – barcodegames (M4-Animationen), ccransh, bumstrum, raimeiyonke (Pistole und Hände), vitvitskyi (Messer), rileylewandowski (Granate), 1799danly und joneborden123 (Soldaten)."),
                ("Animation", "Quaternius, Universal Animation Library (CC0)."),
                ("Materialien", "ambientCG und Poly Haven (CC0)."),
                ("Schriften", "Barlow und Inter (SIL OFL)."),
                ("Klänge", "Aufnahmen von Freesound (CC0; toxicwafflezz und Patrick_Corra unter CC BY) und Kenney (CC0)."),
                ("Mehr", "assets/models/CREDITS.md, assets/textures/CREDITS.md und assets/sounds/CREDITS.md."),
            ] {
                p.spawn(Node { column_gap: Val::Px(24.0), ..default() }).with_children(|r| {
                    r.spawn((Node { width: Val::Px(160.0), flex_shrink: 0.0, ..default() }, children![caps(head, &f.bold, 17.0, AMBER)]));
                    r.spawn(text(line, &f.text, 19.0, SOFT));
                });
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn buttons(
    mut commands: Commands,
    mut state: ResMut<MenuState>,
    mut options: ResMut<Options>,
    mut ui: ResMut<Ui>,
    q: Query<(&Interaction, &Act), (Changed<Interaction>, With<Button>)>,
    mut loads: MessageWriter<LoadMap>,
    mut exit: MessageWriter<AppExit>,
    mut cues: MessageWriter<crate::audio::Cue>,
    session: Option<Res<Session>>,
    resolutions: Res<Resolutions>,
    net: Res<crate::net::Net>,
    mut bench: ResMut<crate::bench::Bench>,
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
        let setup = &mut state.setup;
        match *act {
            Act::Tab(s) => {
                if session.is_some() {
                    state.back = Screen::Pause;
                } else {
                    state.back = Screen::Title;
                }
                state.screen = s;
            }
            Act::Quit => {
                exit.write(AppExit::Success);
            }
            Act::Back => {
                state.screen = if session.is_some() { Screen::Pause } else { state.back };
            }
            Act::Map(i) => {
                setup.map = MAPS[i].name.to_string();
                options.last_map = setup.map.clone();
                if session.is_none() {
                    loads.write(LoadMap(setup.map.clone()));
                }
            }
            Act::Mode(dm) => setup.mode = if dm { Mode::Deathmatch } else { Mode::Defuse },
            Act::Difficulty(d) => {
                setup.difficulty = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard, Difficulty::Expert][d as usize];
                options.difficulty = d;
            }
            Act::Team(t) => setup.team = [Some(Team::Ksk), Some(Team::Opfor), None][t as usize],
            Act::Rounds(r) => setup.max_rounds = r,
            Act::Start => {
                let settings = state.setup.clone();
                commands.remove_resource::<Session>();
                session::request(&mut commands, settings, &mut loads);
                state.screen = Screen::None;
                ui.menu = false;
            }
            Act::Resume => {
                state.screen = Screen::None;
                ui.menu = false;
            }
            Act::SwitchSide => {
                if let Some(s) = &session {
                    net.switch_team(s.me().team.other());
                }
                state.screen = Screen::None;
                ui.menu = false;
            }
            Act::Leave | Act::MainMenu => {
                commands.remove_resource::<Session>();
                state.screen = Screen::Title;
            }
            Act::Again => {
                let settings = session.as_ref().map_or(state.setup.clone(), |s| s.settings.clone());
                commands.remove_resource::<Session>();
                session::request(&mut commands, settings, &mut loads);
                state.screen = Screen::None;
            }
            Act::Sens(d) => options.sensitivity = (options.sensitivity + d).clamp(0.1, 10.0),
            Act::Fov(d) => options.fov = (options.fov + d).clamp(70.0, 110.0),
            Act::Volume(d) => options.volume = (options.volume + d).clamp(0.0, 1.0),
            Act::CrossSize(d) => options.crosshair_size = (options.crosshair_size + d).clamp(2.0, 20.0),
            Act::CrossGap(d) => options.crosshair_gap = (options.crosshair_gap + d).clamp(0.0, 15.0),
            Act::CrossColor => options.crosshair_color = (options.crosshair_color + 1) % CROSSHAIR_COLORS.len() as u8,
            Act::CrossDynamic => options.crosshair_dynamic = !options.crosshair_dynamic,
            Act::Perf => options.perf = (options.perf + 1) % 4,
            Act::Invert => options.invert = !options.invert,
            Act::Display => options.display = (options.display + 1) % if cfg!(target_arch = "wasm32") { 2 } else { 3 },
            Act::Resolution(step) => {
                let steps = resolution_steps(&resolutions);
                let i = steps.iter().position(|s| *s == options.resolution).unwrap_or(0) as i32;
                options.resolution = steps[(i + step as i32).rem_euclid(steps.len() as i32) as usize];
            }
            Act::Vsync => options.vsync = !options.vsync,
            Act::Preset => {
                let next = (options.gfx.preset.min(3) + 1) % 4;
                options.gfx.apply_preset(next);
            }
            Act::Bench => bench.request = session.is_none(),
            Act::FpsLimit => {
                use crate::graphics::FPS_LIMITS;
                let i = FPS_LIMITS.iter().position(|&v| v == options.gfx.fps_limit).unwrap_or(0);
                options.gfx.fps_limit = FPS_LIMITS[(i + 1) % FPS_LIMITS.len()];
            }
            Act::Shadows | Act::Aa | Act::Sharpen(_) | Act::Bloom | Act::Ground | Act::Reflections | Act::Grass(_) | Act::Litter(_) | Act::Fauna | Act::ViewDistance(_) => {
                let web = cfg!(target_arch = "wasm32");
                let g = &mut options.gfx;
                g.preset = crate::graphics::CUSTOM;
                match *act {
                    Act::Shadows => g.shadows = (g.shadows + 1) % if web { 3 } else { 5 },
                    Act::Aa => g.aa = (g.aa + 1) % 3,
                    Act::Sharpen(d) => g.sharpen = ((g.sharpen + d) * 10.0).round().clamp(0.0, 10.0) / 10.0,
                    Act::Bloom => g.bloom = !g.bloom,
                    Act::Ground => g.ground = (g.ground + 1) % if web { 2 } else { 3 },
                    Act::Reflections => g.reflections = !g.reflections,
                    Act::Grass(d) => g.grass = ((g.grass + d) * 10.0).round().clamp(0.0, 15.0) / 10.0,
                    Act::Litter(d) => g.litter = ((g.litter + d) * 10.0).round().clamp(0.0, 15.0) / 10.0,
                    Act::Fauna => g.fauna = !g.fauna,
                    Act::ViewDistance(d) => g.view = ((g.view + d) * 10.0).round().clamp(5.0, 15.0) / 10.0,
                    _ => {}
                }
            }
            Act::Render(step) => {
                // Steps: 50 %, 67 %, 75 %, 100 % of CSS pixels, then the device's full resolution (0).
                const STEPS: [f32; 5] = [0.5, 0.67, 0.75, 1.0, 0.0];
                let i = STEPS.iter().position(|&v| (v - options.render_scale).abs() < 0.01).unwrap_or(3) as i32;
                options.render_scale = STEPS[(i + step as i32).clamp(0, 4) as usize];
            }
        }
    }
}

/// The loading screen: the map's picture, its name and objective, a tip, a moving bar.
fn loading(mut commands: Commands, pending: Res<Pending>, warmup: Res<crate::warmup::Warmup>, fonts: Res<Fonts>, server: Res<AssetServer>, root: Single<(Entity, &mut Visibility, Option<&Children>), With<LoadingBox>>, session: Option<Res<Session>>) {
    let (e, mut vis, children) = root.into_inner();
    let show = pending.0.is_some() || warmup.active();
    vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    if show && children.is_none_or(|c| c.is_empty()) {
        let settings = pending.0.clone().or_else(|| session.as_ref().map(|s| s.settings.clone())).unwrap_or_default();
        let info = map_info(&settings.map);
        let tip = TIPS[settings.map.bytes().map(|b| b as usize).sum::<usize>() % TIPS.len()];
        let f = &*fonts;
        commands.entity(e).with_children(|c| {
            // The picture fills the screen (cropped to its middle), darkened toward the text.
            c.spawn(Node { overflow: Overflow::clip(), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..full() }).with_children(|p| {
                p.spawn((Node { width: Val::Percent(100.0), min_height: Val::Percent(100.0), aspect_ratio: Some(5.0 / 3.0), flex_shrink: 0.0, ..default() }, ImageNode::new(server.load(format!("thumbs/{}.jpg", settings.map))).with_color(Color::srgb(0.55, 0.58, 0.62))));
            });
            c.spawn((full(), BackgroundGradient::from(LinearGradient::to_top(vec![ColorStop::percent(Color::srgba(0.01, 0.015, 0.02, 0.97), 0.0), ColorStop::percent(Color::srgba(0.01, 0.015, 0.02, 0.6), 45.0), ColorStop::percent(Color::srgba(0.01, 0.015, 0.02, 0.15), 100.0)]))));
            c.spawn(Node { position_type: PositionType::Absolute, left: Val::Px(120.0), right: Val::Px(120.0), bottom: Val::Px(90.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(12.0), ..default() }).with_children(|col| {
                col.spawn(Node { column_gap: Val::Px(14.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                    emblem(r, 22.0);
                    r.spawn(caps("Einsatzgebiet wird geladen", &f.bold, 20.0, AMBER));
                });
                col.spawn((text(info.map_or(settings.map.as_str(), |m| m.title), &f.bold, 128.0, Color::WHITE), LetterSpacing::Px(3.0)));
                col.spawn(Node { column_gap: Val::Px(10.0), ..default() }).with_children(|tags| {
                    if settings.mode == Mode::Deathmatch {
                        tag(tags, f, "Deathmatch", OPFOR);
                    } else if let Some(m) = info {
                        tag(tags, f, m.objective, objective_color(m.objective));
                    }
                    if let Some(m) = info {
                        tags.spawn(text(m.blurb, &f.text, 19.0, SOFT));
                    }
                });
                col.spawn((Node { width: Val::Percent(100.0), height: Val::Px(3.0), margin: UiRect::top(Val::Px(26.0)), overflow: Overflow::clip(), border_radius: BorderRadius::all(Val::Px(2.0)), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.1)))).with_children(|t| {
                    t.spawn((
                        Node { position_type: PositionType::Absolute, left: Val::Percent(0.0), width: Val::Percent(28.0), height: Val::Percent(100.0), ..default() },
                        BackgroundGradient::from(LinearGradient::to_right(vec![ColorStop::percent(Color::srgba(1.0, 0.69, 0.0, 0.0), 0.0), ColorStop::percent(AMBER, 70.0), ColorStop::percent(AMBER_HI, 100.0)])),
                        Sweep,
                    ));
                });
                col.spawn(Node { column_gap: Val::Px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                    r.spawn(caps("Tipp", &f.bold, 16.0, AMBER));
                    r.spawn(text(tip, &f.label, 21.0, SOFT));
                });
            });
        });
    }
    if !show
        && let Some(ch) = children
    {
        for c in ch.iter() {
            commands.entity(c).despawn();
        }
    }
}

/// Moves the loading bar's light from left to right, over and over.
fn loading_sweep(time: Res<Time>, mut q: Query<&mut Node, With<Sweep>>) {
    for mut n in &mut q {
        let k = (time.elapsed_secs() * 0.7).fract();
        n.left = Val::Percent(-30.0 + k * 130.0);
    }
}

#[allow(clippy::too_many_arguments)]
fn match_end(mut commands: Commands, session: Option<Res<Session>>, fonts: Res<Fonts>, mut ui: ResMut<Ui>, state: Res<MenuState>, root: Single<(Entity, &mut Visibility, Option<&Children>), With<EndBox>>, mut built: Local<bool>) {
    let (e, mut vis, children) = root.into_inner();
    let over = session.as_ref().is_some_and(|s| s.game.phase == Phase::MatchOver) && state.screen == Screen::None;
    vis.set_if_neq(if over { Visibility::Inherited } else { Visibility::Hidden });
    if !over {
        if *built {
            *built = false;
            if let Some(ch) = children {
                for c in ch.iter() {
                    commands.entity(c).despawn();
                }
            }
        }
        return;
    }
    // The pointer is freed so the buttons can be clicked.
    ui.menu = true;
    if *built {
        return;
    }
    *built = true;
    let s = session.unwrap();
    let g = &s.game;
    let mine = s.me().team;
    let (title, color) = match g.winner {
        Some(t) if t == mine => ("Sieg", AMBER),
        Some(_) => ("Niederlage", ui_kit::DANGER),
        None => ("Unentschieden", Color::WHITE),
    };
    let f = &*fonts;
    let c0 = color.to_srgba();
    commands.entity(e).with_children(|c| {
        c.spawn((full(), BackgroundColor(Color::srgba(0.01, 0.015, 0.02, 0.82))));
        c.spawn(glass(Node { width: Val::Px(960.0), flex_direction: FlexDirection::Column, overflow: Overflow::clip(), ..default() })).with_children(|p| {
            // The banner.
            p.spawn((
                Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, padding: UiRect::new(Val::Px(30.0), Val::Px(30.0), Val::Px(34.0), Val::Px(26.0)), row_gap: Val::Px(6.0), ..default() },
                BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::percent(Color::srgba(c0.red, c0.green, c0.blue, 0.28), 0.0), ColorStop::percent(Color::srgba(c0.red, c0.green, c0.blue, 0.0), 100.0)])),
            ))
            .with_children(|b| {
                b.spawn(caps("Einsatz beendet", &f.bold, 18.0, DIM));
                b.spawn((text(&title.to_uppercase(), &f.bold, 112.0, color), LetterSpacing::Px(8.0), TextShadow { offset: Vec2::new(0.0, 4.0), color: Color::srgba(0.0, 0.0, 0.0, 0.5) }));
                if g.cfg.mode != Mode::Deathmatch {
                    b.spawn(Node { column_gap: Val::Px(22.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                        r.spawn(caps("KSK", &f.bold, 26.0, KSK));
                        r.spawn(text(&format!("{}  :  {}", g.score[0], g.score[1]), &f.digits, 54.0, Color::WHITE));
                        r.spawn(caps("OPFOR", &f.bold, 26.0, OPFOR));
                    });
                }
            });
            // The table.
            p.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::axes(Val::Px(30.0), Val::Px(10.0)), row_gap: Val::Px(2.0), ..default() }).with_children(|t| {
                let row = |t: &mut ChildSpawnerCommands, cells: [&str; 4], color: Color, head: bool, me: bool| {
                    t.spawn((
                        Node { padding: UiRect::axes(Val::Px(16.0), Val::Px(if head { 6.0 } else { 9.0 })), align_items: AlignItems::Center, border_radius: BorderRadius::all(Val::Px(6.0)), ..default() },
                        BackgroundColor(if me { Color::srgba(1.0, 0.69, 0.0, 0.14) } else if head { Color::NONE } else { Color::srgba(1.0, 1.0, 1.0, 0.03) }),
                    ))
                    .with_children(|r| {
                        let size = if head { 15.0 } else { 24.0 };
                        let cell = |t: &str, font: &Handle<Font>, color: Color| {
                            let (t, font, color, spacing) = if head { (t.to_uppercase(), &f.bold, DIM, size * 0.14) } else { (t.to_string(), font, color, 0.0) };
                            (text(&t, font, size, color), LetterSpacing::Px(spacing))
                        };
                        r.spawn((Node { flex_grow: 1.0, ..default() }, children![cell(cells[0], &f.label, color)]));
                        for c in &cells[1..] {
                            r.spawn((Node { width: Val::Px(120.0), justify_content: JustifyContent::End, ..default() }, children![cell(c, &f.digits, Color::WHITE)]));
                        }
                    });
                };
                row(t, ["Spieler", "Abschüsse", "Tode", "Punkte"], DIM, true, false);
                let mut best: Vec<(usize, &zugriff_sim::game::Player)> = g.players.iter().enumerate().collect();
                best.sort_by_key(|(_, q)| (-q.stats.score, -q.stats.kills));
                for (id, q) in best.iter().take(10) {
                    row(t, [&q.name, &q.stats.kills.to_string(), &q.stats.deaths.to_string(), &q.stats.score.to_string()], crate::hud::team_color(q.team), false, *id == s.me);
                }
            });
            p.spawn(Node { column_gap: Val::Px(12.0), padding: UiRect::new(Val::Px(30.0), Val::Px(30.0), Val::Px(14.0), Val::Px(30.0)), justify_content: JustifyContent::End, align_items: AlignItems::Center, ..default() }).with_children(|r| {
                if s.online {
                    r.spawn((Node { flex_grow: 1.0, ..default() }, children![text("Das nächste Spiel beginnt gleich.", &f.label, 22.0, DIM)]));
                    ghost(r, f, "Raum verlassen", Act::MainMenu, Val::Px(260.0), true);
                } else {
                    ghost(r, f, "Hauptmenü", Act::MainMenu, Val::Px(240.0), false);
                    primary(r, f, "Nochmal", Act::Again, Val::Px(260.0), 56.0, 26.0);
                }
            });
        });
    });
}
