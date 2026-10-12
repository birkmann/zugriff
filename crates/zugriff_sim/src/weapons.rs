//! The arsenal: every weapon's numbers, and the bookkeeping of carrying them (magazines, reloads, drawing, the fire
//! rate, the accuracy that worsens as a burst goes on and recovers after it, the recoil that walks the muzzle).
//!
//! The numbers follow the genre's familiar values (damage per round, armour penetration ratio, range falloff per 500
//! units, cycle times, magazines, prices), so a tap, a burst and a spray feel the way players expect. Weapons carry
//! their real designations; the numbers are tuned for play, not taken from the real guns.

use crate::rng::Rng;
use crate::units::METRES_PER_UNIT;
use serde::{Deserialize, Serialize};

const U: f32 = METRES_PER_UNIT as f32;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, PartialOrd, Ord)]
pub enum WeaponId {
    Knife,
    // Pistols.
    P8,
    Mp443,
    P30,
    Rev357,
    // Submachine guns.
    Mp7,
    Pp19,
    Mp5,
    Ump,
    // Shotguns.
    M870,
    M1014,
    // Rifles.
    G36,
    G95,
    Ak74,
    Akm,
    // Sniper rifles.
    Ssg3000,
    G22,
    G28,
    Svd,
    // Machine guns.
    Mg5,
    Pkm,
    // Grenades and the bomb.
    He,
    Flash,
    Smoke,
    Bomb,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Class {
    Knife,
    Pistol,
    Smg,
    Shotgun,
    Rifle,
    Sniper,
    Mg,
    Grenade,
    Bomb,
}

/// Where a weapon is carried (one of each, except grenades).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Slot {
    Primary,
    Secondary,
    Knife,
    Grenade,
    Bomb,
}

impl Slot {
    /// The number key that selects it (1–5).
    pub fn key(self) -> u8 {
        match self {
            Slot::Primary => 1,
            Slot::Secondary => 2,
            Slot::Knife => 3,
            Slot::Grenade => 4,
            Slot::Bomb => 5,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Side {
    Both,
    Ksk,
    Opfor,
}

/// How far a round strays (tangent of the angle, like the classic engines): in a state, `base + scale × accuracy`,
/// where accuracy is `offset + shots³ / divisor` capped at `max`. Shots are the recent burst: +1 a round, recovering at
/// `recovery` per second once the trigger rests.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Spread {
    pub stand: f32,
    pub crouch: f32,
    pub move_base: f32,
    pub move_scale: f32,
    pub air_base: f32,
    pub air_scale: f32,
    pub offset: f32,
    pub divisor: f32,
    pub max: f32,
    pub recovery: f32,
    /// Added when a scoped weapon is fired without the scope.
    pub unscoped: f32,
}

/// The muzzle's climb per round, as the classic "kick back": the first round kicks `up_base`/`lateral_base`, each
/// further round of the burst adds the modifiers, up to the maxima (degrees); the sideways drift turns round on a
/// one-in-`direction_change` chance per round.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Recoil {
    pub up_base: f32,
    pub lateral_base: f32,
    pub up_mod: f32,
    pub lateral_mod: f32,
    pub up_max: f32,
    pub lateral_max: f32,
    pub direction_change: u32,
}

#[derive(Clone, Debug)]
pub struct WeaponDef {
    pub id: WeaponId,
    pub name: &'static str,
    pub class: Class,
    pub side: Side,
    pub price: i32,
    pub kill_reward: i32,
    /// Per round (per pellet for shotguns).
    pub damage: f32,
    /// Damage × this for every 500 units of range.
    pub range_mod: f32,
    /// Armour takes this × 0.5 of the damage off (higher: goes through armour better).
    pub armor_ratio: f32,
    pub pellets: u8,
    /// How much it goes through: metres of wood (see `Material::hardness`), and at most how many surfaces.
    pub penetration: f32,
    pub penetrations: u8,
    pub range: f32,
    /// Seconds between rounds.
    pub cycle: f32,
    pub auto: bool,
    pub clip: u16,
    pub reserve: u16,
    pub reload: f32,
    /// Loads a shell at a time: (start, per shell) seconds.
    pub shells: Option<(f32, f32)>,
    pub draw: f32,
    /// Running speed with it in hand, and scoped (m/s).
    pub speed: f32,
    pub scoped_speed: f32,
    pub spread: Spread,
    pub recoil: Recoil,
    /// Field-of-view factors per zoom level (empty: no scope).
    pub zoom: &'static [f32],
    /// A tracer every n rounds (0: none).
    pub tracer: u8,
}

impl WeaponDef {
    pub fn slot(&self) -> Slot {
        match self.class {
            Class::Knife => Slot::Knife,
            Class::Pistol => Slot::Secondary,
            Class::Grenade => Slot::Grenade,
            Class::Bomb => Slot::Bomb,
            _ => Slot::Primary,
        }
    }

    pub fn is_gun(&self) -> bool {
        !matches!(self.class, Class::Knife | Class::Grenade | Class::Bomb)
    }

    pub fn scoped(&self) -> bool {
        !self.zoom.is_empty()
    }

    /// Damage at `range` metres, before hit zone and armour.
    pub fn damage_at(&self, range: f32) -> f32 {
        self.damage * self.range_mod.powf(range / (500.0 * U))
    }
}

const fn spread(stand: f32, move_base: f32, air_base: f32, offset: f32, divisor: f32, max: f32) -> Spread {
    Spread { stand, crouch: stand * 0.75, move_base, move_scale: stand * 1.6, air_base, air_scale: 0.3, offset, divisor, max, recovery: 8.0, unscoped: 0.0 }
}

const fn recoil(up_base: f32, lateral_base: f32, up_mod: f32, lateral_mod: f32, up_max: f32, lateral_max: f32, direction_change: u32) -> Recoil {
    Recoil { up_base, lateral_base, up_mod, lateral_mod, up_max, lateral_max, direction_change }
}

const NONE_SPREAD: Spread = spread(0.0, 0.0, 0.0, 0.0, 1.0, 0.0);
const NONE_RECOIL: Recoil = recoil(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1);

const BASE: WeaponDef = WeaponDef {
    id: WeaponId::Knife,
    name: "",
    class: Class::Rifle,
    side: Side::Both,
    price: 0,
    kill_reward: 300,
    damage: 30.0,
    range_mod: 0.98,
    armor_ratio: 1.4,
    pellets: 1,
    penetration: 0.6,
    penetrations: 2,
    range: 8192.0 * U,
    cycle: 0.1,
    auto: true,
    clip: 30,
    reserve: 90,
    reload: 2.5,
    shells: None,
    draw: 1.0,
    speed: 221.0 * U,
    scoped_speed: 221.0 * U,
    spread: spread(0.025, 0.04, 0.04, 0.35, 200.0, 1.25),
    recoil: recoil(0.65, 0.35, 0.25, 0.015, 3.5, 2.25, 7),
    zoom: &[],
    tracer: 3,
};

const PISTOL: WeaponDef = WeaponDef {
    class: Class::Pistol,
    kill_reward: 300,
    penetration: 0.25,
    penetrations: 1,
    auto: false,
    draw: 0.8,
    speed: 250.0 * U,
    scoped_speed: 250.0 * U,
    tracer: 0,
    spread: Spread { stand: 0.012, crouch: 0.009, move_base: 0.03, move_scale: 0.02, air_base: 0.1, air_scale: 0.3, offset: 0.4, divisor: 6.0, max: 2.2, recovery: 5.0, unscoped: 0.0 },
    recoil: recoil(1.8, 0.4, 0.3, 0.05, 6.0, 2.0, 4),
    ..BASE
};

const SMG: WeaponDef = WeaponDef {
    class: Class::Smg,
    kill_reward: 600,
    penetration: 0.3,
    penetrations: 1,
    range_mod: 0.84,
    armor_ratio: 1.2,
    draw: 1.0,
    speed: 250.0 * U,
    scoped_speed: 250.0 * U,
    spread: spread(0.03, 0.03, 0.04, 0.45, 220.0, 0.8),
    recoil: recoil(0.45, 0.25, 0.14, 0.012, 3.0, 1.8, 7),
    tracer: 3,
    ..BASE
};

const SNIPER: WeaponDef = WeaponDef {
    class: Class::Sniper,
    auto: false,
    penetration: 1.0,
    penetrations: 3,
    zoom: &[40.0 / 90.0, 15.0 / 90.0],
    tracer: 1,
    // Scoped and standing still, a bolt-action rifle puts the round where the crosshair is (as in the genre).
    spread: Spread { stand: 0.0012, crouch: 0.0, move_base: 0.1, move_scale: 0.0, air_base: 0.3, air_scale: 0.0, offset: 1.0, divisor: 1e9, max: 1.0, recovery: 8.0, unscoped: 0.09 },
    recoil: recoil(2.0, 0.3, 0.0, 0.0, 4.0, 1.0, 2),
    ..BASE
};

const GRENADE: WeaponDef = WeaponDef {
    class: Class::Grenade,
    kill_reward: 300,
    damage: 0.0,
    pellets: 0,
    penetration: 0.0,
    penetrations: 0,
    auto: false,
    clip: 0,
    reserve: 0,
    draw: 0.7,
    speed: 245.0 * U,
    scoped_speed: 245.0 * U,
    spread: NONE_SPREAD,
    recoil: NONE_RECOIL,
    tracer: 0,
    cycle: 0.8,
    ..BASE
};

const DEFS: &[WeaponDef] = &[
    WeaponDef {
        id: WeaponId::Knife,
        name: "KM2000",
        class: Class::Knife,
        kill_reward: 1500,
        damage: 20.0,
        pellets: 0,
        penetration: 0.0,
        penetrations: 0,
        range: 58.0 * U,
        cycle: 0.4,
        clip: 0,
        reserve: 0,
        draw: 0.6,
        speed: 250.0 * U,
        scoped_speed: 250.0 * U,
        spread: NONE_SPREAD,
        recoil: NONE_RECOIL,
        tracer: 0,
        ..BASE
    },
    WeaponDef { id: WeaponId::P8, name: "P8", side: Side::Ksk, price: 200, damage: 34.0, range_mod: 0.79, armor_ratio: 1.0, clip: 15, reserve: 100, reload: 2.7, cycle: 0.15, ..PISTOL },
    WeaponDef { id: WeaponId::Mp443, name: "MP-443", side: Side::Opfor, price: 200, damage: 25.0, range_mod: 0.75, armor_ratio: 1.05, clip: 18, reserve: 120, reload: 2.3, cycle: 0.13, ..PISTOL },
    WeaponDef { id: WeaponId::P30, name: "P30", price: 300, damage: 38.0, range_mod: 0.8, armor_ratio: 1.25, clip: 13, reserve: 52, reload: 2.7, cycle: 0.15, ..PISTOL },
    WeaponDef {
        id: WeaponId::Rev357,
        name: ".357 Revolver",
        price: 700,
        damage: 54.0,
        range_mod: 0.81,
        armor_ratio: 1.5,
        penetration: 0.5,
        penetrations: 2,
        clip: 7,
        reserve: 35,
        reload: 2.2,
        cycle: 0.225,
        spread: Spread { stand: 0.02, crouch: 0.015, move_base: 0.06, move_scale: 0.05, air_base: 0.2, air_scale: 0.3, offset: 0.5, divisor: 3.0, max: 2.5, recovery: 3.0, unscoped: 0.0 },
        recoil: recoil(3.5, 0.6, 0.5, 0.1, 9.0, 3.0, 3),
        ..PISTOL
    },
    WeaponDef { id: WeaponId::Mp7, name: "MP7", side: Side::Ksk, price: 1500, damage: 29.0, range_mod: 0.85, armor_ratio: 1.25, cycle: 0.08, clip: 30, reserve: 120, reload: 3.1, speed: 245.0 * U, scoped_speed: 245.0 * U, ..SMG },
    WeaponDef { id: WeaponId::Pp19, name: "PP-19", side: Side::Opfor, price: 1250, damage: 27.0, range_mod: 0.8, armor_ratio: 1.15, cycle: 0.075, clip: 30, reserve: 120, reload: 2.6, speed: 245.0 * U, scoped_speed: 245.0 * U, ..SMG },
    WeaponDef { id: WeaponId::Mp5, name: "MP5", price: 1500, damage: 26.0, cycle: 0.075, clip: 30, reserve: 120, reload: 2.6, spread: spread(0.025, 0.028, 0.04, 0.45, 220.0, 0.75), ..SMG },
    WeaponDef { id: WeaponId::Ump, name: "UMP", price: 1200, damage: 30.0, range_mod: 0.82, armor_ratio: 1.3, cycle: 0.105, clip: 25, reserve: 100, reload: 3.4, ..SMG },
    WeaponDef {
        id: WeaponId::M870,
        name: "M870",
        class: Class::Shotgun,
        price: 1700,
        kill_reward: 900,
        damage: 26.0,
        range_mod: 0.7,
        armor_ratio: 1.0,
        pellets: 9,
        penetration: 0.1,
        penetrations: 1,
        range: 3000.0 * U,
        cycle: 0.88,
        auto: false,
        clip: 8,
        reserve: 32,
        reload: 0.0,
        shells: Some((0.45, 0.5)),
        speed: 220.0 * U,
        scoped_speed: 220.0 * U,
        spread: Spread { stand: 0.0675, crouch: 0.0675, move_base: 0.0675, move_scale: 0.0, air_base: 0.1, air_scale: 0.0, offset: 1.0, divisor: 1e9, max: 1.0, recovery: 8.0, unscoped: 0.0 },
        recoil: recoil(4.0, 1.0, 0.0, 0.0, 8.0, 2.0, 2),
        tracer: 0,
        ..BASE
    },
    WeaponDef {
        id: WeaponId::M1014,
        name: "M1014",
        class: Class::Shotgun,
        price: 3000,
        kill_reward: 900,
        damage: 22.0,
        range_mod: 0.7,
        armor_ratio: 1.0,
        pellets: 6,
        penetration: 0.1,
        penetrations: 1,
        range: 3048.0 * U,
        cycle: 0.25,
        auto: false,
        clip: 7,
        reserve: 32,
        reload: 0.0,
        shells: Some((0.45, 0.4)),
        speed: 240.0 * U,
        scoped_speed: 240.0 * U,
        spread: Spread { stand: 0.0725, crouch: 0.0725, move_base: 0.0725, move_scale: 0.0, air_base: 0.1, air_scale: 0.0, offset: 1.0, divisor: 1e9, max: 1.0, recovery: 8.0, unscoped: 0.0 },
        recoil: recoil(3.0, 0.8, 0.5, 0.1, 8.0, 2.0, 3),
        tracer: 0,
        ..BASE
    },
    WeaponDef { id: WeaponId::G36, name: "G36", side: Side::Ksk, price: 2250, damage: 30.0, range_mod: 0.96, armor_ratio: 1.4, cycle: 0.09, clip: 25, reserve: 90, reload: 3.3, speed: 220.0 * U, scoped_speed: 220.0 * U, spread: spread(0.028, 0.035, 0.04, 0.35, 215.0, 1.25), recoil: recoil(0.6, 0.3, 0.22, 0.015, 3.5, 2.0, 8), ..BASE },
    WeaponDef { id: WeaponId::G95, name: "G95", side: Side::Ksk, price: 3100, damage: 33.0, range_mod: 0.97, armor_ratio: 1.4, cycle: 0.09, clip: 30, reserve: 90, reload: 3.07, speed: 230.0 * U, scoped_speed: 230.0 * U, spread: spread(0.02, 0.035, 0.035, 0.35, 220.0, 1.25), recoil: recoil(0.575, 0.325, 0.2, 0.011, 4.25, 1.5, 9), ..BASE },
    WeaponDef { id: WeaponId::Ak74, name: "AK-74M", side: Side::Opfor, price: 2000, damage: 30.0, range_mod: 0.98, armor_ratio: 1.55, cycle: 0.09, clip: 35, reserve: 90, reload: 2.95, speed: 215.0 * U, scoped_speed: 215.0 * U, spread: spread(0.0325, 0.04, 0.04, 0.35, 200.0, 1.25), recoil: recoil(0.625, 0.375, 0.25, 0.0125, 4.0, 2.25, 9), ..BASE },
    WeaponDef { id: WeaponId::Akm, name: "AKM", side: Side::Opfor, price: 2700, damage: 36.0, range_mod: 0.98, armor_ratio: 1.55, cycle: 0.1, clip: 30, reserve: 90, reload: 2.43, speed: 221.0 * U, scoped_speed: 221.0 * U, spread: spread(0.0275, 0.04, 0.04, 0.35, 200.0, 1.25), recoil: recoil(0.65, 0.35, 0.25, 0.015, 3.5, 2.25, 7), ..BASE },
    WeaponDef { id: WeaponId::Ssg3000, name: "SSG 3000", price: 2500, damage: 75.0, range_mod: 0.98, armor_ratio: 1.7, cycle: 1.25, clip: 10, reserve: 90, reload: 2.0, speed: 260.0 * U, scoped_speed: 220.0 * U, ..SNIPER },
    WeaponDef {
        id: WeaponId::G22,
        name: "G22",
        price: 4750,
        kill_reward: 100,
        damage: 115.0,
        range_mod: 0.99,
        armor_ratio: 1.95,
        penetration: 1.4,
        cycle: 1.455,
        clip: 10,
        reserve: 30,
        reload: 2.5,
        draw: 1.25,
        speed: 210.0 * U,
        scoped_speed: 150.0 * U,
        zoom: &[40.0 / 90.0, 10.0 / 90.0],
        spread: Spread { stand: 0.001, crouch: 0.0, move_base: 0.15, move_scale: 0.0, air_base: 0.5, air_scale: 0.0, offset: 1.0, divisor: 1e9, max: 1.0, recovery: 8.0, unscoped: 0.12 },
        recoil: recoil(4.0, 0.5, 0.0, 0.0, 6.0, 1.0, 2),
        ..SNIPER
    },
    WeaponDef { id: WeaponId::G28, name: "G28", side: Side::Ksk, price: 4200, damage: 70.0, range_mod: 0.98, armor_ratio: 1.45, cycle: 0.25, clip: 20, reserve: 90, reload: 3.35, speed: 210.0 * U, scoped_speed: 150.0 * U, spread: Spread { stand: 0.015, crouch: 0.01, move_base: 0.1, move_scale: 0.0, air_base: 0.3, air_scale: 0.0, offset: 1.0, divisor: 40.0, max: 3.0, recovery: 3.0, unscoped: 0.05 }, recoil: recoil(1.2, 0.3, 0.3, 0.05, 5.0, 1.5, 4), ..SNIPER },
    WeaponDef { id: WeaponId::Svd, name: "SVD", side: Side::Opfor, price: 4200, damage: 75.0, range_mod: 0.98, armor_ratio: 1.65, cycle: 0.27, clip: 10, reserve: 90, reload: 3.5, speed: 210.0 * U, scoped_speed: 150.0 * U, spread: Spread { stand: 0.015, crouch: 0.01, move_base: 0.1, move_scale: 0.0, air_base: 0.3, air_scale: 0.0, offset: 1.0, divisor: 40.0, max: 3.0, recovery: 3.0, unscoped: 0.05 }, recoil: recoil(1.4, 0.35, 0.3, 0.05, 5.0, 1.5, 4), ..SNIPER },
    WeaponDef { id: WeaponId::Mg5, name: "MG5", class: Class::Mg, side: Side::Ksk, price: 5200, damage: 32.0, range_mod: 0.97, armor_ratio: 1.6, cycle: 0.075, clip: 120, reserve: 240, reload: 5.2, speed: 215.0 * U, scoped_speed: 215.0 * U, spread: spread(0.035, 0.05, 0.05, 0.4, 175.0, 0.9), recoil: recoil(0.7, 0.35, 0.18, 0.02, 4.0, 3.0, 6), ..BASE },
    WeaponDef { id: WeaponId::Pkm, name: "PKM", class: Class::Mg, side: Side::Opfor, price: 5200, damage: 35.0, range_mod: 0.97, armor_ratio: 1.6, cycle: 0.085, clip: 100, reserve: 200, reload: 5.0, speed: 215.0 * U, scoped_speed: 215.0 * U, spread: spread(0.035, 0.05, 0.05, 0.4, 175.0, 0.9), recoil: recoil(0.75, 0.4, 0.2, 0.02, 4.5, 3.0, 6), ..BASE },
    WeaponDef { id: WeaponId::He, name: "DM51", price: 300, ..GRENADE },
    WeaponDef { id: WeaponId::Flash, name: "Blendgranate", price: 200, ..GRENADE },
    WeaponDef { id: WeaponId::Smoke, name: "Rauchgranate", price: 300, ..GRENADE },
    WeaponDef { id: WeaponId::Bomb, name: "Sprengladung", class: Class::Bomb, side: Side::Opfor, draw: 1.0, speed: 250.0 * U, scoped_speed: 250.0 * U, ..GRENADE },
];

impl WeaponId {
    pub const ALL: [WeaponId; 25] = [
        WeaponId::Knife,
        WeaponId::P8,
        WeaponId::Mp443,
        WeaponId::P30,
        WeaponId::Rev357,
        WeaponId::Mp7,
        WeaponId::Pp19,
        WeaponId::Mp5,
        WeaponId::Ump,
        WeaponId::M870,
        WeaponId::M1014,
        WeaponId::G36,
        WeaponId::G95,
        WeaponId::Ak74,
        WeaponId::Akm,
        WeaponId::Ssg3000,
        WeaponId::G22,
        WeaponId::G28,
        WeaponId::Svd,
        WeaponId::Mg5,
        WeaponId::Pkm,
        WeaponId::He,
        WeaponId::Flash,
        WeaponId::Smoke,
        WeaponId::Bomb,
    ];

    pub fn def(self) -> &'static WeaponDef {
        DEFS.iter().find(|d| d.id == self).expect("every weapon has a definition")
    }

    pub fn name(self) -> &'static str {
        self.def().name
    }

    /// A stable lower-case key (sounds, icons, models).
    pub fn key(self) -> &'static str {
        match self {
            WeaponId::Knife => "knife",
            WeaponId::P8 => "p8",
            WeaponId::Mp443 => "mp443",
            WeaponId::P30 => "p30",
            WeaponId::Rev357 => "rev357",
            WeaponId::Mp7 => "mp7",
            WeaponId::Pp19 => "pp19",
            WeaponId::Mp5 => "mp5",
            WeaponId::Ump => "ump",
            WeaponId::M870 => "m870",
            WeaponId::M1014 => "m1014",
            WeaponId::G36 => "g36",
            WeaponId::G95 => "g95",
            WeaponId::Ak74 => "ak74",
            WeaponId::Akm => "akm",
            WeaponId::Ssg3000 => "ssg3000",
            WeaponId::G22 => "g22",
            WeaponId::G28 => "g28",
            WeaponId::Svd => "svd",
            WeaponId::Mg5 => "mg5",
            WeaponId::Pkm => "pkm",
            WeaponId::He => "he",
            WeaponId::Flash => "flash",
            WeaponId::Smoke => "smoke",
            WeaponId::Bomb => "bomb",
        }
    }
}

/// A gun in someone's hands or on the floor: its magazine and the reserve that goes with it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gun {
    pub id: WeaponId,
    pub clip: u16,
    pub reserve: u16,
    /// Its paint (a cosmetic finish, 0 = as it left the factory): it stays with the gun, dropped or picked up.
    #[serde(default)]
    pub finish: u8,
}

impl Gun {
    pub fn new(id: WeaponId) -> Gun {
        let d = id.def();
        Gun { id, clip: d.clip, reserve: d.reserve, finish: 0 }
    }

    pub fn with_finish(self, finish: u8) -> Gun {
        Gun { finish, ..self }
    }
}

/// What the hands are busy with.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Idle,
    /// Taking the weapon out: seconds left.
    Drawing(f32),
    /// Reloading: seconds left, and the whole length. For shell-by-shell guns, the time to the next shell.
    Reloading { left: f32, total: f32 },
    /// The pin is out: seconds cooked, and whether it is an underhand lob.
    PinPulled { lob: bool },
    /// Planting the bomb: seconds done.
    Planting(f32),
}

/// A knife swing in progress.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Swing {
    pub t: f32,
    pub stab: bool,
    pub struck: bool,
}

/// Seconds into a swing when the blade meets what is in front (slash, stab).
pub const STRIKE: (f32, f32) = (0.1, 0.25);
pub const STAB_TIME: f32 = 1.0;

/// Everything a player carries, and what the hands are doing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inventory {
    pub primary: Option<Gun>,
    pub secondary: Option<Gun>,
    pub he: u8,
    pub flash: u8,
    pub smoke: u8,
    pub bomb: bool,
    pub active: WeaponId,
    pub last: WeaponId,
    pub action: Action,
    /// Seconds until the trigger can fire again (and the right button).
    pub cooldown: f32,
    pub cooldown2: f32,
    /// The burst's weight for accuracy (+1 a round, recovering), and rounds in this burst (recoil).
    pub shots: f32,
    pub burst: u32,
    pub recoil_left: bool,
    /// The trigger was held last tick (semi-automatic weapons need a new press).
    pub trigger_held: bool,
    /// Scope level (0 = not scoped).
    pub zoom: u8,
    /// The scope level a bolt-action rifle goes back to once the bolt is worked (0: none), as in the genre.
    #[serde(default)]
    pub rezoom: u8,
    /// Aiming down the sights (guns without a scope, the right button held): 0 from the hip .. 1 aimed, eased.
    pub ads: f32,
    pub swing: Option<Swing>,
    /// Rounds fired in the match with the gun in hand (tracers every n).
    pub fired: u32,
}

impl Default for Inventory {
    fn default() -> Self {
        Inventory {
            primary: None,
            secondary: None,
            he: 0,
            flash: 0,
            smoke: 0,
            bomb: false,
            active: WeaponId::Knife,
            last: WeaponId::Knife,
            action: Action::Idle,
            cooldown: 0.0,
            cooldown2: 0.0,
            shots: 0.0,
            burst: 0,
            recoil_left: false,
            trigger_held: false,
            zoom: 0,
            rezoom: 0,
            ads: 0.0,
            swing: None,
            fired: 0,
        }
    }
}

/// Aiming down the sights: seconds to raise the gun, how much of the spread it takes away, how much slower one walks.
pub const ADS_TIME: f32 = 0.18;
pub const ADS_SPREAD: f32 = 0.45;
pub const ADS_SLOW: f32 = 0.25;

/// The field-of-view factor aimed down the sights, by class (1: no zoom).
pub fn ads_zoom(class: Class) -> f32 {
    match class {
        Class::Rifle => 0.72,
        Class::Mg | Class::Smg => 0.8,
        Class::Pistol => 0.85,
        Class::Shotgun => 0.88,
        _ => 1.0,
    }
}

/// Grenades one may carry of each kind.
pub const MAX_HE: u8 = 1;
pub const MAX_FLASH: u8 = 2;
pub const MAX_SMOKE: u8 = 1;

impl Inventory {
    /// A fresh loadout: knife and the side's pistol.
    pub fn spawn(pistol: WeaponId) -> Inventory {
        let mut inv = Inventory { secondary: Some(Gun::new(pistol)), active: pistol, last: WeaponId::Knife, ..Default::default() };
        inv.action = Action::Drawing(pistol.def().draw);
        inv
    }

    pub fn def(&self) -> &'static WeaponDef {
        self.active.def()
    }

    /// Whether a weapon is carried.
    pub fn has(&self, id: WeaponId) -> bool {
        match id {
            WeaponId::Knife => true,
            WeaponId::He => self.he > 0,
            WeaponId::Flash => self.flash > 0,
            WeaponId::Smoke => self.smoke > 0,
            WeaponId::Bomb => self.bomb,
            _ => self.primary.is_some_and(|g| g.id == id) || self.secondary.is_some_and(|g| g.id == id),
        }
    }

    /// The gun in hand, if the active weapon is a gun.
    pub fn gun(&self) -> Option<&Gun> {
        match self.def().slot() {
            Slot::Primary => self.primary.as_ref(),
            Slot::Secondary => self.secondary.as_ref(),
            _ => None,
        }
        .filter(|g| g.id == self.active)
    }

    pub fn gun_mut(&mut self) -> Option<&mut Gun> {
        let active = self.active;
        match active.def().slot() {
            Slot::Primary => self.primary.as_mut(),
            Slot::Secondary => self.secondary.as_mut(),
            _ => None,
        }
        .filter(|g| g.id == active)
    }

    /// The weapons that a number key cycles through for a slot, in order.
    pub fn in_slot(&self, slot: Slot) -> Vec<WeaponId> {
        match slot {
            Slot::Primary => self.primary.map(|g| g.id).into_iter().collect(),
            Slot::Secondary => self.secondary.map(|g| g.id).into_iter().collect(),
            Slot::Knife => vec![WeaponId::Knife],
            Slot::Grenade => [WeaponId::He, WeaponId::Flash, WeaponId::Smoke].into_iter().filter(|&g| self.has(g)).collect(),
            Slot::Bomb => self.bomb.then_some(WeaponId::Bomb).into_iter().collect(),
        }
    }

    /// Every weapon carried, in slot order (for the mouse wheel).
    pub fn all(&self) -> Vec<WeaponId> {
        [Slot::Primary, Slot::Secondary, Slot::Knife, Slot::Grenade, Slot::Bomb].into_iter().flat_map(|s| self.in_slot(s)).collect()
    }

    /// Takes out a weapon. False if it isn't carried, is already out, or the hands can't let go (pin out, planting).
    pub fn select(&mut self, id: WeaponId) -> bool {
        if id == self.active || !self.has(id) || matches!(self.action, Action::PinPulled { .. } | Action::Planting(_)) {
            return false;
        }
        self.last = self.active;
        self.active = id;
        self.action = Action::Drawing(id.def().draw);
        self.swing = None;
        self.zoom = 0;
        self.rezoom = 0;
        self.ads = 0.0;
        self.burst = 0;
        self.cooldown = 0.0;
        true
    }

    /// The number key for a slot: the slot's first weapon, or the next one if a weapon of the slot is out.
    pub fn select_slot(&mut self, slot: Slot) -> bool {
        let list = self.in_slot(slot);
        if list.is_empty() {
            return false;
        }
        let next = match list.iter().position(|&w| w == self.active) {
            Some(i) => list[(i + 1) % list.len()],
            None => list[0],
        };
        self.select(next)
    }

    /// The mouse wheel: the next (or previous) carried weapon.
    pub fn cycle(&mut self, step: i32) -> bool {
        let list = self.all();
        let i = list.iter().position(|&w| w == self.active).unwrap_or(0) as i32;
        let next = list[(i + step).rem_euclid(list.len() as i32) as usize];
        self.select(next)
    }

    /// The best weapon to hold after losing the one in hand.
    pub fn best(&self) -> WeaponId {
        self.primary.map(|g| g.id).or(self.secondary.map(|g| g.id)).unwrap_or(WeaponId::Knife)
    }

    /// Takes a weapon out of the inventory (dropped, thrown). Returns what was removed.
    pub fn remove(&mut self, id: WeaponId) -> Option<Gun> {
        let out = match id {
            WeaponId::Knife => None,
            WeaponId::He | WeaponId::Flash | WeaponId::Smoke => {
                let n = match id {
                    WeaponId::He => &mut self.he,
                    WeaponId::Flash => &mut self.flash,
                    _ => &mut self.smoke,
                };
                if *n == 0 {
                    return None;
                }
                *n -= 1;
                Some(Gun { id, clip: 0, reserve: 0, finish: 0 })
            }
            WeaponId::Bomb => self.bomb.then(|| {
                self.bomb = false;
                Gun { id, clip: 0, reserve: 0, finish: 0 }
            }),
            _ => {
                if self.primary.is_some_and(|g| g.id == id) {
                    self.primary.take()
                } else if self.secondary.is_some_and(|g| g.id == id) {
                    self.secondary.take()
                } else {
                    None
                }
            }
        };
        if out.is_some() && self.active == id && !self.has(id) {
            let next = if self.last != id && self.has(self.last) { self.last } else { self.best() };
            self.active = id; // so `select` sees a change
            self.action = Action::Idle;
            if !self.select(next) {
                self.active = next;
            }
        }
        out
    }

    /// Whether a new press is needed and the hands are free to act.
    pub fn busy(&self) -> bool {
        !matches!(self.action, Action::Idle) || self.swing.is_some()
    }

    /// Starts a reload if the gun in hand can take rounds and there are some. Returns whether it started.
    pub fn reload(&mut self) -> bool {
        let d = self.def();
        let Some(g) = self.gun() else { return false };
        if self.busy() || g.reserve == 0 || g.clip >= d.clip {
            return false;
        }
        self.zoom = 0;
        self.rezoom = 0;
        self.burst = 0;
        self.action = match d.shells {
            Some((start, _)) => Action::Reloading { left: start, total: start },
            None => Action::Reloading { left: d.reload, total: d.reload },
        };
        true
    }

    /// Time passes: drawing, reloading (the rounds go in at the end, or shell by shell), the fire rate, accuracy
    /// recovering. `firing` is whether the trigger is held (accuracy only recovers when it isn't).
    pub fn tick(&mut self, dt: f32, firing: bool) -> Option<Tick> {
        self.cooldown = (self.cooldown - dt).max(0.0);
        self.cooldown2 = (self.cooldown2 - dt).max(0.0);
        let d = self.def();
        // The bolt is worked: back into the scope.
        if self.rezoom > 0 && self.cooldown <= 0.0 && self.action == Action::Idle {
            self.zoom = self.rezoom.min(d.zoom.len() as u8);
            self.rezoom = 0;
            return Some(Tick::Rezoom);
        }
        if !firing || self.cooldown > 0.0 {
            self.shots = (self.shots - d.spread.recovery * dt * if firing { 0.0 } else { 1.0 }).max(0.0);
        }
        if !firing {
            self.burst = 0;
        }
        match self.action {
            Action::Drawing(t) => {
                let t = t - dt;
                self.action = if t <= 0.0 { Action::Idle } else { Action::Drawing(t) };
                None
            }
            Action::Reloading { left, total } => {
                let left = left - dt;
                if left > 0.0 {
                    self.action = Action::Reloading { left, total };
                    return None;
                }
                let clip = d.clip;
                let shells = d.shells;
                let Some(g) = self.gun_mut() else {
                    self.action = Action::Idle;
                    return None;
                };
                match shells {
                    Some((_, per)) => {
                        if g.clip < clip && g.reserve > 0 {
                            g.clip += 1;
                            g.reserve -= 1;
                        }
                        if g.clip < clip && g.reserve > 0 {
                            self.action = Action::Reloading { left: per + left, total: per };
                        } else {
                            self.action = Action::Idle;
                        }
                        Some(Tick::Shell)
                    }
                    None => {
                        let take = (clip - g.clip).min(g.reserve);
                        g.clip += take;
                        g.reserve -= take;
                        self.action = Action::Idle;
                        Some(Tick::Reloaded)
                    }
                }
            }
            _ => None,
        }
    }

    /// Current spread (tangent) for the gun in hand. `speed_frac` is the ground speed over the weapon's top speed.
    pub fn spread(&self, speed_frac: f32, on_ground: bool, crouched: bool) -> f32 {
        let d = self.def();
        let s = &d.spread;
        let acc = (s.offset + self.shots.powi(3) / s.divisor).min(s.max);
        let moving = smoothstep(0.34, 0.6, speed_frac);
        let still = if crouched { s.crouch } else { s.stand } * acc;
        let mut out = if !on_ground {
            s.air_base + s.air_scale * acc
        } else {
            still + moving * (s.move_base + s.move_scale * acc - still).max(0.0)
        };
        if d.scoped() && self.zoom == 0 {
            out += s.unscoped;
        }
        // Down the sights: tighter (less so on the move and in the air, where it doesn't steady the gun).
        out * (1.0 - ADS_SPREAD * self.ads * if on_ground { 1.0 } else { 0.3 })
    }

    /// Moves the aim towards the sights while `want` (the right button on a gun without a scope), back otherwise.
    pub fn aim(&mut self, want: bool, dt: f32) {
        let can = want && self.def().is_gun() && !self.def().scoped() && !matches!(self.action, Action::Reloading { .. } | Action::Drawing(_));
        let target = if can { 1.0 } else { 0.0 };
        let step = dt / ADS_TIME;
        // Towards the target and no further: held at full aim it stays there (it used to fall back every third tick).
        self.ads = if target > self.ads { (self.ads + step).min(target) } else { (self.ads - step * 1.5).max(target) };
    }

    /// After a round has left: a bolt-action rifle drops out of the scope while the bolt is worked, and goes back to
    /// the same level when it is (unless the right button is pressed meanwhile).
    pub fn after_shot(&mut self) {
        let d = self.def();
        if d.class == Class::Sniper && d.cycle > 1.0 && self.zoom > 0 {
            self.rezoom = self.zoom;
            self.zoom = 0;
        }
    }

    /// A trigger pull with the gun in hand. Returns what happened.
    pub fn pull(&mut self, pressed: bool, held: bool) -> Pull {
        let d = self.def();
        let want = if d.auto { held } else { pressed };
        if !want || self.cooldown > 1e-4 {
            return Pull::None;
        }
        // A shell-by-shell reload is interrupted by the trigger.
        if matches!(self.action, Action::Reloading { .. }) && d.shells.is_some() && self.gun().is_some_and(|g| g.clip > 0) {
            self.action = Action::Idle;
        }
        if self.busy() {
            return Pull::None;
        }
        let Some(g) = self.gun_mut() else { return Pull::None };
        if g.clip == 0 {
            self.cooldown = 0.2;
            return if self.reload() { Pull::Reload } else { Pull::Empty };
        }
        g.clip -= 1;
        self.cooldown = d.cycle;
        self.shots += 1.0;
        self.burst += 1;
        self.fired += 1;
        // (A bolt-action rifle leaves the scope to cycle, but only after the round is out: see `after_shot`. Leaving
        // it here made every scoped shot as wild as one from the hip.)
        Pull::Fired
    }

    /// The recoil of the round just fired (degrees: up, right), given the stance.
    pub fn kick(&mut self, rng: &mut Rng, stance: Stance) -> (f32, f32) {
        let r = self.def().recoil;
        let k = match stance {
            Stance::Air => 2.2,
            Stance::Moving => 1.5,
            Stance::Crouched => 0.85,
            Stance::Standing => 1.0,
        };
        let k = k * (1.0 - 0.2 * self.ads);
        let n = self.burst.saturating_sub(1) as f32;
        let up = (r.up_base + r.up_mod * n) * k;
        let lat = (r.lateral_base + r.lateral_mod * n) * k;
        if r.direction_change > 0 && rng.below(r.direction_change) == 0 {
            self.recoil_left = !self.recoil_left;
        }
        (up, if self.recoil_left { -lat } else { lat })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stance {
    Standing,
    Crouched,
    Moving,
    Air,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pull {
    None,
    Fired,
    /// Out of rounds, a reload started.
    Reload,
    /// Out of rounds and none to load: a click.
    Empty,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tick {
    Reloaded,
    Shell,
    /// A bolt-action rifle went back into its scope.
    Rezoom,
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bolt_action_fires_scoped_then_scopes_back_in() {
        let mut inv = ready(WeaponId::Ssg3000);
        inv.zoom = 1;
        let scoped = inv.spread(0.0, true, false);
        assert_eq!(inv.pull(true, true), Pull::Fired);
        // The round leaves with the scope's accuracy (it used to drop out of the scope first).
        assert!(inv.spread(0.0, true, false) <= scoped * 1.01, "spread at the shot {} vs scoped {scoped}", inv.spread(0.0, true, false));
        inv.after_shot();
        assert_eq!((inv.zoom, inv.rezoom), (0, 1));
        let mut back = false;
        for _ in 0..200 {
            back |= inv.tick(1.0 / 66.0, false) == Some(Tick::Rezoom);
        }
        assert!(back && inv.zoom == 1, "back in the scope after the bolt");
    }

    fn ready(id: WeaponId) -> Inventory {
        let mut inv = Inventory::spawn(WeaponId::P8);
        if id.def().slot() == Slot::Primary {
            inv.primary = Some(Gun::new(id));
        }
        inv.select(id);
        inv.tick(5.0, false);
        inv
    }

    #[test]
    fn every_weapon_has_a_definition_and_a_key() {
        for id in WeaponId::ALL {
            assert_eq!(id.def().id, id);
            assert!(!id.key().is_empty() && !id.name().is_empty());
        }
    }

    #[test]
    fn automatic_fire_rate_and_magazine() {
        let mut inv = ready(WeaponId::Akm);
        let mut fired = 0;
        let dt = 1.0 / 66.0;
        for i in 0..66 {
            if inv.pull(i == 0, true) == Pull::Fired {
                fired += 1;
            }
            inv.tick(dt, true);
        }
        // 600 rounds a minute: 10 a second (a tick's rounding either way).
        assert!((9..=11).contains(&fired), "{fired}");
        assert_eq!(inv.gun().unwrap().clip, 30 - fired);
    }

    #[test]
    fn semi_automatic_needs_a_press() {
        let mut inv = ready(WeaponId::P8);
        assert_eq!(inv.pull(true, true), Pull::Fired);
        inv.tick(1.0, true);
        assert_eq!(inv.pull(false, true), Pull::None);
        assert_eq!(inv.pull(true, true), Pull::Fired);
    }

    #[test]
    fn reloading_fills_from_the_reserve() {
        let mut inv = ready(WeaponId::G95);
        inv.gun_mut().unwrap().clip = 4;
        assert!(inv.reload());
        assert_eq!(inv.pull(true, true), Pull::None, "busy reloading");
        for _ in 0..400 {
            inv.tick(0.01, false);
        }
        let g = inv.gun().unwrap();
        assert_eq!((g.clip, g.reserve), (30, 90 - 26));
        assert!(!inv.reload(), "full");
    }

    #[test]
    fn held_sights_stay_up() {
        let mut inv = ready(WeaponId::G36);
        for _ in 0..40 {
            inv.aim(true, crate::game::TICK);
        }
        for _ in 0..20 {
            inv.aim(true, crate::game::TICK);
            assert_eq!(inv.ads, 1.0);
        }
        for _ in 0..40 {
            inv.aim(false, crate::game::TICK);
        }
        assert_eq!(inv.ads, 0.0);
    }

    #[test]
    fn empty_clicks_and_reloads() {
        let mut inv = ready(WeaponId::P8);
        inv.gun_mut().unwrap().clip = 0;
        assert_eq!(inv.pull(true, true), Pull::Reload);
        inv.gun_mut().unwrap().reserve = 0;
        inv.action = Action::Idle;
        inv.cooldown = 0.0;
        assert_eq!(inv.pull(true, true), Pull::Empty);
    }

    #[test]
    fn shotguns_load_shell_by_shell_and_can_interrupt() {
        let mut inv = ready(WeaponId::M870);
        inv.gun_mut().unwrap().clip = 5;
        assert!(inv.reload());
        let mut shells = 0;
        for _ in 0..100 {
            if inv.tick(0.01, false) == Some(Tick::Shell) {
                shells += 1;
            }
        }
        assert!(shells >= 1 && inv.gun().unwrap().clip > 5);
        assert_eq!(inv.pull(true, true), Pull::Fired, "the trigger interrupts the reload");
    }

    #[test]
    fn spraying_ruins_accuracy_and_it_recovers() {
        let mut inv = ready(WeaponId::Akm);
        let first = inv.spread(0.0, true, false);
        for _ in 0..10 {
            inv.pull(true, true);
            inv.tick(0.1, true);
        }
        let sprayed = inv.spread(0.0, true, false);
        assert!(sprayed > first * 2.5, "{first} → {sprayed}");
        for _ in 0..200 {
            inv.tick(0.01, false);
        }
        assert!((inv.spread(0.0, true, false) - first).abs() < 1e-4);
        assert!(inv.spread(1.0, true, false) > first * 3.0, "running is inaccurate");
        assert!(inv.spread(0.0, false, false) > first * 5.0, "jumping is very inaccurate");
    }

    #[test]
    fn a_rifle_head_tap_kills_through_a_helmet_only_with_the_heavy_rounds() {
        // Head ×4 at 10 m; armour: damage × ratio/2, capped by the armour.
        let through = |id: WeaponId| {
            let d = id.def();
            d.damage_at(10.0) * 4.0 * (d.armor_ratio * 0.5).min(1.0)
        };
        assert!(through(WeaponId::Akm) >= 100.0, "{}", through(WeaponId::Akm));
        assert!(through(WeaponId::G95) < 100.0, "{}", through(WeaponId::G95));
        assert!(WeaponId::G22.def().damage_at(30.0) * 1.0 * (1.95 * 0.5) >= 100.0, "the heavy sniper kills with a chest shot");
    }

    #[test]
    fn slots_and_switching() {
        let mut inv = Inventory::spawn(WeaponId::Mp443);
        inv.primary = Some(Gun::new(WeaponId::Ak74));
        inv.he = 1;
        inv.flash = 2;
        assert!(inv.select_slot(Slot::Primary));
        assert_eq!(inv.active, WeaponId::Ak74);
        assert!(inv.select_slot(Slot::Grenade));
        assert_eq!(inv.active, WeaponId::He);
        assert!(inv.select_slot(Slot::Grenade));
        assert_eq!(inv.active, WeaponId::Flash);
        assert!(inv.cycle(1));
        assert_eq!(inv.active, WeaponId::Ak74, "wheel wraps round");
        // Dropping the gun in hand takes out the next best.
        inv.remove(WeaponId::Ak74);
        assert!(inv.primary.is_none() && inv.active != WeaponId::Ak74);
    }
}
