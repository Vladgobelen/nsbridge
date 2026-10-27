//! Группировка окон по приложению (`exe_path` или `class`).
//! nsbridge отдаёт клиентам готовые группы — виджеты не группируют.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::window::WindowInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppGroup {
    /// Стабильный ключ: `exe_path`, `class:...`, `id:...`.
    pub key: String,
    /// Одна буква для иконки.
    pub label: String,
    /// Все окна группы.
    pub windows: Vec<WindowInfo>,
    /// Активно хотя бы одно окно группы.
    pub has_active: bool,
    /// Все окна группы свёрнуты.
    pub all_hidden: bool,
}

pub fn group_windows(windows: Vec<WindowInfo>) -> Vec<AppGroup> {
    let mut map: HashMap<String, AppGroup> = HashMap::new();

    for win in windows {
        let key = if !win.exe_path.is_empty() {
            win.exe_path.clone()
        } else if !win.class.is_empty() {
            format!("class:{}", win.class)
        } else {
            format!("id:{}", win.id)
        };

        let group = map.entry(key.clone()).or_insert_with(|| {
            let label = if !win.class.is_empty() {
                win.class.chars().next().unwrap_or('?').to_uppercase().to_string()
            } else if !win.title.is_empty() {
                win.title.chars().next().unwrap_or('?').to_uppercase().to_string()
            } else {
                "?".to_string()
            };
            AppGroup {
                key,
                label,
                windows: Vec::new(),
                has_active: false,
                all_hidden: true,
            }
        });

        if win.is_active {
            group.has_active = true;
        }
        if !win.is_hidden {
            group.all_hidden = false;
        }
        group.windows.push(win);
    }

    let mut groups: Vec<AppGroup> = map.into_values().collect();
    groups.sort_by(|a, b| a.key.cmp(&b.key));
    groups
}