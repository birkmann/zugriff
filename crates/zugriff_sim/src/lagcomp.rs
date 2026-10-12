//! Lag compensation for online play. A client draws everyone else a little in the past (the snapshot's trip plus its
//! interpolation delay); a shot it fires at what it sees has to be judged against that picture. The server keeps where
//! every player stood over the last second and, for the moment of one player's weapon tick, puts everyone else back
//! where the shooter saw them (the command's `view_tick`). Damage lands on the real players; only the positions move.
//!
//! The shot's randomness is tied to the command, too: spread and recoil draw from a generator seeded by the command's
//! number, so the shooter's own prediction of its tracer and view kick matches the server's.

use crate::game::{Game, PlayerId, UserCmd};
use crate::rng::Rng;
use glam::Vec3;
use std::collections::VecDeque;

/// How far back shots may reach (ticks; one second at 66 Hz). Older views are clamped to this.
pub const MAX_REWIND: u32 = 66;

/// Where each player stood at a tick: feet, crouch, yaw, alive.
#[derive(Clone, Debug, Default)]
pub struct History {
    frames: VecDeque<(u32, Vec<(Vec3, f32, f32, bool)>)>,
}

impl History {
    fn at(&self, tick: u32) -> Option<&Vec<(Vec3, f32, f32, bool)>> {
        let first = self.frames.front()?.0;
        let i = tick.checked_sub(first)? as usize;
        self.frames.get(i).filter(|(t, _)| *t == tick).map(|(_, f)| f)
    }
}

/// Turns lag compensation on for a game (the server's).
pub fn enable(g: &mut Game) {
    g.lag = Some(History::default());
}

/// Called at the end of each tick: keeps this tick's positions.
pub fn record(g: &mut Game) {
    let tick = g.ticks;
    let frame = g.players.iter().map(|p| (p.mover.origin, p.mover.duck, p.yaw, p.alive)).collect();
    let Some(h) = g.lag.as_mut() else { return };
    h.frames.push_back((tick, frame));
    while h.frames.len() > MAX_REWIND as usize + 2 {
        h.frames.pop_front();
    }
}

/// What `begin` changed, for `end` to put back.
pub struct Rewound(Vec<(PlayerId, Vec3, f32, f32)>);

/// Before a player's weapon tick: the command's own random generator, and everyone else moved to where the shooter
/// saw them. Returns the game's generator to restore.
pub fn begin(g: &mut Game, id: PlayerId, cmd: &UserCmd) -> (Option<Rng>, Rewound) {
    let rng = (cmd.seq != 0).then(|| std::mem::replace(&mut g.rng, Rng::new(seed(id, cmd.seq))));
    let mut moved = Vec::new();
    if cmd.view_tick != 0 && cmd.view_tick < g.ticks {
        let view = cmd.view_tick.max(g.ticks.saturating_sub(MAX_REWIND));
        if let Some(frame) = g.lag.as_ref().and_then(|h| h.at(view)).cloned() {
            for (i, &(origin, duck, yaw, alive)) in frame.iter().enumerate() {
                let Some(p) = g.players.get_mut(i) else { break };
                // Only those alive both then and now can be hit where they were.
                if i == id || !alive || !p.alive {
                    continue;
                }
                moved.push((i, p.mover.origin, p.mover.duck, p.yaw));
                p.mover.origin = origin;
                p.mover.duck = duck;
                p.yaw = yaw;
            }
        }
    }
    (rng, Rewound(moved))
}

/// After the weapon tick: everyone back to now, the game's generator back.
pub fn end(g: &mut Game, rng: Option<Rng>, rewound: Rewound) {
    for (i, origin, duck, yaw) in rewound.0 {
        let p = &mut g.players[i];
        p.mover.origin = origin;
        p.mover.duck = duck;
        p.yaw = yaw;
    }
    if let Some(r) = rng {
        g.rng = r;
    }
}

/// The seed for a player's command.
pub fn seed(id: PlayerId, seq: u32) -> u64 {
    ((id as u64) << 32 | seq as u64).wrapping_mul(0x2545_f491_4f6c_dd1d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tests::{game, run};
    use crate::game::{Btn, Mode};
    use crate::weapons::{Gun, WeaponId};

    fn aim_at(g: &Game, a: PlayerId, target: Vec3) -> (f32, f32) {
        let d = crate::hitbox::Hitboxes::of(target, 0.0, 0.0).chest() - g.eye(a);
        ((-d.x).atan2(-d.z), (d.y / d.length()).asin())
    }

    #[test]
    fn a_shot_hits_where_the_shooter_saw_the_target() {
        let mut g = game(Mode::Defuse);
        enable(&mut g);
        run(&mut g, 7.0, &[]);
        let (a, b) = (0, 1);
        g.players[a].inv.primary = Some(Gun::new(WeaponId::G95));
        g.players[a].inv.select(WeaponId::G95);
        g.players[a].mover.origin = Vec3::new(0.0, 0.0, 12.0);
        let seen_at = Vec3::new(0.0, 0.0, 2.0);
        g.players[b].mover.origin = seen_at;
        g.players[b].armor = 0;
        let (yaw, pitch) = aim_at(&g, a, seen_at);
        let hold = UserCmd { yaw, pitch, ..Default::default() };
        run(&mut g, 1.5, &[(a, hold.clone())]);
        // What the shooter sees: the target at `seen_at`, this tick. On the server it has since stepped 2 m aside.
        let seen = g.ticks;
        for _ in 0..10 {
            g.players[b].mover.origin.x += 0.2;
            g.tick(&[(a, hold.clone())]);
        }
        let now_at = g.players[b].mover.origin;
        // Without lag compensation the shot goes where the target was: a miss.
        g.tick(&[(a, UserCmd { buttons: Btn::ATTACK, seq: 1, ..hold.clone() })]);
        assert_eq!(g.players[b].health, 100, "judged against now, the shot misses");
        run(&mut g, 0.4, &[(a, hold.clone())]);
        // With it, judged against the picture the shooter had: a hit, and the target stays where it really is.
        g.tick(&[(a, UserCmd { buttons: Btn::ATTACK, seq: 2, view_tick: seen, ..hold.clone() })]);
        assert!(g.players[b].health < 100, "judged against what the shooter saw, the shot hits");
        assert!(g.players[b].mover.origin.distance(now_at) < 0.05, "the target is put back");
    }

    #[test]
    fn the_same_command_draws_the_same_spread() {
        let shoot = |seed_game: u64| {
            let mut g = game(Mode::Defuse);
            g.rng = Rng::new(seed_game);
            run(&mut g, 7.0, &[]);
            let cmd = UserCmd { buttons: Btn::ATTACK, pitch: 0.2, seq: 77, ..Default::default() };
            g.events.clear();
            g.tick(&[(0, cmd)]);
            g.events.iter().find_map(|e| match e {
                crate::game::Event::Shot { dir, .. } => Some(*dir),
                _ => None,
            })
        };
        // Whatever state the game's own generator is in (client and server differ), the shot is the same.
        let (x, y) = (shoot(1), shoot(99));
        assert!(x.is_some());
        assert_eq!(x, y);
    }
}
