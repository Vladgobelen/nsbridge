//! Поиск иконок для приложений по `exe_path` и `class`.

use std::path::{Path, PathBuf};

fn find_icon_name(exe_path: &str, class: &str) -> Option<String> {
    let search_dirs = vec![
        PathBuf::from("/usr/share/applications"),
        dirs_home().join(".local/share/applications"),
        dirs_home().join(".local/share/applications/wine/Programs"),
    ];

    for dir in search_dirs {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                    continue;
                }
                if let Some((icon, exec)) = parse_desktop(&path) {
                    if !exec.is_empty() && !exe_path.is_empty() {
                        if exec == exe_path || exe_path.contains(&exec) {
                            return Some(icon);
                        }
                    }
                    if !class.is_empty() && !icon.is_empty() {
                        if icon.to_lowercase() == class.to_lowercase() {
                            return Some(icon);
                        }
                    }
                }
            }
        }
    }

    None
}

fn parse_desktop(path: &Path) -> Option<(String, String)> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut icon = String::new();
    let mut exec = String::new();

    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Icon=") {
            icon = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("Exec=") {
            exec = rest.split_whitespace().next().unwrap_or("").to_string();
        }
    }

    if icon.is_empty() && exec.is_empty() {
        None
    } else {
        Some((icon, exec))
    }
}

pub fn find_icon_path(icon_name: &str) -> Option<PathBuf> {
    if icon_name.is_empty() {
        return None;
    }

    if icon_name.starts_with('/') {
        let p = PathBuf::from(icon_name);
        if p.exists() {
            return Some(p);
        }
        return None;
    }

    let bases = vec![
        PathBuf::from("/usr/share/icons/hicolor"),
        PathBuf::from("/usr/share/icons/Adwaita"),
        PathBuf::from("/usr/share/pixmaps"),
        dirs_home().join(".local/share/icons"),
    ];

    let sizes = ["32x32", "48x48", "64x64", "128x128", "256x256"];

    for base in &bases {
        for size in &sizes {
            let p = base.join(size).join("apps").join(format!("{}.png", icon_name));
            if p.exists() {
                return Some(p);
            }
        }
        let p = base.join(format!("{}.png", icon_name));
        if p.exists() {
            return Some(p);
        }
    }

    for base in &bases {
        let p = base.join("scalable").join("apps").join(format!("{}.svg", icon_name));
        if p.exists() {
            return Some(p);
        }
        let p = base.join("scalable").join(format!("{}.svg", icon_name));
        if p.exists() {
            return Some(p);
        }
    }

    None
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn resolve_icon_path(exe_path: &str, class: &str) -> Option<PathBuf> {
    let icon_name = find_icon_name(exe_path, class)?;
    find_icon_path(&icon_name)
}