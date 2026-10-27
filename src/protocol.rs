//! Протокол обмена сообщениями между `nsbridge` и клиентами (панелью).
//!
//! Клиент → nsbridge (JSON):
//!   {"type":"request"}
//!   {"type":"toggle","id":N}
//!   {"type":"activate","id":N}
//!   {"type":"tray_click","dest":":1.42","path":"/org/kind/StatusNotifierItem","button":1,"x":5,"y":5}
//!
//! nsbridge → клиент (JSON):
//!   {"type":"groups","data":[...]}
//!   {"type":"tray","data":[...]}

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::group::AppGroup;
use crate::tray::TrayOut;
use crate::window::{BinaryKind, WindowInfo};

/// Сообщение от клиента (панели).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Request,
    Toggle { id: u32 },
    Activate { id: u32 },
    TrayClick {
        dest: String,
        path: String,
        button: u8,
        x: i32,
        y: i32,
    },
}

/// Сформировать JSON `groups`.
pub fn encode_groups(groups: &[AppGroup]) -> String {
    #[derive(Serialize)]
    struct Envelope<'a> {
        #[serde(rename = "type")]
        kind: &'a str,
        data: &'a [AppGroup],
    }
    let env = Envelope { kind: "groups", data: groups };
    serde_json::to_string(&env).unwrap_or_else(|_| r#"{"type":"groups","data":[]}"#.to_string())
}

/// Сформировать JSON `tray`.
pub fn encode_tray(items: &[TrayOut]) -> String {
    #[derive(Serialize)]
    struct Out<'a> {
        dest: &'a str,
        path: &'a str,
        title: &'a str,
        icon_path: &'a Option<String>,
        needs_attention: bool,
        status: &'a str,
    }
    let data: Vec<Out> = items
        .iter()
        .map(|it| Out {
            dest: &it.dest,
            path: &it.path,
            title: &it.title,
            icon_path: &it.icon_path,
            needs_attention: it.needs_attention,
            status: &it.status,
        })
        .collect();
    #[derive(Serialize)]
    struct Envelope<'a> {
        #[serde(rename = "type")]
        kind: &'a str,
        data: &'a [Out<'a>],
    }
    let env = Envelope { kind: "tray", data: &data };
    serde_json::to_string(&env).unwrap_or_else(|_| r#"{"type":"tray","data":[]}"#.to_string())
}

/// Записать сообщение в поток: 4 байта длины + текст.
pub fn write_frame<W: Write>(w: &mut W, msg: &str) -> io::Result<()> {
    let len = msg.len() as u32;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(msg.as_bytes())?;
    w.flush()
}

/// Прочитать сообщение из потока (для клиентов — панели).
#[allow(dead_code)]
pub fn read_frame<R: Read>(r: &mut R) -> io::Result<Option<String>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 16 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(Some(String::from_utf8_lossy(&buf).to_string()))
}

/// Старый текстовый `encode_windows` (оставлен для отладки).
#[allow(dead_code)]
pub fn encode_windows(windows: &[WindowInfo]) -> String {
    let mut out = String::new();
    out.push_str("type=windows\n");
    for w in windows {
        let icon = match &w.icon_path {
            Some(p) => p.clone(),
            None => "-".to_string(),
        };
        out.push_str(&format!(
            "win=0x{:x} pid={} class={} instance={} title={} exe={} kind={} active={} hidden={} icon={}\n",
            w.id,
            w.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string()),
            esc(&w.class),
            esc(&w.instance),
            esc(&w.title),
            esc(&w.exe_path),
            w.kind.as_str(),
            if w.is_active { "1" } else { "0" },
            if w.is_hidden { "1" } else { "0" },
            esc(&icon),
        ));
    }
    out
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            ' ' => out.push_str("\\s"),
            '=' => out.push_str("\\e"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out
}

#[allow(dead_code)]
fn unesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('s') => out.push(' '),
                Some('e') => out.push('='),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Разобрать текстовое `windows` обратно в `Vec<WindowInfo>` (для отладки).
#[allow(dead_code)]
pub fn decode_windows(msg: &str) -> Vec<WindowInfo> {
    let mut out = Vec::new();
    for line in msg.lines() {
        if !line.starts_with("win=") {
            continue;
        }
        let mut w = WindowInfo {
            id: 0,
            pid: None,
            title: String::new(),
            class: String::new(),
            instance: String::new(),
            exe_path: String::new(),
            kind: BinaryKind::Unknown,
            is_active: false,
            is_hidden: false,
            icon_path: None,
        };
        let mut rest = line;
        if let Some(space) = rest.find(' ') {
            let first = &rest[..space];
            rest = &rest[space + 1..];
            if let Some(id_hex) = first.strip_prefix("win=0x") {
                w.id = u32::from_str_radix(id_hex, 16).unwrap_or(0);
            }
        }
        let keys = ["pid", "class", "instance", "title", "exe", "kind", "active", "hidden", "icon"];
        let mut parts: Vec<(String, String)> = Vec::new();
        let mut current_key: Option<String> = None;
        let mut current_val = String::new();
        let mut chars = rest.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == ' ' {
                let mut peek = chars.clone();
                let mut word = String::new();
                while let Some(&c) = peek.peek() {
                    if c == '=' { break; }
                    if c == ' ' { break; }
                    word.push(c);
                    peek.next();
                }
                if peek.peek() == Some(&'=') && keys.contains(&word.as_str()) {
                    if let Some(k) = current_key.take() {
                        parts.push((k, current_val.clone()));
                    }
                    current_val.clear();
                    let wlen = word.len();
                    current_key = Some(word);
                    for _ in 0..wlen { chars.next(); }
                    chars.next();
                    continue;
                }
            }
            if current_key.is_some() {
                current_val.push(ch);
            }
        }
        if let Some(k) = current_key.take() {
            parts.push((k, current_val));
        }

        for (k, v) in parts {
            match k.as_str() {
                "pid" => w.pid = v.parse().ok(),
                "class" => w.class = unesc(&v),
                "instance" => w.instance = unesc(&v),
                "title" => w.title = unesc(&v),
                "exe" => w.exe_path = unesc(&v),
                "kind" => {
                    w.kind = match v.as_str() {
                        "native" => BinaryKind::Native,
                        "wine" => BinaryKind::Wine,
                        "flatpak" => BinaryKind::Flatpak,
                        _ => BinaryKind::Unknown,
                    }
                }
                "active" => w.is_active = v == "1",
                "hidden" => w.is_hidden = v == "1",
                "icon" => {
                    if v != "-" {
                        w.icon_path = Some(unesc(&v));
                    }
                }
                _ => {}
            }
        }
        out.push(w);
    }
    out
}