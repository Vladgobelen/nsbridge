//! X11: подключение, атомы, чтение EWMH-свойств, сборка WindowInfo,
//! управление окнами (activate, toggle: minimize/unminimize).
//!
//! ВАЖНО:
//! - KWin не понимает `_NET_WM_STATE_ADD _NET_WM_STATE_HIDDEN` для сворачивания.
//!   Сворачиваем через `WM_CHANGE_STATE` (ICCCM, IconicState) — как `xdotool`.
//! - `_NET_ACTIVE_WINDOW` может указывать на фрейм (не клиент).
//!   Фокус определяем по `_NET_WM_STATE_FOCUSED` (на клиенте).
//! - `subscribe_all_windows()` — подписка на `_NET_WM_STATE` всех окон
//!   (для отслеживания сворачивания/фокуса).
//!
//! Фильтр служебных окон (`is_normal_window`):
//! - KDE (plasmashell) выставляет панели сразу несколько типов:
//!   `DOCK`, `_KDE_NET_WM_WINDOW_TYPE_OVERRIDE`, `NORMAL`. Наличие `NORMAL`
//!   не делает окно нормальным, если есть служебный тип.
//! - Поэтому: если есть хоть один служебный тип (`DOCK`, `DESKTOP`,
//!   `TOOLBAR`, `MENU`, `SPLASH`, `UTILITY`, `DIALOG`, `POPUP_MENU`,
//!   `TOOLTIP`, `NOTIFICATION`, `COMBO`, `DROPDOWN_MENU`, KDE override) —
//!   окно НЕ нормальное. `SKIP_TASKBAR`/`SKIP_PAGER` — тоже.
//! - Только если служебных нет и есть `NORMAL` (или типов нет вообще,
//!   EWMH default = NORMAL) — окно нормальное.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ClientMessageData, ClientMessageEvent,
    ConnectionExt as _, EventMask, Window,
};
use x11rb::rust_connection::RustConnection;

use crate::icons;
use crate::proc;
use crate::window::{BinaryKind, WindowInfo};

#[derive(Debug, Clone, Copy)]
pub struct Atoms {
    pub net_client_list: u32,
    pub net_active_window: u32,
    pub net_wm_pid: u32,
    pub net_wm_name: u32,
    pub wm_name: u32,
    pub wm_class: u32,
    pub net_wm_state: u32,
    pub net_wm_state_hidden: u32,
    pub net_wm_state_focused: u32,
    pub net_wm_state_skip_taskbar: u32,
    pub net_wm_state_skip_pager: u32,
    pub net_wm_window_type: u32,
    pub net_wm_window_type_normal: u32,
    pub net_wm_window_type_dock: u32,
    pub net_wm_window_type_desktop: u32,
    pub net_wm_window_type_toolbar: u32,
    pub net_wm_window_type_menu: u32,
    pub net_wm_window_type_splash: u32,
    pub net_wm_window_type_utility: u32,
    pub net_wm_window_type_dialog: u32,
    pub net_wm_window_type_popup_menu: u32,
    pub net_wm_window_type_tooltip: u32,
    pub net_wm_window_type_notification: u32,
    pub net_wm_window_type_combo: u32,
    pub net_wm_window_type_dropdown_menu: u32,
    pub kde_net_wm_window_type_override: u32,
    pub wm_change_state: u32,
}

pub struct X11 {
    pub conn: RustConnection,
    pub screen_num: usize,
    pub root: Window,
    pub atoms: Atoms,
}

impl X11 {
    pub fn connect() -> Result<Self, Box<dyn std::error::Error>> {
        let (conn, screen_num) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen_num].root;

        conn.change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
        conn.flush()?;

        let intern = |name: &[u8]| -> Result<u32, Box<dyn std::error::Error>> {
            Ok(conn.intern_atom(false, name)?.reply()?.atom)
        };

        let atoms = Atoms {
            net_client_list: intern(b"_NET_CLIENT_LIST")?,
            net_active_window: intern(b"_NET_ACTIVE_WINDOW")?,
            net_wm_pid: intern(b"_NET_WM_PID")?,
            net_wm_name: intern(b"_NET_WM_NAME")?,
            wm_name: intern(b"WM_NAME")?,
            wm_class: intern(b"WM_CLASS")?,
            net_wm_state: intern(b"_NET_WM_STATE")?,
            net_wm_state_hidden: intern(b"_NET_WM_STATE_HIDDEN")?,
            net_wm_state_focused: intern(b"_NET_WM_STATE_FOCUSED")?,
            net_wm_state_skip_taskbar: intern(b"_NET_WM_STATE_SKIP_TASKBAR")?,
            net_wm_state_skip_pager: intern(b"_NET_WM_STATE_SKIP_PAGER")?,
            net_wm_window_type: intern(b"_NET_WM_WINDOW_TYPE")?,
            net_wm_window_type_normal: intern(b"_NET_WM_WINDOW_TYPE_NORMAL")?,
            net_wm_window_type_dock: intern(b"_NET_WM_WINDOW_TYPE_DOCK")?,
            net_wm_window_type_desktop: intern(b"_NET_WM_WINDOW_TYPE_DESKTOP")?,
            net_wm_window_type_toolbar: intern(b"_NET_WM_WINDOW_TYPE_TOOLBAR")?,
            net_wm_window_type_menu: intern(b"_NET_WM_WINDOW_TYPE_MENU")?,
            net_wm_window_type_splash: intern(b"_NET_WM_WINDOW_TYPE_SPLASH")?,
            net_wm_window_type_utility: intern(b"_NET_WM_WINDOW_TYPE_UTILITY")?,
            net_wm_window_type_dialog: intern(b"_NET_WM_WINDOW_TYPE_DIALOG")?,
            net_wm_window_type_popup_menu: intern(b"_NET_WM_WINDOW_TYPE_POPUP_MENU")?,
            net_wm_window_type_tooltip: intern(b"_NET_WM_WINDOW_TYPE_TOOLTIP")?,
            net_wm_window_type_notification: intern(b"_NET_WM_WINDOW_TYPE_NOTIFICATION")?,
            net_wm_window_type_combo: intern(b"_NET_WM_WINDOW_TYPE_COMBO")?,
            net_wm_window_type_dropdown_menu: intern(b"_NET_WM_WINDOW_TYPE_DROPDOWN_MENU")?,
            kde_net_wm_window_type_override: intern(b"_KDE_NET_WM_WINDOW_TYPE_OVERRIDE")?,
            wm_change_state: intern(b"WM_CHANGE_STATE")?,
        };

        Ok(Self { conn, screen_num, root, atoms })
    }

    /// Подписаться на PropertyChange всех окон из _NET_CLIENT_LIST.
    /// Нужно, чтобы ловить _NET_WM_STATE (сворачивание/фокус) каждого окна.
    /// Идемпотентно (повторный вызов — обновляет маску, не ломает).
    pub fn subscribe_all_windows(&self) -> Result<(), Box<dyn std::error::Error>> {
        let client_list = match self.conn
            .get_property(false, self.root, self.atoms.net_client_list, AtomEnum::WINDOW, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
        {
            Some(r) => r.value32().map(|it| it.collect::<Vec<Window>>()).unwrap_or_default(),
            None => Vec::new(),
        };
        for win in client_list {
            let _ = self.conn.change_window_attributes(
                win,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            );
        }
        let _ = self.conn.flush();
        Ok(())
    }

    pub fn prop_u32(&self, win: Window, atom: u32) -> Option<u32> {
        let r = self.conn
            .get_property(false, win, atom, AtomEnum::CARDINAL, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        if r.value_len == 0 || r.value.len() < 4 {
            return None;
        }
        Some(u32::from_ne_bytes([r.value[0], r.value[1], r.value[2], r.value[3]]))
    }

    pub fn prop_string(&self, win: Window, atom: u32) -> Option<String> {
        let r = self.conn
            .get_property(false, win, atom, AtomEnum::ANY, 0, 1024)
            .ok()?
            .reply()
            .ok()?;
        if r.value_len == 0 {
            return None;
        }
        let s = String::from_utf8_lossy(&r.value).to_string();
        let s = s.trim_matches('\0').trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    }

    pub fn prop_wm_class(&self, win: Window) -> (String, String) {
        let r = self.conn
            .get_property(false, win, self.atoms.wm_class, AtomEnum::STRING, 0, 64)
            .ok()
            .and_then(|c| c.reply().ok());
        if let Some(r) = r {
            if r.value_len > 0 {
                let parts: Vec<String> = r.value
                    .split(|&b| b == 0)
                    .filter(|s| !s.is_empty())
                    .map(|s| String::from_utf8_lossy(s).to_string())
                    .collect();
                let instance = parts.get(0).cloned().unwrap_or_default();
                let class = parts.get(1).cloned().unwrap_or_default();
                return (instance, class);
            }
        }
        (String::new(), String::new())
    }

    pub fn prop_atoms(&self, win: Window, atom: u32) -> Vec<u32> {
        let r = self.conn
            .get_property(false, win, atom, AtomEnum::ATOM, 0, 32)
            .ok()
            .and_then(|c| c.reply().ok());
        if let Some(r) = r {
            r.value32().map(|it| it.collect()).unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Служебные типы окон (не показывать в таскбаре, не активировать).
    fn service_types(&self) -> [u32; 14] {
        [
            self.atoms.net_wm_window_type_dock,
            self.atoms.net_wm_window_type_desktop,
            self.atoms.net_wm_window_type_toolbar,
            self.atoms.net_wm_window_type_menu,
            self.atoms.net_wm_window_type_splash,
            self.atoms.net_wm_window_type_utility,
            self.atoms.net_wm_window_type_dialog,
            self.atoms.net_wm_window_type_popup_menu,
            self.atoms.net_wm_window_type_tooltip,
            self.atoms.net_wm_window_type_notification,
            self.atoms.net_wm_window_type_combo,
            self.atoms.net_wm_window_type_dropdown_menu,
            self.atoms.kde_net_wm_window_type_override,
            self.atoms.net_wm_window_type_normal, // проверяется отдельно, не в цикле
        ]
    }

    fn is_normal_window(&self, win: Window) -> bool {
        let states = self.prop_atoms(win, self.atoms.net_wm_state);
        if states.contains(&self.atoms.net_wm_state_skip_taskbar)
            || states.contains(&self.atoms.net_wm_state_skip_pager)
        {
            return false;
        }

        let types = self.prop_atoms(win, self.atoms.net_wm_window_type);
        if types.is_empty() {
            // EWMH default: нет типа = NORMAL.
            return true;
        }

        // Если есть хоть один служебный тип — это не обычное окно,
        // даже если вместе с ним выставлен NORMAL (так делает KDE
        // для панелей: DOCK + OVERRIDE + NORMAL).
        let service = self.service_types();
        for t in &service[..13] {
            if types.contains(t) {
                return false;
            }
        }

        // Служебных нет. Обычное — только если есть NORMAL
        // (или типов нет, но это уже обработано выше).
        types.contains(&self.atoms.net_wm_window_type_normal)
    }

    pub fn collect_windows(&self) -> Vec<WindowInfo> {
        let client_list = match self.conn
            .get_property(false, self.root, self.atoms.net_client_list, AtomEnum::WINDOW, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
        {
            Some(r) => r.value32().map(|it| it.collect::<Vec<Window>>()).unwrap_or_default(),
            None => Vec::new(),
        };

        let mut windows = Vec::new();
        for win in client_list {
            if !self.is_normal_window(win) {
                continue;
            }

            let pid = self.prop_u32(win, self.atoms.net_wm_pid);
            let title = self.prop_string(win, self.atoms.net_wm_name)
                .or_else(|| self.prop_string(win, self.atoms.wm_name))
                .unwrap_or_default();
            let (instance, class) = self.prop_wm_class(win);

            if title.is_empty() && class.is_empty() {
                continue;
            }

            let (exe_path, kind) = match pid {
                Some(p) => proc::resolve_exe(p),
                None => (String::new(), BinaryKind::Unknown),
            };

            let states = self.prop_atoms(win, self.atoms.net_wm_state);
            let is_hidden = states.contains(&self.atoms.net_wm_state_hidden);
            let is_active = states.contains(&self.atoms.net_wm_state_focused);

            let icon_path = icons::resolve_icon_path(&exe_path, &class)
                .map(|p| p.to_string_lossy().to_string());

            windows.push(WindowInfo {
                id: win,
                pid,
                title,
                class,
                instance,
                exe_path,
                kind,
                is_active,
                is_hidden,
                icon_path,
            });
        }
        windows
    }

    /// Сфокусировать окно (`_NET_ACTIVE_WINDOW`).
    pub fn activate_window(&self, win: Window) -> Result<(), Box<dyn std::error::Error>> {
        let ev = ClientMessageEvent::new(
            32,
            win,
            self.atoms.net_active_window,
            ClientMessageData::from([1u32, x11rb::CURRENT_TIME, 0, 0, 0]),
        );
        self.conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ev,
        )?;
        self.conn.flush()?;
        Ok(())
    }

    /// Свернуть окно (`WM_CHANGE_STATE` → `IconicState`).
    /// ICCCM. Как `xdotool windowminimize`.
    pub fn minimize_window(&self, win: Window) -> Result<(), Box<dyn std::error::Error>> {
        let ev = ClientMessageEvent::new(
            32,
            win,
            self.atoms.wm_change_state,
            ClientMessageData::from([3u32, 0, 0, 0, 0]), // 3 = IconicState
        );
        self.conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ev,
        )?;
        self.conn.flush()?;
        Ok(())
    }

    /// Развернуть окно: снять `_NET_WM_STATE_HIDDEN` (REMOVE = 0) + фокус.
    pub fn unminimize_window(&self, win: Window) -> Result<(), Box<dyn std::error::Error>> {
        let ev = ClientMessageEvent::new(
            32,
            win,
            self.atoms.net_wm_state,
            ClientMessageData::from([0u32, self.atoms.net_wm_state_hidden, 0u32, 1u32, 0u32]), // REMOVE = 0
        );
        self.conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ev,
        )?;
        self.conn.flush()?;
        Ok(())
    }

    /// Toggle окна:
    /// фокус (`_NET_WM_STATE_FOCUSED`) → свернуть (`WM_CHANGE_STATE`);
    /// не фокус → активировать (развернуть + фокус).
    pub fn toggle_window(&self, win: Window) -> Result<(), Box<dyn std::error::Error>> {
        // Защита: служебные окна (DOCK, DESKTOP, ...) не активируем и не
        // сворачиваем. Если они как-то попали в groups (баг фильтра),
        // toggle для них — no-op, чтобы не дёргать панель KDE.
        if !self.is_normal_window(win) {
            return Ok(());
        }

        let states = self.prop_atoms(win, self.atoms.net_wm_state);
        let is_active = states.contains(&self.atoms.net_wm_state_focused);

        if is_active {
            self.minimize_window(win)?;
        } else {
            self.activate_window(win)?;
        }
        Ok(())
    }
}