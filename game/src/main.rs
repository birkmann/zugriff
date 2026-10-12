//! ZUGRIFF: the Bevy client. The game itself runs in `zugriff_sim`; this crate draws it, plays its sound, reads input
//! and shows the UI — the same on the web (WebGPU, Trunk) and natively.

mod actors;
mod anim;
mod arsenal;
mod args;
mod audio;
mod buy;
mod clips;
mod effects;
mod finish;
mod gear;
mod hud;
mod input;
mod lamps;
mod level;
mod levelmat;
mod link;
mod menu;
mod net;
mod netmenu;
mod options;
mod perf;
mod preview;
mod props;
mod detail;
mod litter;
mod fauna;
mod wires;
mod backdrop;
mod bench;
mod graphics;
#[cfg(not(target_arch = "wasm32"))]
mod qa;
mod scoreboard;
mod session;
mod sfx;
mod sky;
mod slots;
mod textures;
mod ui_kit;
mod view;
mod viewmodel;
mod warmup;

use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use bevy::window::PresentMode;

fn main() {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    #[cfg(target_arch = "wasm32")]
    let file_path = "assets".to_string();
    #[cfg(not(target_arch = "wasm32"))]
    let file_path = {
        if args::flag("help") {
            println!("ZUGRIFF {}\nUsage: zugriff [--assets PATH] [--play MAP] [--server URL]\nMaps: zg_hafen, zg_altstadt, zg_fliegerhorst\nSee README.md for controls and multiplayer setup.", env!("CARGO_PKG_VERSION"));
            return;
        }
        if args::flag("version") {
            println!("ZUGRIFF {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        zugriff_sim::assets::locate(args::get("assets").map(Into::into))
            .unwrap_or_else(|error| { eprintln!("{error}"); std::process::exit(1) })
            .to_string_lossy().into_owned()
    };

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.56, 0.66, 0.78)))
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "ZUGRIFF".into(),
                        // The app id (X11 class, Wayland app_id): the desktop's launcher (`linux/install.sh`) and dock
                        // group the running game under ZUGRIFF's icon by it.
                        name: Some("zugriff".into()),
                        canvas: Some("#canvas".into()),
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: true,
                        // A shooter wants the lowest input latency; the browser paces frames itself.
                        present_mode: PresentMode::AutoNoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin { file_path: file_path.into(), meta_check: AssetMetaCheck::Never, ..default() })
                .set(bevy::log::LogPlugin { filter: "warn,zugriff=info,zugriff_sim=info,wgpu=error,naga=warn".into(), ..default() }),
        )
        .add_plugins((
            options::OptionsPlugin,
            level::LevelPlugin,
            view::ViewPlugin,
            input::InputPlugin,
            session::SessionPlugin,
            actors::ActorsPlugin,
            viewmodel::ViewmodelPlugin,
            effects::EffectsPlugin,
            audio::AudioPlugin,
            hud::HudPlugin,
            buy::BuyPlugin,
            scoreboard::ScoreboardPlugin,
            menu::MenuPlugin,
            sky::SkyPlugin,
            warmup::WarmupPlugin,
        ))
        .add_plugins((perf::PerfPlugin, graphics::GraphicsPlugin, bench::BenchPlugin))
        .add_plugins((finish::FinishPlugin, preview::PreviewPlugin, arsenal::ArsenalPlugin, slots::SlotsPlugin))
        .add_plugins((net::NetPlugin, netmenu::NetMenuPlugin));
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(qa::QaPlugin);
    app.run();
}
