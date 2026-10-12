//! The navigation graph the bots (and led hostages) walk on. It is built by flooding the level from the spawns with
//! the player's own hull, on a grid of [`CELL`]: from each walkable spot, each of eight neighbours is tried the way
//! movement would take it (step up, across, down), so the graph only holds places a player can actually reach, and
//! its links know how: walking, a jump onto a ledge, a drop (one way), or crouching under something.
//!
//! Each node also knows its callout (`zg_place`), whether it lies in a bomb site, a buy zone or the rescue zone, and
//! how much cover it has (blocked directions at crouch height), for picking spots to hold.
//!
//! Building takes a few seconds for a full map, so `zg-compile --nav` writes it next to the map (`<map>.nav`); the
//! client loads that and only builds at load time if it is missing or stale.

use crate::compile::Level;
use crate::movement::MoveParams;
use crate::world::{Contents, World};
use glam::{Vec2, Vec3};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};

/// Grid spacing (m): 24 units, so two cells fit through the narrowest door.
pub const CELL: f32 = 0.6;
/// Highest drop a link may take (m) — well inside the safe fall.
const MAX_DROP: f32 = 3.6;
/// A standing jump reaches 57 units; ledges up to this (m) get jump links.
const JUMP_UP: f32 = 1.2;
const MAX_NODES: usize = 200_000;

pub mod flags {
    pub const CROUCH: u8 = 1;
    pub const SITE_A: u8 = 2;
    pub const SITE_B: u8 = 4;
    pub const BUY_KSK: u8 = 8;
    pub const BUY_OPFOR: u8 = 16;
    pub const RESCUE: u8 = 32;
    /// Reachable from the spawns and with a way back to them: where bots pick spots and routes end.
    pub const CORE: u8 = 64;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkKind {
    Walk,
    Jump,
    Drop,
    Crouch,
}

#[derive(Clone, Copy, Debug)]
pub struct Link {
    pub to: u32,
    pub kind: LinkKind,
}

#[derive(Clone, Debug)]
pub struct Node {
    /// Feet on the floor.
    pub pos: Vec3,
    pub flags: u8,
    /// Index into `Nav::places` (0 = none).
    pub place: u16,
    /// Blocked directions of eight at crouch height within a metre (0..8): more is better cover.
    pub cover: u8,
    first: u32,
    count: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Nav {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub places: Vec<String>,
    /// The geometry it was built for (see [`geometry_hash`]).
    pub hash: u64,
    columns: HashMap<(i32, i32), Vec<u32>>,
}

/// A hash of the collision geometry, so a nav file built for another version of the map is noticed.
pub fn geometry_hash(world: &World) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u32| {
        h ^= v as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    };
    mix(world.brushes.len() as u32);
    for b in &world.brushes {
        mix(b.contents.0);
        for s in b.sides.iter().filter(|s| !s.bevel) {
            mix((s.plane.d * 64.0).round() as i32 as u32);
            mix((s.plane.n.x * 1000.0).round() as i32 as u32);
            mix((s.plane.n.y * 1000.0).round() as i32 as u32);
            mix((s.plane.n.z * 1000.0).round() as i32 as u32);
        }
    }
    h
}

fn col(p: Vec3) -> (i32, i32) {
    ((p.x / CELL).round() as i32, (p.z / CELL).round() as i32)
}

struct Hull<'a> {
    world: &'a World,
    stand: Vec3,
    crouch: Vec3,
    step: f32,
}

impl Hull<'_> {
    /// Sweeps a hull (feet positions) and returns where the feet end and whether it hit.
    fn sweep(&self, ext: Vec3, from: Vec3, to: Vec3) -> (Vec3, f32, Vec3, bool) {
        let up = Vec3::Y * (ext.y + 0.01);
        let t = self.world.trace(from + up, to + up, ext, Contents::MASK_PLAYER);
        (t.end - up, t.fraction, t.normal, t.start_solid)
    }

    /// Where a hull standing at `feet` finds the floor below (within `depth`), if it's walkable.
    fn floor(&self, ext: Vec3, feet: Vec3, depth: f32) -> Option<Vec3> {
        let (end, frac, n, solid) = self.sweep(ext, feet, feet - Vec3::Y * depth);
        (!solid && frac < 1.0 && n.y >= 0.7).then_some(end)
    }

    /// Tries to move from `p` (on the floor) to the column at `q` (x, z) the way the player would: returns the floor
    /// reached and how. If the cell's centre doesn't fit the hull (a narrow gap, a pocket beside stairs), points up to
    /// a third of a cell off it are tried too.
    fn step_to(&self, p: Vec3, q: Vec2) -> Option<(Vec3, LinkKind)> {
        let d = CELL / 3.0;
        [Vec2::ZERO, Vec2::new(d, 0.0), Vec2::new(-d, 0.0), Vec2::new(0.0, d), Vec2::new(0.0, -d)].into_iter().find_map(|o| self.step_exact(p, q + o))
    }

    fn step_exact(&self, p: Vec3, q: Vec2) -> Option<(Vec3, LinkKind)> {
        for (ext, crouch) in [(self.stand, false), (self.crouch, true)] {
            for (lift, jump) in [(self.step, false), (JUMP_UP, true)] {
                if crouch && jump {
                    continue;
                }
                let (lifted, _, _, solid) = self.sweep(ext, p, p + Vec3::Y * lift);
                if solid {
                    continue;
                }
                if jump && lifted.y < p.y + JUMP_UP - 0.02 {
                    continue;
                }
                let across = Vec3::new(q.x, lifted.y, q.y);
                let (_, frac, _, solid) = self.sweep(ext, lifted, across);
                if solid || frac < 1.0 {
                    continue;
                }
                let Some(ground) = self.floor(ext, across, lift + MAX_DROP) else { continue };
                let dy = ground.y - p.y;
                if jump && dy < self.step {
                    // A jump that only lands as high as a step: the walk link covers it (or it's a gap).
                    continue;
                }
                let kind = if jump {
                    LinkKind::Jump
                } else if dy < -(self.step + 0.1) {
                    LinkKind::Drop
                } else if crouch {
                    LinkKind::Crouch
                } else {
                    LinkKind::Walk
                };
                return Some((ground, kind));
            }
        }
        None
    }
}

impl Nav {
    /// Floods the level from its spawns and hostages.
    pub fn build(level: &Level, params: &MoveParams) -> Nav {
        let world = &level.world;
        let hull = Hull {
            world,
            stand: Vec3::new(params.half_width, params.stand_height * 0.5, params.half_width),
            crouch: Vec3::new(params.half_width, params.crouch_height * 0.5, params.half_width),
            step: params.step,
        };
        let mut nav = Nav { places: vec![String::new()], hash: geometry_hash(world), ..Default::default() };
        let mut queue = VecDeque::new();
        let mut pending: Vec<Vec<Link>> = Vec::new();
        let seeds = level
            .entities
            .iter()
            .filter(|e| e.classname.starts_with("info_player_") || e.classname == "info_hostage")
            .map(|e| e.origin)
            .collect::<Vec<_>>();
        for s in seeds {
            let c = col(s);
            let q = Vec3::new(c.0 as f32 * CELL, s.y + 0.3, c.1 as f32 * CELL);
            let Some(ground) = [q, s + Vec3::Y * 0.3].into_iter().find_map(|q| hull.floor(hull.stand, q, 2.0)) else { continue };
            if nav.find_in_column(ground).is_none() {
                let i = nav.add(ground, false);
                pending.push(Vec::new());
                queue.push_back(i);
            }
        }
        let dirs = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];
        while let Some(i) = queue.pop_front() {
            let p = nav.nodes[i as usize].pos;
            let (cx, cz) = col(p);
            for (dx, dz) in dirs {
                let q = Vec2::new((cx + dx) as f32 * CELL, (cz + dz) as f32 * CELL);
                // (The hull's sweep keeps diagonals from cutting corners.)
                let Some((ground, kind)) = hull.step_to(p, q) else { continue };
                let j = match nav.find_in_column(ground) {
                    Some(j) => j,
                    None => {
                        if nav.nodes.len() >= MAX_NODES {
                            continue;
                        }
                        let j = nav.add(ground, kind == LinkKind::Crouch);
                        pending.push(Vec::new());
                        queue.push_back(j);
                        j
                    }
                };
                if j != i && !pending[i as usize].iter().any(|l| l.to == j) {
                    pending[i as usize].push(Link { to: j, kind });
                }
            }
        }
        // Flatten the links.
        for (i, links) in pending.into_iter().enumerate() {
            nav.nodes[i].first = nav.links.len() as u32;
            nav.nodes[i].count = links.len().min(255) as u8;
            nav.links.extend(links.into_iter().take(255));
        }
        nav.annotate(level);
        nav.mark_core(level);
        nav
    }

    fn add(&mut self, pos: Vec3, crouch: bool) -> u32 {
        let i = self.nodes.len() as u32;
        self.nodes.push(Node { pos, flags: if crouch { flags::CROUCH } else { 0 }, place: 0, cover: 0, first: 0, count: 0 });
        self.columns.entry(col(pos)).or_default().push(i);
        i
    }

    fn find_in_column(&self, p: Vec3) -> Option<u32> {
        self.columns.get(&col(p))?.iter().copied().find(|&i| (self.nodes[i as usize].pos.y - p.y).abs() < 0.7)
    }

    /// Places, sites, zones and cover.
    fn annotate(&mut self, level: &Level) {
        let world = &level.world;
        for n in &mut self.nodes {
            let at = n.pos + Vec3::Y * 0.5;
            for v in level.volumes_of("func_bomb_target") {
                if v.contains(at) {
                    n.flags |= if v.get("site").is_some_and(|s| s.eq_ignore_ascii_case("B")) { flags::SITE_B } else { flags::SITE_A };
                }
            }
            for v in level.volumes_of("func_buyzone") {
                if v.contains(at) {
                    n.flags |= if v.get("team").is_some_and(|t| t.eq_ignore_ascii_case("opfor")) { flags::BUY_OPFOR } else { flags::BUY_KSK };
                }
            }
            if level.volumes_of("func_hostage_rescue").any(|v| v.contains(at)) {
                n.flags |= flags::RESCUE;
            }
            if let Some(name) = level.volumes_of("zg_place").filter(|v| v.contains(at)).last().and_then(|v| v.get("name")) {
                n.place = match self.places.iter().position(|p| p == name) {
                    Some(i) => i as u16,
                    None => {
                        self.places.push(name.to_string());
                        (self.places.len() - 1) as u16
                    }
                };
            }
            let eye = n.pos + Vec3::Y * 1.0;
            let mut blocked = 0;
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.0;
                let d = Vec3::new(a.cos(), 0.0, a.sin());
                if world.trace(eye, eye + d * 1.0, Vec3::ZERO, Contents::MASK_SHOT).hit() {
                    blocked += 1;
                }
            }
            n.cover = blocked;
        }
    }

    /// Marks the nodes reachable from every spawn's node that can also get back to them (drops and ledges that lead
    /// nowhere, rooftops only reachable by accident, are left out).
    fn mark_core(&mut self, level: &Level) {
        let n = self.nodes.len();
        let seeds: Vec<u32> = level.entities.iter().filter(|e| e.classname.starts_with("info_player_")).filter_map(|e| self.nearest(e.origin)).collect();
        if seeds.is_empty() {
            return;
        }
        let mut reverse: Vec<Vec<u32>> = vec![Vec::new(); n];
        for i in 0..n as u32 {
            for l in self.links(i) {
                reverse[l.to as usize].push(i);
            }
        }
        let flood = |start: &[u32], next: &dyn Fn(u32) -> Vec<u32>| -> Vec<bool> {
            let mut seen = vec![false; n];
            let mut queue: VecDeque<u32> = start.iter().copied().collect();
            for &s in start {
                seen[s as usize] = true;
            }
            while let Some(i) = queue.pop_front() {
                for j in next(i) {
                    if !seen[j as usize] {
                        seen[j as usize] = true;
                        queue.push_back(j);
                    }
                }
            }
            seen
        };
        let forward = flood(&seeds, &|i| self.links(i).iter().map(|l| l.to).collect());
        let back = flood(&seeds, &|i| reverse[i as usize].clone());
        for (i, node) in self.nodes.iter_mut().enumerate() {
            if forward[i] && back[i] {
                node.flags |= flags::CORE;
            }
        }
    }

    /// The nearest node in the core (see [`flags::CORE`]); falls back to any node on maps without one.
    pub fn nearest_core(&self, p: Vec3) -> Option<u32> {
        let (cx, cz) = col(p);
        let mut best: Option<(f32, u32)> = None;
        for r in 0i32..=6 {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let Some(list) = self.columns.get(&(cx + dx, cz + dz)) else { continue };
                    for &i in list {
                        let node = &self.nodes[i as usize];
                        if node.flags & flags::CORE == 0 {
                            continue;
                        }
                        let q = node.pos;
                        let dy = p.y - q.y;
                        let penalty = if (-0.5..1.2).contains(&dy) { 0.0 } else { dy.abs() * 4.0 };
                        let d = Vec2::new(q.x - p.x, q.z - p.z).length() + penalty;
                        if best.is_none_or(|b| d < b.0) {
                            best = Some((d, i));
                        }
                    }
                }
            }
            if best.is_some_and(|b| b.0 < (r as f32 + 0.5) * CELL) {
                break;
            }
        }
        best.map(|b| b.1).or_else(|| self.nearest(p))
    }

    pub fn links(&self, i: u32) -> &[Link] {
        let n = &self.nodes[i as usize];
        &self.links[n.first as usize..n.first as usize + n.count as usize]
    }

    pub fn pos(&self, i: u32) -> Vec3 {
        self.nodes[i as usize].pos
    }

    pub fn place(&self, i: u32) -> &str {
        &self.places[self.nodes[i as usize].place as usize]
    }

    /// The node nearest to feet at `p` (same floor preferred), searching a few cells around.
    pub fn nearest(&self, p: Vec3) -> Option<u32> {
        let (cx, cz) = col(p);
        let mut best: Option<(f32, u32)> = None;
        for r in 0i32..=4 {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let Some(list) = self.columns.get(&(cx + dx, cz + dz)) else { continue };
                    for &i in list {
                        let q = self.nodes[i as usize].pos;
                        let dy = p.y - q.y;
                        // Below the feet (on this floor) is fine; far above or below is another floor.
                        let penalty = if (-0.5..1.2).contains(&dy) { 0.0 } else { dy.abs() * 4.0 };
                        let d = Vec2::new(q.x - p.x, q.z - p.z).length() + penalty;
                        if best.is_none_or(|b| d < b.0) {
                            best = Some((d, i));
                        }
                    }
                }
            }
            if best.is_some_and(|b| b.0 < (r as f32 + 0.5) * CELL) {
                break;
            }
        }
        best.map(|b| b.1)
    }

    /// The cheapest route from node `a` to node `b` (both included), if any. `cost` may add a penalty per node (e.g.
    /// to avoid places the enemy watches).
    pub fn route(&self, a: u32, b: u32, extra: &dyn Fn(u32) -> f32) -> Option<Vec<u32>> {
        #[derive(PartialEq)]
        struct Open(f32, u32);
        impl Eq for Open {}
        impl PartialOrd for Open {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for Open {
            fn cmp(&self, o: &Self) -> Ordering {
                o.0.total_cmp(&self.0)
            }
        }
        if a as usize >= self.nodes.len() || b as usize >= self.nodes.len() {
            return None;
        }
        let goal = self.pos(b);
        let mut g: HashMap<u32, f32> = HashMap::new();
        let mut from: HashMap<u32, u32> = HashMap::new();
        let mut open = BinaryHeap::new();
        g.insert(a, 0.0);
        open.push(Open(self.pos(a).distance(goal), a));
        let mut expanded = 0;
        while let Some(Open(_, i)) = open.pop() {
            if i == b {
                let mut path = vec![b];
                let mut at = b;
                while let Some(&p) = from.get(&at) {
                    path.push(p);
                    at = p;
                }
                path.reverse();
                return Some(path);
            }
            expanded += 1;
            if expanded > 60_000 {
                return None;
            }
            let gi = g[&i];
            let pi = self.pos(i);
            for l in self.links(i) {
                let pj = self.pos(l.to);
                // Jumps cost a detour's worth (they're easy to miss, and a way round is usually there).
                let (k, fixed) = match l.kind {
                    LinkKind::Walk => (1.0, 0.0),
                    LinkKind::Drop => (1.2, 0.5),
                    LinkKind::Jump => (1.5, 6.0),
                    LinkKind::Crouch => (2.5, 0.0),
                };
                let cost = gi + pi.distance(pj) * k + fixed + extra(l.to);
                if g.get(&l.to).is_none_or(|&old| cost < old) {
                    g.insert(l.to, cost);
                    from.insert(l.to, i);
                    open.push(Open(cost + pj.distance(goal), l.to));
                }
            }
        }
        None
    }

    /// The link kind from `a` to `b`, if linked.
    pub fn link(&self, a: u32, b: u32) -> Option<LinkKind> {
        self.links(a).iter().find(|l| l.to == b).map(|l| l.kind)
    }

    /// Nodes with all of `mask`'s flags.
    pub fn with_flags(&self, mask: u8) -> impl Iterator<Item = u32> + '_ {
        self.nodes.iter().enumerate().filter(move |(_, n)| n.flags & mask == mask).map(|(i, _)| i as u32)
    }

    // ---- files ----------------------------------------------------------------------------------------------------

    /// A compact little-endian file: header, places, nodes, links.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"ZNAV");
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&self.hash.to_le_bytes());
        out.extend_from_slice(&(self.places.len() as u32).to_le_bytes());
        for p in &self.places {
            let b = p.as_bytes();
            out.extend_from_slice(&(b.len() as u16).to_le_bytes());
            out.extend_from_slice(b);
        }
        out.extend_from_slice(&(self.nodes.len() as u32).to_le_bytes());
        for n in &self.nodes {
            for v in [n.pos.x, n.pos.y, n.pos.z] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.push(n.flags);
            out.extend_from_slice(&n.place.to_le_bytes());
            out.push(n.cover);
            out.extend_from_slice(&n.first.to_le_bytes());
            out.push(n.count);
        }
        out.extend_from_slice(&(self.links.len() as u32).to_le_bytes());
        for l in &self.links {
            out.extend_from_slice(&l.to.to_le_bytes());
            out.push(l.kind as u8);
        }
        out
    }

    pub fn from_bytes(b: &[u8]) -> Option<Nav> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Option<&[u8]> {
            let s = b.get(at..at + n)?;
            at += n;
            Some(s)
        };
        if take(4)? != b"ZNAV" {
            return None;
        }
        let u32_ = |s: &[u8]| u32::from_le_bytes(s.try_into().unwrap());
        if u32_(take(4)?) != 1 {
            return None;
        }
        let hash = u64::from_le_bytes(take(8)?.try_into().ok()?);
        let np = u32_(take(4)?) as usize;
        let mut places = Vec::with_capacity(np);
        for _ in 0..np {
            let len = u16::from_le_bytes(take(2)?.try_into().ok()?) as usize;
            places.push(String::from_utf8(take(len)?.to_vec()).ok()?);
        }
        let nn = u32_(take(4)?) as usize;
        let mut nav = Nav { places, hash, ..Default::default() };
        for i in 0..nn {
            let f = |s: &[u8]| f32::from_le_bytes(s.try_into().unwrap());
            let pos = Vec3::new(f(take(4)?), f(take(4)?), f(take(4)?));
            let flags = take(1)?[0];
            let place = u16::from_le_bytes(take(2)?.try_into().ok()?);
            let cover = take(1)?[0];
            let first = u32_(take(4)?);
            let count = take(1)?[0];
            nav.nodes.push(Node { pos, flags, place, cover, first, count });
            nav.columns.entry(col(pos)).or_default().push(i as u32);
        }
        let nl = u32_(take(4)?) as usize;
        for _ in 0..nl {
            let to = u32_(take(4)?);
            let kind = match take(1)?[0] {
                0 => LinkKind::Walk,
                1 => LinkKind::Jump,
                2 => LinkKind::Drop,
                _ => LinkKind::Crouch,
            };
            nav.links.push(Link { to, kind });
        }
        Some(nav)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, map};

    fn level() -> Level {
        let block = |lo: [f64; 3], hi: [f64; 3]| crate::compile::tests_support::block(lo, hi, "dev/floor");
        let mut b = vec![block([-512.0, -512.0, -16.0], [512.0, 512.0, 0.0])];
        // A wall across the middle with a door (y −32..32).
        b.push(block([-16.0, -512.0, 0.0], [16.0, -32.0, 128.0]));
        b.push(block([-16.0, 32.0, 0.0], [16.0, 512.0, 128.0]));
        // A 40-unit ledge (jumpable) at x 200..300, and stairs up to a 96-high platform at x −400..−300.
        b.push(block([200.0, 200.0, 0.0], [300.0, 300.0, 40.0]));
        b.push(block([-460.0, -400.0, 0.0], [-300.0, -300.0, 96.0]));
        for i in 0..6 {
            let x = -300.0 + i as f64 * 16.0;
            b.push(block([x, -400.0, 0.0], [x + 16.0, -300.0, 96.0 - 16.0 * i as f64]));
        }
        let mut src = format!("{{\n\"classname\" \"worldspawn\"\n{}}}\n", b.concat());
        src += "{\n\"classname\" \"info_player_ksk\"\n\"origin\" \"-300 0 4\"\n}\n";
        compile::compile(&map::parse(&src).unwrap())
    }

    #[test]
    fn floods_through_the_door_and_up_the_stairs() {
        let l = level();
        let nav = Nav::build(&l, &MoveParams::default());
        assert!(nav.nodes.len() > 500, "{}", nav.nodes.len());
        let m = crate::units::METRES_PER_UNIT as f32;
        let at = |x: f32, y: f32, z: f32| crate::units::to_sim(glam::DVec3::new(x as f64, y as f64, z as f64));
        let a = nav.nearest(at(-300.0, 0.0, 0.0)).unwrap();
        let b = nav.nearest(at(300.0, 0.0, 0.0)).unwrap();
        let route = nav.route(a, b, &|_| 0.0).expect("through the door");
        // It goes through the doorway (map y −32..32 → sim z −0.8..0.8) when crossing x = 0.
        for w in route.windows(2) {
            let (p, q) = (nav.pos(w[0]), nav.pos(w[1]));
            if p.x.signum() != q.x.signum() {
                assert!(p.z.abs() < 0.8, "crossed the wall at z = {}", p.z);
            }
        }
        // Up on the platform, by the stairs.
        let top = nav.nearest(at(-400.0, -350.0, 96.0)).unwrap();
        assert!((nav.pos(top).y - 96.0 * m).abs() < 0.05, "{}", nav.pos(top).y);
        assert!(nav.route(a, top, &|_| 0.0).is_some(), "stairs");
        // Onto the ledge: a jump.
        let ledge = nav.nearest(at(250.0, 250.0, 40.0)).unwrap();
        assert!((nav.pos(ledge).y - 40.0 * m).abs() < 0.05);
        let r = nav.route(b, ledge, &|_| 0.0).expect("jump up");
        assert!(r.windows(2).any(|w| nav.link(w[0], w[1]) == Some(LinkKind::Jump)));
        // And off it: a drop or a step.
        assert!(nav.route(ledge, b, &|_| 0.0).is_some());
        // The file round-trips.
        let back = Nav::from_bytes(&nav.to_bytes()).unwrap();
        assert_eq!(back.nodes.len(), nav.nodes.len());
        assert_eq!(back.links.len(), nav.links.len());
        assert_eq!(back.hash, geometry_hash(&l.world));
        assert_eq!(back.nearest(at(300.0, 0.0, 0.0)), Some(b));
    }
}
