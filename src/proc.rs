//! /proc: путь к бинарнику и тип приложения.

use std::fs;
use std::io::Read;

use crate::window::BinaryKind;

pub fn resolve_exe(pid: u32) -> (String, BinaryKind) {
    let exe_link = format!("/proc/{pid}/exe");
    let exe_path = match fs::read_link(&exe_link) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => return (String::new(), BinaryKind::Unknown),
    };

    let base = exe_path.rsplit('/').next().unwrap_or("");

    if matches!(
        base,
        "wine" | "wine64" | "wine-preloader" | "wine64-preloader"
    ) {
        if let Some(w) = wine_exe_from_maps(pid) {
            return (w, BinaryKind::Wine);
        }
        return (exe_path, BinaryKind::Wine);
    }

    if base == "bwrap" {
        return (exe_path, BinaryKind::Flatpak);
    }

    (exe_path, BinaryKind::Native)
}

fn wine_exe_from_maps(pid: u32) -> Option<String> {
    let maps_path = format!("/proc/{pid}/maps");
    let mut f = fs::File::open(&maps_path).ok()?;
    let mut content = String::new();
    f.read_to_string(&mut content).ok()?;

    let mut best: Option<(u64, String)> = None;
    for line in content.lines() {
        let mut it = line.split_whitespace();
        let range = it.next()?;
        let _perms = it.next();
        let _offset = it.next();
        let _dev = it.next();
        let _inode = it.next();
        let path = match it.next() {
            Some(p) => p,
            None => continue,
        };
        if !path.to_lowercase().ends_with(".exe") {
            continue;
        }
        let mut parts = range.split('-');
        let a = parts.next().and_then(|s| u64::from_str_radix(s, 16).ok());
        let b = parts.next().and_then(|s| u64::from_str_radix(s, 16).ok());
        if let (Some(a), Some(b)) = (a, b) {
            let size = b.saturating_sub(a);
            if best.as_ref().map(|(s, _)| size > *s).unwrap_or(true) {
                best = Some((size, path.to_string()));
            }
        }
    }
    best.map(|(_, p)| p)
}