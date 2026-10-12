//! Thrown grenades: they fly under gravity, bounce off the world (losing speed) and go off — the HE grenade and the
//! flashbang on a fuse, the smoke grenade once it has come to rest. The HE blast hurts players it can see; the flash
//! blinds those who look towards it; smoke clouds hide what is behind them (for bots, and on screen).

use crate::game::{Event, Game, Smoke, SMOKE_TIME, TICK};
use crate::units::METRES_PER_UNIT;
use crate::weapons::WeaponId;
use crate::world::Contents;
use glam::Vec3;

const U: f32 = METRES_PER_UNIT as f32;
/// Grenades fall at 0.4 of the players' gravity.
const GRAVITY: f32 = 0.4 * 800.0 * U;
const ELASTICITY: f32 = 0.45;
pub const FUSE: f32 = 1.6;
const HE_DAMAGE: f32 = 98.0;
const HE_RADIUS: f32 = 350.0 * U;
const FLASH_RANGE: f32 = 1500.0 * U;

pub fn tick(g: &mut Game) {
    let world = g.level.clone();
    let world = &world.world;
    let ext = Vec3::splat(2.0 * U);
    let mut blow: Vec<usize> = Vec::new();
    for (i, n) in g.grenades.iter_mut().enumerate() {
        n.age += TICK;
        if !n.rest {
            n.vel.y -= GRAVITY * TICK;
            let mut left = TICK;
            for _ in 0..3 {
                let to = n.pos + n.vel * left;
                let t = world.trace(n.pos, to, ext, Contents::MASK_GRENADE);
                if t.start_solid {
                    // Stuck: nudge out upwards and stop.
                    n.pos.y += 0.02;
                    n.vel = Vec3::ZERO;
                    break;
                }
                n.pos = t.end;
                if t.fraction >= 1.0 {
                    break;
                }
                left *= 1.0 - t.fraction;
                // Bounce: reflect off the plane and lose speed; slide to rest on floors.
                let vn = n.vel.dot(t.normal);
                n.vel = (n.vel - t.normal * vn * 2.0) * ELASTICITY;
                if t.normal.y > 0.7 {
                    n.vel *= 0.8;
                    if n.vel.length() < 0.6 {
                        n.vel = Vec3::ZERO;
                        n.rest = true;
                    }
                }
                if vn.abs() > 1.0 {
                    g.events.push(Event::GrenadeBounce { grenade: n.id, pos: n.pos });
                }
                n.spin *= 0.7;
            }
        }
        let due = match n.kind {
            WeaponId::Smoke => (n.rest && n.age > 1.0) || n.age > 4.0,
            _ => n.age >= FUSE,
        };
        if due {
            blow.push(i);
        }
    }
    for &i in blow.iter().rev() {
        let n = g.grenades.remove(i);
        g.events.push(Event::Detonate { grenade: n.id, kind: n.kind, pos: n.pos });
        match n.kind {
            WeaponId::He => he(g, n.pos, n.thrower),
            WeaponId::Flash => flash(g, n.pos),
            _ => g.smokes.push(Smoke { id: n.id, pos: n.pos, age: 0.0 }),
        }
        g.noise(n.thrower, n.pos);
    }
    for s in &mut g.smokes {
        s.age += TICK;
    }
    let gone: Vec<u32> = g.smokes.iter().filter(|s| s.age >= SMOKE_TIME).map(|s| s.id).collect();
    for id in gone {
        g.events.push(Event::SmokeGone { smoke: id });
    }
    g.smokes.retain(|s| s.age < SMOKE_TIME);
}

fn he(g: &mut Game, at: Vec3, thrower: usize) {
    let centre = at + Vec3::Y * 0.15;
    for i in 0..g.players.len() {
        if !g.players[i].alive {
            continue;
        }
        let hb = g.players[i].hitboxes();
        let d = hb.distance(centre);
        if d >= HE_RADIUS {
            continue;
        }
        // Behind cover: nothing (checked to the chest and the head).
        if !g.clear_line(centre, hb.chest(), false) && !g.clear_line(centre, hb.head(), false) {
            continue;
        }
        let dmg = HE_DAMAGE * (1.0 - d / HE_RADIUS);
        let dir = (hb.chest() - centre).normalize_or_zero();
        crate::combat::damage(g, i, Some(thrower), dmg, None, WeaponId::He, dir, hb.chest(), 1.0, false);
    }
    // Hostages are hurt too.
    for h in &mut g.hostages {
        if h.alive && !h.rescued && h.hitboxes().distance(centre) < HE_RADIUS * 0.5 {
            h.health -= 30;
        }
    }
}

fn flash(g: &mut Game, at: Vec3) {
    let centre = at + Vec3::Y * 0.1;
    for i in 0..g.players.len() {
        if !g.players[i].alive {
            continue;
        }
        let eye = g.eye(i);
        let d = eye.distance(centre);
        if d > FLASH_RANGE || !g.clear_line(eye, centre, true) {
            continue;
        }
        let look = g.players[i].aim_dir();
        let facing = look.dot((centre - eye).normalize_or_zero());
        // Looking at it: up to ~4.5 s; side-on less; turned away a short glare.
        let angle = if facing > 0.6 {
            1.0
        } else if facing > 0.0 {
            0.55
        } else if facing > -0.5 {
            0.3
        } else {
            0.12
        };
        let near = (1.0 - d / FLASH_RANGE).clamp(0.0, 1.0);
        let seconds = 4.6 * angle * (0.35 + 0.65 * near);
        if seconds < 0.25 {
            continue;
        }
        let p = &mut g.players[i];
        if seconds > p.flash_left {
            p.flash_left = seconds;
            p.flash_total = seconds;
        }
        g.events.push(Event::Flashed { player: i, seconds });
    }
}

#[cfg(test)]
mod tests {
    use crate::game::tests::{game, run};
    use crate::game::{Grenade, Mode, Team};
    use crate::weapons::WeaponId;
    use glam::Vec3;

    #[test]
    fn a_grenade_bounces_and_settles() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 7.0, &[]);
        g.grenades.push(Grenade { id: 99, kind: WeaponId::Smoke, pos: Vec3::new(0.0, 1.5, 10.0), vel: Vec3::new(0.0, 3.0, -6.0), thrower: 0, team: Team::Ksk, age: 0.0, rest: false, spin: Vec3::ZERO });
        run(&mut g, 5.0, &[]);
        assert!(g.grenades.is_empty(), "went off");
        assert_eq!(g.smokes.len(), 1);
        let s = g.smokes[0].pos;
        assert!(s.y > -0.1 && s.y < 0.3, "{s}");
        // The cloud hides a line through it.
        assert!(g.smoke_blocks(s + Vec3::new(-6.0, 1.0, 0.0), s + Vec3::new(6.0, 1.0, 0.0)));
        run(&mut g, 18.5, &[]);
        assert!(g.smokes.is_empty());
    }

    #[test]
    fn a_flash_blinds_those_looking_at_it() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 7.0, &[]);
        // Players 0 and 1 stand 5 m from the flash; 0 faces it, 1 faces away.
        let flash = Vec3::new(0.0, 1.0, 0.0);
        g.players[0].mover.origin = Vec3::new(0.0, 0.0, 5.0);
        g.players[0].yaw = 0.0;
        g.players[0].pitch = 0.0;
        g.players[1].mover.origin = Vec3::new(0.0, 0.0, -5.0);
        g.players[1].yaw = 0.0;
        g.grenades.push(Grenade { id: 5, kind: WeaponId::Flash, pos: flash, vel: Vec3::ZERO, thrower: 1, team: Team::Opfor, age: 1.6, rest: true, spin: Vec3::ZERO });
        g.players[0].pitch = -0.1;
        // Keep them where they are for the tick: their commands would turn them to yaw 0 anyway.
        g.tick(&[]);
        assert!(g.players[0].flash_left > 2.5, "{}", g.players[0].flash_left);
        assert!(g.players[1].flash_left < 1.0, "{}", g.players[1].flash_left);
    }
}
