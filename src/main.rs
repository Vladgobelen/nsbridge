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

fn log_windows(tag: &str, wins: &[WindowInfo]) {
    eprintln!("[dbg][nsbridge] {}: {} windows", tag, wins.len());
    for w in wins {
        eprintln!(
            "[dbg][nsbridge]   win=0x{:x} class='{}' exe='{}' icon={:?} active={} hidden={}",
            w.id, w.class, w.exe_path, w.icon_path, w.is_active, w.is_hidden,
        );
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
    eprintln!(
        "[dbg][nsbridge] broadcast: groups={} tray={} bytes_groups={} bytes_tray={}",
        groups.len(),
        tray_items.len(),
        msg_groups.len(),
        msg_tray.len()
    );
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

fn run() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("[nsbridge] starting");

    let x11 = X11::connect()?;
    eprintln!("[nsbridge] connected to X11, root=0x{:x}", x11.root);
    let _ = x11.subscribe_all_windows();

    let mut server = IpcServer::bind(socket_path())?;
    eprintln!("[nsbridge] listening on {}", socket_path().display());

    // D-Bus: подключение + подписка трея.
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
                let before = server.clients_count();
                server.accept_pending();
                let after = server.clients_count();
                eprintln!("[dbg][nsbridge] accept: clients {before} -> {after}");

                if !has_prev {
                    eprintln!("[dbg][nsbridge] initial collect (no prior snapshot)");
                    prev_windows = x11.collect_windows();
                    has_prev = true;
                    log_windows("initial collect", &prev_windows);
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

        // --- D-Bus: всегда дёргаем poll_messages (неблокирующий).
        // rustbus может читать сигналы во внутренний буфер до poll,
        // поэтому POLLIN на dbus fd не гарантирован.
        if let Some(dbus) = dbus_conn.as_mut() {
            let msgs = dbus.poll_messages();
            if !msgs.is_empty() {
                eprintln!("[dbg][nsbridge] dbus: {} messages", msgs.len());
            }
            let mut tray_changed = false;
            for msg in &msgs {
                if tray.handle_message(dbus, msg) {
                    tray_changed = true;
                }
            }
            if tray_changed && server.clients_count() > 0 {
                let tray_items = tray.snapshot();
                let msg = encode_tray(&tray_items);
                eprintln!(
                    "[dbg][nsbridge] broadcast tray: items={} bytes={}",
                    tray_items.len(),
                    msg.len()
                );
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
                eprintln!("[dbg][nsbridge] client msg idx={} payload='{}'", idx, payload);
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
                            log_windows("collect on request", &prev_windows);
                        }
                        send_all(&mut server, idx, &prev_windows, &mut tray);
                    }
                    Ok(ClientMsg::TrayClick { dest, path, button, x, y }) => {
                        if let Some(dbus) = dbus_conn.as_mut() {
                            eprintln!(
                                "[dbg][nsbridge] tray_click dest='{}' path='{}' button={} ({},{})",
                                dest, path, button, x, y
                            );
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