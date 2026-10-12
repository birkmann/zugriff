//! Player movement: the classic ground/air model of the genre (friction, acceleration towards the wished direction,
//! capped air control that makes strafing work, step-up, sliding along walls), against the brush world.
//!
//! Numbers are the genre's familiar values in units (inches) converted to metres, so the feel matches what players
//! know: 250 u/s with a knife, 800 u/s² gravity, a 57-unit jump, 18-unit steps.

use crate::world::{Contents, Trace, World};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

const U: f32 = crate::units::METRES_PER_UNIT as f32;

/// Movement constants. Speeds in m/s, lengths in metres.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct MoveParams {
    pub gravity: f32,
    pub accelerate: f32,
    pub air_accelerate: f32,
    /// Air control only adds speed up to this along the wished direction (the strafe-jump cap).
    pub air_wish_cap: f32,
    pub friction: f32,
    pub stop_speed: f32,
    pub jump_speed: f32,
    pub step: f32,
    /// Speed multipliers while walking (silent) and crouched.
    pub walk: f32,
    pub crouch: f32,
    /// Seconds to fully crouch on the ground (the view eases; the hull switches at once).
    pub duck_time: f32,
    pub stand_height: f32,
    pub crouch_height: f32,
    pub stand_eye: f32,
    pub crouch_eye: f32,
    pub half_width: f32,
    /// Landing faster than this hurts; at `fatal_fall` it kills.
    pub safe_fall: f32,
    pub fatal_fall: f32,
    pub max_velocity: f32,
    pub ladder_speed: f32,
}

impl Default for MoveParams {
    fn default() -> Self {
        MoveParams {
            gravity: 800.0 * U,
            accelerate: 5.5,
            air_accelerate: 10.0,
            air_wish_cap: 30.0 * U,
            friction: 4.0,
            stop_speed: 75.0 * U,
            // v = √(2·g·h) for a 57-unit jump.
            jump_speed: (2.0f32 * 800.0 * 57.0).sqrt() * U,
            step: 18.0 * U,
            walk: 0.52,
            crouch: 0.34,
            duck_time: 0.2,
            stand_height: 72.0 * U,
            crouch_height: 54.0 * U,
            stand_eye: 64.0 * U,
            crouch_eye: 46.0 * U,
            half_width: 16.0 * U,
            safe_fall: 580.0 * U,
            fatal_fall: 1024.0 * U,
            max_velocity: 3500.0 * U,
            ladder_speed: 200.0 * U,
        }
    }
}

crate::bitflags_lite! {
    /// Held buttons that matter to movement.
    pub struct Buttons: u16 {
        const JUMP = 1;
        const DUCK = 2;
        const WALK = 4;
    }
}

/// One tick of movement input.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct MoveCmd {
    /// −1..1 each: forward/back and right/left.
    pub forward: f32,
    pub side: f32,
    /// View angles (radians): yaw about +Y (0 looks down −Z), pitch up positive.
    pub yaw: f32,
    pub pitch: f32,
    pub buttons: Buttons,
    /// The weapon's (or knife's) top speed.
    pub max_speed: f32,
}

/// A player's movement state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mover {
    /// Feet (bottom centre of the hull).
    pub origin: Vec3,
    pub velocity: Vec3,
    pub on_ground: bool,
    /// The hull is the crouched one.
    pub ducked: bool,
    /// 0 standing .. 1 crouched, for the eye height.
    pub duck: f32,
    /// Jump was held last tick (a new press is needed to jump again).
    pub jump_held: bool,
    pub on_ladder: bool,
    /// Surface under the feet, for footsteps.
    pub ground_surface: Option<u16>,
    pub noclip: bool,
}

/// What happened during a tick (for sound, damage and animation).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoveEvents {
    pub jumped: bool,
    /// Landed with this downward speed (m/s).
    pub landed: Option<f32>,
    pub fall_damage: f32,
}

impl Mover {
    pub fn height(&self, p: &MoveParams) -> f32 {
        if self.ducked { p.crouch_height } else { p.stand_height }
    }

    pub fn half_extents(&self, p: &MoveParams) -> Vec3 {
        Vec3::new(p.half_width, self.height(p) * 0.5, p.half_width)
    }

    /// The eye position (the view eases between standing and crouched eye heights).
    pub fn eye(&self, p: &MoveParams) -> Vec3 {
        let base = p.stand_eye + (p.crouch_eye - p.stand_eye) * self.duck;
        self.origin + Vec3::Y * base
    }

    pub fn horizontal_speed(&self) -> f32 {
        Vec2::new(self.velocity.x, self.velocity.z).length()
    }
}


/// Ladders are volumes; while in one and moving, the player climbs along the view.
pub trait Ladders {
    fn ladder_at(&self, feet: Vec3, height: f32) -> bool;
}

impl Ladders for () {
    fn ladder_at(&self, _: Vec3, _: f32) -> bool {
        false
    }
}

struct Ctx<'a> {
    world: &'a World,
    p: &'a MoveParams,
}

impl Ctx<'_> {
    fn trace(&self, m: &Mover, from: Vec3, to: Vec3) -> Trace {
        let ext = m.half_extents(self.p);
        let up = Vec3::Y * ext.y;
        let mut t = self.world.trace(from + up, to + up, ext, Contents::MASK_PLAYER);
        t.end -= up;
        t
    }

    fn hull_fits(&self, feet: Vec3, height: f32) -> bool {
        let ext = Vec3::new(self.p.half_width, height * 0.5, self.p.half_width);
        !self.world.solid_at(feet + Vec3::Y * ext.y, ext, Contents::MASK_PLAYER)
    }
}

fn clip_velocity(v: Vec3, n: Vec3, overbounce: f32) -> Vec3 {
    let backoff = v.dot(n) * overbounce;
    let mut out = v - n * backoff;
    // Don't creep back into the plane.
    let adjust = out.dot(n);
    if adjust < 0.0 {
        out -= n * adjust;
    }
    out
}

/// Advances `m` by `dt` seconds of `cmd`.
pub fn step(world: &World, ladders: &impl Ladders, p: &MoveParams, m: &mut Mover, cmd: &MoveCmd, dt: f32) -> MoveEvents {
    let ctx = Ctx { world, p };
    let mut ev = MoveEvents::default();
    let jump = cmd.buttons.contains(Buttons::JUMP);

    if m.noclip {
        noclip(m, cmd, dt);
        m.jump_held = jump;
        return ev;
    }

    // Ducking.
    duck(&ctx, m, cmd.buttons.contains(Buttons::DUCK), dt);

    // Wish direction from yaw only (looking up or down doesn't slow walking).
    let forward = Vec3::new(-cmd.yaw.sin(), 0.0, -cmd.yaw.cos());
    let right = Vec3::new(cmd.yaw.cos(), 0.0, -cmd.yaw.sin());
    let input = Vec2::new(cmd.forward, cmd.side);
    let input = if input.length() > 1.0 { input.normalize() } else { input };
    let mut wish = forward * input.x + right * input.y;
    let mut wish_speed = wish.length() * cmd.max_speed;
    if wish.length_squared() > 0.0 {
        wish = wish.normalize();
    }
    if m.ducked && m.on_ground {
        wish_speed *= p.crouch;
    } else if cmd.buttons.contains(Buttons::WALK) {
        wish_speed *= p.walk;
    }

    // Ladders: climb along the view while moving.
    m.on_ladder = ladders.ladder_at(m.origin, m.height(p));
    if m.on_ladder {
        let look = Vec3::new(-cmd.yaw.sin() * cmd.pitch.cos(), cmd.pitch.sin(), -cmd.yaw.cos() * cmd.pitch.cos());
        let climb = look * input.x + right * input.y;
        m.velocity = climb.normalize_or_zero() * p.ladder_speed.min(cmd.max_speed);
        if jump && !m.jump_held {
            // Jump off, away from the ladder.
            m.velocity = -forward * p.ladder_speed * 1.2;
            ev.jumped = true;
        }
        slide(&ctx, m, dt);
        categorize(&ctx, m);
        m.jump_held = jump;
        return ev;
    }

    // Half the gravity before the move, half after (exact for constant gravity).
    if !m.on_ground {
        m.velocity.y -= p.gravity * dt * 0.5;
    }

    if m.on_ground && jump && !m.jump_held {
        m.velocity.y = p.jump_speed;
        m.on_ground = false;
        ev.jumped = true;
        m.velocity.y -= p.gravity * dt * 0.5;
    }

    // The downward speed going into the move (the move clips it away on touching the ground).
    let falling = -m.velocity.y;
    let was_on_ground = m.on_ground;
    if m.on_ground {
        friction(p, m, dt);
        accelerate(m, wish, wish_speed, p.accelerate, dt);
        m.velocity.y = 0.0;
        walk_move(&ctx, m, dt);
    } else {
        air_accelerate(m, wish, wish_speed, p, dt);
        slide(&ctx, m, dt);
    }

    categorize(&ctx, m);
    if !m.on_ground {
        m.velocity.y -= p.gravity * dt * 0.5;
    }
    if m.on_ground && !was_on_ground && falling > 0.0 {
        ev.landed = Some(falling);
        if falling > p.safe_fall {
            ev.fall_damage = (falling - p.safe_fall) * 100.0 / (p.fatal_fall - p.safe_fall);
        }
    }
    // Clamp runaway speeds.
    m.velocity = m.velocity.clamp_length_max(p.max_velocity);
    m.jump_held = jump;
    ev
}

fn noclip(m: &mut Mover, cmd: &MoveCmd, dt: f32) {
    let look = Vec3::new(-cmd.yaw.sin() * cmd.pitch.cos(), cmd.pitch.sin(), -cmd.yaw.cos() * cmd.pitch.cos());
    let right = Vec3::new(cmd.yaw.cos(), 0.0, -cmd.yaw.sin());
    let mut v = look * cmd.forward + right * cmd.side;
    if cmd.buttons.contains(Buttons::JUMP) {
        v.y += 1.0;
    }
    if cmd.buttons.contains(Buttons::DUCK) {
        v.y -= 1.0;
    }
    let speed = if cmd.buttons.contains(Buttons::WALK) { 3.0 } else { 12.0 };
    m.velocity = v.normalize_or_zero() * speed;
    m.origin += m.velocity * dt;
    m.on_ground = false;
}

fn duck(ctx: &Ctx, m: &mut Mover, held: bool, dt: f32) {
    let p = ctx.p;
    // Crouching in the air lifts the feet by half the hull difference: the genre's crouch-jump reaches ~66 units.
    let delta = (p.stand_height - p.crouch_height) * 0.5;
    if held {
        if !m.ducked {
            if m.on_ground {
                m.ducked = true;
            } else {
                // Crouching in the air pulls the feet up (the crouch-jump).
                m.ducked = true;
                m.origin.y += delta;
                if !ctx.hull_fits(m.origin, p.crouch_height) {
                    m.origin.y -= delta;
                }
            }
        }
        m.duck = (m.duck + dt / p.duck_time).min(1.0);
    } else if m.ducked {
        // Standing up needs room; in the air the feet go back down first.
        let feet = if m.on_ground { m.origin } else { m.origin - Vec3::Y * delta };
        if ctx.hull_fits(feet, p.stand_height) {
            m.origin = feet;
            m.ducked = false;
        } else if !m.on_ground && ctx.hull_fits(m.origin, p.stand_height) {
            m.ducked = false;
        }
    }
    if !m.ducked {
        m.duck = (m.duck - dt / p.duck_time).max(0.0);
    }
}

fn friction(p: &MoveParams, m: &mut Mover, dt: f32) {
    let speed = m.horizontal_speed();
    if speed < 0.001 {
        m.velocity.x = 0.0;
        m.velocity.z = 0.0;
        return;
    }
    let control = speed.max(p.stop_speed);
    let drop = control * p.friction * dt;
    let k = (speed - drop).max(0.0) / speed;
    m.velocity.x *= k;
    m.velocity.z *= k;
}

fn accelerate(m: &mut Mover, wish: Vec3, wish_speed: f32, accel: f32, dt: f32) {
    let current = m.velocity.dot(wish);
    let add = wish_speed - current;
    if add <= 0.0 {
        return;
    }
    m.velocity += wish * (accel * dt * wish_speed).min(add);
}

fn air_accelerate(m: &mut Mover, wish: Vec3, wish_speed: f32, p: &MoveParams, dt: f32) {
    let capped = wish_speed.min(p.air_wish_cap);
    let current = m.velocity.dot(wish);
    let add = capped - current;
    if add <= 0.0 {
        return;
    }
    m.velocity += wish * (p.air_accelerate * wish_speed * dt).min(add);
}

/// Moves along the velocity, sliding along up to four planes; returns the planes' count touched.
fn slide(ctx: &Ctx, m: &mut Mover, dt: f32) -> usize {
    let primal = m.velocity;
    let mut original = m.velocity;
    let mut planes: Vec<Vec3> = Vec::with_capacity(5);
    let mut left = dt;
    let mut touched = 0;
    for _ in 0..4 {
        if m.velocity.length_squared() < 1e-10 {
            break;
        }
        let end = m.origin + m.velocity * left;
        let t = ctx.trace(m, m.origin, end);
        if t.all_solid {
            m.velocity = Vec3::ZERO;
            return touched;
        }
        if t.fraction > 0.0 {
            m.origin = t.end;
            original = m.velocity;
            planes.clear();
        }
        if t.fraction >= 1.0 {
            break;
        }
        touched += 1;
        left -= left * t.fraction;
        if planes.len() >= 5 {
            m.velocity = Vec3::ZERO;
            break;
        }
        planes.push(t.normal);
        // Find a velocity along every plane touched this move.
        let mut found = false;
        for i in 0..planes.len() {
            let v = clip_velocity(original, planes[i], 1.0);
            if planes.iter().enumerate().all(|(j, n)| j == i || v.dot(*n) >= 0.0) {
                m.velocity = v;
                found = true;
                break;
            }
        }
        if !found {
            if planes.len() != 2 {
                m.velocity = Vec3::ZERO;
                break;
            }
            // In a crease: go along it.
            let dir = planes[0].cross(planes[1]).normalize_or_zero();
            m.velocity = dir * dir.dot(m.velocity);
        }
        // Never bounce back against the original direction (avoids jitter in corners).
        if m.velocity.dot(primal) <= 0.0 {
            m.velocity = Vec3::ZERO;
            break;
        }
    }
    touched
}

/// A ground move: slide, and if a step would take the player further, step up and back down instead.
fn walk_move(ctx: &Ctx, m: &mut Mover, dt: f32) {
    let start = *m;
    let dest = m.origin + m.velocity * dt;
    // Straight to the destination if nothing is in the way.
    let t = ctx.trace(m, m.origin, dest);
    if t.fraction >= 1.0 && !t.start_solid {
        m.origin = t.end;
        stay_on_ground(ctx, m);
        return;
    }
    // Plain slide.
    slide(ctx, m, dt);
    let (slid_origin, slid_velocity) = (m.origin, m.velocity);
    // Step: up, slide, down.
    *m = start;
    let up = ctx.trace(m, m.origin, m.origin + Vec3::Y * ctx.p.step);
    if !up.all_solid {
        m.origin = up.end;
    }
    slide(ctx, m, dt);
    let down = ctx.trace(m, m.origin, m.origin - Vec3::Y * ctx.p.step);
    let stepped_ok = !down.all_solid && (down.fraction >= 1.0 || down.normal.y >= 0.7);
    if stepped_ok {
        m.origin = down.end;
    }
    let flat = |a: Vec3| Vec2::new(a.x - start.origin.x, a.z - start.origin.z).length_squared();
    if !stepped_ok || flat(slid_origin) >= flat(m.origin) {
        m.origin = slid_origin;
        m.velocity = slid_velocity;
    } else {
        // Keep the slide's vertical velocity (none on the ground) with the step's horizontal one.
        m.velocity.y = slid_velocity.y;
    }
    stay_on_ground(ctx, m);
}

/// Keeps walking down small steps and slopes instead of launching off them.
fn stay_on_ground(ctx: &Ctx, m: &mut Mover) {
    let up = ctx.trace(m, m.origin, m.origin + Vec3::Y * 0.05);
    let from = up.end;
    let down = ctx.trace(m, from, from - Vec3::Y * (ctx.p.step + 0.05));
    if down.fraction > 0.0 && down.fraction < 1.0 && !down.start_solid && down.normal.y >= 0.7 {
        let dy = (from.y - down.end.y).abs();
        if dy > 0.001 {
            m.origin = down.end;
        }
    }
}

/// Decides whether the player stands on something walkable.
fn categorize(ctx: &Ctx, m: &mut Mover) {
    // Moving up fast (a jump, a launch): airborne.
    if m.velocity.y > 180.0 * U {
        m.on_ground = false;
        m.ground_surface = None;
        return;
    }
    let t = ctx.trace(m, m.origin, m.origin - Vec3::Y * (2.0 * U));
    if t.fraction < 1.0 && t.normal.y >= 0.7 {
        m.on_ground = true;
        m.ground_surface = t.surface;
        // Settle onto the ground.
        if t.fraction > 0.0 {
            m.origin = t.end;
        }
        if m.velocity.y < 0.0 {
            m.velocity.y = 0.0;
        }
    } else {
        m.on_ground = false;
        m.ground_surface = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, map};

    const TICK: f32 = 1.0 / 66.0;

    /// A floor, and for the tests: crates of several heights, stairs, slopes, a low ceiling and a wall.
    fn level() -> World {
        let block = |lo: [f64; 3], hi: [f64; 3]| crate::compile::tests_support::block(lo, hi, "dev/test");
        let mut brushes = vec![
            block([-2048.0, -2048.0, -32.0], [2048.0, 2048.0, 0.0]),
            // Crates at x = 200 (y spread): 48, 57+ (64) and 72 units tall.
            block([200.0, -400.0, 0.0], [264.0, -336.0, 48.0]),
            block([200.0, -200.0, 0.0], [264.0, -136.0, 64.0]),
            block([200.0, 0.0, 0.0], [264.0, 64.0, 72.0]),
            // A wall at x = 600 across everything.
            block([600.0, -2048.0, 0.0], [632.0, 2048.0, 256.0]),
            // A low tunnel: ceiling at 60 units between x = -600..-400.
            block([-600.0, 300.0, 60.0], [-400.0, 500.0, 128.0]),
        ];
        // Stairs: 16-unit steps, 16 deep, 8 of them, rising towards +x at y = 800..900.
        for i in 0..8 {
            let x0 = -200.0 + i as f64 * 16.0;
            brushes.push(block([x0, 800.0, 0.0], [x0 + 16.0 * (8 - i) as f64, 900.0, 16.0 * (i + 1) as f64]));
        }
        let src = format!("{{\n\"classname\" \"worldspawn\"\n{}}}\n", brushes.concat());
        compile::compile(&map::parse(&src).unwrap()).world
    }

    fn at(x: f64, y: f64, z: f64) -> Vec3 {
        crate::units::to_sim(glam::DVec3::new(x, y, z))
    }

    /// A yaw that faces map direction +x.
    fn face_map_x() -> f32 {
        crate::units::yaw_from_map_angle(0.0)
    }

    fn run(w: &World, m: &mut Mover, cmd: MoveCmd, seconds: f32) -> Vec<MoveEvents> {
        let p = MoveParams::default();
        (0..(seconds / TICK).round() as usize).map(|_| step(w, &(), &p, m, &cmd, TICK)).collect()
    }

    fn standing(at: Vec3) -> Mover {
        Mover { origin: at, on_ground: true, ..Default::default() }
    }

    fn fwd(yaw: f32) -> MoveCmd {
        MoveCmd { forward: 1.0, yaw, max_speed: 250.0 * U, ..Default::default() }
    }

    #[test]
    fn standing_still_stays_put() {
        let w = level();
        let mut m = standing(at(0.0, 0.0, 0.0));
        run(&w, &mut m, MoveCmd { max_speed: 250.0 * U, ..Default::default() }, 2.0);
        assert!(m.on_ground);
        assert!(m.origin.distance(at(0.0, 0.0, 0.0)) < 1e-3, "{}", m.origin);
    }

    #[test]
    fn running_reaches_top_speed_quickly() {
        let w = level();
        let mut m = standing(at(-1500.0, -1500.0, 0.0));
        run(&w, &mut m, fwd(face_map_x()), 0.5);
        let v = m.horizontal_speed() / U;
        assert!((v - 250.0).abs() < 3.0, "{v} u/s after 0.5 s");
        // Walking and crouching are slower.
        let mut m = standing(at(-1500.0, -1500.0, 0.0));
        let mut cmd = fwd(face_map_x());
        cmd.buttons = Buttons::WALK;
        run(&w, &mut m, cmd, 1.0);
        assert!((m.horizontal_speed() / U - 130.0).abs() < 3.0);
        let mut m = standing(at(-1500.0, -1500.0, 0.0));
        cmd.buttons = Buttons::DUCK;
        run(&w, &mut m, cmd, 1.0);
        assert!((m.horizontal_speed() / U - 85.0).abs() < 3.0);
    }

    #[test]
    fn stopping_takes_a_moment() {
        let w = level();
        let mut m = standing(at(-1500.0, -1500.0, 0.0));
        run(&w, &mut m, fwd(face_map_x()), 1.0);
        let stop = MoveCmd { yaw: face_map_x(), max_speed: 250.0 * U, ..Default::default() };
        run(&w, &mut m, stop, 0.1);
        assert!(m.horizontal_speed() > 0.5, "still sliding a little");
        run(&w, &mut m, stop, 0.5);
        assert!(m.horizontal_speed() < 0.01);
    }

    #[test]
    fn a_jump_is_57_units_high() {
        let w = level();
        let mut m = standing(at(0.0, -1000.0, 0.0));
        let mut cmd = MoveCmd { max_speed: 250.0 * U, ..Default::default() };
        cmd.buttons = Buttons::JUMP;
        let mut top: f32 = 0.0;
        let p = MoveParams::default();
        let mut landed = None;
        for _ in 0..80 {
            let e = step(&w, &(), &p, &mut m, &cmd, TICK);
            top = top.max(m.origin.y);
            if e.landed.is_some() {
                landed = e.landed;
                break;
            }
        }
        assert!((top / U - 57.0).abs() < 2.5, "apex {} units", top / U);
        assert!(landed.is_some() && m.on_ground);
        // Holding jump doesn't jump again.
        run(&w, &mut m, cmd, 0.5);
        assert!(m.on_ground && m.origin.y.abs() < 1e-3);
    }

    fn jump_onto(height_crate_y: f64, crouch: bool) -> bool {
        let w = level();
        // Run up to the crate from x = 0 and jump near it.
        let mut m = standing(at(100.0, height_crate_y + 32.0, 0.0));
        let p = MoveParams::default();
        let mut cmd = fwd(face_map_x());
        cmd.max_speed = 250.0 * U;
        for i in 0..200 {
            let dist = at(200.0, 0.0, 0.0).x - m.origin.x;
            cmd.buttons = Buttons::empty();
            if dist < 1.2 && i < 60 {
                cmd.buttons = Buttons::JUMP;
            }
            if crouch && !m.on_ground {
                cmd.buttons |= Buttons::DUCK;
            }
            step(&w, &(), &p, &mut m, &cmd, TICK);
            if m.origin.x > at(210.0, 0.0, 0.0).x && m.on_ground {
                return m.origin.y > 1.0 * U;
            }
            if m.origin.x < at(199.0, 0.0, 0.0).x - p.half_width && m.on_ground && i > 60 {
                break;
            }
        }
        false
    }

    #[test]
    fn jumps_reach_48_units_and_crouch_jumps_reach_64() {
        assert!(jump_onto(-400.0, false), "a plain jump reaches a 48-unit crate");
        assert!(!jump_onto(-200.0, false), "a plain jump doesn't reach 64 units");
        assert!(jump_onto(-200.0, true), "a crouch-jump reaches 64 units");
        assert!(!jump_onto(0.0, true), "nothing reaches 72 units");
    }

    #[test]
    fn stairs_are_walked_up() {
        let w = level();
        let p = MoveParams::default();
        let mut m = standing(at(-300.0, 850.0, 0.0));
        let mut top: f32 = 0.0;
        for _ in 0..90 {
            step(&w, &(), &p, &mut m, &fwd(face_map_x()), TICK);
            top = top.max(m.origin.y);
            // Walked up without leaving the ground (the top step is 128 units up; beyond it the stairs drop off).
            if m.origin.x < at(-80.0, 0.0, 0.0).x {
                assert!(m.on_ground, "left the ground at {} units", m.origin.x / U);
            }
        }
        assert!((top / U - 128.0).abs() < 0.5, "top at {} units", top / U);
    }

    #[test]
    fn walls_stop_and_slide() {
        let w = level();
        // Head into the wall at 45°: blocked in x, keeps moving in the other axis.
        let mut m = standing(at(500.0, -1000.0, 0.0));
        let yaw = crate::units::yaw_from_map_angle(45.0);
        run(&w, &mut m, fwd(yaw), 1.5);
        let wall_x = at(600.0, 0.0, 0.0).x - MoveParams::default().half_width;
        assert!((m.origin.x - wall_x).abs() < 0.01, "{} vs {wall_x}", m.origin.x);
        assert!(m.velocity.z.abs() > 1.0, "still sliding along the wall");
        let p = MoveParams::default();
        assert!(!w.solid_at(m.origin + Vec3::Y * p.stand_height / 2.0, m.half_extents(&p), Contents::MASK_PLAYER));
    }

    #[test]
    fn crouching_gets_under_a_low_ceiling_and_you_cannot_stand_there() {
        let w = level();
        let mut m = standing(at(-700.0, 400.0, 0.0));
        let yaw = face_map_x();
        // Standing: blocked at the tunnel's mouth.
        run(&w, &mut m, fwd(yaw), 1.0);
        assert!(m.origin.x < at(-600.0, 0.0, 0.0).x);
        // Crouched: through into the tunnel.
        let mut cmd = fwd(yaw);
        cmd.buttons = Buttons::DUCK;
        run(&w, &mut m, cmd, 1.5);
        assert!(m.origin.x > at(-560.0, 0.0, 0.0).x, "{}", m.origin.x / U);
        // Let go of crouch inside: still crouched.
        run(&w, &mut m, MoveCmd { yaw, max_speed: 250.0 * U, ..Default::default() }, 0.3);
        assert!(m.ducked);
    }

    #[test]
    fn air_strafing_gains_speed() {
        // The classic: in the air, turning with the strafe key adds speed beyond the ground maximum.
        let w = level();
        let p = MoveParams::default();
        let mut m = standing(at(-1500.0, -1500.0, 0.0));
        run(&w, &mut m, fwd(face_map_x()), 1.0);
        let mut yaw = face_map_x();
        let mut cmd = MoveCmd { side: -1.0, yaw, max_speed: 250.0 * U, buttons: Buttons::JUMP, ..Default::default() };
        step(&w, &(), &p, &mut m, &cmd, TICK);
        cmd.buttons = Buttons::empty();
        for _ in 0..40 {
            yaw += 0.03;
            cmd.yaw = yaw;
            step(&w, &(), &p, &mut m, &cmd, TICK);
        }
        assert!(m.horizontal_speed() / U > 260.0, "{}", m.horizontal_speed() / U);
    }

    #[test]
    fn a_long_fall_hurts() {
        let w = level();
        let mut m = Mover { origin: at(0.0, -1500.0, 300.0), ..Default::default() };
        let events = run(&w, &mut m, MoveCmd { max_speed: 250.0 * U, ..Default::default() }, 3.0);
        let e = events.iter().find(|e| e.landed.is_some()).expect("landed");
        // 300 units: √(2·800·300) ≈ 693 u/s at impact → about 25 damage.
        assert!(e.fall_damage > 15.0 && e.fall_damage < 35.0, "{e:?}");
    }
}
