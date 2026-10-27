//! Хранилище иконок на диске (кеш nsbridge).
//!
//! Структура:
//! ~/.cache/nsbridge/icons/<key>/
//!   └── icon.png   (нормализованный, 64×64)

use std::path::{Path, PathBuf};

/// Базовая папка для хранения иконок.
pub fn base_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("nsbridge/icons");
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".cache/nsbridge/icons")
}

/// Папка для конкретного приложения (по ключу группы).
pub fn app_dir(key: &str) -> PathBuf {
    base_dir().join(sanitize_key(key))
}

/// Очистить ключ от символов, недопустимых в имени папки.
fn sanitize_key(key: &str) -> String {
    key.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect()
}

/// Сохранить RGBA-байты как PNG в папку приложения.
pub fn save_icon_rgba(key: &str, w: u32, h: u32, rgba: &[u8]) -> Option<PathBuf> {
    let dir = app_dir(key);
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("icon.png");

    let file = std::fs::File::create(&path).ok()?;
    let w_buf = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(w_buf, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;

    Some(path)
}

/// Скопировать готовый PNG/ICO файл в кеш.
#[allow(dead_code)]
pub fn save_icon_file(key: &str, src: &Path) -> Option<PathBuf> {
    let dir = app_dir(key);
    std::fs::create_dir_all(&dir).ok()?;
    let dst = dir.join("icon.png");

    let ext = src.extension().and_then(|s| s.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("png") {
        std::fs::copy(src, &dst).ok()?;
        Some(dst)
    } else if ext.eq_ignore_ascii_case("ico") {
        if let Some(icon) = super::exe::decode_ico_file(src) {
            save_icon_rgba(key, icon.width as u32, icon.height as u32, &icon.rgba)
        } else {
            None
        }
    } else {
        None
    }
}

/// Проверить, есть ли готовая иконка для ключа.
pub fn has_icon(key: &str) -> bool {
    app_dir(key).join("icon.png").exists()
}

/// Вернуть путь к готовой иконке (если есть).
pub fn icon_path(key: &str) -> Option<PathBuf> {
    let p = app_dir(key).join("icon.png");
    if p.exists() { Some(p) } else { None }
}