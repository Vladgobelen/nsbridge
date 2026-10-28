//! Поиск иконок по стандарту Freedesktop.
//!
//! Имя иконки берём из `.desktop` (`Icon=`), если есть. Если нет —
//! пробуем `class` окна и basename `exe_path`. Сам файл иконки ищем
//! через `freedesktop-icons`, который читает `index.theme`, ходит по
//! `Inherits` и гарантированно доходит до `hicolor`.

use std::path::{Path, PathBuf};

use freedesktop_icons::lookup;

/// Найти имя иконки (`Icon=`) в `.desktop` по `exe_path` или `class`.
/// Пропускает `.desktop` с пустым `Icon=`.
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
                    if icon.is_empty() {
                        continue;
                    }
                    if !exec.is_empty() && !exe_path.is_empty() {
                        if exec == exe_path || exe_path.contains(&exec) {
                            return Some(icon);
                        }
                    }
                    if !class.is_empty() && icon.to_lowercase() == class.to_lowercase() {
                        return Some(icon);
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

/// Найти файл иконки по имени через `freedesktop-icons`.
/// Размер 64 — под `ICON_CACHE_SIZE`. Кэш включён: повторные вызовы
/// для того же имени не будут заново обходить директории.
pub fn find_icon_path(icon_name: &str) -> Option<PathBuf> {
    if icon_name.is_empty() {
        return None;
    }
    lookup(icon_name)
        .with_size(64)
        .with_cache()
        .find()
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Главная точка входа: `exe_path` + `class` → путь к файлу иконки.
/// Порядок кандидатов:
/// 1. `Icon=` из `.desktop` (если найден и непустой).
/// 2. `class` как имя иконки.
/// 3. basename `exe_path` (без расширения).
pub fn resolve_icon_path(exe_path: &str, class: &str) -> Option<PathBuf> {
    // 1. .desktop -> Icon= -> lookup.
    if let Some(icon_name) = find_icon_name(exe_path, class) {
        if let Some(p) = find_icon_path(&icon_name) {
            return Some(p);
        }
    }

    // 2. class напрямую (mpv -> mpv).
    if !class.is_empty() {
        if let Some(p) = find_icon_path(class) {
            return Some(p);
        }
    }

    // 3. basename exe (mpv для /usr/bin/mpv).
    if !exe_path.is_empty() {
        if let Some(base) = Path::new(exe_path).file_stem().and_then(|s| s.to_str()) {
            if base.to_lowercase() != class.to_lowercase() {
                if let Some(p) = find_icon_path(base) {
                    return Some(p);
                }
            }
        }
    }

    None
}