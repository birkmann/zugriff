//! The local player's input: keys and buttons → the sim's command for the next tick. Quick taps between two ticks are
//! latched so they aren't lost. While a menu is open (buy menu, scoreboard, pause), the game doesn't see the keys.
//!
//! Default keys (the genre's): WASD move, Space jump, Ctrl or C crouch (Ctrl+W closes a browser tab outside
//! fullscreen, hence C), Shift walk, left/right mouse fire/secondary, R reload, E use, G drop, 1–5 weapon slots,
//! Q last weapon, wheel next/previous, B buy menu, Tab scoreboard, Z/X/C radio (X and Z; C is crouch on the web), Esc menu.

use crate::view::Look;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use zugriff_sim::game::{Btn, Buy, Select};
use zugriff_sim::weapons::Slot;

pub struct InputPlugin;

/// Reading the input (the overlays and the game read it after).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InputSet;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Held>().init_resource::<Ui>().add_systems(Update, (virtual_cursor, read).chain().in_set(InputSet).after(crate::view::MouseLook));
    }
}

/// Input gathered between ticks.
#[derive(Resource, Default)]
pub struct Held {
    pub forward: f32,
    pub side: f32,
    pub buttons: Btn,
    /// Presses seen since the last tick (so a tap shorter than a tick still counts).
    pub latched: Btn,
    pub select: Option<Select>,
    pub buy: Vec<Buy>,
}

/// Which overlays are up (they take the keys and the mouse from the game).
#[derive(Resource, Default)]
pub struct Ui {
    /// The main or pause menu.
    pub menu: bool,
    pub buy: bool,
    pub scoreboard: bool,
    pub radio: Option<u8>,
    /// The buy menu's cursor (pixels from the top left, in UI units), moved by the locked mouse.
    pub cursor: Vec2,
    pub clicked: bool,
    /// Pressed keys 1–9 while a numbered menu is open.
    pub digit: Option<u8>,
    /// Typing a chat line (online): the keys are the text's.
    pub typing: bool,
}

impl Ui {
    /// Whether the game (movement, look, weapons) gets the input.
    pub fn playing(&self) -> bool {
        !self.menu && !self.buy && self.radio.is_none() && !self.typing
    }
}

const DIGITS: [KeyCode; 9] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9];

fn virtual_cursor(mut ui: ResMut<Ui>, motion: Res<AccumulatedMouseMotion>, buttons: Res<ButtonInput<MouseButton>>, window: Single<&Window>, scale: Res<UiScale>) {
    ui.clicked = false;
    if !ui.buy {
        let w = window.width() / scale.0;
        let h = window.height() / scale.0;
        ui.cursor = Vec2::new(w * 0.5, h * 0.5);
        return;
    }
    let w = window.width() / scale.0;
    let h = window.height() / scale.0;
    ui.cursor = (ui.cursor + motion.delta / scale.0).clamp(Vec2::ZERO, Vec2::new(w, h));
    ui.clicked = buttons.just_pressed(MouseButton::Left);
}

#[allow(clippy::too_many_arguments)]
fn read(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    look: Res<Look>,
    mut ui: ResMut<Ui>,
    mut held: ResMut<Held>,
    session: Option<Res<crate::session::Session>>,
) {
    ui.digit = DIGITS.iter().position(|k| keys.just_pressed(*k)).map(|i| i as u8 + 1);
    let in_match = session.is_some();
    // Overlays first.
    if in_match && look.locked && !ui.menu && !ui.typing {
        if keys.just_pressed(KeyCode::KeyB) {
            ui.buy = !ui.buy;
            ui.radio = None;
        }
        ui.scoreboard = keys.pressed(KeyCode::Tab);
        let radio_key = if keys.just_pressed(KeyCode::KeyZ) {
            Some(1)
        } else if keys.just_pressed(KeyCode::KeyX) {
            Some(2)
        } else {
            None
        };
        if let Some(r) = radio_key {
            ui.radio = if ui.radio == Some(r) { None } else { Some(r) };
            ui.buy = false;
        }
    } else if !look.locked {
        // (A screenshot run with `--menu buy` keeps the menu open without a captured mouse.)
        ui.buy = ui.buy && crate::args::get("menu").as_deref() == Some("buy");
        ui.radio = None;
        // (`--scoreboard` keeps it up without a captured mouse, for screenshots.)
        ui.scoreboard = in_match && crate::args::flag("scoreboard");
    }
    let playing = in_match && look.locked && ui.playing();
    let key = |k: KeyCode| playing && keys.pressed(k);
    let axis = |a: KeyCode, b: KeyCode| (if key(a) { 1.0 } else { 0.0 }) - (if key(b) { 1.0 } else { 0.0 });
    held.forward = axis(KeyCode::KeyW, KeyCode::KeyS);
    held.side = axis(KeyCode::KeyD, KeyCode::KeyA);
    let mut b = Btn::empty();
    let click = |m: MouseButton| playing && mouse.pressed(m);
    b.set(Btn::ATTACK, click(MouseButton::Left));
    b.set(Btn::ATTACK2, click(MouseButton::Right));
    b.set(Btn::RELOAD, key(KeyCode::KeyR));
    b.set(Btn::USE, key(KeyCode::KeyE));
    b.set(Btn::JUMP, key(KeyCode::Space));
    b.set(Btn::DUCK, key(KeyCode::ControlLeft) || key(KeyCode::KeyC));
    b.set(Btn::WALK, key(KeyCode::ShiftLeft));
    b.set(Btn::DROP, key(KeyCode::KeyG));
    b.set(Btn::INSPECT, key(KeyCode::KeyF));
    held.buttons = b;
    if playing {
        let mut latch = |k: bool, f: Btn| {
            if k {
                held.latched |= f;
            }
        };
        latch(keys.just_pressed(KeyCode::Space), Btn::JUMP);
        latch(keys.just_pressed(KeyCode::KeyG), Btn::DROP);
        latch(keys.just_pressed(KeyCode::KeyE), Btn::USE);
        latch(keys.just_pressed(KeyCode::KeyR), Btn::RELOAD);
        latch(mouse.just_pressed(MouseButton::Left), Btn::ATTACK);
        latch(mouse.just_pressed(MouseButton::Right), Btn::ATTACK2);
        let slots = [(KeyCode::Digit1, Slot::Primary), (KeyCode::Digit2, Slot::Secondary), (KeyCode::Digit3, Slot::Knife), (KeyCode::Digit4, Slot::Grenade), (KeyCode::Digit5, Slot::Bomb)];
        for (k, s) in slots {
            if keys.just_pressed(k) {
                held.select = Some(Select::Slot(s));
            }
        }
        if keys.just_pressed(KeyCode::KeyQ) {
            held.select = Some(Select::Last);
        }
        if scroll.delta.y > 0.5 {
            held.select = Some(Select::Prev);
        } else if scroll.delta.y < -0.5 {
            held.select = Some(Select::Next);
        }
    }
}
