//! The menus' look: colour tokens, glass panels, buttons that fade on hover, segmented choices, key caps and small
//! capital labels. The main menu, the pause menu, the options, the match end and the buy menu are built from these,
//! so they read as one piece.

use crate::hud::{text, Fonts, AMBER};
use bevy::prelude::*;
use bevy::text::LetterSpacing;

pub struct UiKitPlugin;

impl Plugin for UiKitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, fade_hover);
    }
}

pub const INK: Color = Color::srgb(0.02, 0.028, 0.036);
pub const GLASS: Color = Color::srgba(0.03, 0.04, 0.052, 0.93);
pub const GLASS_HI: Color = Color::srgba(0.075, 0.09, 0.11, 0.94);
pub const LINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.09);
pub const LINE_HI: Color = Color::srgba(1.0, 1.0, 1.0, 0.26);
pub const DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.52);
pub const SOFT: Color = Color::srgba(1.0, 1.0, 1.0, 0.8);
pub const AMBER_HI: Color = Color::srgb(1.0, 0.8, 0.3);
pub const AMBER_DEEP: Color = Color::srgb(0.93, 0.58, 0.0);
pub const MONEY: Color = Color::srgb(0.55, 1.0, 0.55);
pub const DANGER: Color = Color::srgb(1.0, 0.36, 0.3);

/// Colours a button fades between (in about a tenth of a second) as the pointer comes and goes.
#[derive(Component, Clone, Copy)]
pub struct Hover {
    pub bg: Color,
    pub bg_hot: Color,
    pub border: Color,
    pub border_hot: Color,
    pub t: f32,
}

impl Hover {
    pub fn new(bg: Color, bg_hot: Color, border: Color, border_hot: Color) -> Self {
        Hover { bg, bg_hot, border, border_hot, t: 0.0 }
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let (a, b) = (a.to_srgba(), b.to_srgba());
    Color::srgba(a.red + (b.red - a.red) * t, a.green + (b.green - a.green) * t, a.blue + (b.blue - a.blue) * t, a.alpha + (b.alpha - a.alpha) * t)
}

fn fade_hover(time: Res<Time>, mut q: Query<(&Interaction, &mut Hover, &mut BackgroundColor, Option<&mut BorderColor>)>) {
    let step = (time.delta_secs() * 11.0).min(1.0);
    for (i, mut h, mut bg, border) in &mut q {
        let goal = if matches!(i, Interaction::None) { 0.0 } else { 1.0 };
        if (h.t - goal).abs() < 1e-3 {
            continue;
        }
        h.t += (goal - h.t) * step;
        if (h.t - goal).abs() < 0.01 {
            h.t = goal;
        }
        bg.set_if_neq(BackgroundColor(mix(h.bg, h.bg_hot, h.t)));
        if let Some(mut b) = border {
            b.set_if_neq(BorderColor::all(mix(h.border, h.border_hot, h.t)));
        }
    }
}

/// Small spaced capitals: section titles, field names, tags.
pub fn caps(t: &str, font: &Handle<Font>, size: f32, color: Color) -> impl Bundle + use<> {
    (text(&t.to_uppercase(), font, size, color), LetterSpacing::Px(size * 0.14))
}

/// A section title: an amber tick and spaced capitals.
pub fn section(c: &mut ChildSpawnerCommands, fonts: &Fonts, title: &str) {
    c.spawn(Node { column_gap: Val::Px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
        r.spawn((Node { width: Val::Px(4.0), height: Val::Px(16.0), border_radius: BorderRadius::all(Val::Px(1.0)), ..default() }, BackgroundColor(AMBER)));
        r.spawn(caps(title, &fonts.bold, 17.0, DIM));
    });
}

/// A dark glass panel with a hairline edge and a soft shadow.
pub fn glass(node: Node) -> impl Bundle {
    (
        Node { border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(14.0)), ..node },
        BackgroundColor(GLASS),
        BorderColor::all(LINE),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.45), Val::Px(0.0), Val::Px(14.0), Val::Px(0.0), Val::Px(36.0)),
    )
}

/// A thin rule across a panel.
pub fn rule(c: &mut ChildSpawnerCommands) {
    c.spawn((Node { height: Val::Px(1.0), width: Val::Percent(100.0), ..default() }, BackgroundColor(LINE)));
}

/// The big call to action: amber, dark capitals, a glow.
pub fn primary<A: Component>(c: &mut ChildSpawnerCommands, fonts: &Fonts, label: &str, act: A, width: Val, height: f32, size: f32) {
    c.spawn((
        Button,
        Node { width, height: Val::Px(height), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(Val::Px(10.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
        BackgroundColor(AMBER),
        BorderColor::all(AMBER_HI),
        Hover::new(AMBER, AMBER_HI, AMBER_HI, Color::WHITE),
        BoxShadow::new(Color::srgba(1.0, 0.62, 0.0, 0.28), Val::Px(0.0), Val::Px(6.0), Val::Px(0.0), Val::Px(28.0)),
        act,
    ))
    .with_children(|b| {
        // A sheen over the top half.
        b.spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Px(0.0), height: Val::Percent(55.0), border_radius: BorderRadius::new(Val::Px(10.0), Val::Px(10.0), Val::Px(0.0), Val::Px(0.0)), ..default() },
            BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::percent(Color::srgba(1.0, 1.0, 1.0, 0.22), 0.0), ColorStop::percent(Color::srgba(1.0, 1.0, 1.0, 0.0), 100.0)])),
            Pickable::IGNORE,
        ));
        b.spawn((caps(label, &fonts.bold, size, INK), Pickable::IGNORE));
    });
}

/// A quiet button: glass with a hairline, lighter under the pointer.
pub fn ghost<A: Component>(c: &mut ChildSpawnerCommands, fonts: &Fonts, label: &str, act: A, width: Val, danger: bool) {
    let fg = if danger { DANGER } else { Color::WHITE };
    let hot_border = if danger { DANGER } else { LINE_HI };
    c.spawn((
        Button,
        Node { width, padding: UiRect::axes(Val::Px(22.0), Val::Px(13.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(Val::Px(9.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.045)),
        BorderColor::all(LINE),
        Hover::new(Color::srgba(1.0, 1.0, 1.0, 0.045), Color::srgba(1.0, 1.0, 1.0, 0.13), LINE, hot_border),
        act,
        children![(caps(label, &fonts.bold, 22.0, fg), Pickable::IGNORE)],
    ));
}

/// Choices side by side in one pill; the chosen one amber.
pub fn segmented<A: Component>(c: &mut ChildSpawnerCommands, fonts: &Fonts, items: Vec<(&str, A, bool)>) {
    c.spawn((
        Node { width: Val::Percent(100.0), padding: UiRect::all(Val::Px(4.0)), column_gap: Val::Px(4.0), border_radius: BorderRadius::all(Val::Px(10.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        BorderColor::all(LINE),
    ))
    .with_children(|r| {
        for (label, act, on) in items {
            let (bg, hot, fg) = if on { (AMBER, AMBER_HI, INK) } else { (Color::srgba(1.0, 1.0, 1.0, 0.0), Color::srgba(1.0, 1.0, 1.0, 0.09), SOFT) };
            r.spawn((
                Button,
                Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), padding: UiRect::vertical(Val::Px(10.0)), justify_content: JustifyContent::Center, border_radius: BorderRadius::all(Val::Px(7.0)), ..default() },
                BackgroundColor(bg),
                Hover::new(bg, hot, Color::NONE, Color::NONE),
                act,
                children![(text(label, &fonts.label, 22.0, fg), Pickable::IGNORE)],
            ));
        }
    });
}

/// A key cap: a raised chip with the key's name.
pub fn keycap(c: &mut ChildSpawnerCommands, fonts: &Fonts, key: &str) {
    c.spawn((
        Node {
            min_width: Val::Px(40.0),
            height: Val::Px(38.0),
            padding: UiRect::horizontal(Val::Px(11.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect { left: Val::Px(1.0), right: Val::Px(1.0), top: Val::Px(1.0), bottom: Val::Px(3.0) },
            border_radius: BorderRadius::all(Val::Px(7.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.07)),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.22)),
        children![text(key, &fonts.bold, 20.0, Color::WHITE)],
    ));
}

/// A small tag: spaced capitals on a tinted chip.
pub fn tag(c: &mut ChildSpawnerCommands, fonts: &Fonts, t: &str, color: Color) {
    let s = color.to_srgba();
    c.spawn((
        Node { padding: UiRect::axes(Val::Px(9.0), Val::Px(3.0)), border_radius: BorderRadius::all(Val::Px(5.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
        BackgroundColor(Color::srgba(s.red, s.green, s.blue, 0.14)),
        BorderColor::all(Color::srgba(s.red, s.green, s.blue, 0.45)),
        children![caps(t, &fonts.bold, 15.0, color)],
    ));
}
