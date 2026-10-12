//! Scripted checks for the native build (the reliable automated loop; headless WebGPU pages freeze).
//!
//! `--shot <path.png>` waits until the level is in and has rendered for a while, saves a screenshot and exits.
//! `--shots <n> --every <s>` takes n screenshots, `s` seconds apart (`<path>_1.png` …). The camera starts where
//! `--pose x,y,z,yaw,pitch` says (see `args`); `--wait <s>` sets the first wait (default 5 s — shaders compile in the
//! background at first, and nothing is drawn until they are ready). `--autoplay` (with `--play <map>`) lets a bot
//! play for the local player, so a match can be watched from its eyes. `--give <weapon>` (its short name, e.g. `g36`)
//! puts that weapon in the local player's hands, `--ads` holds the right button (down the sights), and `--look
//! yaw,pitch` (degrees) turns the view: with `--nobots`, a still sight picture for checking the sights' alignment.
//! `--fire` holds the left button once the weapon is in hand (a burst, knife swings, a grenade's pin out), and
//! `--duck` crouches. `--release <s>` lets go of it after s seconds (the grenade is thrown); `--fire-at <s>` starts it later than 1 s.

use crate::level::Current;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

pub struct QaPlugin;

impl Plugin for QaPlugin {
    fn build(&self, app: &mut App) {
        let args: Vec<String> = std::env::args().collect();
        let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
        if arg("--give").is_some() || args.iter().any(|a| a == "--ads" || a == "--fire" || a == "--duck") || arg("--look").is_some() {
            app.add_systems(Update, hold.after(crate::input::InputSet));
        }
        if arg("--keys").is_some() {
            app.add_systems(PreUpdate, press_keys.after(bevy::input::InputSystems));
        }
        if arg("--clicks").is_some() {
            app.add_systems(First, click_ui.before(bevy::picking::PickingSystems::Input));
        }
        let Some(shot) = arg("--shot") else { return };
        let wait = arg("--wait").and_then(|f| f.parse().ok()).unwrap_or(5.0);
        let shots = arg("--shots").and_then(|f| f.parse().ok()).unwrap_or(1);
        let every = arg("--every").and_then(|f| f.parse().ok()).unwrap_or(3.0);
        app.insert_resource(Script { shot, wait, shots, every, waited: 0.0, taken: 0, counted: 0, last_taken_at: None }).add_systems(Last, run);
    }
}

#[derive(Resource)]
struct Script {
    shot: String,
    wait: f32,
    shots: u32,
    every: f32,
    waited: f32,
    taken: u32,
    counted: u32,
    last_taken_at: Option<u32>,
}

fn run(mut commands: Commands, mut script: ResMut<Script>, current: Res<Current>, mut exit: MessageWriter<AppExit>, time: Res<Time>) {
    if !current.spawned {
        return;
    }
    script.counted += 1;
    script.waited += time.delta_secs();
    let due = script.wait + script.taken as f32 * script.every;
    if script.taken < script.shots && script.waited >= due {
        let path = if script.shots == 1 { script.shot.clone() } else { script.shot.replace(".png", &format!("_{}.png", script.taken + 1)) };
        info!("qa: screenshot → {path}");
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        script.taken += 1;
        script.last_taken_at = Some(script.counted);
    }
    // Give the last capture a few frames to reach the disk.
    if script.taken >= script.shots && script.last_taken_at.is_some_and(|t| script.counted > t + 20) {
        exit.write(AppExit::Success);
    }
}

/// `--give`, `--ads`, `--look`: the weapon once the match is on, then the right button and the view every frame.
fn hold(session: Option<ResMut<crate::session::Session>>, mut held: ResMut<crate::input::Held>, mut look: ResMut<crate::view::Look>, mut given: Local<bool>, time: Res<Time>, mut since: Local<f32>) {
    let Some(mut s) = session else { return };
    // After the spawn has handed out the round's weapons.
    if !*given && s.ticks > 40 {
        *given = true;
        if let Some(w) = crate::args::get("give").and_then(|n| zugriff_sim::weapons::WeaponId::ALL.into_iter().find(|w| w.key() == n)) {
            let me = s.me;
            let gun = s.game.players[me].issue(w);
            let inv = &mut s.game.players[me].inv;
            use zugriff_sim::weapons::{Slot, WeaponId};
            match (w, w.def().slot()) {
                (WeaponId::He, _) => inv.he = 1,
                (WeaponId::Flash, _) => inv.flash = 1,
                (WeaponId::Smoke, _) => inv.smoke = 1,
                (WeaponId::Knife, _) => {}
                (_, Slot::Primary) => inv.primary = Some(gun),
                _ => inv.secondary = Some(gun),
            }
            inv.select(w);
        }
    }
    if *given {
        *since += time.delta_secs();
    }
    // `--fire-at` seconds after the weapon is in hand (default 1, once drawn), for as long as `--release` allows.
    let release = crate::args::get("release").and_then(|v| v.parse::<f32>().ok()).unwrap_or(f32::INFINITY);
    let at = crate::args::get("fire-at").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
    if crate::args::flag("fire") && *since > at && *since < at + release {
        // `--button 2`: the right button instead (the knife's stab, a grenade's underhand lob).
        held.buttons |= if crate::args::get("button").as_deref() == Some("2") { zugriff_sim::game::Btn::ATTACK2 } else { zugriff_sim::game::Btn::ATTACK };
    }
    if crate::args::flag("duck") {
        held.buttons |= zugriff_sim::game::Btn::DUCK;
    }
    if crate::args::flag("ads") {
        held.buttons |= zugriff_sim::game::Btn::ATTACK2;
    }
    // `--zoom <level>`: a scoped weapon held at that scope level once drawn (a right-click's edge is easily lost).
    if let Some(z) = crate::args::get("zoom").and_then(|v| v.parse::<u8>().ok())
        && *since > 1.5
    {
        let me = s.me;
        s.game.players[me].inv.zoom = z;
    }
    if let Some(v) = crate::args::get("look") {
        let v: Vec<f32> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if let [yaw, pitch] = v[..] {
            look.yaw = yaw.to_radians();
            look.pitch = pitch.to_radians();
        }
    }
}

/// `--keys "3@4,2@6"`: presses the number keys at those seconds after start (with `--qa-lock`, as if playing).
fn press_keys(mut keys: ResMut<ButtonInput<KeyCode>>, time: Res<Time>, mut done: Local<Vec<usize>>, mut held: Local<Option<KeyCode>>) {
    if let Some(k) = held.take() {
        keys.release(k);
    }
    let spec = crate::args::get("keys").unwrap_or_default();
    for (i, part) in spec.split(',').enumerate() {
        let Some((k, t)) = part.split_once('@') else { continue };
        let Ok(t) = t.parse::<f32>() else { continue };
        if done.contains(&i) || time.elapsed_secs() < t {
            continue;
        }
        let code = match k {
            "1" => KeyCode::Digit1,
            "2" => KeyCode::Digit2,
            "3" => KeyCode::Digit3,
            "4" => KeyCode::Digit4,
            "5" => KeyCode::Digit5,
            "q" => KeyCode::KeyQ,
            _ => continue,
        };
        done.push(i);
        info!("qa: pressing {code:?}");
        keys.press(code);
        *held = Some(code);
        return;
    }
}

/// `--clicks "500,250@5;180,640@9"`: click UI coordinates in the 1080-high design space.
/// Feed window input, not button actions, so captures exercise normal UI hit testing.
fn click_ui(
    window: Single<(Entity, &mut Window), With<bevy::window::PrimaryWindow>>,
    mut events: MessageWriter<bevy::window::WindowEvent>,
    mut mouse: MessageWriter<bevy::input::mouse::MouseButtonInput>,
    time: Res<Time>,
    mut done: Local<Vec<usize>>,
    mut release: Local<bool>,
) {
    use bevy::input::{ButtonState, mouse::MouseButtonInput};
    use bevy::window::{CursorMoved, WindowEvent};
    let (entity, mut window) = window.into_inner();
    if *release {
        let input = MouseButtonInput { window: entity, button: MouseButton::Left, state: ButtonState::Released };
        mouse.write(input);
        events.write(WindowEvent::MouseButtonInput(input));
        *release = false;
    }
    for (i, part) in crate::args::get("clicks").unwrap_or_default().split(';').enumerate() {
        let Some((xy, at)) = part.split_once('@') else { continue };
        let Some((x, y)) = xy.split_once(',') else { continue };
        let (Ok(x), Ok(y), Ok(at)) = (x.parse::<f32>(), y.parse::<f32>(), at.parse::<f32>()) else { continue };
        if done.contains(&i) || time.elapsed_secs() < at { continue; }
        let position = Vec2::new(x, y) * (window.height() / 1080.0).clamp(0.45, 2.0);
        // Update the position consumed by UI without asking the OS to warp its cursor
        // (Wayland disallows warping an unlocked cursor).
        window.bypass_change_detection().set_cursor_position(Some(position));
        events.write(WindowEvent::CursorMoved(CursorMoved { window: entity, position, delta: None }));
        let input = MouseButtonInput { window: entity, button: MouseButton::Left, state: ButtonState::Pressed };
        mouse.write(input);
        events.write(WindowEvent::MouseButtonInput(input));
        info!("qa: clicking UI at {x},{y}");
        done.push(i);
        *release = true;
        break;
    }
}
