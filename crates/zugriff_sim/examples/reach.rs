//! Can bots walk from each spawn to every objective? `cargo run --release -p zugriff_sim --example reach -- <map.map>`
use std::sync::Arc;
use zugriff_sim::nav::{flags, Nav};
use zugriff_sim::{compile, map};

fn main() {
    let path = std::env::args().nth(1).expect("map");
    let level = Arc::new(compile::compile(&map::parse(&std::fs::read_to_string(&path).unwrap()).unwrap()));
    let nav = Nav::build(&level, &zugriff_sim::movement::MoveParams::default());
    let spawn = |c: &str| level.entities_of(c).next().map(|e| e.origin).unwrap();
    let mut goals: Vec<(String, glam::Vec3)> = level.entities_of("info_hostage").enumerate().map(|(i, e)| (format!("hostage {i}"), e.origin)).collect();
    for (name, mask) in [("site A", flags::SITE_A), ("site B", flags::SITE_B), ("rescue", flags::RESCUE)] {
        if let Some(n) = nav.with_flags(mask).next() {
            goals.push((name.into(), nav.pos(n)));
        } else {
            println!("{name}: no nodes");
        }
    }
    for team in ["info_player_ksk", "info_player_opfor"] {
        let from = spawn(team);
        let a = nav.nearest(from).unwrap();
        for (name, g) in &goals {
            let b = nav.nearest(*g);
            let r = b.and_then(|b| nav.route(a, b, &|_| 0.0));
            println!("{team} → {name} {:?}: {}", (g * 10.0).round() / 10.0, r.map_or("NO ROUTE".to_string(), |r| format!("{} nodes, ends {:?}", r.len(), (nav.pos(*r.last().unwrap()) * 10.0).round() / 10.0)));
        }
    }
}
