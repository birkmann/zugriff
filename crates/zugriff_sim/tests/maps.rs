//! Every map in `assets/maps` compiles without warnings and has what its mode needs.

use zugriff_sim::{compile, map};

#[test]
fn every_map_compiles_cleanly() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/maps");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "map") {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&path).unwrap();
        let level = compile::compile(&map::parse(&src).unwrap_or_else(|e| panic!("{name}: {e}")));
        assert!(level.warnings.is_empty(), "{name}: {:?}", level.warnings);
        let count = |c: &str| level.entities_of(c).count();
        assert!(count("info_player_ksk") >= 5 && count("info_player_opfor") >= 5, "{name}: needs 5 spawns per side");
        if name.starts_with("zg_") {
            assert!(level.volumes_of("func_buyzone").count() >= 2, "{name}: buy zones");
            let defuse = level.volumes_of("func_bomb_target").count();
            let rescue = level.volumes_of("func_hostage_rescue").count();
            assert!(defuse >= 2 || (rescue >= 1 && count("info_hostage") >= 1), "{name}: needs bomb sites or hostages");
        }
        checked += 1;
    }
    assert!(checked > 0);
}
