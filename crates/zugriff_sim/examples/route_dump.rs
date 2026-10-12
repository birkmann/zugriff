//! Prints a route's nodes and links: `--example route_dump -- <map> x,y,z x,y,z` (sim metres).
use std::sync::Arc;
use zugriff_sim::nav::Nav;
use zugriff_sim::{compile, map};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let level = Arc::new(compile::compile(&map::parse(&std::fs::read_to_string(&a[1]).unwrap()).unwrap()));
    let nav = Nav::build(&level, &zugriff_sim::movement::MoveParams::default());
    let p = |s: &str| { let v: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect(); glam::Vec3::new(v[0], v[1], v[2]) };
    let (from, to) = (nav.nearest(p(&a[2])).unwrap(), nav.nearest(p(&a[3])).unwrap());
    let r = nav.route(from, to, &|_| 0.0).unwrap();
    let lo: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    for k in lo..r.len().min(lo + 30) {
        let kind = if k > 0 { format!("{:?}", nav.link(r[k - 1], r[k]).unwrap()) } else { "-".into() };
        println!("{k}: {:?} {kind} flags={} cover={}", (nav.pos(r[k]) * 100.0).round() / 100.0, nav.nodes[r[k] as usize].flags, nav.nodes[r[k] as usize].cover);
    }
}
