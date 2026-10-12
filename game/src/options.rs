//! The player's settings, kept between sessions: in the browser's local storage on the web, in
//! `~/.config/zugriff/options.json` natively.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub struct OptionsPlugin;

impl Plugin for OptionsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load())
            .init_resource::<Resolutions>()
            .add_systems(Update, (apply_render_scale, list_resolutions, apply_display).chain())
            .add_systems(Last, save_on_change);
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub name: String,
    pub sensitivity: f32,
    pub invert: bool,
    /// Horizontal field of view at 4:3 (degrees).
    pub fov: f32,
    pub volume: f32,
    pub music: f32,
    /// Crosshair: colour index, size, gap, thickness, and whether it opens with the spread.
    pub crosshair_color: u8,
    pub crosshair_size: f32,
    pub crosshair_gap: f32,
    pub crosshair_thickness: f32,
    pub crosshair_dynamic: bool,
    /// The performance overlay: 0 off, 1 small, 2 medium, 3 large (F3 steps through them).
    pub perf: u8,
    /// Rendered pixels per screen (CSS) pixel on the web: 1 is sharp on ordinary screens and cheap on high-DPI ones;
    /// 0 means the device's full resolution.
    pub render_scale: f32,
    /// How the game shows: 0 in a window, 1 borderless over the desktop, 2 exclusive fullscreen (see `DISPLAY_NAMES`;
    /// on the web 0 and 1 only: the page or the browser's fullscreen).
    pub display: u8,
    /// The window's size, or the monitor's mode in exclusive fullscreen (physical pixels); None: the desktop's.
    pub resolution: Option<(u32, u32)>,
    /// Wait for the display's refresh (no tearing, a little more latency).
    pub vsync: bool,
    pub difficulty: u8,
    pub last_map: String,
    /// The finish each weapon comes in (weapon key → finish key; see finish.rs). Missing: as from the factory.
    pub finishes: std::collections::BTreeMap<String, String>,
    /// Online: servers added by address (the server list asks them next to the default one).
    pub servers: Vec<String>,
    /// The graphics settings (graphics.rs).
    pub gfx: crate::graphics::Graphics,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            name: "Spieler".into(),
            sensitivity: 2.0,
            invert: false,
            fov: 90.0,
            volume: 0.8,
            music: 0.5,
            crosshair_color: 0,
            crosshair_size: 9.0,
            crosshair_gap: 4.0,
            crosshair_thickness: 2.0,
            crosshair_dynamic: true,
            perf: 0,
            render_scale: 1.0,
            display: 0,
            resolution: None,
            vsync: false,
            difficulty: 1,
            last_map: "zg_hafen".into(),
            finishes: Default::default(),
            servers: Vec::new(),
            gfx: Default::default(),
        }
    }
}

pub const CROSSHAIR_COLORS: [(&str, Color); 5] = [
    ("Grün", Color::srgba(0.2, 1.0, 0.25, 0.9)),
    ("Gelb", Color::srgba(1.0, 0.9, 0.2, 0.9)),
    ("Cyan", Color::srgba(0.2, 0.95, 1.0, 0.9)),
    ("Weiß", Color::srgba(1.0, 1.0, 1.0, 0.9)),
    ("Magenta", Color::srgba(1.0, 0.3, 0.9, 0.9)),
];

/// The local player's name (read once at match start).
impl Options {
    /// The chosen finish of a weapon (an index into `finish::FINISHES`; 0: the factory's).
    pub fn finish_of(&self, w: zugriff_sim::weapons::WeaponId) -> u8 {
        self.finishes.get(w.key()).and_then(|k| crate::finish::FINISHES.iter().position(|f| f.key == k)).unwrap_or(0) as u8
    }

    /// Every weapon's finish, by `WeaponId as usize` (as the sim's `Player::finishes` has them).
    pub fn finish_table(&self) -> Vec<u8> {
        zugriff_sim::weapons::WeaponId::ALL.iter().map(|w| (*w as usize, self.finish_of(*w))).fold(Vec::new(), |mut v, (i, f)| {
            if v.len() <= i {
                v.resize(i + 1, 0);
            }
            v[i] = f;
            v
        })
    }

    pub fn set_finish(&mut self, w: zugriff_sim::weapons::WeaponId, f: u8) {
        if f == 0 {
            self.finishes.remove(w.key());
        } else {
            self.finishes.insert(w.key().to_string(), crate::finish::finish(f).key.to_string());
        }
    }
}

pub fn player_name() -> String {
    let n = load().name;
    if n.trim().is_empty() { "Spieler".into() } else { n }
}

#[cfg(target_arch = "wasm32")]
fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

pub fn load() -> Options {
    #[cfg(target_arch = "wasm32")]
    let text = storage().and_then(|s| s.get_item("zugriff.options").ok().flatten());
    #[cfg(not(target_arch = "wasm32"))]
    let text = path().and_then(|p| std::fs::read_to_string(p).ok());
    text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from)?;
    #[cfg(target_os = "macos")]
    let base = std::path::PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config")))?;
    Some(base.join("zugriff/options.json"))
}

fn save_on_change(options: Res<Options>) {
    if !options.is_changed() || options.is_added() {
        return;
    }
    let Ok(text) = serde_json::to_string_pretty(&*options) else { return };
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = storage() {
        let _ = s.set_item("zugriff.options", &text);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(p) = path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, text);
    }
}

/// On the web the canvas renders at `render_scale` pixels per CSS pixel (the device's own ratio when 0).
/// The display modes' names, for the options menu.
pub const DISPLAY_NAMES: [&str; 3] = ["Fenster", "Randlos", "Vollbild"];

/// The resolutions to offer (physical pixels, largest first): the monitor's modes and the usual sizes that fit on it,
/// for the options menu. In exclusive fullscreen a size the monitor has no mode for keeps its current mode. Empty on
/// the web, where the page's size is the browser's.
#[derive(Resource, Default, PartialEq)]
pub struct Resolutions(pub Vec<UVec2>);

/// The primary monitor, or (when none is marked primary, as on some X11 setups) the first one.
fn monitor<'a>(monitors: &'a Query<(&bevy::window::Monitor, Has<bevy::window::PrimaryMonitor>)>) -> Option<&'a bevy::window::Monitor> {
    monitors.iter().find(|(_, primary)| *primary).or_else(|| monitors.iter().next()).map(|(m, _)| m)
}

fn list_resolutions(monitors: Query<(&bevy::window::Monitor, Has<bevy::window::PrimaryMonitor>)>, mut res: ResMut<Resolutions>) {
    if cfg!(target_arch = "wasm32") {
        return;
    }
    let Some(m) = monitor(&monitors) else { return };
    // What the monitor reports (some systems report only its current mode), and the usual window sizes that fit on it.
    const USUAL: [(u32, u32); 9] = [(1280, 720), (1366, 768), (1600, 900), (1920, 1080), (2560, 1080), (2560, 1440), (3440, 1440), (3840, 1600), (3840, 2160)];
    let mut sizes: Vec<UVec2> = m.video_modes.iter().map(|v| v.physical_size).filter(|s| s.x >= 1024 && s.y >= 576).collect();
    sizes.extend(USUAL.iter().map(|&(w, h)| UVec2::new(w, h)).filter(|s| s.x <= m.physical_width && s.y <= m.physical_height));
    sizes.push(UVec2::new(m.physical_width, m.physical_height));
    sizes.sort_by(|a, b| (b.x * b.y).cmp(&(a.x * a.y)).then(b.x.cmp(&a.x)));
    sizes.dedup();
    if res.set_if_neq(Resolutions(sizes)) {
        info!("display: {} resolutions on {:?} ({}×{})", res.0.len(), m.name, m.physical_width, m.physical_height);
    }
}

/// Puts the window in the chosen mode, size and presentation — only when the choice changes, so a window the player
/// resizes by hand stays as they left it.
fn apply_display(
    options: Res<Options>,
    monitors: Query<(&bevy::window::Monitor, Has<bevy::window::PrimaryMonitor>)>,
    mut window: Single<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut applied: Local<Option<(u8, Option<(u32, u32)>, bool)>>,
) {
    use bevy::window::{MonitorSelection, PresentMode, VideoModeSelection, WindowMode};
    let key = (options.display, options.resolution, options.vsync);
    if *applied == Some(key) {
        return;
    }
    let first = applied.is_none();
    *applied = Some(key);
    if cfg!(target_arch = "wasm32") {
        // A browser only goes fullscreen right after a click: not on loading the page (the page starts windowed).
        let mode = if options.display > 0 && !first { WindowMode::BorderlessFullscreen(MonitorSelection::Current) } else { WindowMode::Windowed };
        if window.mode != mode {
            window.mode = mode;
        }
        return;
    }
    let monitor = monitor(&monitors);
    window.mode = match options.display {
        1 => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
        2 => {
            // The monitor's mode at that size, at its highest refresh rate.
            let pick = options.resolution.and_then(|(w, h)| {
                monitor?.video_modes.iter().filter(|v| v.physical_size == UVec2::new(w, h)).max_by_key(|v| (v.refresh_rate_millihertz, v.bit_depth)).copied()
            });
            WindowMode::Fullscreen(MonitorSelection::Current, pick.map_or(VideoModeSelection::Current, VideoModeSelection::Specific))
        }
        _ => WindowMode::Windowed,
    };
    if options.display == 0
        && let Some((w, h)) = options.resolution
    {
        // No bigger than the monitor.
        let (mw, mh) = monitor.map_or((w, h), |m| (m.physical_width, m.physical_height));
        window.resolution.set_physical_resolution(w.min(mw), h.min(mh));
    }
    window.present_mode = if options.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
}

fn apply_render_scale(options: Res<Options>, mut window: Single<&mut Window, With<bevy::window::PrimaryWindow>>) {
    if !cfg!(target_arch = "wasm32") {
        return;
    }
    let want = (options.render_scale > 0.0).then_some(options.render_scale);
    if window.resolution.scale_factor_override() != want {
        window.resolution.set_scale_factor_override(want);
    }
}
