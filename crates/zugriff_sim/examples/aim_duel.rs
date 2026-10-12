//! How hard a difficulty is to play against: one bot hunts a dummy that never shoots back (standing, or strafing with
//! `strafe`), in deathmatch. Per encounter it times, from the first clear line of sight: the first shot (reaction), the
//! first hit, the kill; and counts the hit rate and headshots.
//! The bot always holds a G36 (`pistol`: a P30), so the numbers compare across difficulties.
//! `cargo run --release -p zugriff_sim --example aim_duel -- <map> <easy|normal|hard|expert> [seconds] [strafe] [pistol]`
use std::sync::Arc;
use zugriff_sim::bots::{Bots, Difficulty};
use zugriff_sim::game::{Config, Event, Game, Mode, Phase, Team, UserCmd, TICK};
use zugriff_sim::nav::Nav;
use zugriff_sim::weapons::{Gun, WeaponId};
use zugriff_sim::{compile, map};

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let level = Arc::new(compile::compile(&map::parse(&std::fs::read_to_string(&args[1]).unwrap()).unwrap()));
    let difficulty = match args.get(2).map(String::as_str) {
        Some("easy") => Difficulty::Easy,
        Some("hard") => Difficulty::Hard,
        Some("expert") => Difficulty::Expert,
        _ => Difficulty::Normal,
    };
    let seconds: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(600.0);
    let strafe = args.iter().any(|a| a == "strafe");
    let weapon = if args.iter().any(|a| a == "pistol") { WeaponId::P30 } else { WeaponId::G36 };
    let nav = Arc::new(Nav::build(&level, &zugriff_sim::movement::MoveParams::default()));
    let (mut react, mut first_hit, mut kill, mut dist) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut shots, mut hits, mut heads, mut encounters) = (0u32, 0u32, 0u32, 0u32);
    for seed in 0..4u64 {
        let mut g = Game::new(level.clone(), Config { mode: Mode::Deathmatch, warmup: 0.0, dm_time: seconds + 30.0, ..Default::default() }, seed + 1);
        g.nav = Some(nav.clone());
        let mut bots = Bots::new(nav.clone(), difficulty, seed + 11);
        let bot = g.add_player("bot", Team::Ksk, true);
        let dummy = g.add_player("dummy", Team::Opfor, false);
        // The encounter under way: when sight began, first shot and hit, and whether it is still open.
        let mut since: Option<(f32, Option<f32>, Option<f32>)> = None;
        let mut t = 0.0f32;
        let mut side = 1.0f32;
        let mut side_t = 0.0f32;
        while t < seconds / 4.0 {
            let mut cmds = bots.think(&g);
            let _ = bots.drain_radio();
            side_t -= TICK;
            if side_t <= 0.0 {
                side = -side;
                side_t = 0.5 + (t * 7.3).sin().abs() * 0.4;
            }
            // The chosen gun, never out of rounds.
            {
                let inv = &mut g.players[bot].inv;
                let pistol = weapon == WeaponId::P30;
                if pistol {
                    inv.primary = None;
                }
                let slot = if pistol { &mut inv.secondary } else { &mut inv.primary };
                if slot.is_none_or(|gun| gun.id != weapon) {
                    *slot = Some(Gun::new(weapon));
                    inv.active = weapon;
                }
                if let Some(gun) = slot {
                    gun.reserve = 90;
                }
            }
            cmds.iter_mut().filter(|(id, _)| *id == bot).for_each(|(_, c)| {
                c.buy.clear();
                c.select = None;
            });
            cmds.push((dummy, UserCmd { side: if strafe { side } else { 0.0 }, ..Default::default() }));
            g.tick(&cmds);
            t += TICK;
            if g.phase != Phase::Live {
                continue;
            }
            let (b, d) = (&g.players[bot], &g.players[dummy]);
            let sees = b.alive && d.alive && g.clear_line(g.eye(bot), d.hitboxes().chest(), true) && b.mover.origin.distance(d.mover.origin) < 60.0;
            if sees && since.is_none() {
                since = Some((g.time, None, None));
                encounters += 1;
                dist.push(b.mover.origin.distance(d.mover.origin));
            }
            for e in g.events.drain(..) {
                match e {
                    Event::Shot { player, .. } if player == bot => {
                        shots += 1;
                        if let Some((s, first, _)) = &mut since
                            && first.is_none()
                        {
                            *first = Some(g.time);
                            react.push(g.time - *s);
                        }
                    }
                    Event::Hit { victim, attacker: Some(a), zone, .. } if victim == dummy && a == bot => {
                        hits += 1;
                        heads += (zone == zugriff_sim::hitbox::Zone::Head) as u32;
                        if let Some((s, _, hit)) = &mut since
                            && hit.is_none()
                        {
                            *hit = Some(g.time);
                            first_hit.push(g.time - *s);
                        }
                    }
                    Event::Kill { victim, .. } if victim == dummy => {
                        if let Some((s, _, _)) = since.take() {
                            kill.push(g.time - s);
                        }
                    }
                    _ => {}
                }
            }
            // Out of sight for a while without a kill: the encounter is over.
            if !sees && since.is_some_and(|(s, _, _)| g.time - s > 6.0) {
                since = None;
            }
        }
    }
    println!(
        "{:?}{} {:?}: {} encounters at {:.0} m median · reaction {:.2} s · first hit {:.2} s · kill {:.2} s ({} kills) · hit rate {:.0}% · headshots {:.0}% of hits",
        difficulty,
        if strafe { " vs strafing" } else { "" },
        weapon,
        encounters,
        median(&mut dist),
        median(&mut react),
        median(&mut first_hit),
        median(&mut kill),
        kill.len(),
        hits as f32 / shots.max(1) as f32 * 100.0,
        heads as f32 / hits.max(1) as f32 * 100.0
    );
}
