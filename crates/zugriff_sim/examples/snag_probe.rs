//! Walks a player from a spot through waypoints the way a bot steers, printing where it gets to — for finding why bots
//! snag somewhere: `--example snag_probe -- <map> x,y,z  x,y,z [x,y,z ...]` (sim metres, first is the start).
use glam::{Vec2, Vec3};
use std::sync::Arc;
use zugriff_sim::game::{Config, Game, Mode, Team, UserCmd, TICK};
use zugriff_sim::{compile, map};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let level = Arc::new(compile::compile(&map::parse(&std::fs::read_to_string(&args[1]).unwrap()).unwrap()));
    let pts: Vec<Vec3> = args[2..]
        .iter()
        .map(|a| {
            let v: Vec<f32> = a.split(',').map(|s| s.parse().unwrap()).collect();
            Vec3::new(v[0], v[1], v[2])
        })
        .collect();
    let mode = if level.volumes_of("func_bomb_target").next().is_some() { Mode::Defuse } else { Mode::Rescue };
    let mut g = Game::new(level, Config { mode, warmup: 0.0, freeze: 0.0, ..Default::default() }, 1);
    let id = g.add_player("probe", Team::Ksk, false);
    g.add_player("far", Team::Opfor, false);
    for _ in 0..5 {
        g.tick(&[]);
    }
    g.players[id].mover.origin = pts[0];
    let mut i = 1;
    let mut t = 0.0;
    while i < pts.len() && t < 6.0 {
        let o = g.players[id].mover.origin;
        let to = pts[i];
        let d = Vec2::new(to.x - o.x, to.z - o.z);
        if d.length() < 0.3 && (to.y - o.y).abs() < 0.4 {
            println!("  reached {i} at {:.2?}", o);
            i += 1;
            continue;
        }
        let yaw = (-d.x).atan2(-d.y);
        let cmd = UserCmd { forward: 1.0, yaw, ..Default::default() };
        g.tick(&[(id, cmd)]);
        t += TICK;
        if (t / TICK) as u32 % 10 == 0 {
            let m = &g.players[id].mover;
            println!("t={t:.2} at {:.2?} ground={} → {i} {:.2?}", m.origin, m.on_ground, to);
        }
    }
}
