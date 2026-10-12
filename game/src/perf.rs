//! The performance overlay: how fast the game runs and where each frame's time goes. F3 steps through its sizes and
//! off (Shift+F3 back), the options menu too; the choice is kept.
//! - Small: one compact line — the frame rate, the frame time, the 1 % low and a strip of the last frames.
//! - Medium: adds the averages and lows, the split of a frame (the sim's ticks, the bots, the rest of the update,
//!   rendering and waiting) and the frame-time chart stacked by that split, with the 120/60/30 fps lines.
//! - Large: a wider chart (with the GPU's time where the device can measure it), the frame rate over the last minute,
//!   the spread of the frame times, the render passes, and the machine and the scene.
//!
//! In a match the chosen size sits beside the HUD (small and medium under the radar, large on the right under the kill
//! feed); over the menus, the buy menu and the loading screen only the small line shows, in the bottom right corner,
//! so the overlay never covers what is read or clicked there. Everything is see-through and takes no clicks.
//!
//! Everything is measured here, except the sim's cost, which `session::tick` adds to [`SimCost`]. On the web the whole
//! frame runs on one thread, so "rendering · waiting" is the render work plus the wait for the browser's next frame;
//! natively rendering runs beside the next update and that share is mostly the wait for it and the display.

use crate::hud::{text, Fonts, AMBER};
use crate::input::Ui;
use crate::options::Options;
use crate::session::{Pending, Session};
use bevy::asset::RenderAssetUsages;
use bevy::diagnostic::DiagnosticsStore;
use bevy::ecs::entity::Entities;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::PrimaryWindow;
use std::collections::VecDeque;
use std::time::Duration;
use zugriff_sim::game::TICK_HZ;

pub struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        // Times the render passes: on the CPU everywhere, on the GPU where the device has timestamp queries.
        app.add_plugins(RenderDiagnosticsPlugin)
            .init_resource::<Perf>()
            .add_systems(PostStartup, (spawn, size_from_args))
            .add_systems(First, begin_frame)
            .add_systems(Update, (toggle, unpickable))
            .add_systems(Last, (end_frame, show, texts, charts, own_cost).chain());
    }
}

/// The sizes' names, for the options menu.
pub const SIZE_NAMES: [&str; 4] = ["Aus", "Klein", "Mittel", "Groß"];

/// The sim's cost in this frame (all ticks run in it), added by `session::tick`.
#[derive(Resource, Default)]
pub struct SimCost {
    bots: f32,
    sim: f32,
    ticks: u32,
}

impl SimCost {
    pub fn add(&mut self, bots: Duration, sim: Duration) {
        self.bots += bots.as_secs_f32() * 1000.0;
        self.sim += sim.as_secs_f32() * 1000.0;
        self.ticks += 1;
    }
}

/// Frames kept (the large chart shows them all).
const FRAMES: usize = 756;
/// Frame-rate samples kept: four a second, a minute.
const HISTORY: usize = 240;
const HISTORY_EVERY: f32 = 0.25;

/// One frame (ms). `update` is the main world's whole update, `sim` and `bots` are parts of it; `gpu` is negative when
/// it isn't measured.
#[derive(Clone, Copy, Default)]
struct Sample {
    frame: f32,
    update: f32,
    sim: f32,
    bots: f32,
    gpu: f32,
    ticks: u8,
}

impl Sample {
    /// The frame's parts, bottom up: sim, bots, the rest of the update, rendering and waiting.
    fn parts(&self) -> [f32; 4] {
        let other = (self.update - self.sim - self.bots).max(0.0);
        [self.sim, self.bots, other, (self.frame - self.update).max(0.0)]
    }
}

#[derive(Resource, Default)]
struct Perf {
    frames: VecDeque<Sample>,
    /// Mean and lowest frame rate per quarter second.
    history: VecDeque<(f32, f32)>,
    /// The quarter second being gathered: time, frames, longest frame (ms).
    bucket: (f32, u32, f32),
    began: Option<Instant>,
    /// The frame chart's top (ms).
    scale: f32,
    text_in: f32,
    chart_in: f32,
    slow_in: f32,
    /// The overlay's own cost per frame (ms, smoothed), and when this frame's share began.
    own: f32,
    own_began: Option<Instant>,
    /// The size on screen (0 none): the chosen one, or only the small one over menus.
    shown: u8,
}

#[derive(Component)]
struct PerfPanel(u8);

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Field {
    Fps,
    FrameMs,
    SmallLine,
    StatValues,
    Part(usize),
    FrameCaption,
    HistoryCaption,
    SpreadCaption,
    PassNames,
    PassValues,
    SysValues,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartKind {
    /// The small size's strip of frame times.
    Strip,
    /// Frame times stacked by their parts.
    Frames,
    /// The frame rate over the last minute.
    History,
    /// How the frame times are spread.
    Spread,
}

#[derive(Component)]
struct Chart {
    kind: ChartKind,
    size: u8,
    w: usize,
    h: usize,
}

/// A frame-rate line's label on the frame chart (at this many ms).
#[derive(Component)]
struct Guide(f32);

const GOOD: [u8; 3] = [110, 230, 120];
const WARN: [u8; 3] = [255, 176, 0];
const BAD: [u8; 3] = [255, 64, 51];
/// The parts' colours: sim, bots, rest of the update, rendering and waiting; then the GPU's line.
const PARTS: [[u8; 3]; 5] = [[120, 184, 255], [190, 130, 255], [80, 210, 200], [92, 99, 106], [255, 238, 190]];
const PART_NAMES: [&str; 5] = ["Sim-Ticks", "Bots", "Update (Rest)", "Rendern · Warten", "GPU"];
const DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.55);
// UI blends in linear light, so a panel over the bright sky needs a high alpha to read as dark.
const PANEL: Color = Color::srgba(0.02, 0.03, 0.04, 0.8);

fn rgb(c: [u8; 3]) -> Color {
    Color::srgb_u8(c[0], c[1], c[2])
}

/// Green up to 60 fps' frame time, amber to 30 fps', red beyond.
fn grade(ms: f32) -> [u8; 3] {
    if ms <= 1000.0 / 58.0 {
        GOOD
    } else if ms <= 1000.0 / 29.0 {
        WARN
    } else {
        BAD
    }
}

fn begin_frame(mut perf: ResMut<Perf>) {
    perf.began = Some(Instant::now());
}

/// Records the frame: its time since the last one, the update's time so far (this runs last), the sim's share.
fn end_frame(real: Res<Time<Real>>, options: Res<Options>, store: Res<DiagnosticsStore>, mut perf: ResMut<Perf>, mut cost: ResMut<SimCost>) {
    let frame = real.delta_secs() * 1000.0;
    let update = perf.began.map_or(0.0, |b| b.elapsed().as_secs_f32() * 1000.0);
    let taken = std::mem::take(&mut *cost);
    if frame <= 0.0 {
        return;
    }
    // The GPU's time: the top-level passes (render/<pass>/elapsed_gpu), only looked up while the overlay is up.
    let gpu = if options.perf >= 2 {
        let mut sum = 0.0;
        let mut any = false;
        for d in store.iter() {
            let p = d.path().as_str();
            if p.starts_with("render/") && p.ends_with("/elapsed_gpu") && p.matches('/').count() == 2 {
                if let Some(v) = d.value() {
                    sum += v as f32;
                    any = true;
                }
            }
        }
        if any { sum } else { -1.0 }
    } else {
        -1.0
    };
    perf.frames.push_back(Sample { frame, update, sim: taken.sim, bots: taken.bots, gpu, ticks: taken.ticks.min(255) as u8 });
    while perf.frames.len() > FRAMES {
        perf.frames.pop_front();
    }
    let (t, n, worst) = &mut perf.bucket;
    *t += frame / 1000.0;
    *n += 1;
    *worst = worst.max(frame);
    if *t >= HISTORY_EVERY {
        let sample = (*n as f32 / *t, 1000.0 / *worst);
        perf.bucket = (0.0, 0, 0.0);
        perf.history.push_back(sample);
        while perf.history.len() > HISTORY {
            perf.history.pop_front();
        }
    }
}

/// `perf=1|2|3` (`--perf` natively) opens the overlay at that size for this run, without changing the saved choice.
fn size_from_args(mut options: ResMut<Options>) {
    if let Some(n) = crate::args::get("perf").and_then(|v| v.parse::<u8>().ok()) {
        options.bypass_change_detection().perf = n.min(3);
    }
}

fn toggle(keys: Res<ButtonInput<KeyCode>>, mut options: ResMut<Options>) {
    if keys.just_pressed(KeyCode::F3) {
        let back = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        options.perf = (options.perf + if back { 3 } else { 1 }) % 4;
    }
}

/// Which size is on screen and where the small one sits (see the module's notes). Nodes are only written when the
/// place changes, so the layout doesn't run every frame.
#[allow(clippy::too_many_arguments)]
fn show(options: Res<Options>, ui: Res<Ui>, session: Option<Res<Session>>, pending: Res<Pending>, warmup: Res<crate::warmup::Warmup>, mut perf: ResMut<Perf>, mut panels: Query<(&PerfPanel, &mut Visibility, &mut Node)>) {
    let over_ui = ui.menu || ui.buy || session.is_none() || pending.0.is_some() || warmup.active();
    let shown = if over_ui { options.perf.min(1) } else { options.perf };
    perf.shown = shown;
    for (p, mut vis, mut node) in &mut panels {
        vis.set_if_neq(if p.0 == shown { Visibility::Inherited } else { Visibility::Hidden });
        if p.0 == 1 {
            let place = if over_ui { (Val::Auto, Val::Auto, Val::Px(16.0), Val::Px(12.0)) } else { (Val::Px(RADAR_LEFT), Val::Px(UNDER_RADAR), Val::Auto, Val::Auto) };
            if (node.left, node.top, node.right, node.bottom) != place {
                (node.left, node.top, node.right, node.bottom) = place;
            }
        }
    }
}

/// Under the radar (1080p UI pixels): the radar and its place name end above this.
const UNDER_RADAR: f32 = 300.0;
const RADAR_LEFT: f32 = 20.0;

/// The overlay sits over the menus; it must not take their clicks.
fn unpickable(mut commands: Commands, mut done: Local<bool>, panels: Query<Entity, With<PerfPanel>>, children: Query<&Children>) {
    if *done || panels.is_empty() {
        return;
    }
    *done = true;
    for root in &panels {
        commands.entity(root).insert(Pickable::IGNORE);
        for e in children.iter_descendants(root) {
            commands.entity(e).insert(Pickable::IGNORE);
        }
    }
}

#[derive(Default)]
struct Stats {
    /// Over the last half second.
    fps: f32,
    frame: f32,
    /// Over the frames kept.
    mean: f32,
    low1: f32,
    low01: f32,
    median: f32,
    p99: f32,
    min: f32,
    max: f32,
    sd: f32,
    /// Means over the last second: the parts, the GPU (if measured), ticks per frame.
    parts: [f32; 4],
    gpu: Option<f32>,
    ticks: f32,
}

fn stats(frames: &VecDeque<Sample>) -> Stats {
    let mut s = Stats::default();
    if frames.is_empty() {
        return s;
    }
    let (mut sum, mut n) = (0.0, 0);
    for f in frames.iter().rev() {
        sum += f.frame;
        n += 1;
        if sum >= 500.0 {
            break;
        }
    }
    s.fps = n as f32 * 1000.0 / sum;
    s.frame = sum / n as f32;
    let mut sorted: Vec<f32> = frames.iter().map(|f| f.frame).collect();
    sorted.sort_by(f32::total_cmp);
    let count = sorted.len();
    let total: f32 = sorted.iter().sum();
    s.mean = count as f32 * 1000.0 / total;
    // The lows: the frame rate over the slowest 1 % and 0.1 % of frames.
    let low = |share: f32| {
        let k = ((count as f32 * share).ceil() as usize).max(1);
        k as f32 * 1000.0 / sorted[count - k..].iter().sum::<f32>()
    };
    s.low1 = low(0.01);
    s.low01 = low(0.001);
    s.median = sorted[count / 2];
    s.p99 = sorted[((count as f32 * 0.99) as usize).min(count - 1)];
    s.min = sorted[0];
    s.max = sorted[count - 1];
    let mean_ms = total / count as f32;
    s.sd = (sorted.iter().map(|v| (v - mean_ms).powi(2)).sum::<f32>() / count as f32).sqrt();
    let (mut t, mut m, mut gpu, mut gpu_n, mut ticks) = (0.0, 0, 0.0, 0, 0.0);
    for f in frames.iter().rev() {
        for (acc, v) in s.parts.iter_mut().zip(f.parts()) {
            *acc += v;
        }
        if f.gpu >= 0.0 {
            gpu += f.gpu;
            gpu_n += 1;
        }
        ticks += f.ticks as f32;
        m += 1;
        t += f.frame;
        if t >= 1000.0 {
            break;
        }
    }
    for v in &mut s.parts {
        *v /= m as f32;
    }
    s.gpu = (gpu_n > 0).then(|| gpu / gpu_n as f32);
    s.ticks = ticks / m as f32;
    s
}

fn set(t: &mut Text, s: String) {
    if t.0 != s {
        t.0 = s;
    }
}

#[allow(clippy::too_many_arguments)]
fn texts(
    real: Res<Time<Real>>,
    mut perf: ResMut<Perf>,
    store: Res<DiagnosticsStore>,
    info: Option<Res<RenderAdapterInfo>>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    entities: &Entities,
    assets: (Res<Assets<Mesh>>, Res<Assets<StandardMaterial>>, Res<Assets<Image>>),
    session: Option<Res<Session>>,
    mut fields: Query<(&Field, &mut Text, &mut TextColor)>,
) {
    perf.own_began = Some(Instant::now());
    perf.text_in -= real.delta_secs();
    if perf.shown == 0 || perf.text_in > 0.0 {
        return;
    }
    perf.text_in = 0.25;
    let s = stats(&perf.frames);
    let large = perf.shown == 3;
    let passes = if large { passes(&store) } else { (String::new(), String::new()) };
    let system = if large { system(info.as_deref(), &window, ui_scale.0, entities, &assets, session.as_deref(), perf.own) } else { String::new() };
    let (hist_mean, hist_min) = {
        let n = perf.history.len().max(1) as f32;
        (perf.history.iter().map(|h| h.0).sum::<f32>() / n, perf.history.iter().map(|h| h.1).fold(f32::INFINITY, f32::min))
    };
    let frame_total = s.parts.iter().sum::<f32>().max(1e-3);
    for (field, mut t, mut color) in &mut fields {
        match *field {
            Field::Fps => {
                set(&mut t, format!("{:.0}", s.fps));
                color.0 = rgb(grade(s.frame));
            }
            Field::FrameMs => set(&mut t, format!("{:.2} ms", s.frame)),
            Field::SmallLine => set(&mut t, format!("{:.1} ms · 1 % {:.0}", s.frame, s.low1)),
            Field::StatValues => set(
                &mut t,
                format!(
                    "{:.0} fps\n{:.0} / {:.0} fps\n{:.2} ms\n{:.1} / {:.1} ms\n{:.2} ms\n{:.2} · {} Hz",
                    s.mean, s.low1, s.low01, s.median, s.min, s.max, s.sd, s.ticks, TICK_HZ
                ),
            ),
            Field::Part(i) => {
                let line = match (i, s.gpu) {
                    (4, Some(g)) => format!("{g:.2} ms"),
                    (4, None) => "nicht messbar".to_string(),
                    _ => format!("{:.2} ms · {:>2.0} %", s.parts[i], s.parts[i] / frame_total * 100.0),
                };
                set(&mut t, line);
            }
            Field::FrameCaption => set(&mut t, format!("Bildzeit je Bild · letzte {} Bilder · bis {:.0} ms", perf.frames.len(), perf.scale)),
            Field::HistoryCaption => {
                let line = if perf.history.is_empty() { "Bildrate · letzte 60 s".to_string() } else { format!("Bildrate · {:.0} s · Ø {hist_mean:.0} · min {hist_min:.0}", perf.history.len() as f32 * HISTORY_EVERY) };
                set(&mut t, line);
            }
            Field::SpreadCaption => set(&mut t, format!("Verteilung · Median {:.2} ms · 99 % {:.2} ms", s.median, s.p99)),
            Field::PassNames => set(&mut t, passes.0.clone()),
            Field::PassValues => set(&mut t, passes.1.clone()),
            Field::SysValues => set(&mut t, system.clone()),
        }
    }
}

/// The render passes' times, the slowest first: names, and "CPU / GPU" values.
fn passes(store: &DiagnosticsStore) -> (String, String) {
    let mut rows: Vec<(String, Option<f64>, Option<f64>)> = Vec::new();
    for d in store.iter() {
        let p = d.path().as_str();
        let Some(rest) = p.strip_prefix("render/") else { continue };
        let (name, gpu) = if let Some(n) = rest.strip_suffix("/elapsed_cpu") {
            (n, false)
        } else if let Some(n) = rest.strip_suffix("/elapsed_gpu") {
            (n, true)
        } else {
            continue;
        };
        let v = d.smoothed();
        let i = rows.iter().position(|r| r.0 == name).unwrap_or_else(|| {
            rows.push((name.to_string(), None, None));
            rows.len() - 1
        });
        if gpu {
            rows[i].2 = v;
        } else {
            rows[i].1 = v;
        }
    }
    if rows.is_empty() {
        return ("keine Messwerte".into(), String::new());
    }
    let key = |r: &(String, Option<f64>, Option<f64>)| r.2.or(r.1).unwrap_or(0.0);
    rows.sort_by(|a, b| key(b).total_cmp(&key(a)));
    rows.truncate(9);
    let ms = |v: Option<f64>| v.map_or("–".to_string(), |v| format!("{v:.2}"));
    let names = rows.iter().map(|r| short(&r.0.replace('/', " › "), 30)).collect::<Vec<_>>().join("\n");
    let values = rows.iter().map(|r| format!("{} / {} ms", ms(r.1), ms(r.2))).collect::<Vec<_>>().join("\n");
    (names, values)
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n - 1).chain(std::iter::once('…')).collect() }
}

/// The lines under `SYS_NAMES`.
fn system(info: Option<&RenderAdapterInfo>, window: &Window, ui: f32, entities: &Entities, assets: &(Res<Assets<Mesh>>, Res<Assets<StandardMaterial>>, Res<Assets<Image>>), session: Option<&Session>, own: f32) -> String {
    let (gpu, api, driver) = match info {
        Some(i) => (short(&i.name, 34), format!("{:?} · {:?}", i.backend, i.device_type), short(&format!("{} {}", i.driver, i.driver_info).trim().to_string(), 34)),
        None => ("–".into(), "–".into(), "–".into()),
    };
    let driver = if driver.is_empty() { "–".to_string() } else { driver };
    let res = format!("{} × {} px · ×{:.2} · UI ×{:.2}", window.physical_width(), window.physical_height(), window.scale_factor(), ui);
    let memory = memory().map_or("–".to_string(), |b| format!("{:.0} MB", b as f64 / 1_048_576.0));
    let (map, people) = match session {
        Some(s) => {
            let alive = s.game.players.iter().filter(|p| p.alive).count();
            (format!("{} · Tick {}", s.settings.map, s.ticks), format!("{alive}/{} leben · {} Granaten", s.game.players.len(), s.game.grenades.len()))
        }
        None => ("kein Spiel".into(), "–".into()),
    };
    [
        gpu,
        api,
        driver,
        res,
        format!("{:?}", window.present_mode),
        format!("{}", entities.count_spawned()),
        format!("{} · {} · {}", assets.0.len(), assets.1.len(), assets.2.len()),
        memory,
        map,
        people,
        format!("{own:.2} ms je Bild"),
    ]
    .join("\n")
}

const SYS_NAMES: &str = "Grafikkarte\nSchnittstelle\nTreiber\nAuflösung\nBildausgabe\nEntitäten\nMeshes/Mat./Bilder\nSpeicher\nKarte\nSpieler\nDiese Anzeige";

/// The memory in use: the WebAssembly heap on the web, the resident set natively (Linux).
fn memory() -> Option<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        Some(core::arch::wasm32::memory_size::<0>() as u64 * 65_536)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        Some(pages * 4096)
    }
}

/// An RGBA picture drawn on the CPU (straight alpha, sRGB).
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Canvas {
    /// Dark enough that busy walls behind the panel don't show through the chart.
    fn new(w: usize, h: usize) -> Self {
        Canvas { w, h, px: [0, 0, 0, 150].repeat(w * h) }
    }

    /// Paints over a pixel. Integer maths, as this runs over whole charts several times a second (on the web too).
    fn blend(&mut self, x: i32, y: i32, c: [u8; 3], a: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let o = (y as usize * self.w + x as usize) * 4;
        let a = (a.clamp(0.0, 1.0) * 255.0) as u32;
        if a == 255 {
            self.px[o..o + 3].copy_from_slice(&c);
            self.px[o + 3] = 255;
            return;
        }
        let keep = 255 - a;
        for k in 0..3 {
            self.px[o + k] = ((c[k] as u32 * a + self.px[o + k] as u32 * keep + 127) / 255) as u8;
        }
        self.px[o + 3] = (a + (self.px[o + 3] as u32 * keep + 127) / 255) as u8;
    }

    /// Fills [x0, x1) × [y0, y1).
    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: [u8; 3], a: f32) {
        for y in y0.max(0)..y1.min(self.h as i32) {
            for x in x0.max(0)..x1.min(self.w as i32) {
                self.blend(x, y, c, a);
            }
        }
    }

    /// A horizontal line across, dashed or not.
    fn hline(&mut self, y: i32, c: [u8; 3], a: f32, dashed: bool) {
        for x in 0..self.w as i32 {
            if !dashed || x % 6 < 3 {
                self.blend(x, y, c, a);
            }
        }
    }

    /// The pixel row for `v` on a scale from 0 (bottom) to `top`.
    fn y(&self, v: f32, top: f32) -> i32 {
        let k = (v / top).clamp(0.0, 1.0);
        (self.h as f32 - 1.0 - k * (self.h as f32 - 1.0)).round() as i32
    }

    /// A line through the points (one per column), joined column to column.
    fn polyline(&mut self, ys: &[(i32, i32)], c: [u8; 3], a: f32) {
        let mut last: Option<i32> = None;
        for &(x, y) in ys {
            let (lo, hi) = match last {
                Some(l) => (l.min(y), l.max(y)),
                None => (y, y),
            };
            for yy in lo..=hi {
                self.blend(x, yy, c, a);
            }
            last = Some(y);
        }
    }
}

/// The frame chart's top: the next step above the slowest frames.
fn frame_scale(frames: &VecDeque<Sample>) -> f32 {
    const STEPS: [f32; 7] = [20.0, 40.0, 70.0, 100.0, 200.0, 500.0, 1000.0];
    let mut v: Vec<f32> = frames.iter().map(|f| f.frame.max(f.update)).collect();
    if v.is_empty() {
        return STEPS[0];
    }
    v.sort_by(f32::total_cmp);
    // The slowest 1 % may run off the top (their bars get a red cap), so a few hitches don't flatten the rest.
    let high = v[((v.len() as f32 * 0.99) as usize).min(v.len() - 1)] * 1.2;
    STEPS.into_iter().find(|&s| s >= high).unwrap_or(1000.0)
}

/// What the overlay itself cost this frame (its texts and charts; the rest is Bevy's UI, which it adds to).
fn own_cost(mut perf: ResMut<Perf>) {
    if let Some(b) = perf.own_began.take() {
        let ms = b.elapsed().as_secs_f32() * 1000.0;
        perf.own = perf.own * 0.98 + ms * 0.02;
    }
}

fn charts(
    real: Res<Time<Real>>,
    mut perf: ResMut<Perf>,
    charts: Query<(&Chart, &ImageNode)>,
    mut images: ResMut<Assets<Image>>,
    mut guides: Query<(&Guide, &mut Node, &mut Visibility)>,
) {
    // The frame charts scroll at 15 Hz; the history and the spread change slowly and are drawn with the texts.
    perf.chart_in -= real.delta_secs();
    perf.slow_in -= real.delta_secs();
    let fast = perf.chart_in <= 0.0;
    let slow = perf.slow_in <= 0.0;
    if perf.shown == 0 || !(fast || slow) {
        return;
    }
    if fast {
        perf.chart_in = 1.0 / 15.0;
    }
    if slow {
        perf.slow_in = 0.25;
    }
    perf.scale = frame_scale(&perf.frames);
    let scale = perf.scale;
    for (g, mut node, mut vis) in &mut guides {
        node.top = Val::Percent((1.0 - g.0 / scale) * 100.0);
        vis.set_if_neq(if g.0 < scale * 0.93 { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (chart, node) in &charts {
        let due = if matches!(chart.kind, ChartKind::Strip | ChartKind::Frames) { fast } else { slow };
        if chart.size != perf.shown || !due {
            continue;
        }
        let mut c = Canvas::new(chart.w, chart.h);
        match chart.kind {
            ChartKind::Strip => draw_strip(&mut c, &perf.frames),
            ChartKind::Frames => draw_frames(&mut c, &perf.frames, scale),
            ChartKind::History => draw_history(&mut c, &perf.history),
            ChartKind::Spread => draw_spread(&mut c, &perf.frames, scale),
        }
        if let Some(mut img) = images.get_mut(&node.image) {
            img.data = Some(c.px);
        }
    }
}

/// The last frames as bars up to 33 ms, coloured by how slow they are, with the 60 fps line.
fn draw_strip(c: &mut Canvas, frames: &VecDeque<Sample>) {
    let top = 1000.0 / 30.0;
    let y60 = c.y(1000.0 / 60.0, top);
    c.hline(y60, [255, 255, 255], 0.18, true);
    let skip = frames.len().saturating_sub(c.w);
    let start = c.w.saturating_sub(frames.len()) as i32;
    for (i, f) in frames.iter().skip(skip).enumerate() {
        let x = start + i as i32;
        let y = c.y(f.frame, top);
        c.rect(x, y, x + 1, c.h as i32, grade(f.frame), 1.0);
    }
}

/// Frame times stacked by their parts, the GPU's time as a line, and the 120/60/30 fps lines.
fn draw_frames(c: &mut Canvas, frames: &VecDeque<Sample>, top: f32) {
    for fps in [120.0, 60.0, 30.0] {
        let ms = 1000.0 / fps;
        if ms < top {
            let y = c.y(ms, top);
            c.hline(y, [255, 255, 255], 0.2, true);
        }
    }
    let skip = frames.len().saturating_sub(c.w);
    let start = c.w.saturating_sub(frames.len()) as i32;
    let mut gpu = Vec::new();
    for (i, f) in frames.iter().skip(skip).enumerate() {
        let x = start + i as i32;
        let mut base = 0.0;
        for (k, v) in f.parts().into_iter().enumerate() {
            let (y0, y1) = (c.y(base + v, top), c.y(base, top));
            c.rect(x, y0, x + 1, y1 + 1, PARTS[k], 1.0);
            base += v;
        }
        // A frame past 30 fps' time gets a red cap.
        if f.frame > 1000.0 / 30.0 || base > top {
            let y = c.y(base, top);
            c.rect(x, y, x + 1, y + 2, BAD, 1.0);
        }
        if f.gpu >= 0.0 {
            gpu.push((x, c.y(f.gpu, top)));
        }
    }
    c.polyline(&gpu, PARTS[4], 0.9);
}

/// The mean frame rate per quarter second as an area, the lowest as a line, over the last minute.
fn draw_history(c: &mut Canvas, history: &VecDeque<(f32, f32)>) {
    const STEPS: [f32; 8] = [60.0, 90.0, 120.0, 165.0, 240.0, 360.0, 500.0, 1000.0];
    let high = history.iter().map(|h| h.0).fold(0.0, f32::max) * 1.1;
    let top = STEPS.into_iter().find(|&s| s >= high).unwrap_or(1000.0);
    for fps in [30.0, 60.0, 120.0] {
        if fps < top {
            let y = c.y(fps, top);
            c.hline(y, [255, 255, 255], 0.2, true);
        }
    }
    if history.is_empty() {
        return;
    }
    let (w, n) = (c.w, HISTORY);
    let offset = n - history.len();
    let mut mean_line = Vec::new();
    let mut min_line = Vec::new();
    for x in 0..w {
        let i = x * n / w;
        if i < offset {
            continue;
        }
        let (mean, low) = history[i - offset];
        let y = c.y(mean, top);
        c.rect(x as i32, y, x as i32 + 1, c.h as i32, WARN, 0.28);
        mean_line.push((x as i32, y));
        min_line.push((x as i32, c.y(low, top)));
    }
    c.polyline(&min_line, BAD, 0.8);
    c.polyline(&mean_line, WARN, 1.0);
}

/// How many frames took how long (0 to the frame chart's top), with the median and the 99th percentile marked.
fn draw_spread(c: &mut Canvas, frames: &VecDeque<Sample>, top: f32) {
    if frames.is_empty() {
        return;
    }
    let bar = 3usize;
    let bins = c.w / bar;
    let mut count = vec![0u32; bins];
    let mut sorted: Vec<f32> = frames.iter().map(|f| f.frame).collect();
    sorted.sort_by(f32::total_cmp);
    for &v in &sorted {
        let b = ((v / top) * bins as f32) as usize;
        count[b.min(bins - 1)] += 1;
    }
    let most = *count.iter().max().unwrap_or(&1) as f32;
    for (b, &n) in count.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let mid = (b as f32 + 0.5) / bins as f32 * top;
        // A square root, so the rare slow frames still show next to the common ones.
        let y = c.y((n as f32 / most).sqrt(), 1.0);
        let x = (b * bar) as i32;
        c.rect(x, y, x + bar as i32 - 1, c.h as i32, grade(mid), 1.0);
    }
    let mark = |c: &mut Canvas, v: f32, col: [u8; 3]| {
        let x = ((v / top).min(1.0) * (c.w as f32 - 1.0)) as i32;
        for y in 0..c.h as i32 {
            if y % 4 < 2 {
                c.blend(x, y, col, 0.9);
            }
        }
    };
    let n = sorted.len();
    mark(c, sorted[n / 2], [255, 255, 255]);
    mark(c, sorted[((n as f32 * 0.99) as usize).min(n - 1)], BAD);
}

fn spawn(mut commands: Commands, fonts: Res<Fonts>, mut images: ResMut<Assets<Image>>) {
    let f = &*fonts;
    let mut image = |w: usize, h: usize| images.add(Image::new_fill(Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 }, TextureDimension::D2, &[0, 0, 0, 0], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::all()));
    // `place`: left/top/right edges (Auto where unused); the small one is moved by `show`.
    let panel = |left: Val, top: Val, right: Val, width: Val, pad: f32| {
        (
            Node {
                position_type: PositionType::Absolute,
                left,
                top,
                right,
                width,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(pad)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.1)),
            Visibility::Hidden,
            GlobalZIndex(40),
        )
    };
    let row = |gap: f32| Node { column_gap: Val::Px(gap), align_items: AlignItems::FlexStart, ..default() };
    let col = |gap: f32| Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(gap), ..default() };
    let table = |size: f32| TextFont::from(f.text.clone()).with_font_size(size);
    let left = TextLayout::new(Justify::Left, LineBreak::NoWrap);
    let right = TextLayout::new(Justify::Right, LineBreak::NoWrap);

    // Small: one line — frame rate, frame time and low, the strip.
    let strip = image(96, 20);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(RADAR_LEFT),
                top: Val::Px(UNDER_RADAR),
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.62)),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.08)),
            Visibility::Hidden,
            GlobalZIndex(40),
            PerfPanel(1),
            Name::new("perf small"),
        ))
        .with_children(|p| {
            p.spawn(Node { column_gap: Val::Px(4.0), align_items: AlignItems::Baseline, ..default() }).with_children(|r| {
                r.spawn((text("0", &f.digits, 26.0, rgb(GOOD)), Field::Fps));
                r.spawn(text("FPS", &f.bold, 12.0, DIM));
            });
            p.spawn((text("", &f.label, 15.0, Color::WHITE), Field::SmallLine));
            p.spawn((Node { width: Val::Px(96.0), height: Val::Px(20.0), ..default() }, ImageNode::new(strip), Chart { kind: ChartKind::Strip, size: 1, w: 96, h: 20 }));
        });

    // The header, the frame rate, the numbers and the parts (medium and large).
    let header = |p: &mut ChildSpawnerCommands, size: u8| {
        p.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn(text("LEISTUNG", &f.bold, 15.0, AMBER));
            r.spawn(text(&format!("F3 · {}", SIZE_NAMES[size as usize].to_uppercase()), &f.label, 13.0, DIM));
        });
    };
    let big = |p: &mut ChildSpawnerCommands| {
        p.spawn(Node { min_width: Val::Px(70.0), ..col(0.0) }).with_children(|c| {
            c.spawn((text("0", &f.digits, 40.0, rgb(GOOD)), Field::Fps));
            c.spawn(text("FPS", &f.bold, 13.0, DIM));
            c.spawn((text("", &f.label, 15.0, Color::WHITE), Field::FrameMs));
        });
    };
    let numbers = |p: &mut ChildSpawnerCommands| {
        p.spawn(row(8.0)).with_children(|r| {
            r.spawn((Text::new("Ø Bildrate\n1 % / 0,1 % Tief\nMedian\nMin / Max\nSchwankung σ\nTicks je Bild"), table(12.0), TextColor(DIM), left));
            r.spawn((Text::new(""), table(12.0), TextColor(Color::WHITE), right, Field::StatValues));
        });
    };
    let parts = |p: &mut ChildSpawnerCommands, n: usize| {
        p.spawn(col(2.0)).with_children(|c| {
            for (i, name) in PART_NAMES.iter().enumerate().take(n) {
                c.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                    let swatch = if i == 4 { Node { width: Val::Px(9.0), height: Val::Px(2.0), ..default() } } else { Node { width: Val::Px(9.0), height: Val::Px(9.0), border_radius: BorderRadius::all(Val::Px(2.0)), ..default() } };
                    r.spawn((swatch, BackgroundColor(rgb(PARTS[i]))));
                    r.spawn((Node { width: Val::Px(104.0), ..default() }, children![(Text::new(*name), table(12.0), TextColor(DIM), left)]));
                    r.spawn((Node { width: Val::Px(96.0), justify_content: JustifyContent::End, ..default() }, children![(Text::new(""), table(12.0), TextColor(Color::WHITE), right, Field::Part(i))]));
                });
            }
        });
    };
    let caption = |p: &mut ChildSpawnerCommands, field: Field| {
        p.spawn((text("", &f.label, 13.0, DIM), field));
    };
    let frames_chart = |p: &mut ChildSpawnerCommands, size: u8, w: usize, h: usize, img: Handle<Image>| {
        p.spawn(Node { width: Val::Px(w as f32), height: Val::Px(h as f32), ..default() }).with_children(|c| {
            c.spawn((Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, ImageNode::new(img), Chart { kind: ChartKind::Frames, size, w, h }));
            for fps in [120.0, 60.0, 30.0] {
                c.spawn((
                    Node { position_type: PositionType::Absolute, right: Val::Px(3.0), margin: UiRect::top(Val::Px(-12.0)), ..default() },
                    Guide(1000.0 / fps),
                    Visibility::Hidden,
                    children![(text(&format!("{fps:.0} fps"), &f.label, 11.0, Color::srgba(1.0, 1.0, 1.0, 0.5)))],
                ));
            }
        });
    };

    // Medium: under the radar.
    let (mw, mh) = (300usize, 72usize);
    let medium_frames = image(mw, mh);
    commands.spawn((panel(Val::Px(RADAR_LEFT), Val::Px(UNDER_RADAR), Val::Auto, Val::Px(mw as f32 + 22.0), 10.0), PerfPanel(2), Name::new("perf medium"))).with_children(|p| {
        header(p, 2);
        p.spawn(row(12.0)).with_children(|r| {
            big(r);
            numbers(r);
        });
        caption(p, Field::FrameCaption);
        frames_chart(p, 2, mw, mh, medium_frames);
        parts(p, 5);
    });

    // Large: on the right, under the kill feed and above the money and ammunition.
    let (lw, lh) = (560usize, 104usize);
    let (hw, hh) = (274usize, 64usize);
    let (large_frames, history, spread) = (image(lw, lh), image(hw, hh), image(hw, hh));
    commands.spawn((panel(Val::Auto, Val::Px(236.0), Val::Px(20.0), Val::Px(lw as f32 + 22.0), 10.0), PerfPanel(3), Name::new("perf large"))).with_children(|p| {
        header(p, 3);
        p.spawn(row(14.0)).with_children(|r| {
            big(r);
            numbers(r);
            parts(r, 5);
        });
        caption(p, Field::FrameCaption);
        frames_chart(p, 3, lw, lh, large_frames);
        p.spawn(row(12.0)).with_children(|r| {
            for (field, kind, img) in [(Field::HistoryCaption, ChartKind::History, history), (Field::SpreadCaption, ChartKind::Spread, spread)] {
                r.spawn(col(3.0)).with_children(|c| {
                    caption(c, field);
                    c.spawn((Node { width: Val::Px(hw as f32), height: Val::Px(hh as f32), ..default() }, ImageNode::new(img), Chart { kind, size: 3, w: hw, h: hh }));
                });
            }
        });
        p.spawn(row(12.0)).with_children(|r| {
            r.spawn(Node { width: Val::Px(hw as f32), ..col(3.0) }).with_children(|c| {
                c.spawn(text("RENDER-DURCHGÄNGE · CPU / GPU", &f.bold, 12.0, AMBER));
                c.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|t| {
                    t.spawn((Text::new(""), table(11.0), TextColor(DIM), left, Field::PassNames));
                    t.spawn((Text::new(""), table(11.0), TextColor(Color::WHITE), right, Field::PassValues));
                });
            });
            r.spawn(Node { width: Val::Px(hw as f32), ..col(3.0) }).with_children(|c| {
                c.spawn(text("SYSTEM", &f.bold, 12.0, AMBER));
                c.spawn(Node { justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(10.0), ..default() }).with_children(|t| {
                    t.spawn((Text::new(SYS_NAMES), table(11.0), TextColor(DIM), left));
                    t.spawn((Text::new(""), table(11.0), TextColor(Color::WHITE), right, Field::SysValues));
                });
            });
        });
    });
}
