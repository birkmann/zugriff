//! Follows one bot through a round and prints where it is: `--example trace_bot -- <map> <team ksk|opfor> <seconds>`.
use std::sync::Arc;
use zugriff_sim::bots::{Bots, Difficulty};
use zugriff_sim::game::{Config, Game, Mode, Team, TICK_HZ};
use zugriff_sim::nav::Nav;
use zugriff_sim::{compile, map};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let level = Arc::new(compile::compile(&map::parse(&std::fs::read_to_string(&args[1]).unwrap()).unwrap()));
    let team = if args.get(2).map(String::as_str) == Some("opfor") { Team::Opfor } else { Team::Ksk };
    let seconds: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(60.0);
    let nav = Arc::new(Nav::build(&level, &zugriff_sim::movement::MoveParams::default()));
    let mode = if level.volumes_of("func_bomb_target").next().is_some() { Mode::Defuse } else { Mode::Rescue };
    let mut g = Game::new(level.clone(), Config { mode, warmup: 0.0, ..Default::default() }, 3);
    g.nav = Some(nav.clone());
    let mut bots = Bots::new(nav, Difficulty::Normal, 3);
    let id = g.add_player("probe", team, true);
    // One opponent far away so the round doesn't end.
    g.add_player("far", team.other(), true);
    for i in 0..(seconds * TICK_HZ) as usize {
        let mut cmds = bots.think(&g);
        cmds.retain(|(p, _)| *p == id);
        g.tick(&cmds);
        g.events.clear();
        if i % 33 == 0 {
            let p = &g.players[id];
            println!("{:5.1}s {:?} {:?} {}", g.time, (p.mover.origin * 10.0).round() / 10.0, g.place_at(p.mover.origin).unwrap_or("-"), bots.describe(id));
            for (k, h) in g.hostages.iter().enumerate().filter(|(_, h)| h.leader.is_some()) {
                println!("       hostage {k} at {:?} route {} next {:?} v={:.2} ground={}", (h.mover.origin * 10.0).round() / 10.0, h.route.len(), h.route.first().map(|w| (*w * 10.0).round() / 10.0), h.mover.horizontal_speed(), h.mover.on_ground);
            }
        }
    }
}
