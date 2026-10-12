//! `zg-balance <map.map> [--seeds N] [--rounds R] [--difficulty easy|normal|hard|expert] [--mode dm]`
//!
//! Plays bot-against-bot matches on a map, headless, and reports what a map designer needs: rounds won per side and
//! how, plant and defuse rates, round lengths, kills, and bots that stood stuck (alive, wanting to move, not moving).
//! Snags — a bot pushing to move for 2.5 s but getting less than 0.6 m — are gathered by spot and listed with where
//! they were and what the bot wanted (`--snags N` lists that many spots, default 12).

use std::sync::Arc;
use zugriff_sim::bots::{Bots, Difficulty};
use zugriff_sim::game::{Config, Event, Game, Mode, Phase, Reason, Team, TICK, TICK_HZ};
use zugriff_sim::nav::Nav;
use zugriff_sim::{compile, map};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
    let Some(path) = args.get(1).filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: zg-balance <map.map> [--seeds N] [--rounds R] [--difficulty normal] [--mode dm]");
        std::process::exit(2);
    };
    let seeds: u64 = arg("--seeds").and_then(|v| v.parse().ok()).unwrap_or(10);
    let rounds: u32 = arg("--rounds").and_then(|v| v.parse().ok()).unwrap_or(16);
    let difficulty = match arg("--difficulty").as_deref() {
        Some("easy") => Difficulty::Easy,
        Some("hard") => Difficulty::Hard,
        Some("expert") => Difficulty::Expert,
        _ => Difficulty::Normal,
    };
    let src = std::fs::read_to_string(path).expect("map");
    let level = Arc::new(compile::compile(&map::parse(&src).expect("parse")));
    let started = std::time::Instant::now();
    let nav = Arc::new(Nav::build(&level, &zugriff_sim::movement::MoveParams::default()));
    println!("{path}: nav {} nodes in {:.1} s", nav.nodes.len(), started.elapsed().as_secs_f32());
    let mode = if arg("--mode").as_deref() == Some("dm") {
        Mode::Deathmatch
    } else if level.volumes_of("func_bomb_target").next().is_some() {
        Mode::Defuse
    } else {
        Mode::Rescue
    };
    let mut wins = [0u32; 2];
    let mut reasons: std::collections::BTreeMap<String, u32> = Default::default();
    let (mut plants, mut defuses, mut explosions, mut rescued, mut kills, mut played) = (0, 0, 0, 0, 0, 0);
    let mut round_time = 0.0f32;
    let mut stuck_seconds = 0.0f32;
    let mut alive_seconds = 0.0f32;
    let mut first_contact = Vec::new();
    let (mut think_time, mut tick_time) = (0.0f64, 0.0f64);
    // Snag spots: position, count, an example of what the bot was doing.
    let mut snags: Vec<(glam::Vec3, u32, String)> = Vec::new();
    let mut snag_seconds = 0.0f32;
    for seed in 0..seeds {
        let cfg = Config { mode, max_rounds: rounds, warmup: 0.0, dm_time: 180.0, ..Default::default() };
        let mut g = Game::new(level.clone(), cfg, seed * 7919 + 1);
        g.nav = Some(nav.clone());
        let mut bots = Bots::new(nav.clone(), difficulty, seed);
        let mut rng = zugriff_sim::rng::Rng::new(seed);
        for team in [Team::Ksk, Team::Opfor] {
            for _ in 0..5 {
                let name = Bots::name_for(&g, team, &mut rng);
                g.add_player(&name, team, true);
            }
        }
        let mut last_pos: Vec<glam::Vec3> = g.players.iter().map(|p| p.mover.origin).collect();
        let mut still: Vec<f32> = vec![0.0; g.players.len()];
        // Per player: where the push began and for how long it has lasted, and whether this snag was counted.
        let mut push: Vec<(glam::Vec3, f32, bool)> = g.players.iter().map(|p| (p.mover.origin, 0.0, false)).collect();
        let mut round_started = 0.0f32;
        let mut contact_this_round: Option<f32> = None;
        let limit = (rounds as f32 * 200.0 * TICK_HZ) as usize;
        for _ in 0..limit {
            let t0 = std::time::Instant::now();
            let cmds = bots.think(&g);
            let t1 = std::time::Instant::now();
            g.tick(&cmds);
            think_time += (t1 - t0).as_secs_f64();
            tick_time += t1.elapsed().as_secs_f64();
            let _ = bots.drain_radio();
            let events: Vec<Event> = g.events.drain(..).collect();
            for e in events {
                match e {
                    Event::Planted { .. } => plants += 1,
                    Event::Defused { .. } => defuses += 1,
                    Event::Exploded { .. } => {
                        explosions += 1;
                        if args.iter().any(|a| a == "--why") {
                            eprintln!("-- round {} exploded; bomb {:?}", g.round, g.bomb);
                            for p in g.players.iter().filter(|p| p.alive && p.team == Team::Ksk) {
                                eprintln!("   {} at {:?} ({}) — {}", p.name, (p.mover.origin * 10.0).round() / 10.0, g.place_at(p.mover.origin).unwrap_or("-"), bots.describe(p.id));
                            }
                        }
                    }
                    Event::HostageRescued { .. } => rescued += 1,
                    Event::Kill { .. } => {
                        kills += 1;
                        if contact_this_round.is_none() {
                            contact_this_round = Some(g.time - round_started);
                        }
                    }
                    Event::FreezeEnd => {
                        round_started = g.time;
                        contact_this_round = None;
                    }
                    Event::RoundEnd { winner, reason, .. } => {
                        if reason == Reason::TimeRanOut && args.iter().any(|a| a == "--why") {
                            eprintln!("-- round {} ran out; bomb {:?}", g.round, g.bomb);
                            for (i, h) in g.hostages.iter().enumerate() {
                                eprintln!("   hostage {i} at {:?} alive={} rescued={} leader={:?}", (h.mover.origin * 10.0).round() / 10.0, h.alive, h.rescued, h.leader);
                            }
                            let side = if g.cfg.mode == Mode::Rescue { Team::Ksk } else { Team::Opfor };
                            for p in g.players.iter().filter(|p| p.alive && p.team == side) {
                                eprintln!("   {} at {:?} ({}) — {}", p.name, (p.mover.origin * 10.0).round() / 10.0, g.place_at(p.mover.origin).unwrap_or("-"), bots.describe(p.id));
                            }
                        }
                        played += 1;
                        round_time += g.time - round_started;
                        if let Some(c) = contact_this_round {
                            first_contact.push(c);
                        }
                        if let Some(w) = winner {
                            wins[w.index()] += 1;
                        }
                        let r = match reason {
                            Reason::Elimination => format!("{} eliminated", winner.map_or("?", |w| w.other().name())),
                            Reason::BombExploded => "bomb exploded".into(),
                            Reason::BombDefused => "bomb defused".into(),
                            Reason::TimeRanOut => "time ran out".into(),
                            Reason::HostagesRescued => "hostages rescued".into(),
                            Reason::Draw => "draw".into(),
                        };
                        *reasons.entry(r).or_default() += 1;
                    }
                    _ => {}
                }
            }
            if g.phase == Phase::Live {
                for (i, p) in g.players.iter().enumerate() {
                    let wants = p.alive && cmds.iter().any(|(id, c)| *id == i && c.forward.abs() + c.side.abs() > 0.3);
                    if !wants {
                        push[i] = (p.mover.origin, 0.0, false);
                    } else {
                        push[i].1 += TICK;
                        if p.mover.origin.distance(push[i].0) > 0.6 {
                            push[i] = (p.mover.origin, 0.0, false);
                        } else if push[i].1 > 2.5 {
                            snag_seconds += TICK;
                            if !push[i].2 {
                                push[i].2 = true;
                                let at = p.mover.origin;
                                // Another body in the way?
                                let near = g.players.iter().filter(|q| q.alive && q.id != p.id && q.mover.origin.distance(at) < 1.2).map(|q| format!("{} {:.1} m", q.name, q.mover.origin.distance(at))).collect::<Vec<_>>();
                                let blocked = if near.is_empty() { String::new() } else { format!(" [next to {}]", near.join(", ")) };
                                match snags.iter_mut().find(|s| s.0.distance(at) < 1.5) {
                                    Some(s) => s.1 += 1,
                                    None => snags.push((at, 1, format!("{} ({}){blocked}", bots.describe(p.id), g.place_at(at).unwrap_or("-")))),
                                }
                                if args.iter().any(|a| a == "--why") && !near.is_empty() {
                                    eprintln!("snag {} at {:?}{blocked}", p.name, (at * 10.0).round() / 10.0);
                                }
                            }
                        }
                    }
                }
                for (i, p) in g.players.iter().enumerate() {
                    if !p.alive {
                        continue;
                    }
                    alive_seconds += TICK;
                    let moved = p.mover.origin.distance(last_pos[i]);
                    last_pos[i] = p.mover.origin;
                    if moved < 0.05 * TICK {
                        still[i] += TICK;
                    } else {
                        still[i] = 0.0;
                    }
                    // Standing still over 12 s outside a hold is suspicious (defenders hold on purpose, so only
                    // attackers and hunters count).
                    if still[i] > 12.0 && p.team == Team::Opfor && !matches!(g.bomb, zugriff_sim::game::Bomb::Planted { .. }) {
                        stuck_seconds += TICK;
                    }
                }
            }
            if g.phase == Phase::MatchOver {
                break;
            }
        }
        eprintln!("seed {seed}: KSK {} – {} OPFOR ({} rounds)", g.score[0], g.score[1], g.round);
    }
    println!("rounds played: {played} over {seeds} matches ({:.1} min wall)", started.elapsed().as_secs_f32() / 60.0);
    let total = (wins[0] + wins[1]).max(1) as f32;
    println!("round wins: KSK {} ({:.0}%), OPFOR {} ({:.0}%)", wins[0], wins[0] as f32 / total * 100.0, wins[1], wins[1] as f32 / total * 100.0);
    for (r, n) in &reasons {
        println!("  {r}: {n}");
    }
    println!("plants {plants} ({:.0}% of rounds), defuses {defuses}, explosions {explosions}, hostages rescued {rescued}", plants as f32 / played.max(1) as f32 * 100.0);
    println!("kills {kills} ({:.1} per round), avg round {:.0} s", kills as f32 / played.max(1) as f32, round_time / played.max(1) as f32);
    if !first_contact.is_empty() {
        first_contact.sort_by(f32::total_cmp);
        println!("first kill after {:.0} s (median)", first_contact[first_contact.len() / 2]);
    }
    println!("cpu: bots {:.1} s, sim {:.1} s", think_time, tick_time);
    println!("attackers standing still >12 s: {:.1}% of their alive time", stuck_seconds / alive_seconds.max(1.0) * 200.0);
    let events: u32 = snags.iter().map(|s| s.1).sum();
    println!("snags: {events} at {} spots, {:.2}% of alive time ({:.1} per round)", snags.len(), snag_seconds / alive_seconds.max(1.0) * 100.0, events as f32 / played.max(1) as f32);
    snags.sort_by(|a, b| b.1.cmp(&a.1));
    let n: usize = arg("--snags").and_then(|v| v.parse().ok()).unwrap_or(12);
    for (at, count, what) in snags.iter().take(n) {
        println!("  {count:>3}× at {:?} — {what}", (*at * 10.0).round() / 10.0);
    }
}
