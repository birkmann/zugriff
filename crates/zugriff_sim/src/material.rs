//! What a surface is made of, from its texture name: it decides footstep sounds, impact effects and how easily bullets
//! go through.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Material {
    Concrete,
    Brick,
    Plaster,
    Metal,
    Wood,
    Glass,
    Dirt,
    Grass,
    Tile,
    Flesh,
}

impl Material {
    /// By texture name (`mat/<set>` or `dev/<name>`): the first keyword that matches wins.
    pub fn of(surface: &str) -> Material {
        let s = surface.to_ascii_lowercase();
        let has = |k: &[&str]| k.iter().any(|w| s.contains(w));
        if has(&["glass", "window"]) {
            Material::Glass
        } else if has(&["metal", "steel", "container", "rust", "iron", "corrugated"]) {
            Material::Metal
        } else if has(&["wood", "crate", "plank", "beam", "pallet"]) {
            Material::Wood
        } else if has(&["grass"]) {
            Material::Grass
        } else if has(&["dirt", "gravel", "sand", "ground", "soil"]) {
            Material::Dirt
        } else if has(&["tile"]) {
            Material::Tile
        } else if has(&["brick"]) {
            Material::Brick
        } else if has(&["plaster", "ceiling"]) {
            Material::Plaster
        } else {
            Material::Concrete
        }
    }

    /// How much harder than wood it is to shoot through: a round's penetration power is divided by this per metre.
    pub fn hardness(self) -> f32 {
        match self {
            Material::Glass => 0.2,
            Material::Wood => 1.0,
            Material::Plaster => 1.4,
            Material::Dirt | Material::Grass => 2.5,
            Material::Tile => 2.5,
            Material::Brick => 3.0,
            Material::Metal => 3.2,
            Material::Concrete => 4.0,
            Material::Flesh => 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_map_to_materials() {
        assert_eq!(Material::of("mat/metal_painted_red"), Material::Metal);
        assert_eq!(Material::of("mat/wood_crate"), Material::Wood);
        assert_eq!(Material::of("dev/crate"), Material::Wood);
        assert_eq!(Material::of("mat/bricks090"), Material::Brick);
        assert_eq!(Material::of("dev/glass"), Material::Glass);
        assert_eq!(Material::of("mat/cobblestone"), Material::Concrete);
        assert_eq!(Material::of("mat/tiles_floor"), Material::Tile);
        assert_eq!(Material::of("mat/grass"), Material::Grass);
    }
}
