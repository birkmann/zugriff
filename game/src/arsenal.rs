//! The arsenal (a lobby tab): every gun of a side in four columns, each card showing the gun in the finish chosen for
//! it; the panel on the right shows the picked gun large, its finish, and every finish it can wear as a swatch (the
//! gun itself in that paint). A click on a swatch paints the gun: `Options::finishes`, saved with the options, and
//! from the next match on what one buys or is issued comes in it.
//!
//! The menu (`menu.rs`) only puts up an [`ArsenalRoot`] for the tab; this module fills it, and rebuilds it when the
//! side, the pick or a finish changes. The pictures are the game's own renders (`preview.rs`).

use bevy::prelude::*;
use bevy::text::LetterSpacing;
use zugriff_sim::game::Team;
use zugriff_sim::weapons::{Class, WeaponId};

use crate::finish::{finish, FINISHES};
use crate::hud::{text, Fonts, AMBER, KSK, OPFOR};
use crate::options::Options;
use crate::preview::{Previews, Size};
use crate::ui_kit::{caps, section, tag, Hover, DIM, GLASS_HI, LINE, LINE_HI, SOFT};

pub struct ArsenalPlugin;

impl Plugin for ArsenalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Arsenal>().add_systems(Update, (clicks, fill).chain());
    }
}

/// Where the menu wants the arsenal.
#[derive(Component)]
pub struct ArsenalRoot;

#[derive(Resource)]
struct Arsenal {
    side: Team,
    pick: WeaponId,
    /// What the root was last filled with.
    built: Option<(Entity, u64)>,
}

impl Default for Arsenal {
    fn default() -> Self {
        Arsenal { side: Team::Ksk, pick: WeaponId::G95, built: None }
    }
}

#[derive(Component, Clone, Copy)]
enum Act {
    Side(Team),
    Pick(WeaponId),
    Paint(WeaponId, u8),
}

const COLUMNS: [(&str, &[WeaponId]); 4] = [
    ("Pistolen", &[WeaponId::P8, WeaponId::Mp443, WeaponId::P30, WeaponId::Rev357]),
    ("Maschinenpistolen", &[WeaponId::Mp7, WeaponId::Pp19, WeaponId::Mp5, WeaponId::Ump]),
    ("Schwere Waffen", &[WeaponId::M870, WeaponId::M1014, WeaponId::Mg5, WeaponId::Pkm]),
    ("Gewehre", &[WeaponId::G36, WeaponId::G95, WeaponId::Ak74, WeaponId::Akm, WeaponId::Ssg3000, WeaponId::G28, WeaponId::Svd, WeaponId::G22]),
];

fn guns(side: Team, list: &[WeaponId]) -> Vec<WeaponId> {
    list.iter().copied().filter(|w| side.may_buy(w.def().side)).collect()
}

fn class_name(w: WeaponId) -> &'static str {
    match w.def().class {
        Class::Pistol => "Pistole",
        Class::Smg => "Maschinenpistole",
        Class::Shotgun => "Flinte",
        Class::Rifle => "Sturmgewehr",
        Class::Sniper => "Scharfschützengewehr",
        Class::Mg => "Maschinengewehr",
        _ => "",
    }
}

fn clicks(q: Query<(&Interaction, &Act), (Changed<Interaction>, With<Button>)>, mut arsenal: ResMut<Arsenal>, mut options: ResMut<Options>, mut cues: MessageWriter<crate::audio::Cue>) {
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
        match *act {
            Act::Side(t) => {
                arsenal.side = t;
                if !t.may_buy(arsenal.pick.def().side) {
                    arsenal.pick = if t == Team::Ksk { WeaponId::G95 } else { WeaponId::Ak74 };
                }
            }
            Act::Pick(w) => arsenal.pick = w,
            Act::Paint(w, f) => {
                if options.finish_of(w) != f {
                    options.set_finish(w, f);
                }
            }
        }
    }
}

/// Fills the root (again) when it is new or what it shows has changed.
#[allow(clippy::too_many_arguments)]
fn fill(
    mut commands: Commands,
    mut arsenal: ResMut<Arsenal>,
    options: Res<Options>,
    fonts: Res<Fonts>,
    root: Option<Single<(Entity, Option<&Children>), With<ArsenalRoot>>>,
    mut previews: ResMut<Previews>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(root) = root else {
        arsenal.built = None;
        return;
    };
    let (e, children) = root.into_inner();
    let mut key = arsenal.side as u64 * 1000 + arsenal.pick as u64;
    for w in WeaponId::ALL {
        key = key.wrapping_mul(31).wrapping_add(options.finish_of(w) as u64);
    }
    if arsenal.built == Some((e, key)) {
        return;
    }
    arsenal.built = Some((e, key));
    if let Some(ch) = children {
        for c in ch.iter() {
            commands.entity(c).despawn();
        }
    }
    let f = &*fonts;
    let (side, pick) = (arsenal.side, arsenal.pick);
    let mut pic = |w: WeaponId, fin: u8, size: Size| previews.get(&mut images, w, fin, size);
    commands.entity(e).with_children(|body| {
        // Left: the side and its guns.
        body.spawn(Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, row_gap: Val::Px(18.0), min_width: Val::Px(0.0), ..default() }).with_children(|left| {
            left.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(18.0), ..default() }).with_children(|r| {
                r.spawn((text("ARSENAL", &f.bold, 44.0, Color::WHITE), LetterSpacing::Px(3.0)));
                for t in [Team::Ksk, Team::Opfor] {
                    let on = side == t;
                    let col = if t == Team::Ksk { KSK } else { OPFOR };
                    let bg = if on { col.with_alpha(0.22) } else { Color::srgba(1.0, 1.0, 1.0, 0.03) };
                    r.spawn((
                        Button,
                        Node { padding: UiRect::axes(Val::Px(22.0), Val::Px(9.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(8.0)), ..default() },
                        BackgroundColor(bg),
                        BorderColor::all(if on { col } else { LINE }),
                        Hover::new(bg, if on { bg } else { Color::srgba(1.0, 1.0, 1.0, 0.08) }, if on { col } else { LINE }, if on { col } else { LINE_HI }),
                        Act::Side(t),
                    ))
                    .with_children(|b| {
                        b.spawn((caps(t.name(), &f.bold, 18.0, if on { Color::WHITE } else { DIM }), Pickable::IGNORE));
                    });
                }
                r.spawn(Node { flex_grow: 1.0, ..default() });
                r.spawn(text("Lackierungen gelten ab dem nächsten Einsatz.", &f.text, 18.0, DIM));
            });
            left.spawn(Node { column_gap: Val::Px(14.0), flex_grow: 1.0, min_height: Val::Px(0.0), ..default() }).with_children(|cols| {
                for (name, list) in COLUMNS {
                    cols.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), ..default() }).with_children(|col| {
                        col.spawn(Node { padding: UiRect::axes(Val::Px(4.0), Val::Px(4.0)), border: UiRect::bottom(Val::Px(1.0)), ..default() }).insert(BorderColor::all(LINE)).with_children(|h| {
                            h.spawn(caps(name, &f.bold, 17.0, SOFT));
                        });
                        for w in guns(side, list) {
                            let fin = options.finish_of(w);
                            card(col, f, w, fin, pic(w, fin, Size::Tile), w == pick);
                        }
                    });
                }
            });
        });
        // Right: the gun picked, large, and its finishes.
        let fin = options.finish_of(pick);
        let fd = finish(fin);
        body.spawn((
            Node { width: Val::Px(640.0), flex_shrink: 0.0, flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), padding: UiRect::all(Val::Px(22.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(14.0)), ..default() },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.052, 0.94)),
            BorderColor::all(LINE),
        ))
        .with_children(|p| {
            p.spawn(caps(class_name(pick), &f.bold, 15.0, AMBER));
            p.spawn((text(pick.name(), &f.bold, 50.0, Color::WHITE), LetterSpacing::Px(1.0)));
            p.spawn((
                Node { width: Val::Percent(100.0), height: Val::Px(196.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(Val::Px(12.0)), ..default() },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.025)),
                BackgroundGradient::from(RadialGradient::new(UiPosition::CENTER, RadialGradientShape::Ellipse(Val::Percent(55.0), Val::Percent(60.0)), vec![ColorStop::percent(fd.grade.color().with_alpha(0.2), 0.0), ColorStop::percent(fd.grade.color().with_alpha(0.0), 100.0)])),
            ))
            .with_children(|m| {
                m.spawn((Node { height: Val::Px(186.0), aspect_ratio: Some(Size::ASPECT), ..default() }, ImageNode::new(pic(pick, fin, Size::Large))));
            });
            p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(12.0), ..default() }).with_children(|r| {
                r.spawn(text(fd.name, &f.bold, 30.0, fd.grade.color()));
                tag(r, f, fd.grade.name(), fd.grade.color());
            });
            p.spawn(text(fd.about, &f.text, 19.0, SOFT));
            p.spawn(Node { height: Val::Px(4.0), ..default() });
            section(p, f, "Lackierung wählen");
            p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(8.0), row_gap: Val::Px(8.0), ..default() }).with_children(|g| {
                for (i, x) in FINISHES.iter().enumerate() {
                    swatch(g, f, pick, i as u8, x.name, x.grade.color(), pic(pick, i as u8, Size::Tile), i as u8 == fin);
                }
            });
        });
    });
}

/// A gun's card: its name, its picture in its finish, the finish's name over a bar in its grade's colour.
fn card(c: &mut ChildSpawnerCommands, f: &Fonts, w: WeaponId, fin: u8, image: Handle<Image>, on: bool) {
    let fd = finish(fin);
    let (bg, border) = if on { (Color::srgba(1.0, 0.69, 0.0, 0.14), AMBER) } else { (GLASS_HI, LINE) };
    c.spawn((
        Button,
        Node { height: Val::Px(104.0), flex_direction: FlexDirection::Column, padding: UiRect::new(Val::Px(12.0), Val::Px(12.0), Val::Px(8.0), Val::Px(0.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(10.0)), overflow: Overflow::clip(), ..default() },
        BackgroundColor(bg),
        BorderColor::all(border),
        Hover::new(bg, if on { bg } else { Color::srgba(1.0, 1.0, 1.0, 0.07) }, border, if on { border } else { LINE_HI }),
        Act::Pick(w),
    ))
    .with_children(|t| {
        t.spawn((Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|r| {
            r.spawn((text(w.name(), &f.label, 19.0, Color::WHITE), Pickable::IGNORE));
            r.spawn((caps(if fin == 0 { "" } else { fd.name }, &f.bold, 12.0, fd.grade.color()), Pickable::IGNORE));
        });
        t.spawn((Node { flex_grow: 1.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, min_height: Val::Px(0.0), ..default() }, Pickable::IGNORE)).with_children(|m| {
            m.spawn((Node { height: Val::Px(62.0), aspect_ratio: Some(Size::ASPECT), ..default() }, ImageNode::new(image), Pickable::IGNORE));
        });
        t.spawn((Node { height: Val::Px(3.0), margin: UiRect::horizontal(Val::Px(-12.0)), ..default() }, BackgroundColor(if fin == 0 { Color::NONE } else { fd.grade.color() }), Pickable::IGNORE));
    });
}

/// A finish to pick: the gun in it, its name, its grade's bar.
#[allow(clippy::too_many_arguments)]
fn swatch(c: &mut ChildSpawnerCommands, f: &Fonts, w: WeaponId, i: u8, name: &str, grade: Color, image: Handle<Image>, on: bool) {
    let (bg, border) = if on { (Color::srgba(1.0, 0.69, 0.0, 0.14), AMBER) } else { (Color::srgba(1.0, 1.0, 1.0, 0.035), LINE) };
    c.spawn((
        Button,
        Node { width: Val::Px(136.0), height: Val::Px(86.0), flex_direction: FlexDirection::Column, padding: UiRect::new(Val::Px(6.0), Val::Px(6.0), Val::Px(4.0), Val::Px(0.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(8.0)), overflow: Overflow::clip(), ..default() },
        BackgroundColor(bg),
        BorderColor::all(border),
        Hover::new(bg, if on { bg } else { Color::srgba(1.0, 1.0, 1.0, 0.08) }, border, if on { border } else { LINE_HI }),
        Act::Paint(w, i),
    ))
    .with_children(|t| {
        t.spawn((Node { flex_grow: 1.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, min_height: Val::Px(0.0), ..default() }, Pickable::IGNORE)).with_children(|m| {
            m.spawn((Node { height: Val::Px(46.0), aspect_ratio: Some(Size::ASPECT), ..default() }, ImageNode::new(image), Pickable::IGNORE));
        });
        t.spawn((text(name, &f.label, 15.0, if on { Color::WHITE } else { SOFT }), Pickable::IGNORE));
        t.spawn((Node { height: Val::Px(3.0), margin: UiRect::new(Val::Px(-6.0), Val::Px(-6.0), Val::Px(3.0), Val::Px(0.0)), ..default() }, BackgroundColor(grade), Pickable::IGNORE));
    });
}
