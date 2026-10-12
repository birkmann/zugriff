//! The weapon slots: when the weapon in hand changes (the number keys 1–5, Q, the mouse wheel), what is carried shows
//! at the right for a moment, one row per weapon with its slot's number, its picture (the game's own render, in its
//! finish) and its name, the one in hand lit.

use bevy::prelude::*;
use zugriff_sim::weapons::{Slot, WeaponId};

use crate::hud::{text, Fonts, AMBER};
use crate::input::Ui;
use crate::preview::{Previews, Size};
use crate::session::Session;
use crate::ui_kit::{DIM, GLASS_HI, LINE};

pub struct SlotsPlugin;

impl Plugin for SlotsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SlotsState>().add_systems(Startup, spawn).add_systems(Update, show.after(crate::session::SessionSet));
    }
}

/// How long the list stays up after a change (s), and how long it fades.
const SHOW: f32 = 2.2;
const FADE: f32 = 0.4;

#[derive(Component)]
struct SlotsRoot;

#[derive(Resource, Default)]
struct SlotsState {
    active: Option<WeaponId>,
    left: f32,
    built: Option<u64>,
}

fn spawn(mut commands: Commands) {
    commands.spawn((
        Node { position_type: PositionType::Absolute, right: Val::Px(26.0), bottom: Val::Px(290.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), align_items: AlignItems::End, ..default() },
        Visibility::Hidden,
        SlotsRoot,
        GlobalZIndex(3),
    ));
}

fn slot_key(s: Slot) -> u8 {
    s.key()
}

#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    time: Res<Time>,
    session: Option<Res<Session>>,
    ui: Res<Ui>,
    fonts: Res<Fonts>,
    mut state: ResMut<SlotsState>,
    mut previews: ResMut<Previews>,
    mut images: ResMut<Assets<Image>>,
    root: Single<(Entity, &mut Visibility, Option<&Children>), With<SlotsRoot>>,
    mut rows: Query<(&mut BackgroundColor, &SlotRow)>,
) {
    let (e, mut vis, children) = root.into_inner();
    let Some(s) = session else {
        *vis = Visibility::Hidden;
        state.active = None;
        return;
    };
    let me = s.me();
    let inv = &me.inv;
    if state.active != Some(inv.active) {
        if state.active.is_some() {
            state.left = SHOW;
        }
        state.active = Some(inv.active);
    }
    state.left = (state.left - time.delta_secs()).max(0.0);
    let on = state.left > 0.0 && me.alive && !ui.menu && !ui.buy && !crate::args::flag("nohud");
    vis.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
    if !on {
        return;
    }
    let fade = (state.left / FADE).min(1.0);
    // The carried weapons, slot by slot (each grenade its own row under 4).
    let list: Vec<(u8, WeaponId, u8)> = [Slot::Primary, Slot::Secondary, Slot::Knife, Slot::Grenade, Slot::Bomb]
        .into_iter()
        .flat_map(|slot| inv.in_slot(slot).into_iter().map(move |w| (slot_key(slot), w)))
        .map(|(k, w)| (k, w, crate::finish::of(inv, w)))
        .collect();
    let key = list.iter().fold(inv.active as u64, |h, (k, w, f)| h.wrapping_mul(131).wrapping_add(*k as u64 * 1000 + *w as u64 * 10 + *f as u64));
    if state.built != Some(key) {
        state.built = Some(key);
        if let Some(ch) = children {
            for c in ch.iter() {
                commands.entity(c).despawn();
            }
        }
        let f = &*fonts;
        commands.entity(e).with_children(|col| {
            for (k, w, fin) in &list {
                let active = *w == inv.active;
                let picture = if w.def().is_gun() || matches!(w, WeaponId::Knife | WeaponId::He | WeaponId::Flash | WeaponId::Smoke) {
                    Some(previews.get(&mut images, *w, *fin, Size::Tile))
                } else {
                    None
                };
                col.spawn((
                    Node { width: Val::Px(300.0), height: Val::Px(54.0), align_items: AlignItems::Center, column_gap: Val::Px(12.0), padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(8.0)), ..default() },
                    BackgroundColor(if active { Color::srgba(1.0, 0.69, 0.0, 0.2) } else { GLASS_HI.with_alpha(0.7) }),
                    BorderColor::all(if active { AMBER } else { LINE }),
                    SlotRow(active),
                ))
                .with_children(|r| {
                    r.spawn(text(&k.to_string(), &f.digits, 24.0, if active { AMBER } else { DIM }));
                    r.spawn(Node { width: Val::Px(124.0), height: Val::Px(46.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|m| {
                        if let Some(p) = picture.clone() {
                            m.spawn((Node { height: Val::Px(44.0), aspect_ratio: Some(Size::ASPECT), ..default() }, ImageNode::new(p).with_color(if active { Color::WHITE } else { Color::srgba(1.0, 1.0, 1.0, 0.6) })));
                        }
                    });
                    r.spawn(text(w.name(), &f.label, 19.0, if active { Color::WHITE } else { DIM }));
                });
            }
        });
    }
    // The list fades out at the end.
    for (mut bg, row) in &mut rows {
        let a = if row.0 { 0.2 } else { 0.7 } * fade;
        let c = if row.0 { Color::srgba(1.0, 0.69, 0.0, a) } else { GLASS_HI.with_alpha(a) };
        bg.set_if_neq(BackgroundColor(c));
    }
}

#[derive(Component)]
struct SlotRow(bool);
