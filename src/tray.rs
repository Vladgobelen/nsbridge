//! Состояние трея: обёртка над `dbus::watcher::Watcher` + кеш иконок.

use std::collections::HashMap;

use crate::dbus::watcher::{self, IconPixmapData, TrayItem, Watcher};
use crate::dbus::DBus;
use crate::icons;

#[derive(Debug, Clone)]
pub struct TrayOut {
    pub dest: String,
    pub path: String,
    pub title: String,
    pub icon_path: Option<String>,
    pub needs_attention: bool,
    pub status: String,
}

pub struct TrayState {
    watcher: Watcher,
    icon_cache: HashMap<String, Option<String>>,
}

impl TrayState {
    pub fn new() -> Self {
        Self {
            watcher: Watcher::new(),
            icon_cache: HashMap::new(),
        }
    }

    pub fn subscribe(&mut self, dbus: &mut DBus) -> Result<(), String> {
        eprintln!("[dbus][tray] subscribing...");
        self.watcher.subscribe(dbus)?;
        eprintln!(
            "[dbus][tray] subscribed, {} items",
            self.watcher.items().len()
        );
        Ok(())
    }

    pub fn is_loaded(&self) -> bool {
        self.watcher.is_loaded()
    }

    pub fn handle_message(
        &mut self,
        dbus: &mut DBus,
        msg: &rustbus::message_builder::MarshalledMessage,
    ) -> bool {
        match self.watcher.classify(msg) {
            None => false,
            Some(watcher::ReloadKind::List) => {
                eprintln!("[dbus][tray] list changed, reloading...");
                match self.watcher.reload_list(dbus) {
                    Ok(()) => {
                        eprintln!(
                            "[dbus][tray] reloaded, {} items",
                            self.watcher.items().len()
                        );
                        self.icon_cache.clear();
                        true
                    }
                    Err(e) => {
                        eprintln!("[dbus][tray] reload failed: {e}");
                        false
                    }
                }
            }
            Some(watcher::ReloadKind::Item(dest, path)) => {
                eprintln!(
                    "[dbus][tray] item changed dest='{}' path='{}'",
                    dest, path
                );
                if self.watcher.refresh_item(dbus, &dest, &path) {
                    if let Some(it) = self
                        .watcher
                        .items()
                        .iter()
                        .find(|i| i.dest == dest && i.path == path)
                    {
                        let key = pixmap_key(it);
                        self.icon_cache.remove(&it.icon_name);
                        self.icon_cache.remove(&key);
                    }
                    true
                } else {
                    false
                }
            }
            Some(watcher::ReloadKind::RemoveByDest(dest)) => {
                eprintln!("[dbus][tray] dest '{}' lost owner, removing item", dest);
                self.watcher.remove_by_dest(&dest)
            }
        }
    }

    pub fn snapshot(&mut self) -> Vec<TrayOut> {
        // Клонируем items, чтобы не держать immutable borrow на self.watcher,
        // пока resolve_icon мутирует self (icon_cache).
        let items: Vec<TrayItem> = self.watcher.items().to_vec();
        let mut out = Vec::with_capacity(items.len());
        for it in &items {
            let icon_path = self.resolve_icon(it);
            out.push(TrayOut {
                dest: it.dest.clone(),
                path: it.path.clone(),
                title: it.title.clone(),
                icon_path,
                needs_attention: it.needs_attention,
                status: it.status.clone(),
            });
        }
        out
    }

    fn resolve_icon(&mut self, it: &TrayItem) -> Option<String> {
        // 1. IconName (файл).
        if !it.icon_name.is_empty() {
            if let Some(cached) = self.icon_cache.get(&it.icon_name) {
                return cached.clone();
            }
            let resolved =
                icons::resolve_icon_name(&it.icon_name).map(|p| p.to_string_lossy().to_string());
            self.icon_cache
                .insert(it.icon_name.clone(), resolved.clone());
            if resolved.is_some() {
                return resolved;
            }
        }

        // 2. IconPixmap (raw ARGB) → сохранить в кеш, вернуть путь.
        if let Some(pix) = &it.icon_pixmap {
            let key = pixmap_key(it);
            if let Some(cached) = self.icon_cache.get(&key) {
                return cached.clone();
            }
            let resolved = save_pixmap_icon(&key, pix);
            self.icon_cache.insert(key, resolved.clone());
            return resolved;
        }

        None
    }

    pub fn activate(
        &self,
        dbus: &mut DBus,
        dest: &str,
        path: &str,
        button: u8,
        x: i32,
        y: i32,
    ) -> Result<(), String> {
        self.watcher.activate(dbus, dest, path, button, x, y)
    }

    #[allow(dead_code)]
    pub fn items(&self) -> &[TrayItem] {
        self.watcher.items()
    }
}

/// Ключ кеша для IconPixmap. `title` стабильнее `dest` (dest меняется
/// при перезапуске приложения). Если `title` пустой — fallback на `path`.
fn pixmap_key(it: &TrayItem) -> String {
    if !it.title.is_empty() {
        format!("pixmap:{}", it.title)
    } else {
        format!("pixmap:{}", it.path)
    }
}

/// ARGB32 (A, R, G, B) → RGBA.
fn argb_to_rgba(argb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(argb.len());
    for chunk in argb.chunks(4) {
        if chunk.len() < 4 {
            break;
        }
        let (a, r, g, b) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        out.extend_from_slice(&[r, g, b, a]);
    }
    out
}

/// Сохранить IconPixmap в кеш nsbridge (нормализованный 64×64 PNG).
fn save_pixmap_icon(key: &str, pix: &IconPixmapData) -> Option<String> {
    if pix.width <= 0 || pix.height <= 0 {
        return None;
    }
    let rgba = argb_to_rgba(&pix.argb);
    let target = icons::ICON_CACHE_SIZE as usize;
    let normalized: Vec<u8> = if pix.width as usize == target && pix.height as usize == target {
        rgba
    } else {
        icons::resize_public(
            &rgba,
            pix.width as usize,
            pix.height as usize,
            target,
            target,
        )
    };
    let saved =
        icons::store::save_icon_rgba(key, icons::ICON_CACHE_SIZE, icons::ICON_CACHE_SIZE, &normalized);
    saved.map(|p| p.to_string_lossy().to_string())
}