//! Shots to kill with a scoped rifle, as a player fires it: the shooter stands still, scoped, and aims exactly at the
//! head, chest, stomach or legs of an armoured, helmeted target about `d` metres away (a fresh target each time).
//! `cargo run --release -p zugriff_sim --example sniper_check -- <map> <ssg3000|g22|g28|svd> [metres]`
use glam::Vec3;
use std::sync::Arc;
use zugriff_sim::game::{Btn, Config, Event, Game, Mode, Phase, Team, UserCmd};
use zugriff_sim::hitbox::Zone;
use zugriff_sim::weapons::{Gun, WeaponId};
use zugriff_sim::{compile, map};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let level = Arc::new(compile::compile(&map::parse(&std::fs::read_to_string(&args[1]).unwrap()).unwrap()));
    let weapon = match args.get(2).map(String::as_str) {
        Some("g22") => WeaponId::G22,
        Some("g28") => WeaponId::G28,
        Some("svd") => WeaponId::Svd,
        _ => WeaponId::Ssg3000,
    };
    let dist: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(25.0);
    for zone in [Zone::Head, Zone::Chest, Zone::Stomach, Zone::Legs] {
        let mut shots_to_kill = Vec::new();
        let mut hits_dmg = Vec::new();
        for trial in 0..12u64 {
            let mut g = Game::new(level.clone(), Config { mode: Mode::Deathmatch, warmup: 0.0, dm_time: 999.0, ..Default::default() }, trial + 1);
            let a = g.add_player("shooter", Team::Ksk, false);
            let t = g.add_player("target", Team::Opfor, false);
            for _ in 0..3 {
                g.tick(&[]);
            }
            while g.phase != Phase::Live {
                g.tick(&[]);
            }
            // Open ground: the shooter where it is, the target `dist` away along the first clear direction.
            let from = g.players[a].mover.origin;
            let mut placed = false;
            for k in 0..16 {
                let ang = k as f32 * std::f32::consts::TAU / 16.0;
                let to = from + Vec3::new(ang.cos(), 0.0, ang.sin()) * dist;
                g.players[t].mover.origin = to;
                g.players[t].mover.velocity = Vec3::ZERO;
                for _ in 0..30 {
                    g.tick(&[]);
                }
                if g.clear_line(g.eye(a), g.players[t].hitboxes().head(), true) && g.clear_line(g.eye(a), g.players[t].hitboxes().stomach(), true) && g.players[t].mover.origin.distance(to) < 0.5 {
                    placed = true;
                    break;
                }
            }
            if !placed {
                continue;
            }
            {
                let p = &mut g.players[t];
                p.armor = 100;
                p.helmet = true;
                p.health = 100;
                let s = &mut g.players[a];
                s.inv.primary = Some(Gun::new(weapon));
                s.inv.active = weapon;
            }
            let mut fired = 0;
            let mut scoped = false;
            for tick in 0..(66 * 12) {
                if !g.players[t].alive {
                    break;
                }
                let eye = g.eye(a);
                let hb = g.players[t].hitboxes();
                let point = match zone {
                    Zone::Head => hb.head(),
                    Zone::Chest => hb.chest(),
                    Zone::Stomach => hb.stomach(),
                    Zone::Legs => g.players[t].mover.origin + Vec3::Y * 0.45,
                };
                let d = point - eye;
                let yaw = (-d.x).atan2(-d.z);
                let pitch = (d.y / d.length()).asin();
                // Take the punch out, as a player's mouse does.
                let p = &g.players[a];
                let (yaw, pitch) = (yaw - p.punch.y.to_radians(), pitch - p.punch.x.to_radians());
                let mut buttons = Btn::empty();
                let inv = &g.players[a].inv;
                if inv.zoom == 0 && inv.cooldown <= 0.0 && !scoped {
                    buttons |= Btn::ATTACK2;
                    scoped = true;
                } else if inv.zoom > 0 && inv.cooldown <= 0.0 && tick % 2 == 0 {
                    buttons |= Btn::ATTACK;
                } else if inv.zoom == 0 {
                    scoped = false;
                }
                g.tick(&[(a, UserCmd { yaw, pitch, buttons, ..Default::default() })]);
                for e in g.events.drain(..) {
                    match e {
                        Event::Shot { player, from, dir, .. } if player == a => {
                            fired += 1;
                            if std::env::var("DEBUG").is_ok() {
                                let want = (point - from).normalize();
                                let p = &g.players[a];
                                eprintln!("shot from {from:.2?} (eye {eye:.2?}) dir {dir:.3?} want {want:.3?}; cmd pitch {pitch:.3} player pitch {:.3} punch {:.2?} zoom {}", p.pitch, p.punch, p.inv.zoom);
                            }
                        }
                        Event::Hit { victim, damage, zone: z, pos, .. } if victim == t => {
                            if std::env::var("DEBUG").is_ok() {
                                let hb = g.players[t].hitboxes();
                                eprintln!("hit {z:?} at {pos:.2?}; aimed {point:.2?}; head {:.2?} chest {:.2?} origin {:.2?}", hb.head(), hb.chest(), g.players[t].mover.origin);
                            }
                            hits_dmg.push((z, damage))
                        }
                        _ => {}
                    }
                }
            }
            if !g.players[t].alive {
                shots_to_kill.push(fired);
            }
        }
        let mean = shots_to_kill.iter().sum::<i32>() as f32 / shots_to_kill.len().max(1) as f32;
        let dmg: Vec<String> = hits_dmg.iter().take(6).map(|(z, d)| format!("{z:?} {d}")).collect();
        println!("{weapon:?} at {dist} m, aimed at {zone:?}: killed {}/12, {mean:.1} shots on average; hits: {}", shots_to_kill.len(), dmg.join(", "));
    }
}
