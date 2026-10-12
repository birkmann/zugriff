//! Finds holes beside walkable ground: next to a nav node, a drop into nothing (no floor within 40 m).
//! `--example holes -- <map>` prints 2 m cells (map units) where that happens.
use std::collections::BTreeSet;
use std::sync::Arc;
use zugriff_sim::nav::Nav;
use zugriff_sim::world::Contents;
use zugriff_sim::{compile, map};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let level = Arc::new(compile::compile(&map::parse(&std::fs::read_to_string(&a[1]).unwrap()).unwrap()));
    let nav = Nav::build(&level, &zugriff_sim::movement::MoveParams::default());
    let w = &level.world;
    let mut cells = BTreeSet::new();
    for n in &nav.nodes {
        for k in 0..8 {
            let ang = k as f32 * std::f32::consts::TAU / 8.0;
            let p = n.pos + glam::Vec3::new(ang.cos(), 0.0, ang.sin()) * 0.7 + glam::Vec3::Y * 0.5;
            // Only where a player could go (no wall in the way at knee height).
            if w.trace(n.pos + glam::Vec3::Y * 0.5, p, glam::Vec3::splat(0.2), Contents::MASK_PLAYER).hit() {
                continue;
            }
            let t = w.trace(p, p - glam::Vec3::Y * 3.5, glam::Vec3::ZERO, Contents::MASK_PLAYER);
            if !t.hit() {
                cells.insert(((p.x / 0.0254 / 80.0).round() as i32 * 80, (-p.z / 0.0254 / 80.0).round() as i32 * 80));
            }
        }
    }
    for c in cells.iter().take(60) {
        println!("void beside walkable ground near map ({}, {})", c.0, c.1);
    }
    println!("{} cells", cells.len());
}
