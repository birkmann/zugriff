//! The quick performance test: finds the best graphics preset this machine holds at the display's refresh rate.
//! Over the menu's map the camera walks the same route at eye height for each preset tried (through the map's callouts,
//! along the navigation graph, looking about as a player does), after a warm-up walk that loads what is seen along it:
//! each preset settles until its frames are calm (grass and litter grown again, new shaders compiled; the web takes
//! seconds), then four and a half seconds are measured, without V-Sync or the frame limit. It starts at "high" and
//! steps up while a preset holds, down while it doesn't: two or three presets, under twenty seconds.
//! A preset holds when 95 % of its frames are within the refresh interval and its 1 % low stays above 85 % of the rate.
//!
//! Run from the graphics menu, once by itself on a first start (natively), or with `--bench` (`--bench all` measures every preset,
//! `--bench-exit` quits when done, printing the results).

use crate::graphics::{Graphics, PRESET_NAMES};
use crate::hud::{text, Fonts, AMBER};
use crate::level::Current;
use crate::menu::{MenuState, Screen};
use crate::options::Options;
use crate::session::Session;
use crate::view::PlayerCamera;
use bevy::prelude::*;
use bevy::transform::TransformSystems;

pub struct BenchPlugin;

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bench>()
            .add_systems(Update, ((start, measure).chain(), census))
            .add_systems(PostUpdate, fly.after(crate::view::place_camera).before(TransformSystems::Propagate));
    }
}

/// Seconds each preset settles at least (and until its frames are calm for a second, at most ten), then is measured,
/// and the walk's pace (m/s). Before the first, a warm-up walk over the measured stretch, one and a half times as fast,
/// loads what is seen along it.
const SETTLE: f32 = 1.0;
const SETTLE_MAX: f32 = 10.0;
const MEASURE: f32 = 4.5;
const PACE: f32 = 5.5;
const WARM_PACE: f32 = 1.5;

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Warm,
    Settle,
    Measure,
}

#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub avg_fps: f32,
    pub low_fps: f32,
    pub p95_ms: f32,
    pub holds: bool,
}

#[derive(Clone, Debug)]
pub struct Summary {
    /// The preset chosen, the rate aimed for, and each preset tried.
    pub chosen: u8,
    pub target: f32,
    pub tried: Vec<(u8, Stats)>,
}

struct Run {
    route: Vec<Vec3>,
    level: u8,
    phase: Phase,
    /// Seconds into the phase, and how long the frames have been calm (none over three frame budgets).
    t: f32,
    calm: f32,
    frames: Vec<f32>,
    tried: Vec<(u8, Stats)>,
    target: f32,
    all: bool,
    /// What to put back: the settings, V-Sync, the menu's page.
    saved: (Graphics, bool, Screen),
    banner: Entity,
}

#[derive(Resource, Default)]
pub struct Bench {
    run: Option<Run>,
    /// Asked for (the menu's button, `--bench`, a first start).
    pub request: bool,
    pub summary: Option<Summary>,
    /// Changes with every step (the menu shows the progress and the result).
    pub version: u32,
}

impl Bench {
    pub fn running(&self) -> bool {
        self.run.is_some()
    }
}

/// Launched for a check, a screenshot or an online test (then a first start doesn't test by itself).
fn scripted() -> bool {
    ["play", "map", "pose", "shot", "give", "autoplay", "menu", "join", "create", "say", "scoreboard"].iter().any(|k| crate::args::get(k).is_some() || crate::args::flag(k))
}

/// The walk: the map's callouts, nearest first from the first, joined along the navigation graph (eye height).
fn route(current: &Current) -> Vec<Vec3> {
    let Some(nav) = current.nav.as_ref() else { return Vec::new() };
    let mut seen = std::collections::HashSet::new();
    let firsts: Vec<u32> = (0..nav.nodes.len() as u32).filter(|&i| !nav.place(i).is_empty() && seen.insert(nav.place(i).to_string())).collect();
    let Some(&first) = firsts.first() else { return Vec::new() };
    let mut order = vec![first];
    let mut left: Vec<u32> = firsts[1..].to_vec();
    while let Some(&at) = order.last()
        && !left.is_empty()
        && order.len() < 7
    {
        let p = nav.pos(at);
        let (i, _) = left.iter().enumerate().min_by(|a, b| nav.pos(*a.1).distance(p).total_cmp(&nav.pos(*b.1).distance(p))).unwrap();
        order.push(left.swap_remove(i));
    }
    let mut path: Vec<Vec3> = Vec::new();
    for w in order.windows(2) {
        let Some(nodes) = nav.route(w[0], w[1], &|_| 0.0) else { continue };
        for n in nodes {
            let p = nav.pos(n) + Vec3::Y * 1.62;
            if path.last().is_none_or(|l| l.distance(p) > 0.5) {
                path.push(p);
            }
        }
    }
    path
}

/// The point `s` metres along a path, and a little way ahead (for the look).
fn along(path: &[Vec3], s: f32) -> Vec3 {
    let mut left = s;
    for w in path.windows(2) {
        let d = w[0].distance(w[1]);
        if left <= d {
            return w[0].lerp(w[1], left / d.max(1e-4));
        }
        left -= d;
    }
    *path.last().unwrap_or(&Vec3::ZERO)
}

fn stats(frames: &mut [f32], target: f32) -> Stats {
    let n = frames.len().max(1);
    let sum: f32 = frames.iter().sum();
    frames.sort_by(|a, b| a.total_cmp(b));
    let p95_ms = frames.get(((n as f32 * 0.95) as usize).min(n - 1)).copied().unwrap_or(0.0);
    let worst = (n / 100).max(1);
    let low_ms = frames.iter().rev().take(worst).sum::<f32>() / worst as f32;
    let (avg_fps, low_fps) = (1000.0 * n as f32 / sum.max(1e-3), 1000.0 / low_ms.max(1e-3));
    Stats { avg_fps, low_fps, p95_ms, holds: p95_ms <= 1000.0 / target && low_fps >= target * 0.85 }
}

/// The rate to hold: the display's refresh (60 where it isn't known).
fn target(monitors: &Query<(&bevy::window::Monitor, Has<bevy::window::PrimaryMonitor>)>) -> f32 {
    let m = monitors.iter().find(|(_, p)| *p).or_else(|| monitors.iter().next()).map(|(m, _)| m);
    // (A browser shows no faster than the display, which a page can't ask: 45 holds on a 50 or a 60 Hz one.)
    if cfg!(target_arch = "wasm32") {
        return 45.0;
    }
    m.and_then(|m| m.refresh_rate_millihertz).map_or(60.0, |mhz| (mhz as f32 / 1000.0).round().clamp(30.0, 360.0))
}

#[allow(clippy::too_many_arguments)]
fn start(
    mut commands: Commands,
    mut bench: ResMut<Bench>,
    mut options: ResMut<Options>,
    mut menu: ResMut<MenuState>,
    current: Res<Current>,
    session: Option<Res<Session>>,
    fonts: Res<Fonts>,
    monitors: Query<(&bevy::window::Monitor, Has<bevy::window::PrimaryMonitor>)>,
    time: Res<Time<Real>>,
    mut asked: Local<bool>,
) {
    if bench.running() {
        return;
    }
    // `--bench`, or a first start (once the menu's map is in and has been on show for a moment).
    if !*asked && current.spawned && session.is_none() && time.elapsed_secs() > 3.0 {
        *asked = true;
        // (Not by itself on the web: a page's frames there are too uneven for a quick test to judge; the menu's button
        // still runs it.)
        let first_start = !options.gfx.tested && !scripted() && !cfg!(target_arch = "wasm32");
        if crate::args::get("bench").is_some() || crate::args::flag("bench") || first_start {
            bench.request = true;
        }
    }
    if !bench.request || !current.spawned || session.is_some() {
        return;
    }
    // (The navigation graph loads after the level: wait for it.)
    if current.nav.is_none() {
        return;
    }
    bench.request = false;
    let route = route(&current);
    if route.len() < 2 {
        warn!("bench: no route on {} (no callouts on the navigation graph)", current.name);
        return;
    }
    let target = target(&monitors);
    let all = crate::args::get("bench").as_deref() == Some("all");
    let saved = (options.gfx.clone(), options.vsync, menu.screen);
    let banner = commands
        .spawn((
            Node { position_type: PositionType::Absolute, top: Val::Px(36.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
            GlobalZIndex(50),
            children![(
                Node { padding: UiRect::axes(Val::Px(22.0), Val::Px(10.0)), border_radius: BorderRadius::all(Val::Px(8.0)), ..default() },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.72)),
                children![(text("Leistungstest", &fonts.label, 24.0, AMBER), BenchText)],
            )],
        ))
        .id();
    let first = if all { 0 } else { 2 };
    // (Measured flat out even when the window isn't focused: unfocused, Bevy holds a window to 60 frames a second.)
    #[cfg(not(target_arch = "wasm32"))]
    commands.insert_resource(bevy::winit::WinitSettings { focused_mode: bevy::winit::UpdateMode::Continuous, unfocused_mode: bevy::winit::UpdateMode::Continuous });
    options.gfx.apply_preset(first);
    options.vsync = false;
    menu.screen = Screen::None;
    info!("bench: {} m route on {}, aiming for {target} fps", route.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>() as i32, current.name);
    bench.run = Some(Run { route, level: first, phase: Phase::Warm, t: 0.0, calm: 0.0, frames: Vec::new(), tried: Vec::new(), target, all, saved, banner });
    bench.version += 1;
}

#[derive(Component)]
struct BenchText;

#[allow(clippy::too_many_arguments)]
fn measure(
    mut commands: Commands,
    mut bench: ResMut<Bench>,
    mut options: ResMut<Options>,
    mut menu: ResMut<MenuState>,
    session: Option<Res<Session>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut label: Query<&mut Text, With<BenchText>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(run) = bench.run.as_mut() else { return };
    // Esc or a match starting: back as it was.
    if keys.just_pressed(KeyCode::Escape) || session.is_some() {
        let (gfx, vsync, screen) = run.saved.clone();
        commands.entity(run.banner).despawn();
        #[cfg(not(target_arch = "wasm32"))]
        commands.insert_resource(bevy::winit::WinitSettings::game());
        (options.gfx, options.vsync, menu.screen) = (gfx, vsync, screen);
        bench.run = None;
        bench.version += 1;
        info!("bench: cancelled");
        return;
    }
    let dt = time.delta_secs();
    run.t += dt;
    run.calm = if dt < 3.0 / run.target { run.calm + dt } else { 0.0 };
    match run.phase {
        Phase::Warm if run.t >= MEASURE / WARM_PACE => (run.phase, run.t, run.calm) = (Phase::Settle, 0.0, 0.0),
        Phase::Settle if (run.t >= SETTLE && run.calm >= 1.0) || run.t >= SETTLE_MAX => {
            (run.phase, run.t) = (Phase::Measure, 0.0);
            run.frames.clear();
        }
        _ => {}
    }
    if run.phase == Phase::Measure {
        run.frames.push(dt * 1000.0);
        // `--bench-trace <file>`: every measured frame (preset, seconds in, ms), for finding hitches.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = crate::args::get("bench-trace") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(f, "{} {:.3} {:.2}", run.level, run.t, dt * 1000.0);
            }
        }
    }
    let step = run.tried.len() + 1;
    let what = if run.phase == Phase::Measure { "misst" } else { "bereitet vor" };
    let shown = format!("Leistungstest · {} {what} · Ziel {:.0} fps · Schritt {step} · Esc bricht ab", PRESET_NAMES[run.level as usize], run.target);
    for mut t in &mut label {
        if t.0 != shown {
            t.0 = shown.clone();
        }
    }
    if run.phase != Phase::Measure || run.t < MEASURE {
        return;
    }
    let s = stats(&mut run.frames, run.target);
    info!("bench: {:<8} Ø {:>6.1} fps · 1 % {:>6.1} fps · 95 % ≤ {:>5.1} ms · {}", PRESET_NAMES[run.level as usize], s.avg_fps, s.low_fps, s.p95_ms, if s.holds { "hält" } else { "hält nicht" });
    run.tried.push((run.level, s));
    let tried = |l: u8| run.tried.iter().any(|(t, _)| *t == l);
    // The next to try: every one in turn (`all`), else up while it holds and down while it doesn't.
    let next = if run.all {
        (run.level < 3).then_some(run.level + 1)
    } else if s.holds {
        (run.level < 3 && !tried(run.level + 1)).then_some(run.level + 1)
    } else {
        (run.level > 0 && !tried(run.level - 1)).then_some(run.level - 1)
    };
    if let Some(l) = next {
        run.level = l;
        (run.phase, run.t, run.calm) = (Phase::Settle, 0.0, 0.0);
        run.frames.clear();
        options.gfx.apply_preset(l);
        bench.version += 1;
        return;
    }
    // The best that held (the lowest if none did).
    let chosen = run.tried.iter().filter(|(_, s)| s.holds).map(|(l, _)| *l).max().unwrap_or(0);
    let (saved, _, screen) = run.saved.clone();
    let summary = Summary { chosen, target: run.target, tried: run.tried.clone() };
    commands.entity(run.banner).despawn();
    #[cfg(not(target_arch = "wasm32"))]
    commands.insert_resource(bevy::winit::WinitSettings::game());
    let mut gfx = saved;
    gfx.apply_preset(chosen);
    gfx.tested = true;
    options.gfx = gfx;
    options.vsync = run.saved.1;
    // (A first start's test ends in the lobby it started over.)
    menu.screen = if screen == Screen::None { Screen::Title } else { screen };
    info!("bench: chosen {} for {} fps", PRESET_NAMES[chosen as usize], summary.target);
    println!("bench: chosen {} ({})", PRESET_NAMES[chosen as usize], summary.tried.iter().map(|(l, s)| format!("{} {:.0}/{:.0} fps", PRESET_NAMES[*l as usize], s.avg_fps, s.low_fps)).collect::<Vec<_>>().join(", "));
    bench.summary = Some(summary);
    bench.run = None;
    bench.version += 1;
    if crate::args::flag("bench-exit") {
        exit.write(AppExit::Success);
    }
}

/// The camera on the walk: each preset from the start, so every one draws the same views.
fn fly(bench: Res<Bench>, mut cam: Single<&mut Transform, With<PlayerCamera>>) {
    let Some(run) = bench.run.as_ref() else { return };
    // (Settling at the start, where each preset's measured walk begins.)
    let (s, t) = match run.phase {
        Phase::Warm => (run.t * PACE * WARM_PACE, run.t * WARM_PACE),
        Phase::Settle => (0.0, 0.0),
        Phase::Measure => (run.t * PACE, run.t),
    };
    let eye = along(&run.route, s);
    let ahead = along(&run.route, s + 5.0);
    let dir = (ahead - eye).with_y(0.0).normalize_or(Vec3::NEG_Z);
    // Looking about as a player does: a slow sweep to either side and a little down.
    let yaw = (t * 0.9).sin() * 0.45;
    let look = Quat::from_rotation_y(yaw) * dir - Vec3::Y * 0.08;
    **cam = Transform::from_translation(eye).looking_to(look, Vec3::Y);
}

/// `--census`: once, what the scene is made of (for finding what the render thread spends its frame on).
pub fn census(
    time: Res<Time<Real>>,
    mut done: Local<bool>,
    all: Query<Entity>,
    meshes: Query<(&Mesh3d, Option<&Name>, Option<&ChildOf>)>,
    lightmapped: Query<(), With<bevy::pbr::Lightmap>>,
    ranged: Query<(), With<bevy::camera::visibility::VisibilityRange>>,
    points: Query<(), With<PointLight>>,
    spots: Query<(), With<SpotLight>>,
    nodes: Query<(), With<Node>>,
    visible: Query<&ViewVisibility, With<Mesh3d>>,
    casters: Query<(), (With<Mesh3d>, Without<bevy::light::NotShadowCaster>)>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
) {
    if *done || !crate::args::flag("census") || time.elapsed_secs() < 30.0 {
        return;
    }
    *done = true;
    let seen = visible.iter().filter(|v| v.get()).count();
    info!(
        "census: {} entities, {} meshes ({} drawn now, {} cast shadows, {} lightmapped, {} ranged), {} point + {} spot lights, {} UI nodes",
        all.iter().count(),
        meshes.iter().count(),
        seen,
        casters.iter().count(),
        lightmapped.iter().count(),
        ranged.iter().count(),
        points.iter().count(),
        spots.iter().count(),
        nodes.iter().count()
    );
    // Meshes by the name of their nearest named ancestor (a prop's model, a detail cell …).
    let mut by: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (_, name, parent) in &meshes {
        let mut label = name.map(|n| n.as_str().to_string());
        let mut p = parent.map(|c| c.parent());
        let mut hops = 0;
        while label.is_none() && hops < 6 {
            let Some(e) = p else { break };
            label = names.get(e).ok().map(|n| n.as_str().to_string());
            p = parents.get(e).ok().map(|c| c.parent());
            hops += 1;
        }
        let key = label.unwrap_or_else(|| "?".into());
        let key = key.split([' ', '#', '@']).next().unwrap_or("?").to_string();
        *by.entry(key).or_default() += 1;
    }
    let mut v: Vec<_> = by.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, n) in v.iter().take(25) {
        info!("census:   {n:>6}  {k}");
    }
}
