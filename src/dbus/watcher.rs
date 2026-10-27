//! StatusNotifierWatcher (KDE/StatusNotifierItem) — трей.

use rustbus::message_builder::MarshalledMessage;
use rustbus::params::{Base, Container, Param};

use super::DBus;

pub const WATCHER_DEST: &str = "org.kde.StatusNotifierWatcher";
pub const WATCHER_PATH: &str = "/StatusNotifierWatcher";
pub const WATCHER_IFACE: &str = "org.kde.StatusNotifierWatcher";
pub const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";

/// Одна иконка из `IconPixmap` (ARGB32, big-endian).
#[derive(Debug, Clone)]
pub struct IconPixmapData {
    pub width: i32,
    pub height: i32,
    /// ARGB32 (A, R, G, B), как пришло из D-Bus.
    pub argb: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TrayItem {
    pub dest: String,
    pub path: String,
    pub title: String,
    pub icon_name: String,
    pub icon_pixmap: Option<IconPixmapData>,
    pub needs_attention: bool,
    pub status: String,
}

pub struct Watcher {
    items: Vec<TrayItem>,
    loaded: bool,
}

impl Watcher {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            loaded: false,
        }
    }

    pub fn items(&self) -> &[TrayItem] {
        &self.items
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn subscribe(&mut self, dbus: &mut DBus) -> Result<(), String> {
        dbus.add_match("type='signal',interface='org.kde.StatusNotifierWatcher'")?;
        dbus.add_match("type='signal',interface='org.kde.StatusNotifierItem'")?;
        self.reload_list(dbus)?;
        self.loaded = true;
        Ok(())
    }

    pub fn reload_list(&mut self, dbus: &mut DBus) -> Result<(), String> {
        let reply = dbus.call(
            WATCHER_DEST,
            WATCHER_PATH,
            "org.freedesktop.DBus.Properties",
            "Get",
            &[WATCHER_IFACE, "RegisteredStatusNotifierItems"],
        )?;

        let mut parser = reply.body.parser();
        let param = parser
            .get_param()
            .map_err(|e| format!("RegisteredStatusNotifierItems: param get: {e}"))?;
        let list = extract_string_array(param)
            .ok_or_else(|| "RegisteredStatusNotifierItems: not a string array".to_string())?;

        let mut items = Vec::new();
        for raw in list {
            let (dest, path) = match split_item(&raw) {
                Some(x) => x,
                None => continue,
            };
            if let Ok(item) = read_item_props(dbus, &dest, &path) {
                items.push(item);
            }
        }
        self.items = items;
        Ok(())
    }

    pub fn classify(&self, msg: &MarshalledMessage) -> Option<ReloadKind> {
        let header = &msg.dynheader;
        let member = header.member.as_deref().unwrap_or("");
        let iface = header.interface.as_deref().unwrap_or("");
        let path = header.object.clone().unwrap_or_default();

        match (iface, member) {
            ("org.kde.StatusNotifierWatcher", "StatusNotifierItemRegistered")
            | ("org.kde.StatusNotifierWatcher", "StatusNotifierItemUnregistered") => {
                Some(ReloadKind::List)
            }
            ("org.kde.StatusNotifierItem", "NewTitle")
            | ("org.kde.StatusNotifierItem", "NewIcon")
            | ("org.kde.StatusNotifierItem", "NewAttentionIcon")
            | ("org.kde.StatusNotifierItem", "NewStatus") => {
                let dest = self
                    .items
                    .iter()
                    .find(|it| it.path == path)
                    .map(|it| it.dest.clone())?;
                Some(ReloadKind::Item(dest, path))
            }
            _ => None,
        }
    }

    pub fn refresh_item(&mut self, dbus: &mut DBus, dest: &str, path: &str) -> bool {
        match read_item_props(dbus, dest, path) {
            Ok(new_item) => {
                for it in self.items.iter_mut() {
                    if it.dest == dest && it.path == path {
                        *it = new_item;
                        return true;
                    }
                }
                false
            }
            Err(_) => false,
        }
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
        let method = match button {
            1 => "Activate",
            3 => "SecondaryActivate",
            _ => "ContextMenu",
        };
        let _ = dbus.call_i32_pair(dest, path, ITEM_IFACE, method, x, y)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum ReloadKind {
    List,
    Item(String, String),
}

pub fn split_item(s: &str) -> Option<(String, String)> {
    let slash = s.find('/')?;
    let dest = &s[..slash];
    let path = &s[slash..];
    if dest.is_empty() || path.is_empty() {
        return None;
    }
    Some((dest.to_string(), path.to_string()))
}

fn extract_string_array(p: Param) -> Option<Vec<String>> {
    let inner = match p {
        Param::Container(Container::Variant(v)) => v.value,
        _ => return None,
    };
    match inner {
        Param::Container(Container::Array(a)) => {
            let mut out = Vec::new();
            for v in a.values {
                match v {
                    Param::Base(Base::String(s))
                    | Param::Base(Base::ObjectPath(s))
                    | Param::Base(Base::Signature(s)) => out.push(s),
                    Param::Base(Base::StringRef(s))
                    | Param::Base(Base::ObjectPathRef(s))
                    | Param::Base(Base::SignatureRef(s)) => out.push(s.to_string()),
                    _ => return None,
                }
            }
            Some(out)
        }
        Param::Container(Container::ArrayRef(a)) => {
            let mut out = Vec::new();
            for v in a.values {
                match v {
                    Param::Base(Base::String(s))
                    | Param::Base(Base::ObjectPath(s))
                    | Param::Base(Base::Signature(s)) => out.push(s.clone()),
                    Param::Base(Base::StringRef(s))
                    | Param::Base(Base::ObjectPathRef(s))
                    | Param::Base(Base::SignatureRef(s)) => out.push(s.to_string()),
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
}

fn extract_string(p: Param) -> Option<String> {
    let inner = match p {
        Param::Container(Container::Variant(v)) => v.value,
        _ => return None,
    };
    match inner {
        Param::Base(Base::String(s))
        | Param::Base(Base::ObjectPath(s))
        | Param::Base(Base::Signature(s)) => Some(s),
        Param::Base(Base::StringRef(s))
        | Param::Base(Base::ObjectPathRef(s))
        | Param::Base(Base::SignatureRef(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn extract_bool(p: Param) -> Option<bool> {
    let inner = match p {
        Param::Container(Container::Variant(v)) => v.value,
        _ => return None,
    };
    match inner {
        Param::Base(Base::Boolean(b)) => Some(b),
        Param::Base(Base::BooleanRef(b)) => Some(*b),
        _ => None,
    }
}

/// Развернуть `Param` (variant of `a(iiay)`) в первую иконку.
/// Обычно `IconPixmap` содержит одну иконку (иногда несколько размеров —
/// берём самую большую).
fn extract_pixmap(p: Param) -> Option<IconPixmapData> {
    let inner = match p {
        Param::Container(Container::Variant(v)) => v.value,
        _ => return None,
    };
    let values: Vec<&Param> = match &inner {
        Param::Container(Container::Array(a)) => a.values.iter().collect(),
        Param::Container(Container::ArrayRef(a)) => a.values.iter().collect(),
        _ => return None,
    };

    let mut best: Option<IconPixmapData> = None;
    for v in values {
        let (w, h, argb) = match v {
            Param::Container(Container::Struct(fields)) => {
                if fields.len() != 3 {
                    continue;
                }
                let w = match &fields[0] {
                    Param::Base(Base::Int32(x)) => *x,
                    Param::Base(Base::Int32Ref(x)) => **x,
                    _ => continue,
                };
                let h = match &fields[1] {
                    Param::Base(Base::Int32(x)) => *x,
                    Param::Base(Base::Int32Ref(x)) => **x,
                    _ => continue,
                };
                let bytes: Vec<u8> = match &fields[2] {
                    Param::Container(Container::Array(a)) => a
                        .values
                        .iter()
                        .filter_map(|p| match p {
                            Param::Base(Base::Byte(x)) => Some(*x),
                            Param::Base(Base::ByteRef(x)) => Some(**x),
                            _ => None,
                        })
                        .collect(),
                    Param::Container(Container::ArrayRef(a)) => a
                        .values
                        .iter()
                        .filter_map(|p| match p {
                            Param::Base(Base::Byte(x)) => Some(*x),
                            Param::Base(Base::ByteRef(x)) => Some(**x),
                            _ => None,
                        })
                        .collect(),
                    _ => continue,
                };
                (w, h, bytes)
            }
            _ => continue,
        };

        if argb.len() < (w * h * 4) as usize {
            continue;
        }

        let candidate = IconPixmapData { width: w, height: h, argb };
        let replace = match &best {
            None => true,
            Some(b) => (candidate.width * candidate.height) > (b.width * b.height),
        };
        if replace {
            best = Some(candidate);
        }
    }
    best
}

pub fn read_item_props(
    dbus: &mut DBus,
    dest: &str,
    path: &str,
) -> Result<TrayItem, String> {
    let title = get_property_string(dbus, dest, path, ITEM_IFACE, "Title")
        .unwrap_or_default();
    let icon_name = get_property_string(dbus, dest, path, ITEM_IFACE, "IconName")
        .unwrap_or_default();
    let status = get_property_string(dbus, dest, path, ITEM_IFACE, "Status")
        .unwrap_or_default();
    let needs_attention =
        get_property_bool(dbus, dest, path, ITEM_IFACE, "NeedsAttention")
            .unwrap_or(false);
    let icon_pixmap =
        get_property_pixmap(dbus, dest, path, ITEM_IFACE, "IconPixmap");

    Ok(TrayItem {
        dest: dest.to_string(),
        path: path.to_string(),
        title,
        icon_name,
        icon_pixmap,
        needs_attention,
        status,
    })
}

fn get_property_string(
    dbus: &mut DBus,
    dest: &str,
    path: &str,
    iface: &str,
    prop: &str,
) -> Option<String> {
    let reply = dbus
        .call(dest, path, "org.freedesktop.DBus.Properties", "Get", &[iface, prop])
        .ok()?;
    let mut parser = reply.body.parser();
    let param = parser.get_param().ok()?;
    extract_string(param)
}

fn get_property_bool(
    dbus: &mut DBus,
    dest: &str,
    path: &str,
    iface: &str,
    prop: &str,
) -> Option<bool> {
    let reply = dbus
        .call(dest, path, "org.freedesktop.DBus.Properties", "Get", &[iface, prop])
        .ok()?;
    let mut parser = reply.body.parser();
    let param = parser.get_param().ok()?;
    extract_bool(param)
}

fn get_property_pixmap(
    dbus: &mut DBus,
    dest: &str,
    path: &str,
    iface: &str,
    prop: &str,
) -> Option<IconPixmapData> {
    let reply = dbus
        .call(dest, path, "org.freedesktop.DBus.Properties", "Get", &[iface, prop])
        .ok()?;
    let mut parser = reply.body.parser();
    let param = parser.get_param().ok()?;
    extract_pixmap(param)
}