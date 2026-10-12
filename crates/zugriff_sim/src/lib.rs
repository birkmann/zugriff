//! ZUGRIFF's game simulation. It is pure Rust with no rendering: the Bevy client (`game`), the headless
//! balance runs and a future dedicated server all drive it with the same inputs.
//!
//! Space: metres, Y up, like Bevy. Maps are authored in inches with Z up (Valve 220 `.map`); see [`units`].

/// Small bit sets (serde-friendly) without pulling in a flags crate.
#[macro_export]
macro_rules! bitflags_lite {
    ($(#[$m:meta])* pub struct $name:ident: $t:ty { $(const $f:ident = $v:expr;)* }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
        pub struct $name(pub $t);
        impl $name {
            $(pub const $f: $name = $name($v);)*
            pub const fn empty() -> Self { $name(0) }
            pub const fn contains(self, o: $name) -> bool { self.0 & o.0 == o.0 }
            pub fn set(&mut self, o: $name, on: bool) { if on { self.0 |= o.0 } else { self.0 &= !o.0 } }
        }
        impl std::ops::BitOr for $name { type Output = $name; fn bitor(self, o: $name) -> $name { $name(self.0 | o.0) } }
        impl std::ops::BitOrAssign for $name { fn bitor_assign(&mut self, o: $name) { self.0 |= o.0 } }
    };
}

pub mod bots;
pub mod combat;
pub mod compile;
pub mod game;
pub mod grenades;
pub mod hitbox;
pub mod lagcomp;
pub mod lightmap;
pub mod map;
pub mod material;
pub mod movement;
pub mod nav;
pub mod net;
pub mod rng;
pub mod units;
pub mod weapons;
pub mod world;

#[cfg(not(target_arch = "wasm32"))]
pub mod assets;
