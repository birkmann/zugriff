//! `zg-compile <map.map> [--luxel <m>] [--bake-json <out.json>] [--nav <out.nav>]`
//!
//! Compiles a map and reports what came out (faces, brushes, entities, warnings). With `--bake-json` it also charts the
//! lightmap and writes the input for `tools/lightmap/bake.py`: faces with their lightmap coordinates, the
//! sun and the sky. With `--nav` it builds the bots' navigation graph and writes it (the client loads `<map>.nav`).

use glam::{EulerRot, Quat, Vec3};
use serde_json::json;
use zugriff_sim::lightmap::{ATLAS_WIDTH, LUXEL};
use zugriff_sim::{compile, lightmap, map, units};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
    let Some(path) = args.get(1).filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: zg-compile <map.map> [--luxel <m>] [--bake-json <out.json>] [--nav <out.nav>]");
        std::process::exit(2);
    };
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    let parsed = map::parse(&src).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    let mut level = compile::compile(&parsed);
    for w in &level.warnings {
        eprintln!("warning: {w}");
    }
    let luxel = arg("--luxel").and_then(|v| v.parse().ok()).unwrap_or(LUXEL);
    lightmap::chart(&mut level, luxel, ATLAS_WIDTH);
    let atlas = level.lightmap.expect("charted");
    println!(
        "{path}: {} faces, {} brushes, {} entities, {} volumes; lightmap {}×{} at {luxel} m/luxel",
        level.faces.len(),
        level.world.brushes.len(),
        level.entities.len(),
        level.volumes.len(),
        atlas.width,
        atlas.height
    );

    if let Some(out) = arg("--bake-json") {
        let env = level.entities_of("light_environment").next();
        let num = |key: &str, default: f32| env.and_then(|e| e.get(key)).and_then(|v| v.parse().ok()).unwrap_or(default);
        let rgb = |key: &str, default: [f32; 3]| {
            env.and_then(|e| e.get(key))
                .map(|v| v.split_whitespace().filter_map(|c| c.parse::<f32>().ok()).map(|c| c / 255.0).collect::<Vec<_>>())
                .filter(|c| c.len() >= 3)
                .map_or(default, |c| [c[0], c[1], c[2]])
        };
        // The direction the sunlight travels, as the client's DirectionalLight points.
        let rot = Quat::from_euler(EulerRot::YXZ, units::yaw_from_map_angle(num("yaw", 35.0) as f64), num("pitch", -50.0).to_radians(), 0.0);
        let sun_dir = rot * Vec3::NEG_Z;
        let faces: Vec<_> = level
            .faces
            .iter()
            .filter(|f| !f.lm.is_empty())
            .map(|f| {
                json!({
                    "surface": level.world.surface_name(f.surface),
                    "points": f.points.iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>(),
                    "lm": f.lm.iter().map(|t| [t.x, t.y]).collect::<Vec<_>>(),
                })
            })
            .collect();
        // Placed models: the bake loads each GLB at its place (sim metres, Y up, yaw about Y) and bakes it into its
        // square; it also shadows the faces around it. Its yaw here turns it about sim +Y.
        let props: Vec<_> = level
            .prop_charts
            .iter()
            .map(|c| {
                let e = &level.entities[c.entity];
                json!({
                    "model": e.get("model").unwrap_or(""),
                    "origin": [e.origin.x, e.origin.y, e.origin.z],
                    // A model's front is its +Z; turned half round from a view's yaw, it faces the map angle.
                    "yaw": e.yaw + std::f32::consts::PI,
                    "scale": e.get("scale").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0),
                    "rect": c.rect,
                })
            })
            .collect();
        // Lamps (`light_zg`): map-space directions → sim (x, z, −y).
        let vec3 = |v: Option<&str>| -> [f32; 3] {
            let c: Vec<f32> = v.unwrap_or("0 0 -1").split_whitespace().filter_map(|c| c.parse().ok()).collect();
            if c.len() >= 3 { [c[0], c[2], -c[1]] } else { [0.0, -1.0, 0.0] }
        };
        let lights: Vec<_> = level
            .entities_of("light_zg")
            .map(|e| {
                let rgb: Vec<f32> = e.get("color").unwrap_or("255 255 255").split_whitespace().filter_map(|c| c.parse::<f32>().ok()).map(|c| c / 255.0).collect();
                json!({
                    "kind": e.get("kind").unwrap_or("point"),
                    "origin": [e.origin.x, e.origin.y, e.origin.z],
                    "dir": vec3(e.get("dir")),
                    "along": vec3(e.get("along")),
                    "power": e.get("power").and_then(|v| v.parse::<f32>().ok()).unwrap_or(100.0),
                    "color": rgb,
                    "cone": e.get("cone").and_then(|v| v.parse::<f32>().ok()).unwrap_or(90.0),
                    "size": e.get("size").map(|v| v.split_whitespace().filter_map(|c| c.parse::<f32>().ok()).collect::<Vec<_>>()).unwrap_or_default(),
                })
            })
            .collect();
        let doc = json!({
            "map": path,
            "atlas": { "width": atlas.width, "height": atlas.height, "luxel": atlas.luxel, "hash": format!("{:016x}", atlas.hash) },
            "sun": { "direction": [sun_dir.x, sun_dir.y, sun_dir.z], "color": rgb("color", [1.0, 0.95, 0.88]) },
            "sky": { "color": rgb("ambient", [0.47, 0.53, 0.63]) },
            "faces": faces,
            "props": props,
            "lights": lights,
        });
        std::fs::write(&out, serde_json::to_string(&doc).unwrap()).unwrap_or_else(|e| fail(&format!("{out}: {e}")));
        println!("wrote {out}");
    }

    if let Some(out) = arg("--nav") {
        let started = std::time::Instant::now();
        let nav = zugriff_sim::nav::Nav::build(&level, &zugriff_sim::movement::MoveParams::default());
        let bytes = nav.to_bytes();
        std::fs::write(&out, &bytes).unwrap_or_else(|e| fail(&format!("{out}: {e}")));
        println!(
            "wrote {out}: {} nodes, {} links, {} places, {} KB in {:.1} s",
            nav.nodes.len(),
            nav.links.len(),
            nav.places.len() - 1,
            bytes.len() / 1024,
            started.elapsed().as_secs_f32()
        );
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}
