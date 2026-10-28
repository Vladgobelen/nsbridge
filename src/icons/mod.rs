//! Поиск и кеширование иконок для окон и трея.

pub mod store;
mod exe;
mod png;
mod resize;
mod resolver;
mod svg;

use std::path::{Path, PathBuf};

/// Размер, в котором nsbridge хранит все иконки в кеше.
pub const ICON_CACHE_SIZE: u32 = 64;

/// Публичная обёртка над `resize::resize` (RGBA → RGBA) для `tray.rs`.
pub fn resize_public(
    rgba: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    resize::resize(rgba, src_w, src_h, dst_w, dst_h)
}

pub fn resolve_icon_path(exe_path: &str, class: &str) -> Option<PathBuf> {
    let key = if !exe_path.is_empty() {
        exe_path
    } else {
        class
    };
    if key.is_empty() {
        return None;
    }

    if let Some(p) = store::icon_path(key) {
        return Some(p);
    }

    // Wine/PE — извлекаем иконку из .exe (как раньше).
    if exe_path.to_lowercase().ends_with(".exe") {
        if let Some(i) = exe::extract_from_exe(Path::new(exe_path)) {
            return normalize_and_save(key, &i.rgba, i.width, i.height);
        }
        return None;
    }

    // Стандартный путь: resolver (desktop / class / exe stem) -> файл.
    if let Some(path) = resolver::resolve_icon_path(exe_path, class) {
        if let Some((rgba, w, h)) = decode_by_ext(&path) {
            return normalize_and_save(key, &rgba, w, h);
        }
    }

    None
}

pub fn resolve_icon_name(name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }

    let candidates: Vec<&str> = if let Some(base) = name.strip_suffix("-symbolic") {
        if base.is_empty() {
            vec![name]
        } else {
            vec![base, name]
        }
    } else {
        vec![name]
    };

    for cand in candidates {
        let key = format!("icon:{cand}");
        if let Some(p) = store::icon_path(&key) {
            return Some(p);
        }
        if let Some(path) = resolver::find_icon_path(cand) {
            if let Some((rgba, w, h)) = decode_by_ext(&path) {
                if let Some(saved) = normalize_and_save(&key, &rgba, w, h) {
                    return Some(saved);
                }
            }
        }
    }

    None
}

fn decode_by_ext(path: &Path) -> Option<(Vec<u8>, usize, usize)> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("svg") {
        svg::decode(path, ICON_CACHE_SIZE).map(|i| (i.rgba, i.width, i.height))
    } else if ext.eq_ignore_ascii_case("ico") {
        exe::decode_ico_file(path).map(|i| (i.rgba, i.width, i.height))
    } else {
        png::decode(path).map(|i| (i.rgba, i.width, i.height))
    }
}

fn normalize_and_save(key: &str, rgba: &[u8], w: usize, h: usize) -> Option<PathBuf> {
    let (nw, nh) = (ICON_CACHE_SIZE as usize, ICON_CACHE_SIZE as usize);
    let normalized: Vec<u8> = if w == nw && h == nh {
        rgba.to_vec()
    } else {
        resize::resize(rgba, w, h, nw, nh)
    };
    store::save_icon_rgba(key, ICON_CACHE_SIZE, ICON_CACHE_SIZE, &normalized)
}