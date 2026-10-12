//! The in-game HUD in Bevy UI, laid out like the classic tactical shooters: health and armour bottom left, money and
//! ammunition bottom right, the score and round timer top centre, the kill feed top right, the radar top left, the
//! crosshair (opening with the spread) in the middle. Around them: round messages, hints for what can be done here,
//! the plant/defuse bar, radio lines, where damage came from, the flash's white-out, the scope, the death panel.
//! Sizes are authored for 1080 px of height and scaled with the window.

use crate::input::Ui;
use crate::level::Current;
use crate::options::{CROSSHAIR_COLORS, Options};
use crate::session::{Frame, Session};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use zugriff_sim::game::{dir_from, Bomb, Event, Mode, Phase, Reason, Team};
use zugriff_sim::weapons::{Action, Class};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudState>()
            .add_systems(Startup, spawn)
            .add_systems(Update, (scale_to_window, collect, vitals, top, crosshair, feed, chat, center, hints, progress, overlays, radar_map, radar, damage_arrows, death_panel).chain().after(crate::session::SessionSet));
    }
}

pub const AMBER: Color = Color::srgba(1.0, 0.69, 0.0, 0.95);
pub const PLATE: Color = Color::srgba(0.0, 0.0, 0.0, 0.42);
pub const KSK: Color = Color::srgb(0.47, 0.72, 1.0);
pub const OPFOR: Color = Color::srgb(1.0, 0.63, 0.28);
pub const RED: Color = Color::srgb(1.0, 0.25, 0.2);

pub fn team_color(t: Team) -> Color {
    match t {
        Team::Ksk => KSK,
        Team::Opfor => OPFOR,
    }
}

/// The fonts, loaded once.
#[derive(Resource, Clone)]
pub struct Fonts {
    pub digits: Handle<Font>,
    pub label: Handle<Font>,
    pub bold: Handle<Font>,
    pub text: Handle<Font>,
}

#[derive(Component)]
struct HudRoot;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Txt {
    Health,
    Armor,
    Money,
    Ammo,
    Reserve,
    Weapon,
    Timer,
    ScoreKsk,
    ScoreOpfor,
    Center,
    CenterSub,
    Place,
    Hint,
    Spectating,
    Death,
    Phase,
    Progress,
    MoneyDelta,
}

#[derive(Component)]
struct CrossBar(u8);
#[derive(Component)]
struct CrossRoot;
#[derive(Component)]
struct KillFeed;
#[derive(Component)]
struct ChatFeed;
#[derive(Component)]
struct CenterBox;
#[derive(Component)]
struct HintBox;
#[derive(Component)]
struct ProgressBox;
#[derive(Component)]
struct ProgressFill;
#[derive(Component)]
struct FlashOverlay;
#[derive(Component)]
struct ScopeOverlay;
#[derive(Component)]
struct Vignette;
#[derive(Component)]
struct DamageArrow(usize);
#[derive(Component)]
struct DeathBox;
#[derive(Component)]
struct SpecBox;
#[derive(Component)]
struct AmmoBox;
#[derive(Component)]
struct HelmetIcon;
#[derive(Component)]
struct TimerPlate;
#[derive(Component)]
struct AliveDots(Team);
#[derive(Component)]
struct RadarBox;
#[derive(Component)]
struct RadarImage;
#[derive(Component)]
struct PhasePlate;
#[derive(Component)]
struct KitIcon;
#[derive(Component)]
struct BombCarrierIcon;

struct Kill {
    killer: Option<(String, Team)>,
    victim: (String, Team),
    weapon: String,
    headshot: bool,
    wallbang: bool,
    mine: bool,
    at: f32,
}

#[derive(Resource, Default)]
pub struct HudState {
    kills: Vec<Kill>,
    kills_dirty: bool,
    chat: Vec<(String, Color, f32)>,
    chat_dirty: bool,
    center: Option<(String, String, Color, f32)>,
    damage: Vec<(Vec3, f32)>,
    killed_by: Option<(String, String, bool, i32)>,
    money_delta: Option<(i32, f32)>,
    radar: Option<RadarMap>,
    radar_gen: u32,
    time: f32,
}

impl HudState {
    /// A line in the chat feed (online chat and the server's notices).
    pub fn say(&mut self, line: String, color: Color) {
        let now = self.time;
        self.chat.push((line, color, now));
        self.chat_dirty = true;
    }
}

/// The radar's picture of the map: drawn from the navigation graph (walkable floor, lighter higher up, walls dark).
/// Each frame the part around the player is copied, turned with the view, into the small radar image.
struct RadarMap {
    pixels: Vec<[u8; 4]>,
    w: usize,
    h: usize,
    /// World x, z at pixel (0, 0), and pixels per metre.
    origin: Vec2,
    ppm: f32,
}

/// The radar image's size in pixels.
const RADAR_PX: usize = 176;

const RADAR: f32 = 236.0;
/// The radar shows this far around the player (m).
const RADAR_RANGE: f32 = 34.0;

fn plate(padding: f32) -> impl Bundle {
    (
        Node { padding: UiRect::axes(Val::Px(padding * 1.4), Val::Px(padding * 0.5)), column_gap: Val::Px(10.0), align_items: AlignItems::Center, border_radius: BorderRadius::all(Val::Px(7.0)), ..default() },
        BackgroundColor(PLATE),
    )
}

fn cross_icon() -> impl Bundle {
    let arm = |w: f32, h: f32| {
        (
            Node { position_type: PositionType::Absolute, left: Val::Px((26.0 - w) / 2.0), top: Val::Px((26.0 - h) / 2.0), width: Val::Px(w), height: Val::Px(h), border_radius: BorderRadius::all(Val::Px(1.5)), ..default() },
            BackgroundColor(AMBER),
        )
    };
    (Node { width: Val::Px(26.0), height: Val::Px(26.0), ..default() }, children![arm(26.0, 8.0), arm(8.0, 26.0)])
}

/// A small shield (the vest) with a dome on top when a helmet is worn.
fn armor_icon() -> impl Bundle {
    (
        Node { width: Val::Px(24.0), height: Val::Px(28.0), ..default() },
        children![
            (
                Node { position_type: PositionType::Absolute, left: Val::Px(2.0), top: Val::Px(8.0), width: Val::Px(20.0), height: Val::Px(20.0), border_radius: BorderRadius::new(Val::Px(3.0), Val::Px(3.0), Val::Px(10.0), Val::Px(10.0)), ..default() },
                BackgroundColor(AMBER)
            ),
            (
                Node { position_type: PositionType::Absolute, left: Val::Px(5.0), top: Val::Px(0.0), width: Val::Px(14.0), height: Val::Px(7.0), border_radius: BorderRadius::new(Val::Px(7.0), Val::Px(7.0), Val::Px(1.0), Val::Px(1.0)), ..default() },
                BackgroundColor(AMBER),
                HelmetIcon
            )
        ],
    )
}

pub fn text(t: &str, font: &Handle<Font>, size: f32, color: Color) -> impl Bundle + use<> {
    (Text::new(t), TextFont::from(font.clone()).with_font_size(size), TextColor(color))
}

fn set(t: &mut Text, s: &str) {
    if t.0 != s {
        t.0 = s.to_string();
    }
}

/// The scope: black with a clear round window and fine cross lines.
fn scope_image() -> Image {
    let n = 512u32;
    let mut data = vec![0u8; (n * n * 4) as usize];
    for y in 0..n {
        for x in 0..n {
            let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            let edge = ((r - 0.94) / 0.03).clamp(0.0, 1.0);
            let line = (u.abs() < 0.0025 || v.abs() < 0.0025) && r < 0.94;
            let thick = (u.abs() < 0.008 && v.abs() > 0.45 || v.abs() < 0.008 && u.abs() > 0.45) && r < 0.94;
            let a = if line || thick { 0.95 } else { edge };
            let i = ((y * n + x) * 4) as usize;
            data[i + 3] = (a * 255.0) as u8;
        }
    }
    Image::new(Extent3d { width: n, height: n, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}

fn spawn(mut commands: Commands, assets: Res<AssetServer>, mut images: ResMut<Assets<Image>>) {
    let fonts = Fonts {
        digits: assets.load("fonts/BarlowCondensed-SemiBold.ttf"),
        label: assets.load("fonts/BarlowCondensed-Medium.ttf"),
        bold: assets.load("fonts/BarlowCondensed-Bold.ttf"),
        text: assets.load("fonts/Inter.ttf"),
    };
    commands.insert_resource(fonts.clone());
    let f = &fonts;
    let big = |t: &str, k: Txt| (text(t, &f.digits, 58.0, AMBER), k);
    let small = |t: &str| text(t, &f.label, 26.0, AMBER);
    let abs = |left: Option<f32>, right: Option<f32>, top: Option<f32>, bottom: Option<f32>| Node {
        position_type: PositionType::Absolute,
        left: left.map_or(Val::Auto, Val::Px),
        right: right.map_or(Val::Auto, Val::Px),
        top: top.map_or(Val::Auto, Val::Px),
        bottom: bottom.map_or(Val::Auto, Val::Px),
        ..default()
    };
    let full = || Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };

    let root = commands.spawn((full(), Visibility::Hidden, HudRoot, Name::new("hud"), GlobalZIndex(1))).id();
    let radar_image = images.add(Image::new_fill(Extent3d { width: RADAR_PX as u32, height: RADAR_PX as u32, depth_or_array_layers: 1 }, TextureDimension::D2, &[0, 0, 0, 0], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::all()));

    // Overlays under everything else: the scope, low health, the flash.
    let scope = images.add(scope_image());
    commands.spawn((
        full(),
        Visibility::Hidden,
        ScopeOverlay,
        ChildOf(root),
        children![(
            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
            children![
                (Node { flex_grow: 1.0, height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::BLACK)),
                (Node { height: Val::Percent(100.0), aspect_ratio: Some(1.0), ..default() }, ImageNode::new(scope)),
                (Node { flex_grow: 1.0, height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::BLACK)),
            ]
        )],
    ));
    commands.spawn((full(), BackgroundColor(Color::srgba(0.6, 0.0, 0.0, 0.0)), Vignette, ChildOf(root)));

    // Bottom left: health and armour; the chat above.
    commands.spawn((
        Node { column_gap: Val::Px(14.0), ..abs(Some(24.0), None, None, Some(20.0)) },
        ChildOf(root),
        children![(plate(14.0), children![cross_icon(), big("100", Txt::Health)]), (plate(14.0), children![armor_icon(), big("0", Txt::Armor)])],
    ));
    commands.spawn((Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), width: Val::Px(620.0), ..abs(Some(26.0), None, None, Some(118.0)) }, ChatFeed, ChildOf(root)));

    // Bottom right: money over ammunition, with the weapon's name.
    commands.spawn((
        Node { flex_direction: FlexDirection::Column, align_items: AlignItems::End, row_gap: Val::Px(10.0), ..abs(None, Some(24.0), None, Some(20.0)) },
        ChildOf(root),
        children![
            (text("", &f.label, 28.0, Color::srgb(0.5, 1.0, 0.5)), Txt::MoneyDelta),
            (Node { column_gap: Val::Px(8.0), ..default() }, children![
                (plate(8.0), KitIcon, children![text("ENTSCHÄRFERSET", &f.bold, 20.0, KSK)]),
                (plate(8.0), BombCarrierIcon, children![text("SPRENGLADUNG", &f.bold, 20.0, OPFOR)]),
            ]),
            (plate(14.0), children![small("$"), big("800", Txt::Money)]),
            (text("", &f.label, 24.0, Color::srgba(1.0, 1.0, 1.0, 0.8)), Txt::Weapon),
            (plate(14.0), AmmoBox, children![big("", Txt::Ammo), small("|"), (text("", &f.digits, 38.0, AMBER), Txt::Reserve)]),
        ],
    ));

    // Top centre: the score, the alive markers and the round timer.
    let dots = |t: Team| (Node { column_gap: Val::Px(4.0), align_items: AlignItems::Center, min_width: Val::Px(90.0), justify_content: if t == Team::Ksk { JustifyContent::End } else { JustifyContent::Start }, ..default() }, AliveDots(t));
    commands.spawn((
        Node { position_type: PositionType::Absolute, top: Val::Px(14.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
        ChildOf(root),
        children![(
            Node { align_items: AlignItems::Center, column_gap: Val::Px(12.0), ..default() },
            children![
                dots(Team::Ksk),
                (plate(10.0), children![(text("0", &f.digits, 40.0, KSK), Txt::ScoreKsk)]),
                (plate(12.0), TimerPlate, children![(text("1:55", &f.digits, 44.0, Color::WHITE), Txt::Timer)]),
                (plate(10.0), children![(text("0", &f.digits, 40.0, OPFOR), Txt::ScoreOpfor)]),
                dots(Team::Opfor),
            ]
        )],
    ));
    // Under it: the phase (warmup, buy time).
    commands.spawn((
        Node { position_type: PositionType::Absolute, top: Val::Px(74.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
        ChildOf(root),
        children![(plate(8.0), PhasePlate, children![(text("", &f.label, 24.0, Color::WHITE), Txt::Phase)])],
    ));

    // Top right: the kill feed.
    commands.spawn((Node { flex_direction: FlexDirection::Column, align_items: AlignItems::End, row_gap: Val::Px(5.0), ..abs(None, Some(20.0), Some(16.0), None) }, KillFeed, ChildOf(root)));

    // Top left: the radar, and the callout under it.
    commands.spawn((
        Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), align_items: AlignItems::Center, ..abs(Some(20.0), None, Some(18.0), None) },
        ChildOf(root),
        children![
            (
                Node { width: Val::Px(RADAR), height: Val::Px(RADAR), ..default() },
                RadarBox,
                children![(Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, ImageNode::new(radar_image), RadarImage)]
            ),
            (text("", &f.label, 24.0, Color::srgba(1.0, 1.0, 1.0, 0.85)), Txt::Place),
        ],
    ));

    // Centre: crosshair, round message, hint, progress.
    commands.spawn((Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), top: Val::Percent(50.0), width: Val::Px(0.0), height: Val::Px(0.0), ..default() }, CrossRoot, ChildOf(root)))
        .with_children(|c| {
            for i in 0..5 {
                c.spawn((Node { position_type: PositionType::Absolute, ..default() }, BackgroundColor(CROSSHAIR_COLORS[0].1), CrossBar(i)));
            }
        });
    commands.spawn((
        Node { position_type: PositionType::Absolute, top: Val::Percent(24.0), width: Val::Percent(100.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(4.0), ..default() },
        CenterBox,
        Visibility::Hidden,
        ChildOf(root),
        children![(text("", &f.bold, 54.0, Color::WHITE), Txt::Center, TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.55) }), (text("", &f.label, 30.0, Color::srgba(1.0, 1.0, 1.0, 0.9)), Txt::CenterSub, TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.55) })],
    ));
    commands.spawn((
        Node { position_type: PositionType::Absolute, top: Val::Percent(60.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
        HintBox,
        Visibility::Hidden,
        ChildOf(root),
        children![(plate(10.0), children![(text("", &f.label, 26.0, Color::WHITE), Txt::Hint)])],
    ));
    commands.spawn((
        Node { position_type: PositionType::Absolute, top: Val::Percent(66.0), width: Val::Percent(100.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(6.0), ..default() },
        ProgressBox,
        Visibility::Hidden,
        ChildOf(root),
        children![
            (text("", &f.label, 24.0, Color::WHITE), Txt::Progress, TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.55) }),
            (
                Node { width: Val::Px(360.0), height: Val::Px(12.0), border_radius: BorderRadius::all(Val::Px(6.0)), overflow: Overflow::clip(), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                children![(Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(AMBER), ProgressFill)]
            ),
        ],
    ));
    // Damage direction markers around the centre.
    for i in 0..4 {
        commands.spawn((
            Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), top: Val::Percent(50.0), width: Val::Px(0.0), height: Val::Px(0.0), ..default() },
            UiTransform::IDENTITY,
            Visibility::Hidden,
            DamageArrow(i),
            ChildOf(root),
            children![(Node { position_type: PositionType::Absolute, left: Val::Px(-45.0), top: Val::Px(-190.0), width: Val::Px(90.0), height: Val::Px(12.0), border_radius: BorderRadius::all(Val::Px(6.0)), ..default() }, BackgroundColor(Color::srgba(1.0, 0.1, 0.05, 0.75)))],
        ));
    }
    // Bottom centre: whom we watch, how we died.
    commands.spawn((
        Node { position_type: PositionType::Absolute, bottom: Val::Px(110.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
        SpecBox,
        Visibility::Hidden,
        ChildOf(root),
        children![(plate(10.0), children![(text("", &f.label, 28.0, Color::WHITE), Txt::Spectating)])],
    ));
    commands.spawn((
        Node { position_type: PositionType::Absolute, bottom: Val::Px(170.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
        DeathBox,
        Visibility::Hidden,
        ChildOf(root),
        children![(plate(12.0), children![(text("", &f.label, 28.0, Color::WHITE), Txt::Death)])],
    ));
    // The flash's white-out, over everything in the world.
    commands.spawn((full(), BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.0)), FlashOverlay, ChildOf(root)));
}

fn scale_to_window(window: Single<&Window, With<PrimaryWindow>>, mut scale: ResMut<UiScale>) {
    let s = (window.height() / 1080.0).clamp(0.45, 2.0);
    if (scale.0 - s).abs() > 1e-3 {
        scale.0 = s;
    }
}

fn name_of(s: &Session, id: usize) -> (String, Team) {
    let p = &s.game.players[id];
    (p.name.clone(), p.team)
}

/// Reads this frame's events into the HUD's memory.
fn collect(time: Res<Time>, frame: Res<Frame>, session: Option<Res<Session>>, mut hud: ResMut<HudState>, mut root: Single<&mut Visibility, With<HudRoot>>, ui: Res<Ui>) {
    hud.time += time.delta_secs();
    let now = hud.time;
    let Some(s) = session else {
        **root = Visibility::Hidden;
        return;
    };
    **root = if ui.menu || crate::args::flag("nohud") { Visibility::Hidden } else { Visibility::Inherited };
    if frame.started {
        hud.kills.clear();
        hud.chat.clear();
        hud.kills_dirty = true;
        hud.chat_dirty = true;
        hud.killed_by = None;
    }
    let me = s.me;
    let my_team = s.me().team;
    for e in &frame.events {
        match e {
            Event::Kill { killer, victim, weapon, headshot, wallbang, .. } => {
                let killer_name = killer.filter(|k| k != victim).map(|k| name_of(&s, k));
                hud.kills.push(Kill {
                    killer: killer_name,
                    victim: name_of(&s, *victim),
                    weapon: weapon.name().to_string(),
                    headshot: *headshot,
                    wallbang: *wallbang,
                    mine: *killer == Some(me) || *victim == me,
                    at: now,
                });
                hud.kills_dirty = true;
                if *victim == me {
                    let (kname, hp) = killer.filter(|&k| k != me).map_or(("sich selbst".into(), 0), |k| (s.game.players[k].name.clone(), s.game.players[k].health.max(0)));
                    hud.killed_by = Some((kname, weapon.name().to_string(), *headshot, hp));
                }
            }
            Event::Hit { victim, attacker: Some(a), .. } if *victim == me && *a != me => {
                let from = s.game.players[*a].mover.origin;
                hud.damage.push((from, now));
            }
            Event::Radio { player, text } => {
                let p = &s.game.players[*player];
                if p.team == my_team {
                    hud.chat.push((format!("[Funk] {}: {}", p.name, text), team_color(p.team), now));
                    hud.chat_dirty = true;
                }
            }
            Event::Money { player, amount, .. } if *player == me && *amount != 0 => {
                let total = hud.money_delta.filter(|(_, t)| now - t < 0.3).map_or(0, |(a, _)| a) + amount;
                hud.money_delta = Some((total, now));
            }
            Event::Bought { player, .. } if *player == me => {}
            Event::RoundStart { round } => {
                let sub = if s.game.cfg.mode == Mode::Deathmatch { String::new() } else { "Kaufzeit – [B] öffnet das Kaufmenü".to_string() };
                hud.center = Some((format!("Runde {round}"), sub, Color::WHITE, now + 2.5));
                hud.killed_by = None;
            }
            Event::FreezeEnd => {
                let text = if my_team == Team::Opfor && s.game.cfg.mode == Mode::Defuse { "Los, los, los!" } else { "Zugriff!" };
                hud.center = Some((text.into(), String::new(), AMBER, now + 1.5));
            }
            Event::WarmupEnd => hud.center = Some(("Das Spiel beginnt!".into(), String::new(), AMBER, now + 2.5)),
            Event::Planted { site, .. } => hud.center = Some(("Die Sprengladung wurde gelegt!".into(), format!("Zielpunkt {site}"), RED, now + 3.0)),
            Event::HostageRescued { .. } => hud.center = Some(("Geisel gerettet".into(), String::new(), KSK, now + 2.0)),
            Event::HostageKilled { .. } => hud.center = Some(("Eine Geisel wurde getötet".into(), String::new(), RED, now + 2.0)),
            Event::Halftime => hud.center = Some(("Halbzeit".into(), "Seitenwechsel".into(), AMBER, now + 4.0)),
            Event::RoundEnd { winner, reason, mvp } => {
                let title = match winner {
                    Some(Team::Ksk) => "KSK gewinnt die Runde",
                    Some(Team::Opfor) => "OPFOR gewinnt die Runde",
                    None => "Unentschieden",
                };
                let why = match reason {
                    Reason::Elimination => "Alle Gegner ausgeschaltet",
                    Reason::BombExploded => "Das Ziel wurde zerstört",
                    Reason::BombDefused => "Die Sprengladung wurde entschärft",
                    Reason::TimeRanOut => "Die Zeit ist abgelaufen",
                    Reason::HostagesRescued => "Die Geiseln sind gerettet",
                    Reason::Draw => "",
                };
                let mvp = mvp.map(|m| format!(" · Bester Spieler: {}", s.game.players[m].name)).unwrap_or_default();
                hud.center = Some((title.into(), format!("{why}{mvp}"), winner.map_or(Color::WHITE, team_color), now + 5.0));
            }
            _ => {}
        }
    }
    hud.kills.retain(|k| now - k.at < if k.mine { 10.0 } else { 7.0 });
    if hud.kills.len() > 6 {
        let drop = hud.kills.len() - 6;
        hud.kills.drain(..drop);
        hud.kills_dirty = true;
    }
    let before = hud.chat.len();
    hud.chat.retain(|c| now - c.2 < 9.0);
    if hud.chat.len() > 6 {
        let drop = hud.chat.len() - 6;
        hud.chat.drain(..drop);
    }
    if hud.chat.len() != before {
        hud.chat_dirty = true;
    }
    hud.damage.retain(|d| now - d.1 < 1.6);
}

fn fmt_time(t: f32) -> String {
    let t = t.max(0.0).ceil() as i32;
    format!("{}:{:02}", t / 60, t % 60)
}

#[allow(clippy::type_complexity)]
fn vitals(
    session: Option<Res<Session>>,
    spectate: Res<crate::view::Spectate>,
    hud: Res<HudState>,
    mut texts: Query<(&Txt, &mut Text, &mut TextColor)>,
    mut helmet: Query<&mut Visibility, (With<HelmetIcon>, Without<AmmoBox>, Without<KitIcon>)>,
    mut ammo: Query<&mut Visibility, (With<AmmoBox>, Without<HelmetIcon>, Without<KitIcon>)>,
    mut kit: Query<&mut Visibility, (With<KitIcon>, Without<HelmetIcon>, Without<AmmoBox>, Without<BombCarrierIcon>)>,
    mut bomb: Query<&mut Visibility, (With<BombCarrierIcon>, Without<HelmetIcon>, Without<AmmoBox>, Without<KitIcon>)>,
) {
    let Some(s) = session else { return };
    let viewed = s.viewed(spectate.target);
    let p = &s.game.players[viewed];
    let gun = p.inv.gun();
    for (k, mut t, mut c) in &mut texts {
        match k {
            Txt::Health => {
                set(&mut t, &p.health.max(0).to_string());
                c.0 = if p.health <= 25 { RED } else { AMBER };
            }
            Txt::Armor => set(&mut t, &p.armor.to_string()),
            Txt::Money => set(&mut t, &s.me().money.to_string()),
            Txt::MoneyDelta => match hud.money_delta.filter(|(_, at)| hud.time - at < 2.5) {
                Some((a, _)) => {
                    set(&mut t, &if a > 0 { format!("+${a}") } else { format!("-${}", -a) });
                    c.0 = if a > 0 { Color::srgb(0.5, 1.0, 0.5) } else { RED };
                }
                None => set(&mut t, ""),
            },
            Txt::Ammo => set(&mut t, &gun.map_or(String::new(), |g| g.clip.to_string())),
            Txt::Reserve => set(&mut t, &gun.map_or(String::new(), |g| g.reserve.to_string())),
            Txt::Weapon => {
                let name = match p.inv.def().class {
                    Class::Grenade => format!("{} ×{}", p.inv.active.name(), match p.inv.active {
                        zugriff_sim::weapons::WeaponId::He => p.inv.he,
                        zugriff_sim::weapons::WeaponId::Flash => p.inv.flash,
                        _ => p.inv.smoke,
                    }),
                    _ => p.inv.active.name().to_string(),
                };
                set(&mut t, &name);
            }
            _ => {}
        }
    }
    for mut v in &mut helmet {
        *v = if p.helmet { Visibility::Inherited } else { Visibility::Hidden };
    }
    for mut v in &mut ammo {
        *v = if gun.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    }
    for mut v in &mut kit {
        *v = if p.defuser && p.alive { Visibility::Inherited } else { Visibility::Hidden };
    }
    for mut v in &mut bomb {
        *v = if p.inv.bomb && p.alive { Visibility::Inherited } else { Visibility::Hidden };
    }
}

#[allow(clippy::type_complexity)]
fn top(
    mut commands: Commands,
    session: Option<Res<Session>>,
    fonts: Res<Fonts>,
    time: Res<Time>,
    mut texts: Query<(&Txt, &mut Text, &mut TextColor)>,
    mut timer_plate: Query<&mut BackgroundColor, (With<TimerPlate>, Without<PhasePlate>)>,
    mut phase_plate: Query<&mut Visibility, With<PhasePlate>>,
    dots: Query<(Entity, &AliveDots, Option<&Children>)>,
    mut last: Local<Vec<(bool, Team)>>,
) {
    let Some(s) = session else { return };
    let g = &s.game;
    let planted = matches!(g.bomb, Bomb::Planted { .. });
    let (timer, phase_text) = match g.phase {
        Phase::Warmup => (fmt_time(g.phase_left), format!("Aufwärmphase – Spielbeginn in {}", fmt_time(g.phase_left))),
        Phase::Freeze => (fmt_time(g.phase_left), "Kaufzeit".to_string()),
        Phase::Live if g.cfg.mode == Mode::Deathmatch => (fmt_time(g.phase_left), String::new()),
        Phase::Live => {
            let buy_left = g.cfg.buy_time - g.live_for;
            let t = if planted { String::new() } else { fmt_time(g.phase_left) };
            (t, if buy_left > 0.0 && g.in_buyzone(s.me()) && s.me().alive { format!("Kaufzeit {}", fmt_time(buy_left)) } else { String::new() })
        }
        Phase::Over => (String::new(), String::new()),
        Phase::MatchOver => (String::new(), String::new()),
    };
    for (k, mut t, mut c) in &mut texts {
        match k {
            Txt::Timer => {
                if planted {
                    set(&mut t, "BOMBE");
                    let pulse = (time.elapsed_secs() * 6.0).sin() * 0.5 + 0.5;
                    c.0 = Color::srgb(1.0, 0.2 + 0.3 * pulse, 0.15);
                } else {
                    set(&mut t, &timer);
                    c.0 = if g.phase == Phase::Live && g.phase_left < 10.0 { RED } else { Color::WHITE };
                }
            }
            Txt::ScoreKsk => set(&mut t, &if g.cfg.mode == Mode::Deathmatch { g.players.iter().filter(|p| p.team == Team::Ksk).map(|p| p.stats.kills).sum::<i32>().to_string() } else { g.score[0].to_string() }),
            Txt::ScoreOpfor => set(&mut t, &if g.cfg.mode == Mode::Deathmatch { g.players.iter().filter(|p| p.team == Team::Opfor).map(|p| p.stats.kills).sum::<i32>().to_string() } else { g.score[1].to_string() }),
            Txt::Phase => set(&mut t, &phase_text),
            _ => {}
        }
    }
    for mut bg in &mut timer_plate {
        bg.0 = if planted { Color::srgba(0.35, 0.0, 0.0, 0.55) } else { PLATE };
    }
    for mut v in &mut phase_plate {
        *v = if phase_text.is_empty() { Visibility::Hidden } else { Visibility::Inherited };
    }
    // Alive markers: one per player, dimmed when dead.
    let now: Vec<(bool, Team)> = g.players.iter().map(|p| (p.alive, p.team)).collect();
    if *last == now {
        return;
    }
    *last = now;
    for (e, d, children) in &dots {
        if let Some(ch) = children {
            for c in ch.iter() {
                commands.entity(c).despawn();
            }
        }
        let mut list: Vec<bool> = g.players.iter().filter(|p| p.team == d.0).map(|p| p.alive).collect();
        list.sort_by_key(|a| !a);
        if d.0 == Team::Ksk {
            list.reverse();
        }
        commands.entity(e).with_children(|c| {
            for alive in list {
                let col = team_color(d.0).with_alpha(if alive { 0.95 } else { 0.2 });
                c.spawn((Node { width: Val::Px(12.0), height: Val::Px(22.0), border_radius: BorderRadius::all(Val::Px(3.0)), ..default() }, BackgroundColor(col)));
            }
        });
    }
    let _ = &fonts;
}

#[allow(clippy::too_many_arguments)]
fn crosshair(
    session: Option<Res<Session>>,
    spectate: Res<crate::view::Spectate>,
    options: Res<Options>,
    ui: Res<Ui>,
    (time, fixed, ui_scale, window): (Res<Time>, Res<Time<Fixed>>, Res<UiScale>, Single<&Window, With<bevy::window::PrimaryWindow>>),
    mut open_px: Local<f32>,
    mut root: Single<&mut Visibility, With<CrossRoot>>,
    mut bars: Query<(&CrossBar, &mut Node, &mut BackgroundColor)>,
) {
    let Some(s) = session else { return };
    let viewed = s.viewed(spectate.target);
    let p = &s.game.players[viewed];
    // The spread and the raise come from the sim's ticks; interpolated between them (and the spread eased on top), the
    // lines move smoothly instead of stepping at the tick rate.
    let snap = s.lerp(viewed, fixed.overstep_fraction());
    // Down the sights the arms fade out and the dot stays: it sits on the front sight's tip, where the shots go. A gun
    // whose sights can't be brought up keeps the whole crosshair.
    let fade = if crate::viewmodel::has_sights(p.inv.active) { 1.0 - ((snap.ads - 0.15) / 0.35).clamp(0.0, 1.0) } else { 1.0 };
    let hide = !p.alive || p.inv.zoom > 0 || crate::gear::in_optic(&p.inv) || p.inv.def().class == Class::Sniper || ui.buy;
    root.set_if_neq(if hide { Visibility::Hidden } else { Visibility::Inherited });
    if hide {
        return;
    }
    // The gap in pixels at 1080p: the spread's angle over the vertical field of view.
    let fov = crate::view::vertical_fov(options.fov);
    let open = if options.crosshair_dynamic { ((snap.spread.atan() / (fov * 0.5)) * 540.0).min(120.0) } else { 0.0 };
    *open_px += (open - *open_px) * (1.0 - (-time.delta_secs() * 30.0).exp());
    // Whole physical pixels, so the arms don't change length by a pixel as the gap moves.
    let k = (ui_scale.0 * window.scale_factor()).max(1e-3);
    let px = |v: f32| (v * k).round().max(1.0) / k;
    let gap = px(options.crosshair_gap + *open_px);
    let (len, th) = (px(options.crosshair_size), px(options.crosshair_thickness));
    let color = CROSSHAIR_COLORS[(options.crosshair_color as usize).min(CROSSHAIR_COLORS.len() - 1)].1;
    let arms = color.with_alpha(color.alpha() * fade);
    for (b, mut n, mut c) in &mut bars {
        c.set_if_neq(BackgroundColor(if b.0 == 4 { color } else { arms }));
        let (l, t, w, h) = match b.0 {
            0 => (-gap - len, -(th / 2.0).round(), len, th),
            1 => (gap, -(th / 2.0).round(), len, th),
            2 => (-(th / 2.0).round(), -gap - len, th, len),
            3 => (-(th / 2.0).round(), gap, th, len),
            _ => (-1.0, -1.0, 2.0, 2.0),
        };
        let (l, t, w, h) = (Val::Px(l), Val::Px(t), Val::Px(w), Val::Px(h));
        if n.left != l || n.top != t || n.width != w || n.height != h {
            n.left = l;
            n.top = t;
            n.width = w;
            n.height = h;
        }
    }
}

fn feed(mut commands: Commands, mut hud: ResMut<HudState>, fonts: Res<Fonts>, feed: Single<(Entity, Option<&Children>), With<KillFeed>>) {
    if !hud.kills_dirty {
        return;
    }
    hud.kills_dirty = false;
    let (e, children) = feed.into_inner();
    if let Some(ch) = children {
        for c in ch.iter() {
            commands.entity(c).despawn();
        }
    }
    commands.entity(e).with_children(|c| {
        for k in &hud.kills {
            let bg = if k.mine { Color::srgba(0.35, 0.05, 0.03, 0.6) } else { Color::srgba(0.0, 0.0, 0.0, 0.45) };
            c.spawn((
                Node { column_gap: Val::Px(8.0), padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)), align_items: AlignItems::Center, border_radius: BorderRadius::all(Val::Px(5.0)), border: UiRect::all(Val::Px(if k.mine { 1.0 } else { 0.0 })), ..default() },
                BackgroundColor(bg),
                BorderColor::all(Color::srgba(1.0, 0.3, 0.2, 0.8)),
            ))
            .with_children(|row| {
                if let Some((name, team)) = &k.killer {
                    row.spawn(text(name, &fonts.label, 24.0, team_color(*team)));
                }
                row.spawn(text(&k.weapon, &fonts.bold, 20.0, Color::srgba(1.0, 1.0, 1.0, 0.9)));
                if k.wallbang {
                    row.spawn((Node { width: Val::Px(14.0), height: Val::Px(16.0), border: UiRect::all(Val::Px(2.0)), ..default() }, BorderColor::all(Color::WHITE)));
                }
                if k.headshot {
                    row.spawn((
                        Node { width: Val::Px(16.0), height: Val::Px(16.0), border: UiRect::all(Val::Px(2.0)), border_radius: BorderRadius::all(Val::Px(8.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                        BorderColor::all(Color::WHITE),
                        children![(Node { width: Val::Px(4.0), height: Val::Px(4.0), border_radius: BorderRadius::all(Val::Px(2.0)), ..default() }, BackgroundColor(Color::WHITE))],
                    ));
                }
                row.spawn(text(&k.victim.0, &fonts.label, 24.0, team_color(k.victim.1)));
            });
        }
    });
}

fn chat(mut commands: Commands, mut hud: ResMut<HudState>, fonts: Res<Fonts>, feed: Single<(Entity, Option<&Children>), With<ChatFeed>>) {
    if !hud.chat_dirty {
        return;
    }
    hud.chat_dirty = false;
    let (e, children) = feed.into_inner();
    if let Some(ch) = children {
        for c in ch.iter() {
            commands.entity(c).despawn();
        }
    }
    commands.entity(e).with_children(|c| {
        for (line, color, _) in &hud.chat {
            c.spawn((text(line, &fonts.label, 22.0, *color), TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.55) }));
        }
    });
}

fn center(hud: Res<HudState>, mut vis: Single<&mut Visibility, With<CenterBox>>, mut texts: Query<(&Txt, &mut Text, &mut TextColor)>) {
    let active = hud.center.as_ref().filter(|c| hud.time < c.3);
    **vis = if active.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    let Some((title, sub, color, until)) = active else { return };
    let fade = ((until - hud.time) / 0.5).clamp(0.0, 1.0);
    for (k, mut t, mut c) in &mut texts {
        match k {
            Txt::Center => {
                set(&mut t, title);
                c.0 = color.with_alpha(fade);
            }
            Txt::CenterSub => {
                set(&mut t, sub);
                c.0 = Color::srgba(1.0, 1.0, 1.0, 0.9 * fade);
            }
            _ => {}
        }
    }
}

/// What can be done here: defuse, plant, lead a hostage, pick a weapon up, buy.
fn hints(session: Option<Res<Session>>, ui: Res<Ui>, look: Res<crate::view::Look>, mut vis: Single<&mut Visibility, With<HintBox>>, mut texts: Query<(&Txt, &mut Text)>) {
    let Some(s) = session else { return };
    let g = &s.game;
    let p = s.me();
    let mut hint = String::new();
    if !look.locked && !ui.menu && !p.bot {
        hint = "Ins Bild klicken, um zu spielen".into();
    } else if p.alive && !ui.buy {
        let eye = g.eye(s.me);
        let look = dir_from(p.yaw, p.pitch);
        if let Bomb::Planted { pos, defuser, .. } = g.bomb
            && p.team == Team::Ksk
            && eye.distance(pos) < 2.0
            && defuser.is_none()
        {
            hint = if p.defuser { "[E] halten: Entschärfen (5 s)".into() } else { "[E] halten: Entschärfen (10 s) – ohne Entschärferset".into() };
        } else if p.inv.bomb && g.site_at(p.mover.origin).is_some() && g.phase == Phase::Live {
            hint = if p.inv.active == zugriff_sim::weapons::WeaponId::Bomb { "Linke Maustaste halten: Sprengladung legen".into() } else { "[5] Sprengladung nehmen".into() };
        } else if p.team == Team::Ksk
            && let Some(h) = g.hostages.iter().find(|h| h.alive && !h.rescued && (h.mover.origin + Vec3::Y).distance(eye) < 2.2 && ((h.mover.origin + Vec3::Y) - eye).normalize_or_zero().dot(look) > 0.6)
        {
            hint = if h.leader == Some(s.me) { "[E] Geisel warten lassen".into() } else { "[E] Geisel führen".into() };
        } else if let Some(d) = g.dropped.iter().find(|d| d.pos.distance(eye) < 2.0 && (d.pos - eye).normalize_or_zero().dot(look) > 0.8) {
            hint = format!("[E] Aufnehmen: {}", d.gun.id.name());
        } else if g.can_buy(p) && matches!(g.phase, Phase::Freeze) {
            hint = "[B] Kaufmenü".into();
        }
    }
    **vis = if hint.is_empty() { Visibility::Hidden } else { Visibility::Inherited };
    for (k, mut t) in &mut texts {
        if *k == Txt::Hint {
            set(&mut t, &hint);
        }
    }
}

/// Plant/defuse bar: for the player we look through, else for a teammate at the bomb, so the
/// status stays readable while spectating or covering the defuser.
#[allow(clippy::type_complexity)]
fn progress(
    session: Option<Res<Session>>,
    spectate: Res<crate::view::Spectate>,
    mut vis: Single<&mut Visibility, With<ProgressBox>>,
    mut fill: Single<(&mut Node, &mut BackgroundColor), With<ProgressFill>>,
    mut texts: Query<(&Txt, &mut Text)>,
) {
    let Some(s) = session else { return };
    let g = &s.game;
    let viewed = s.viewed(spectate.target);
    let my_team = s.me().team;
    let who = |id: usize| if id == viewed { String::new() } else { format!("{} ", g.players[id].name) };
    let planter = std::iter::once(viewed)
        .chain(0..g.players.len())
        .find(|&i| g.players[i].alive && (i == viewed || g.players[i].team == my_team) && matches!(g.players[i].inv.action, Action::Planting(_)));
    let defusing = match g.bomb {
        Bomb::Planted { defuser: Some(d), left, .. } if g.players[d].alive && (d == viewed || g.players[d].team == my_team) => Some((d, left)),
        _ => None,
    };
    let (label, k, color) = if let Some(i) = planter
        && let Action::Planting(t) = g.players[i].inv.action
    {
        let lead = if i == viewed { "Sprengladung wird gelegt …".to_string() } else { format!("{}legt die Sprengladung …", who(i)) };
        (lead, t / g.cfg.plant_time, OPFOR)
    } else if let Some((d, left)) = defusing {
        let p = &g.players[d];
        let done = p.defusing.unwrap_or(0.0);
        let total = if p.defuser { g.cfg.defuse_kit_time } else { g.cfg.defuse_time };
        let rest = (total - done).max(0.0);
        let kit = if p.defuser { "" } else { " (ohne Set)" };
        let lead = if d == viewed { "Entschärfe".to_string() } else { format!("{}entschärft", who(d)) };
        // Red when the charge goes off before the defuse can finish.
        let color = if rest > left { RED } else { KSK };
        (format!("{lead}{kit} … {rest:.1} s"), done / total, color)
    } else {
        (String::new(), -1.0, AMBER)
    };
    **vis = if k >= 0.0 { Visibility::Inherited } else { Visibility::Hidden };
    let (node, bg) = &mut *fill;
    node.width = Val::Percent(k.clamp(0.0, 1.0) * 100.0);
    bg.0 = color;
    for (kk, mut t) in &mut texts {
        if *kk == Txt::Progress {
            set(&mut t, &label);
        }
    }
}

#[allow(clippy::type_complexity)]
fn overlays(
    session: Option<Res<Session>>,
    spectate: Res<crate::view::Spectate>,
    mut flash: Single<&mut BackgroundColor, (With<FlashOverlay>, Without<Vignette>)>,
    mut vignette: Single<&mut BackgroundColor, (With<Vignette>, Without<FlashOverlay>)>,
    mut scope: Single<&mut Visibility, With<ScopeOverlay>>,
) {
    let Some(s) = session else { return };
    let viewed = s.viewed(spectate.target);
    let p = &s.game.players[viewed];
    let a = if p.flash_total > 0.0 && p.alive { (p.flash_left / (p.flash_total * 0.55)).clamp(0.0, 1.0) } else { 0.0 };
    flash.0 = Color::srgba(1.0, 1.0, 1.0, a.powf(0.8));
    // (Not while scoped: over the scope's black it turned the whole screen red.)
    let low = if p.alive && p.health < 30 && p.inv.zoom == 0 && !crate::gear::in_optic(&p.inv) { (30 - p.health) as f32 / 30.0 * 0.25 } else { 0.0 };
    vignette.0 = Color::srgba(0.6, 0.0, 0.0, low);
    **scope = if p.alive && (p.inv.zoom > 0 || crate::gear::in_optic(&p.inv)) { Visibility::Inherited } else { Visibility::Hidden };
}

/// Builds the radar picture when a new map is in.
fn radar_map(current: Res<Current>, mut hud: ResMut<HudState>) {
    if hud.radar_gen == current.generation {
        return;
    }
    let Some(nav) = current.nav.clone() else { return };
    hud.radar_gen = current.generation;
    if nav.nodes.is_empty() {
        hud.radar = None;
        return;
    }
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    let (mut ylo, mut yhi) = (f32::MAX, f32::MIN);
    for n in &nav.nodes {
        lo = lo.min(Vec2::new(n.pos.x, n.pos.z));
        hi = hi.max(Vec2::new(n.pos.x, n.pos.z));
        ylo = ylo.min(n.pos.y);
        yhi = yhi.max(n.pos.y);
    }
    let ppm = 2.0;
    let origin = lo - Vec2::splat(3.0);
    let size = ((hi - lo + Vec2::splat(6.0)) * ppm).ceil();
    let (w, h) = (size.x as usize, size.y as usize);
    let mut height = vec![f32::MIN; w * h];
    for n in &nav.nodes {
        let cx = ((n.pos.x - origin.x) * ppm) as i32;
        let cz = ((n.pos.z - origin.y) * ppm) as i32;
        for dz in -1..=1 {
            for dx in -1..=1 {
                let (x, z) = (cx + dx, cz + dz);
                if x >= 0 && z >= 0 && (x as usize) < w && (z as usize) < h {
                    let i = z as usize * w + x as usize;
                    height[i] = height[i].max(n.pos.y);
                }
            }
        }
    }
    let span = (yhi - ylo).max(1.0);
    let mut pixels = vec![[0u8; 4]; w * h];
    for z in 0..h {
        for x in 0..w {
            let i = z * w + x;
            if height[i] > f32::MIN {
                let k = ((height[i] - ylo) / span).clamp(0.0, 1.0);
                let c = 78.0 + 95.0 * k;
                pixels[i] = [(c * 0.92) as u8, (c * 0.98) as u8, c as u8, 255];
            } else {
                // A wall where empty meets floor.
                let edge = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dz)| {
                    let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                    nx >= 0 && nz >= 0 && (nx as usize) < w && (nz as usize) < h && height[nz as usize * w + nx as usize] > f32::MIN
                });
                if edge {
                    pixels[i] = [22, 24, 26, 255];
                }
            }
        }
    }
    hud.radar = Some(RadarMap { pixels, w, h, origin, ppm });
}

/// Draws the radar: the map around the player turned with the view, team-mates, enemies seen or heard just now, the
/// bomb and the hostages, the player's arrow in the middle, inside a round frame.
#[allow(clippy::too_many_arguments)]
fn radar(
    session: Option<Res<Session>>,
    spectate: Res<crate::view::Spectate>,
    hud: Res<HudState>,
    time: Res<Time>,
    node: Single<&ImageNode, With<RadarImage>>,
    mut images: ResMut<Assets<Image>>,
    mut texts: Query<(&Txt, &mut Text)>,
    mut since: Local<f32>,
) {
    let (Some(s), Some(map)) = (session, hud.radar.as_ref()) else { return };
    *since += time.delta_secs();
    if *since < 1.0 / 30.0 {
        return;
    }
    *since = 0.0;
    let viewed = s.viewed(spectate.target);
    let me = &s.game.players[viewed];
    let n = RADAR_PX;
    let half = n as f32 / 2.0;
    // Metres per radar pixel.
    let mpp = RADAR_RANGE * 2.0 / n as f32;
    let (sin, cos) = me.yaw.sin_cos();
    let at = Vec2::new(me.mover.origin.x, me.mover.origin.z);
    // Radar pixel (right, down) → world offset: forward is up.
    let fwd = Vec2::new(-sin, -cos);
    let right = Vec2::new(cos, -sin);
    let to_px = |w: Vec3| -> Vec2 {
        let d = Vec2::new(w.x, w.z) - at;
        Vec2::new(d.dot(right), -d.dot(fwd)) / mpp + Vec2::splat(half)
    };
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (u, v) = (x as f32 + 0.5 - half, y as f32 + 0.5 - half);
            let r = (u * u + v * v).sqrt() / half;
            let o = (y * n + x) * 4;
            if r > 1.0 {
                continue;
            }
            let rim = r > 0.965;
            let world = at + (right * u - fwd * v) * mpp;
            let p = (world - map.origin) * map.ppm;
            let (px, pz) = (p.x as i32, p.y as i32);
            let c = if px >= 0 && pz >= 0 && (px as usize) < map.w && (pz as usize) < map.h { map.pixels[pz as usize * map.w + px as usize] } else { [0, 0, 0, 0] };
            let (cr, cg, cb, ca) = if rim {
                (200, 200, 205, 200)
            } else if c[3] > 0 {
                (c[0], c[1], c[2], 225)
            } else {
                (12, 15, 17, 170)
            };
            out[o] = cr;
            out[o + 1] = cg;
            out[o + 2] = cb;
            out[o + 3] = ca;
        }
    }
    let mut dot = |p: Vec2, radius: f32, c: [u8; 3], clamp: bool| {
        let mut q = p - Vec2::splat(half);
        let lim = half - radius - 3.0;
        if q.length() > lim {
            if !clamp {
                return;
            }
            q = q.normalize() * lim;
        }
        let q = q + Vec2::splat(half);
        let r2 = radius * radius;
        for y in (q.y - radius).floor() as i32..=(q.y + radius).ceil() as i32 {
            for x in (q.x - radius).floor() as i32..=(q.x + radius).ceil() as i32 {
                if x < 0 || y < 0 || x as usize >= n || y as usize >= n {
                    continue;
                }
                let d2 = (x as f32 + 0.5 - q.x).powi(2) + (y as f32 + 0.5 - q.y).powi(2);
                if d2 <= r2 {
                    let o = (y as usize * n + x as usize) * 4;
                    let edge = d2 > (radius - 1.2).max(0.0).powi(2);
                    let k = if edge { 0.35 } else { 1.0 };
                    out[o] = (c[0] as f32 * k) as u8;
                    out[o + 1] = (c[1] as f32 * k) as u8;
                    out[o + 2] = (c[2] as f32 * k) as u8;
                    out[o + 3] = 255;
                }
            }
        }
    };
    let rgb = |c: Color| {
        let s = c.to_srgba();
        [(s.red * 255.0) as u8, (s.green * 255.0) as u8, (s.blue * 255.0) as u8]
    };
    let eye = s.game.eye(viewed);
    for h in s.game.hostages.iter().filter(|h| h.alive && !h.rescued) {
        dot(to_px(h.mover.origin), 3.5, [240, 240, 240], false);
    }
    for p in &s.game.players {
        if p.id == viewed || !p.alive {
            continue;
        }
        if p.team == me.team {
            dot(to_px(p.mover.origin), 4.0, rgb(team_color(p.team)), true);
        } else {
            let head = p.hitboxes().head();
            let seen = (head - eye).normalize_or_zero().dot(dir_from(me.yaw, me.pitch)) > 0.5 && s.game.clear_line(eye, head, true);
            if seen || (p.noise < 0.8 && p.mover.origin.distance(me.mover.origin) < 30.0) {
                dot(to_px(p.mover.origin), 4.0, [255, 70, 55], false);
            }
        }
    }
    let blink = (time.elapsed_secs() * 4.0).sin() > 0.0;
    match s.game.bomb {
        Bomb::Planted { pos, .. } => dot(to_px(pos), 5.0, if blink { [255, 60, 40] } else { [255, 176, 0] }, true),
        Bomb::Dropped { pos, .. } if me.team == Team::Opfor => dot(to_px(pos), 4.5, [255, 176, 0], true),
        _ => {}
    }
    // The player: a white arrow pointing up.
    for y in 0..14 {
        let w = (y as f32 * 0.45) as i32;
        for x in -w..=w {
            let (px, py) = (half as i32 + x, half as i32 - 7 + y);
            let o = (py as usize * n + px as usize) * 4;
            out[o..o + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    if let Some(mut img) = images.get_mut(&node.image) {
        img.data = Some(out);
    }
    let place = s.game.place_at(me.mover.origin).unwrap_or("").to_string();
    for (k, mut t) in &mut texts {
        if *k == Txt::Place {
            set(&mut t, &place);
        }
    }
}

fn damage_arrows(session: Option<Res<Session>>, hud: Res<HudState>, look: Res<crate::view::Look>, mut arrows: Query<(&DamageArrow, &mut UiTransform, &mut Visibility)>) {
    let Some(s) = session else { return };
    let me = s.me();
    for (a, mut tf, mut vis) in &mut arrows {
        let Some(&(from, at)) = hud.damage.iter().rev().nth(a.0) else {
            *vis = Visibility::Hidden;
            continue;
        };
        if !me.alive {
            *vis = Visibility::Hidden;
            continue;
        }
        let d = from - me.mover.origin;
        let yaw_to = (-d.x).atan2(-d.z);
        let rel = crate::session::wrap(look.yaw - yaw_to);
        tf.rotation = Rot2::radians(rel);
        let _ = at;
        *vis = Visibility::Inherited;
    }
}

fn death_panel(session: Option<Res<Session>>, hud: Res<HudState>, spectate: Res<crate::view::Spectate>, mut death: Single<&mut Visibility, (With<DeathBox>, Without<SpecBox>)>, mut spec: Single<&mut Visibility, (With<SpecBox>, Without<DeathBox>)>, mut texts: Query<(&Txt, &mut Text)>) {
    let Some(s) = session else { return };
    let dead = !s.me().alive;
    let watching = spectate.target.filter(|_| dead);
    **death = if dead && hud.killed_by.is_some() && spectate.dead_for < 6.0 { Visibility::Inherited } else { Visibility::Hidden };
    **spec = if watching.is_some() || (dead && s.game.cfg.mode != Mode::Deathmatch && spectate.dead_for > 2.5) { Visibility::Inherited } else { Visibility::Hidden };
    for (k, mut t) in &mut texts {
        match k {
            Txt::Death => {
                if let Some((who, weapon, hs, hp)) = &hud.killed_by {
                    let hs = if *hs { ", Kopftreffer" } else { "" };
                    let hp = if *hp > 0 { format!(" – {who} hat noch {hp} HP") } else { String::new() };
                    set(&mut t, &format!("Getötet von {who} ({weapon}{hs}){hp}"));
                }
            }
            Txt::Spectating => {
                let line = match watching {
                    Some(w) => format!("Beobachte: {}  ·  Maustasten wechseln", s.game.players[w].name),
                    None if s.game.cfg.mode == Mode::Deathmatch || s.game.phase == Phase::Warmup => "Wiedereinstieg gleich …".to_string(),
                    None => "Warte auf die nächste Runde …".to_string(),
                };
                set(&mut t, &line);
            }
            _ => {}
        }
    }
}
