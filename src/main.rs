mod dbus;
mod group;
mod icons;
mod ipc;
mod proc;
mod protocol;
mod tray;
mod window;
mod x11;

use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::path::PathBuf;

use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use x11rb::connection::Connection;

use crate::dbus::DBus;
use crate::group::group_windows;
use crate::ipc::{socket_path, IpcServer};
use crate::protocol::{encode_groups, encode_tray, ClientMsg};
use crate::tray::TrayState;
use crate::window::WindowInfo;
use crate::x11::X11;

fn main() {
    if let Err(e) = run() {
        eprintln!("nsbridge: fatal error: {e}");
        std::process::exit(1);
    }
}

fn broadcast_all(
    server: &mut IpcServer,
    prev_windows: &[WindowInfo],
    tray: &mut TrayState,
) {
    let groups = group_windows(prev_windows.to_vec());
    let msg_groups = encode_groups(&groups);
    let tray_items = tray.snapshot();
    let msg_tray = encode_tray(&tray_items);
    server.broadcast(&msg_groups);
    server.broadcast(&msg_tray);
}

fn send_all(
    server: &mut IpcServer,
    idx: usize,
    prev_windows: &[WindowInfo],
    tray: &mut TrayState,
) {
    let groups = group_windows(prev_windows.to_vec());
    let msg_groups = encode_groups(&groups);
    server.send_to(idx, &msg_groups);

    let tray_items = tray.snapshot();
    let msg_tray = encode_tray(&tray_items);
    server.send_to(idx, &msg_tray);
}

/// Восстановить иконки для закреплённых приложений, даже если они
/// не запущены. `pinned.json` хранит `key` (exe_path или `class:...`)
/// и абсолютный путь к `icon.png` в кэше nsbridge. Если кэш снесён —
/// путь битый. Здесь по `key` заново резолвим иконку через ту же
/// логику (`icons::resolve_icon_path`), что и для живых окон, и
/// записываем `icon.png` в тот же каталог (`store::app_dir(key)`),
/// поэтому путь в `pinned.json` снова становится валидным.
fn restore_pinned_icons() {
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => return,
    };
    let path = home.join(".config/nspanel/widgets/window_list_rust/pinned.json");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let arr: Vec<serde_json::Value> = match serde_json::from_str(&content) {
        Ok(a) => a,
        Err(_) => {
            eprintln!("[nsbridge] pinned.json parse failed, skipping restore");
            return;
        }
    };

    let mut restored = 0usize;
    let mut skipped = 0usize;

    for item in arr {
        let key = match item.get("key").and_then(|v| v.as_str()) {
            Some(k) if !k.is_empty() => k,
            _ => continue,
        };

        // Уже есть в кэше — не трогаем.
        if icons::store::icon_path(key).is_some() {
            skipped += 1;
            continue;
        }

        let (exe, class) = if let Some(c) = key.strip_prefix("class:") {
            ("", c)
        } else if key.starts_with("id:") {
            // Нестабильный ключ (id окна) — восстановить нечего.
            continue;
        } else {
            (key, "")
        };

        if icons::resolve_icon_path(exe, class).is_some() {
            restored += 1;
            eprintln!("[nsbridge] restored pinned icon: {key}");
        } else {
            eprintln!("[nsbridge] pinned icon not found: {key}");
        }
    }

    if restored > 0 || skipped > 0 {
        eprintln!(
            "[nsbridge] pinned icons: restored {}, already cached {}",
            restored, skipped
        );
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("[nsbridge] starting");

    let x11 = X11::connect()?;
    eprintln!("[nsbridge] connected to X11, root=0x{:x}", x11.root);
    let _ = x11.subscribe_all_windows();

    // Восстанавливаем иконки закреплённых до того, как панель подключится
    // и начнёт запрашивать пути.
    restore_pinned_icons();

    let mut server = IpcServer::bind(socket_path())?;
    eprintln!("[nsbridge] listening on {}", socket_path().display());

    let mut dbus_conn: Option<DBus> = match DBus::session() {
        Ok(d) => Some(d),
        Err(e) => {
            eprintln!("[dbus] session bus connect failed: {e} (tray disabled)");
            None
        }
    };
    let mut tray = TrayState::new();
    if let Some(dbus) = dbus_conn.as_mut() {
        match tray.subscribe(dbus) {
            Ok(()) => eprintln!("[dbus][tray] ready, {} items", tray.items().len()),
            Err(e) => eprintln!("[dbus][tray] subscribe failed: {e} (tray may be empty)"),
        }
    }

    let mut prev_windows: Vec<WindowInfo> = Vec::new();
    let mut has_prev = false;

    let x11_fd = x11.conn.stream().as_raw_fd();

    loop {
        let mut raw_fds: Vec<RawFd> = Vec::new();
        raw_fds.push(x11_fd);
        raw_fds.push(server.as_raw_fd());
        let client_fds = server.client_fds();
        raw_fds.extend(client_fds.iter().copied());

        let mut fds: Vec<PollFd> = Vec::new();
        for fd in &raw_fds {
            fds.push(PollFd::new(
                unsafe { BorrowedFd::borrow_raw(*fd) },
                PollFlags::POLLIN,
            ));
        }

        match poll(&mut fds, PollTimeout::NONE) {
            Ok(0) => continue,
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => continue,
            Err(e) => return Err(format!("poll failed: {e}").into()),
        }

        // --- accept ---
        if let Some(revents) = fds[1].revents() {
            if revents.contains(PollFlags::POLLIN) {
                server.accept_pending();

                if !has_prev {
                    prev_windows = x11.collect_windows();
                    has_prev = true;
                }
                broadcast_all(&mut server, &prev_windows, &mut tray);
            }
        }

        // --- X11 ---
        if let Some(revents) = fds[0].revents() {
            if revents.contains(PollFlags::POLLIN) {
                loop {
                    let ev = match x11.conn.poll_for_event() {
                        Ok(Some(ev)) => ev,
                        Ok(None) => break,
                        Err(e) => return Err(format!("x11 poll_for_event: {e}").into()),
                    };

                    server.accept_pending();

                    match ev {
                        x11rb::protocol::Event::PropertyNotify(e) => {
                            let relevant = if e.window == x11.root {
                                e.atom == x11.atoms.net_client_list
                                    || e.atom == x11.atoms.net_active_window
                            } else {
                                e.atom == x11.atoms.net_wm_state
                            };
                            if !relevant {
                                continue;
                            }

                            if e.window == x11.root
                                && e.atom == x11.atoms.net_client_list
                            {
                                let _ = x11.subscribe_all_windows();
                            }

                            let wins = x11.collect_windows();
                            let changed = !has_prev || windows_changed(&prev_windows, &wins);
                            if has_prev && !changed {
                                continue;
                            }
                            prev_windows = wins;
                            has_prev = true;

                            if server.clients_count() > 0 {
                                broadcast_all(&mut server, &prev_windows, &mut tray);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // --- D-Bus ---
        if let Some(dbus) = dbus_conn.as_mut() {
            let msgs = dbus.poll_messages();
            let mut tray_changed = false;
            for msg in &msgs {
                if tray.handle_message(dbus, msg) {
                    tray_changed = true;
                }
            }
            if tray_changed && server.clients_count() > 0 {
                let tray_items = tray.snapshot();
                let msg = encode_tray(&tray_items);
                server.broadcast(&msg);
            }
        }

        // --- clients ---
        let mut client_has_data = false;
        let client_start = 2;
        let client_end = client_start + server.client_fds().len();
        for i in client_start..client_end {
            if i >= fds.len() {
                break;
            }
            if let Some(revents) = fds[i].revents() {
                if revents.contains(PollFlags::POLLIN) {
                    client_has_data = true;
                    break;
                }
            }
        }
        if client_has_data {
            let messages = server.read_client_messages();
            for (idx, payload) in messages {
                match serde_json::from_str::<ClientMsg>(&payload) {
                    Ok(ClientMsg::Activate { id }) => {
                        let _ = x11.activate_window(id);
                    }
                    Ok(ClientMsg::Toggle { id }) => {
                        let _ = x11.toggle_window(id);
                    }
                    Ok(ClientMsg::Request) => {
                        if !has_prev {
                            prev_windows = x11.collect_windows();
                            has_prev = true;
                        }
                        send_all(&mut server, idx, &prev_windows, &mut tray);
                    }
                    Ok(ClientMsg::TrayClick { dest, path, button, x, y }) => {
                        if let Some(dbus) = dbus_conn.as_mut() {
                            if let Err(e) = tray.activate(dbus, &dest, &path, button, x, y) {
                                eprintln!("[dbus][tray] activate failed: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[nsbridge] bad client message: {e} ({payload})");
                    }
                }
            }
        }
    }
}

fn windows_changed(a: &[WindowInfo], b: &[WindowInfo]) -> bool {
    if a.len() != b.len() {
        return true;
    }
    for (x, y) in a.iter().zip(b.iter()) {
        if x.id != y.id
            || x.is_active != y.is_active
            || x.is_hidden != y.is_hidden
            || x.title != y.title
            || x.class != y.class
            || x.exe_path != y.exe_path
            || x.icon_path != y.icon_path
        {
            return true;
        }
    }
    false
}