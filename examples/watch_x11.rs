//! Слушает PropertyNotify на корневом окне и на указанном окне.
//! Печатает: atom (имя, если известно), window, state.
//!
//! Запуск: cargo run --example watch_x11 -- 0x8a00003

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::xproto::ConnectionExt as _;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = std::env::args().nth(1).unwrap_or_default();
    let target_win: u32 = if target.starts_with("0x") {
        u32::from_str_radix(&target[2..], 16).unwrap_or(0)
    } else {
        target.parse().unwrap_or(0)
    };

    let (conn, screen_num) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen_num].root;

    // Подписываемся на PropertyNotify корневого окна.
    conn.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;
    // И на target-окно (если указано).
    if target_win != 0 {
        conn.change_window_attributes(
            target_win,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
    }
    conn.flush()?;

    // Кэш атомов: id -> имя (для вывода).
    let mut atom_names: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    // Известные атомы.
    let known = [
        "_NET_CLIENT_LIST", "_NET_ACTIVE_WINDOW", "_NET_WM_STATE",
        "_NET_WM_STATE_HIDDEN", "_NET_WM_STATE_FOCUSED", "_NET_WM_NAME",
        "WM_NAME", "WM_STATE", "_NET_WM_DESKTOP", "_NET_CURRENT_DESKTOP",
        "_NET_WM_WINDOW_TYPE", "WM_CLASS", "_NET_WM_PID",
    ];
    for name in &known {
        if let Ok(r) = conn.intern_atom(false, name.as_bytes())?.reply() {
            atom_names.insert(r.atom, name.to_string());
        }
    }

    eprintln!("[watch] root=0x{:x}, target=0x{:x}", root, target_win);
    eprintln!("[watch] listening for PropertyNotify...");
    eprintln!("[watch] --- сверни/разверни окно, смотри что меняется ---");

    loop {
        let ev = conn.wait_for_event()?;
        if let x11rb::protocol::Event::PropertyNotify(e) = ev {
            let atom_name = atom_names
                .get(&e.atom)
                .cloned()
                .unwrap_or_else(|| format!("(atom {})", e.atom));
            let win_name = if e.window == root {
                "root".to_string()
            } else if e.window == target_win {
                format!("target 0x{:x}", e.window)
            } else {
                format!("0x{:x}", e.window)
            };
            let state = match e.state {
                Property::NEW_VALUE => "NEW",
                Property::DELETE => "DELETE",
                _ => "?",
            };
            eprintln!(
                "[watch] PropertyNotify: atom={} ({}) window={} state={}",
                e.atom, atom_name, win_name, state
            );
        }
    }
}