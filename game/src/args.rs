//! Launch options: `?name=value` in the page URL on the web, `--name value` on the command line natively.

/// The value of option `name`, if given.
pub fn get(name: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let search = web_sys::window()?.location().search().ok()?;
        web_sys::UrlSearchParams::new_with_str(&search).ok()?.get(name)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let flag = format!("--{name}");
        let args: Vec<String> = std::env::args().collect();
        args.iter().position(|a| *a == flag).and_then(|i| args.get(i + 1).cloned())
    }
}

/// A camera pose `x,y,z,yaw,pitch` (metres, degrees) from the `pose` option.
pub fn pose() -> Option<[f32; 5]> {
    let v: Vec<f32> = get("pose")?.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    (v.len() == 5).then(|| [v[0], v[1], v[2], v[3], v[4]])
}

/// Whether a bare flag is given (`?name` or `?name=…` on the web, `--name` natively).
pub fn flag(name: &str) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        get(name).is_some()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let flag = format!("--{name}");
        std::env::args().any(|a| a == flag)
    }
}
