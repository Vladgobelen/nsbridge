//! Низкоуровневый D-Bus-клиент для nsbridge.

pub mod watcher;

use std::os::fd::{AsRawFd, RawFd};

use rustbus::connection::ll_conn::force_finish_on_error;
use rustbus::connection::Timeout;
use rustbus::message_builder::MarshalledMessage;
use rustbus::{MessageBuilder, RpcConn};

pub struct DBus {
    conn: RpcConn,
}

impl DBus {
    pub fn session() -> Result<Self, String> {
        let conn = RpcConn::session_conn(Timeout::Infinite)
            .map_err(|e| format!("session bus connect failed: {e}"))?;
        Ok(Self { conn })
    }

    pub fn as_raw_fd(&self) -> RawFd {
        self.conn.conn().as_raw_fd()
    }

    pub fn call(
        &mut self,
        dest: &str,
        path: &str,
        iface: &str,
        method: &str,
        args: &[&str],
    ) -> Result<MarshalledMessage, String> {
        if !is_valid_bus_name(dest) {
            return Err(format!("invalid bus name: '{dest}'"));
        }
        if !path.starts_with('/') {
            return Err(format!("invalid object path: '{path}'"));
        }

        let builder = MessageBuilder::new()
            .call(method)
            .with_interface(iface)
            .on(path)
            .at(dest);
        let mut msg = builder.build();
        for arg in args {
            msg.body
                .push_param(*arg)
                .map_err(|e| format!("push_param failed: {e}"))?;
        }

        let id = self
            .conn
            .send_message(&mut msg)
            .and_then(|ctx| ctx.write_all().map_err(force_finish_on_error))
            .map_err(|e| format!("send failed: {e}"))?;

        self.conn
            .wait_response(id, Timeout::Infinite)
            .map_err(|e| format!("wait failed: {e}"))
    }

    /// Вызов с двумя i32-аргументами (Activate(x, y), SecondaryActivate(x, y),
    /// ContextMenu(x, y)).
    pub fn call_i32_pair(
        &mut self,
        dest: &str,
        path: &str,
        iface: &str,
        method: &str,
        a: i32,
        b: i32,
    ) -> Result<MarshalledMessage, String> {
        if !is_valid_bus_name(dest) {
            return Err(format!("invalid bus name: '{dest}'"));
        }
        if !path.starts_with('/') {
            return Err(format!("invalid object path: '{path}'"));
        }

        let builder = MessageBuilder::new()
            .call(method)
            .with_interface(iface)
            .on(path)
            .at(dest);
        let mut msg = builder.build();
        msg.body
            .push_param(a)
            .map_err(|e| format!("push_param(a): {e}"))?;
        msg.body
            .push_param(b)
            .map_err(|e| format!("push_param(b): {e}"))?;

        let id = self
            .conn
            .send_message(&mut msg)
            .and_then(|ctx| ctx.write_all().map_err(force_finish_on_error))
            .map_err(|e| format!("send failed: {e}"))?;

        self.conn
            .wait_response(id, Timeout::Infinite)
            .map_err(|e| format!("wait failed: {e}"))
    }

    pub fn poll_messages(&mut self) -> Vec<MarshalledMessage> {
        // Сначала вычитываем ВЕСЬ доступный ввод-вывод из сокета D-Bus
        // в внутреннюю очередь RpcConn. Без этого try_get_signal()
        // всегда возвращает None, потому что сокет не прочитан.
        match self.conn.refill_all() {
            Ok(_) => {}
            Err(e) => {
                eprintln!("[dbus] refill_all error: {e}");
                return Vec::new();
            }
        }

        // Теперь забираем все разобранные сигналы из очереди.
        let mut out = Vec::new();
        loop {
            match self.conn.try_get_signal() {
                Some(msg) => out.push(msg),
                None => break,
            }
        }
        out
    }

    pub fn add_match(&mut self, rule: &str) -> Result<(), String> {
        let builder = MessageBuilder::new()
            .call("AddMatch")
            .with_interface("org.freedesktop.DBus")
            .on("/org/freedesktop/DBus")
            .at("org.freedesktop.DBus");
        let mut msg = builder.build();
        msg.body
            .push_param(rule)
            .map_err(|e| format!("push_param failed: {e}"))?;

        let id = self
            .conn
            .send_message(&mut msg)
            .and_then(|ctx| ctx.write_all().map_err(force_finish_on_error))
            .map_err(|e| format!("send failed: {e}"))?;
        self.conn
            .wait_response(id, Timeout::Infinite)
            .map_err(|e| format!("wait failed: {e}"))?;
        Ok(())
    }
}

pub fn is_valid_bus_name(dest: &str) -> bool {
    if dest.is_empty() {
        return false;
    }
    if dest.starts_with(':') {
        return dest.len() > 1
            && dest[1..]
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '-');
    }
    if !dest.contains('.') {
        return false;
    }
    let mut first = true;
    for seg in dest.split('.') {
        if seg.is_empty() {
            return false;
        }
        let b = seg.as_bytes()[0];
        if first {
            if !(b.is_ascii_alphabetic() || b == b'_') {
                return false;
            }
            first = false;
        } else if !(b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            return false;
        }
        if !seg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return false;
        }
    }
    true
}