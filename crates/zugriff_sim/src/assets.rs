//! Asset lookup shared by the desktop client and dedicated server.
use std::path::{Path, PathBuf};

/// Explicit overrides take precedence; installed bundles never need the source checkout.
pub fn locate(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    let override_path = explicit.or_else(|| std::env::var_os("ZUGRIFF_ASSETS").map(PathBuf::from));
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    resolve(override_path, &executable, Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")))
}

fn resolve(explicit: Option<PathBuf>, executable: &Path, source: &Path) -> Result<PathBuf, String> {
    let candidates = if let Some(path) = explicit {
        vec![path]
    } else {
        let parent = executable.parent().ok_or("Cannot locate executable directory")?;
        vec![parent.join("assets"), parent.join("../Resources/assets"), source.to_path_buf()]
    };
    for path in &candidates {
        if path.join("maps/zg_hafen.map").is_file() {
            return path.canonicalize().map_err(|e| format!("{}: {e}", path.display()));
        }
    }
    Err(format!("Game assets not found. Keep the assets folder with the game, or set --assets PATH / ZUGRIFF_ASSETS. Searched: {candidates:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_assets_work_without_source_or_working_directory() {
        let root = std::env::temp_dir().join(format!("zugriff-assets-{}", std::process::id()));
        let assets = root.join("assets");
        std::fs::create_dir_all(assets.join("maps")).unwrap();
        std::fs::write(assets.join("maps/zg_hafen.map"), "test").unwrap();
        assert_eq!(resolve(None, &root.join("zugriff"), Path::new("/missing-source")).unwrap(), assets.canonicalize().unwrap());
        assert!(resolve(Some(root.join("missing")), &root.join("zugriff"), &assets).is_err());
        assert_eq!(resolve(Some(assets.clone()), Path::new("/missing/zugriff"), Path::new("/missing")).unwrap(), assets.canonicalize().unwrap());
        std::fs::create_dir_all(root.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(root.join("Contents/Resources")).unwrap();
        std::fs::rename(&assets, root.join("Contents/Resources/assets")).unwrap();
        assert!(resolve(None, &root.join("Contents/MacOS/zugriff"), Path::new("/missing")).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
}
