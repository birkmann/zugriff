//! Using the weapon in hand, each tick: firing (hitscan rounds that go through thin walls, lose power per material and
//! hit the players' and hostages' hitboxes), the recoil's punch, reloading, scoping, the knife, throwing grenades and
//! planting the bomb. Damage and armour follow the genre's rules: hit zone multipliers, armour taking `ratio/2` of the
//! damage and wearing down, the helmet for the head.

use crate::game::{dir_from, Btn, Event, Game, Grenade, Phase, PlayerId, Team};
use crate::hitbox::Zone;
use crate::material::Material;
use crate::weapons::{Action, Class, Pull, Stance, Tick, WeaponId, STAB_TIME, STRIKE};
use crate::world::Contents;
use glam::Vec3;

/// Handles the weapon in a player's hands for one tick.
pub fn weapon_tick(g: &mut Game, id: PlayerId, held: Btn, pressed: Btn) {
    let frozen = matches!(g.phase, Phase::Freeze | Phase::MatchOver);
    let attack = held.contains(Btn::ATTACK) && !frozen;
    let attack_pressed = pressed.contains(Btn::ATTACK) && !frozen;
    let attack2 = held.contains(Btn::ATTACK2) && !frozen;
    let attack2_pressed = pressed.contains(Btn::ATTACK2) && !frozen;
    let dt = crate::game::TICK;

    let active = g.players[id].inv.active;
    let class = active.def().class;
    // Timers.
    let firing_now = attack && class != Class::Grenade && class != Class::Bomb;
    if let Some(t) = g.players[id].inv.tick(dt, firing_now) {
        match t {
            Tick::Reloaded => {}
            Tick::Shell => g.events.push(Event::Shell { player: id }),
            Tick::Rezoom => {
                let level = g.players[id].inv.zoom;
                g.events.push(Event::Zoom { player: id, level });
            }
        }
    }
    if pressed.contains(Btn::RELOAD) && g.players[id].inv.reload() {
        g.events.push(Event::Reload { player: id, weapon: active });
    }
    match class {
        Class::Knife => knife(g, id, attack, attack2),
        Class::Grenade => grenade(g, id, attack, attack2, attack_pressed, attack2_pressed),
        Class::Bomb => bomb(g, id, attack),
        _ => {
            // The right button scopes a scoped weapon, and raises the sights on any other gun (held).
            g.players[id].inv.aim(attack2, dt);
            // The right button while the bolt is worked: stay out of the scope (no automatic return).
            if attack2_pressed && g.players[id].inv.rezoom > 0 {
                g.players[id].inv.rezoom = 0;
                g.players[id].inv.cooldown2 = 0.25;
            } else if attack2_pressed && active.def().scoped() && !g.players[id].inv.busy() && g.players[id].inv.cooldown2 <= 0.0 {
                let inv = &mut g.players[id].inv;
                inv.zoom = (inv.zoom + 1) % (active.def().zoom.len() as u8 + 1);
                inv.cooldown2 = 0.25;
                let level = inv.zoom;
                g.events.push(Event::Zoom { player: id, level });
            }
            match g.players[id].inv.pull(attack_pressed, attack) {
                Pull::Fired => {
                    fire(g, id);
                    g.players[id].inv.after_shot();
                }
                Pull::Reload => g.events.push(Event::Reload { player: id, weapon: active }),
                Pull::Empty => g.events.push(Event::DryFire { player: id }),
                Pull::None => {}
            }
        }
    }
    g.players[id].inv.trigger_held = attack;
}

fn stance(g: &Game, id: PlayerId) -> Stance {
    let p = &g.players[id];
    if !p.mover.on_ground {
        Stance::Air
    } else if p.mover.horizontal_speed() > p.inv.def().speed * 0.5 {
        Stance::Moving
    } else if p.mover.ducked {
        Stance::Crouched
    } else {
        Stance::Standing
    }
}

/// A round (or a shotgun's pellets) leaves the gun.
fn fire(g: &mut Game, id: PlayerId) {
    let def = g.players[id].inv.def();
    let eye = g.eye(id);
    let p = &g.players[id];
    let speed_frac = p.mover.horizontal_speed() / def.speed.max(0.1);
    let spread = p.inv.spread(speed_frac, p.mover.on_ground, p.mover.ducked);
    let aim = p.aim_dir();
    let (right, up) = basis(aim);
    let tracer = def.tracer > 0 && p.inv.fired % def.tracer as u32 == 0;
    let pellets = def.pellets.max(1);
    let mut first_end = eye + aim * 10.0;
    for i in 0..pellets {
        // The classic spread: the sum of two uniforms in each axis (more rounds near the centre).
        let x = g.rng.range(-0.5, 0.5) + g.rng.range(-0.5, 0.5);
        let y = g.rng.range(-0.5, 0.5) + g.rng.range(-0.5, 0.5);
        let dir = (aim + right * x * spread + up * y * spread).normalize();
        let end = bullet(g, id, eye, dir, def.id);
        if i == 0 {
            first_end = end;
        }
        if pellets > 1 && i > 0 && i < 4 {
            g.events.push(Event::Shot { player: id, weapon: def.id, from: eye, dir, end, tracer: false });
        }
    }
    g.events.push(Event::Shot { player: id, weapon: def.id, from: eye, dir: aim, end: first_end, tracer });
    g.noise(id, eye);
    g.players[id].gunshot = Some((eye, g.time));
    // The recoil.
    let st = stance(g, id);
    let r = def.recoil;
    let mut rng = g.rng;
    let (up_kick, lat_kick) = g.players[id].inv.kick(&mut rng, st);
    g.rng = rng;
    let p = &mut g.players[id];
    p.punch.x = (p.punch.x + up_kick).min(r.up_max.max(up_kick));
    p.punch.y = (p.punch.y + lat_kick).clamp(-r.lateral_max, r.lateral_max);
}

fn basis(dir: Vec3) -> (Vec3, Vec3) {
    let right = dir.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(dir).normalize();
    (right, up)
}

/// One round along `dir`: hits the nearest body or surface, goes through thin walls while it has power, and returns
/// where it ended (for the tracer).
pub fn bullet(g: &mut Game, shooter: PlayerId, eye: Vec3, dir: Vec3, weapon: WeaponId) -> Vec3 {
    let def = weapon.def();
    let world = g.level.clone();
    let world = &world.world;
    let mut start = eye;
    let mut remaining = def.range;
    let mut travelled = 0.0;
    let mut power = def.penetration;
    let mut pens = def.penetrations;
    let mut damage_scale = 1.0f32;
    let mut wallbang = false;
    let mut hit_players: Vec<PlayerId> = vec![shooter];
    let mut hit_hostages: Vec<usize> = Vec::new();
    let mut end = eye + dir * remaining;
    for _ in 0..8 {
        let t = world.trace(start, start + dir * remaining, Vec3::ZERO, Contents::MASK_SHOT_ALL);
        let wall = if t.start_solid { 0.0 } else { t.fraction * remaining };
        // The nearest body in front of the wall.
        let mut body: Option<(f32, Option<PlayerId>, Option<usize>, Zone)> = None;
        for p in &g.players {
            if !p.alive || hit_players.contains(&p.id) {
                continue;
            }
            let hb = p.hitboxes();
            if !hb.near_ray(start, dir, wall) {
                continue;
            }
            if let Some((zone, d)) = hb.ray(start, dir, wall)
                && body.is_none_or(|b| d < b.0)
            {
                body = Some((d, Some(p.id), None, zone));
            }
        }
        for (i, h) in g.hostages.iter().enumerate() {
            if !h.alive || h.rescued || hit_hostages.contains(&i) {
                continue;
            }
            if let Some((zone, d)) = h.hitboxes().ray(start, dir, wall)
                && body.is_none_or(|b| d < b.0)
            {
                body = Some((d, None, Some(i), zone));
            }
        }
        if let Some((d, pid, hid, zone)) = body {
            let at = start + dir * d;
            let range = travelled + d;
            let amount = def.damage_at(range) * damage_scale;
            if let Some(v) = pid {
                hit_players.push(v);
                damage(g, v, Some(shooter), amount, Some(zone), weapon, dir, at, def.armor_ratio, wallbang);
            }
            if let Some(h) = hid {
                hit_hostages.push(h);
                hurt_hostage(g, h, Some(shooter), amount * zone.multiplier(), at, dir);
            }
            // Through a body only with penetration to spare.
            if pens == 0 || power <= 0.05 {
                return at;
            }
            pens -= 1;
            power -= 0.1;
            damage_scale *= 0.6;
            travelled += d;
            remaining -= d;
            start = at + dir * 0.05;
            end = at;
            continue;
        }
        if !t.hit() {
            return start + dir * remaining;
        }
        let at = start + dir * wall;
        end = at;
        let Some(b) = t.brush else { return at };
        let surface = t.surface.unwrap_or(0);
        let material = Material::of(world.surface_name(surface));
        if !t.start_solid {
            g.events.push(Event::Impact { pos: at, normal: t.normal, material, exit: false });
        }
        // Through the wall?
        let is_window = world.brushes[b as usize].contents == Contents::WINDOW;
        let Some((thick, exit_surface)) = world.exit(b, at, dir) else { return at };
        let cost = thick * material.hardness();
        if pens == 0 || cost > power {
            return at;
        }
        power -= cost;
        pens -= if is_window { 0 } else { 1 };
        damage_scale *= (1.0 - 0.5 * cost / def.penetration.max(0.01)).max(0.25) * if is_window { 0.9 } else { 0.8 };
        wallbang |= !is_window;
        let out = at + dir * thick;
        let exit_material = Material::of(world.surface_name(exit_surface));
        g.events.push(Event::Impact { pos: out, normal: dir, material: exit_material, exit: true });
        travelled += wall + thick;
        remaining -= wall + thick;
        start = out + dir * 0.004;
        if remaining <= 0.0 {
            return out;
        }
    }
    end
}

/// Hurts a player, through armour. `zone: None` is damage without a hit zone (falls, blasts): it counts as the chest
/// for armour only if `armor_ratio` < 1 is given with it.
#[allow(clippy::too_many_arguments)]
pub fn damage(g: &mut Game, victim: PlayerId, attacker: Option<PlayerId>, amount: f32, zone: Option<Zone>, weapon: WeaponId, dir: Vec3, pos: Vec3, armor_ratio: f32, wallbang: bool) {
    if !g.players[victim].alive || amount <= 0.0 {
        return;
    }
    if let Some(a) = attacker
        && a != victim
        && !g.cfg.friendly_fire
        && g.players[a].team == g.players[victim].team
    {
        return;
    }
    if let Some(a) = attacker.filter(|&a| a != victim) {
        let from = g.eye(a);
        g.players[victim].hurt = Some((a, from, g.time));
    }
    let mut dmg = amount * zone.map_or(1.0, |z| z.multiplier());
    let armoured = match zone {
        Some(z) => z.armoured(g.players[victim].helmet),
        // Blasts: armour helps; falls: no.
        None => weapon != WeaponId::Knife,
    };
    let v = &mut g.players[victim];
    let mut armor_hit = false;
    if v.armor > 0 && armoured {
        let ratio = (armor_ratio * 0.5).min(1.0);
        let mut new = dmg * ratio;
        let mut taken = (dmg - new) * 0.5;
        if taken > v.armor as f32 {
            taken = v.armor as f32;
            new = dmg - taken * 2.0;
        }
        v.armor = (v.armor - taken.ceil() as i32).max(0);
        dmg = new;
        armor_hit = true;
    }
    let dmg_i = dmg.round().max(1.0) as i32;
    v.health -= dmg_i;
    v.tag = v.tag.min(if zone == Some(Zone::Legs) { 0.45 } else { 0.55 });
    if let Some(a) = attacker {
        if a < v.damage_from.len() {
            v.damage_from[a] += dmg_i as f32;
        }
    }
    let dead = v.health <= 0;
    if let Some(a) = attacker
        && a != victim
    {
        g.players[a].stats.damage += dmg_i.min(100) as f32;
    }
    g.events.push(Event::Hit { victim, attacker, pos, dir, damage: dmg_i, zone: zone.unwrap_or(Zone::Chest), armor: armor_hit });
    if dead {
        let headshot = zone == Some(Zone::Head);
        g.kill(victim, attacker, weapon, headshot, wallbang);
    }
}

fn hurt_hostage(g: &mut Game, h: usize, by: Option<PlayerId>, amount: f32, pos: Vec3, dir: Vec3) {
    let host = &mut g.hostages[h];
    host.health -= amount.round() as i32;
    g.events.push(Event::HostageHit { hostage: h, pos, dir });
    if host.health <= 0 && host.alive {
        host.alive = false;
        host.leader = None;
        g.events.push(Event::HostageKilled { hostage: h, by });
        if let Some(b) = by {
            g.give_money(b, crate::game::money::HOSTAGE_KILL, "Geisel getötet");
            g.players[b].stats.score -= 4;
        }
    }
}

fn knife(g: &mut Game, id: PlayerId, attack: bool, attack2: bool) {
    let dt = crate::game::TICK;
    let inv = &mut g.players[id].inv;
    if let Some(mut s) = inv.swing {
        s.t += dt;
        let strike_at = if s.stab { STRIKE.1 } else { STRIKE.0 };
        let strike = !s.struck && s.t >= strike_at;
        if strike {
            s.struck = true;
        }
        let length = if s.stab { STAB_TIME } else { crate::weapons::WeaponId::Knife.def().cycle };
        inv.swing = if s.t >= length { None } else { Some(s) };
        if strike {
            knife_strike(g, id, s.stab);
        }
        return;
    }
    if inv.busy() || inv.cooldown > 0.0 {
        return;
    }
    if attack || attack2 {
        let stab = attack2 && !attack;
        inv.swing = Some(crate::weapons::Swing { t: 0.0, stab, struck: false });
        inv.cooldown = if stab { STAB_TIME } else { WeaponId::Knife.def().cycle };
        g.events.push(Event::KnifeSwing { player: id, stab });
    }
}

fn knife_strike(g: &mut Game, id: PlayerId, stab: bool) {
    let eye = g.eye(id);
    let dir = g.players[id].aim_dir();
    let reach = if stab { 48.0 } else { 64.0 } * crate::units::METRES_PER_UNIT as f32;
    let world = g.level.clone();
    let t = world.world.trace(eye, eye + dir * reach, Vec3::ZERO, Contents::MASK_SHOT);
    let wall = t.fraction * reach;
    // Players in reach (a slightly fat ray: the blade sweeps).
    let mut best: Option<(PlayerId, f32, Zone)> = None;
    for p in &g.players {
        if !p.alive || p.id == id {
            continue;
        }
        let hb = p.hitboxes();
        for off in [Vec3::ZERO, Vec3::Y * 0.12, Vec3::Y * -0.12] {
            if let Some((z, d)) = hb.ray(eye + off, dir, wall)
                && best.is_none_or(|b| d < b.1)
            {
                best = Some((p.id, d, z));
            }
        }
    }
    if let Some((v, d, _)) = best {
        // From behind: the victim faces away from the attacker.
        let vfwd = dir_from(g.players[v].yaw, 0.0);
        let behind = vfwd.dot(dir) > 0.5;
        let amount = match (stab, behind) {
            (true, true) => 180.0,
            (true, false) => 65.0,
            (false, true) => 90.0,
            (false, false) => if g.players[id].inv.cooldown < 0.2 { 25.0 } else { 20.0 },
        };
        let at = eye + dir * d;
        damage(g, v, Some(id), amount, None, WeaponId::Knife, dir, at, 0.85, false);
        g.events.push(Event::KnifeHit { player: id, pos: at, material: Some(Material::Flesh) });
    } else if t.hit() {
        let material = t.surface.map(|s| Material::of(world.world.surface_name(s)));
        g.events.push(Event::KnifeHit { player: id, pos: t.end, material });
        g.events.push(Event::Impact { pos: t.end, normal: t.normal, material: material.unwrap_or(Material::Concrete), exit: false });
    }
}

/// Grenade: the trigger pulls the pin; letting go throws (left: overhand, right: underhand lob, both: in between).
fn grenade(g: &mut Game, id: PlayerId, attack: bool, attack2: bool, attack_pressed: bool, attack2_pressed: bool) {
    let inv = &mut g.players[id].inv;
    match inv.action {
        Action::Idle if inv.cooldown <= 0.0 && (attack_pressed || attack2_pressed) => {
            inv.action = Action::PinPulled { lob: !attack };
        }
        Action::PinPulled { lob } => {
            let lob = lob && !attack;
            inv.action = Action::PinPulled { lob };
            if !attack && !attack2 {
                throw(g, id, lob, attack2);
            }
        }
        _ => {}
    }
}

fn throw(g: &mut Game, id: PlayerId, lob: bool, _both: bool) {
    let kind = g.players[id].inv.active;
    if g.players[id].inv.remove(kind).is_none() {
        g.players[id].inv.action = crate::weapons::Action::Idle;
        return;
    }
    let p = &g.players[id];
    // The classic throw: aimed a little above the view, at 675 u/s (a lob a third of it), plus the thrower's speed.
    let pitch_deg = p.pitch.to_degrees();
    let pitch = (pitch_deg + (90.0 - pitch_deg.abs()) * 10.0 / 90.0).to_radians();
    let dir = dir_from(p.yaw, pitch);
    let speed = if lob { 0.35 } else { 1.0 } * 675.0 * crate::units::METRES_PER_UNIT as f32;
    let vel = dir * speed + p.mover.velocity * 1.25;
    let eye = p.eye(&g.params);
    let from = eye + dir * 0.3 - Vec3::Y * 0.1;
    // Don't start inside a wall.
    let t = g.level.world.trace(eye, from, Vec3::splat(0.05), Contents::MASK_GRENADE);
    let from = t.end;
    let team: Team = p.team;
    let gid = g.id();
    let spin = Vec3::new(g.rng.range(-8.0, 8.0), g.rng.range(-8.0, 8.0), g.rng.range(-8.0, 8.0));
    g.grenades.push(Grenade { id: gid, kind, pos: from, vel, thrower: id, team, age: 0.0, rest: false, spin });
    g.events.push(Event::Throw { player: id, grenade: gid, kind });
    let inv = &mut g.players[id].inv;
    inv.cooldown = 0.5;
    if matches!(inv.action, Action::PinPulled { .. }) {
        inv.action = Action::Idle;
    }
}

/// The bomb: hold the trigger on a site to plant (three seconds standing still).
fn bomb(g: &mut Game, id: PlayerId, attack: bool) {
    let on_site = g.site_at(g.players[id].mover.origin).is_some() && g.players[id].mover.on_ground;
    let live = g.phase == Phase::Live;
    let inv = &mut g.players[id].inv;
    match inv.action {
        Action::Planting(t) => {
            if !attack || !on_site || !live {
                inv.action = Action::Idle;
                return;
            }
            let t = t + crate::game::TICK;
            if t >= g.cfg.plant_time {
                inv.action = Action::Idle;
                g.plant(id);
            } else {
                inv.action = Action::Planting(t);
            }
        }
        Action::Idle if attack && on_site && live => {
            inv.action = Action::Planting(0.0);
            g.events.push(Event::PlantStart { player: id });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tests::{game, run};
    use crate::game::{Mode, Select, UserCmd};

    /// Puts shooter `a` and target `b` facing each other `d` metres apart along x at z = 0 (clear of the walls).
    fn duel(g: &mut Game, a: PlayerId, b: PlayerId, from: Vec3, to: Vec3) -> (f32, f32) {
        g.players[a].mover.origin = from;
        g.players[b].mover.origin = to;
        g.players[a].mover.velocity = Vec3::ZERO;
        let eye = from + Vec3::Y * g.params.stand_eye;
        let head = g.players[b].hitboxes().head();
        let chest = g.players[b].hitboxes().chest();
        let _ = head;
        let d = chest - eye;
        let yaw = (-d.x).atan2(-d.z);
        let pitch = (d.y / d.length()).asin();
        (yaw, pitch)
    }

    #[test]
    fn a_rifle_kills_in_a_few_chest_shots() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 7.0, &[]);
        let (a, b) = (0, 1); // KSK 0 vs OPFOR 0
        g.players[a].inv.primary = Some(crate::weapons::Gun::new(WeaponId::G95));
        g.players[a].inv.select(WeaponId::G95);
        let (yaw, pitch) = duel(&mut g, a, b, Vec3::new(0.0, 0.0, 12.0), Vec3::new(0.0, 0.0, 2.0));
        g.tick(&[(a, UserCmd { yaw, pitch, ..Default::default() })]);
        run(&mut g, 1.5, &[(a, UserCmd { yaw, pitch, ..Default::default() })]);
        // Tap: one round every 0.4 s.
        let mut shots = 0;
        for _ in 0..12 {
            g.players[b].mover.origin = Vec3::new(0.0, 0.0, 2.0);
            g.tick(&[(a, UserCmd { yaw, pitch, buttons: Btn::ATTACK, ..Default::default() })]);
            shots += 1;
            run(&mut g, 0.4, &[(a, UserCmd { yaw, pitch, ..Default::default() })]);
            if !g.players[b].alive {
                break;
            }
        }
        assert!(!g.players[b].alive, "still alive with {} hp after {shots}", g.players[b].health);
        assert!(shots <= 5, "{shots} shots");
        assert!(g.events.iter().any(|e| matches!(e, Event::Kill { killer: Some(0), victim: 1, .. })));
    }

    #[test]
    fn rounds_go_through_thin_wood_but_not_thick_concrete() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 7.0, &[]);
        let m = crate::units::METRES_PER_UNIT as f32;
        let (a, b) = (0, 1);
        g.players[a].inv.primary = Some(crate::weapons::Gun::new(WeaponId::Akm));
        g.players[a].inv.select(WeaponId::Akm);
        // The wooden wall is at map x = 300..316 (sim x 7.62..8.03), z (map y) −200..200.
        let (yaw, pitch) = duel(&mut g, a, b, Vec3::new(4.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0));
        run(&mut g, 1.5, &[(a, UserCmd { yaw, pitch, ..Default::default() })]);
        g.players[b].armor = 0;
        g.tick(&[(a, UserCmd { yaw, pitch, buttons: Btn::ATTACK, ..Default::default() })]);
        assert!(g.players[b].health < 100, "the wood wall lets the round through");
        assert!(g.events.iter().any(|e| matches!(e, Event::Impact { exit: true, .. })));
        // The concrete wall at map x = −400..−272 (128 units, 3.25 m) stops it.
        let (yaw, pitch) = duel(&mut g, a, b, Vec3::new(-5.0, 0.0, 0.0), Vec3::new(-12.0, 0.0, 0.0));
        let _ = m;
        run(&mut g, 1.0, &[(a, UserCmd { yaw, pitch, ..Default::default() })]);
        let before = g.players[b].health;
        g.tick(&[(a, UserCmd { yaw, pitch, buttons: Btn::ATTACK, ..Default::default() })]);
        assert_eq!(g.players[b].health, before);
    }

    #[test]
    fn armour_and_helmet_reduce_damage() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 7.0, &[]);
        let v = 1;
        g.players[v].armor = 100;
        g.players[v].helmet = false;
        let at = Vec3::ZERO;
        damage(&mut g, v, Some(0), 33.0, Some(Zone::Head), WeaponId::G95, Vec3::X, at, 1.4, false);
        assert!(!g.players[v].alive, "no helmet: a full headshot kills");
        let v = 3;
        g.players[v].armor = 100;
        g.players[v].helmet = true;
        damage(&mut g, v, Some(0), 33.0, Some(Zone::Head), WeaponId::G95, Vec3::X, at, 1.4, false);
        assert_eq!(g.players[v].health, 100 - 92);
        assert!(g.players[v].armor < 100);
        // Friendly fire is off.
        let before = g.players[2].health;
        damage(&mut g, 2, Some(0), 50.0, Some(Zone::Chest), WeaponId::G95, Vec3::X, at, 1.4, false);
        assert_eq!(g.players[2].health, before);
    }

    #[test]
    fn spraying_climbs_and_the_punch_recovers() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 7.0, &[]);
        let a = 1;
        g.players[a].inv.primary = Some(crate::weapons::Gun::new(WeaponId::Akm));
        g.players[a].inv.select(WeaponId::Akm);
        run(&mut g, 1.5, &[]);
        let fire = UserCmd { buttons: Btn::ATTACK, pitch: 0.3, ..Default::default() };
        run(&mut g, 0.8, &[(a, fire)]);
        assert!(g.players[a].punch.x > 1.5, "{:?}", g.players[a].punch);
        run(&mut g, 1.0, &[]);
        assert!(g.players[a].punch.length() < 0.01);
    }

    #[test]
    fn a_thrown_he_grenade_hurts_nearby_players() {
        let mut g = game(Mode::Defuse);
        run(&mut g, 7.0, &[]);
        let a = 0;
        g.players[a].inv.he = 1;
        let sel = UserCmd { select: Some(Select::Weapon(WeaponId::He)), ..Default::default() };
        g.tick(&[(a, sel)]);
        run(&mut g, 1.0, &[]);
        // Everyone else stands around a spot 6 m in front.
        let from = g.players[a].mover.origin;
        let yaw = g.players[a].yaw;
        let spot = from + dir_from(yaw, 0.0) * 4.0;
        for i in 1..g.players.len() {
            if g.players[i].team == Team::Opfor {
                g.players[i].mover.origin = spot + Vec3::new(i as f32 * 0.3 - 1.5, 0.0, 0.0);
            }
        }
        let hold = UserCmd { yaw, pitch: -0.6, buttons: Btn::ATTACK, ..Default::default() };
        run(&mut g, 0.3, &[(a, hold)]);
        run(&mut g, 3.0, &[(a, UserCmd { yaw, pitch: -0.6, ..Default::default() })]);
        assert!(g.events.iter().any(|e| matches!(e, Event::Detonate { kind: WeaponId::He, .. })));
        assert!(g.players.iter().filter(|p| p.team == Team::Opfor).any(|p| p.health < 100), "someone was hurt");
    }
}
