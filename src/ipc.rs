//! Unix-сокет-сервер для nsbridge.
//!
//! Клиенты (панель, виджеты) подключаются, читают группы, шлют команды.
//! nsbridge рассылает всем клиентам новые снимки при изменениях.
//!
//! ВАЖНО: мёртвые клиенты (EOF при чтении или ошибка при записи) сразу
//! удаляются из списка. Иначе при частых переподключениях панели они
//! накапливаются, broadcast пишет в них, получает BrokenPipe, а новые
//! клиенты (только что подключившиеся) могут быть ошибочно удалены
//! при очистке "мёртвых" индексов.

use std::io::{self, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use crate::protocol;

pub fn socket_path() -> PathBuf {
    let uid = nix::unistd::getuid();
    PathBuf::from(format!("/run/user/{}/nsbridge.sock", uid))
}

struct Client {
    stream: UnixStream,
    buffer: Vec<u8>,
}

pub struct IpcServer {
    listener: UnixListener,
    path: PathBuf,
    clients: Vec<Client>,
}

impl IpcServer {
    pub fn bind<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            path,
            clients: Vec::new(),
        })
    }

    pub fn accept_pending(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(true);
                    eprintln!("[nsbridge] client connected (total {})", self.clients.len() + 1);
                    self.clients.push(Client {
                        stream,
                        buffer: Vec::new(),
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    eprintln!("[nsbridge] accept error: {e}");
                    break;
                }
            }
        }
    }

    pub fn broadcast(&mut self, msg: &str) {
        let mut dead = Vec::new();
        for (i, client) in self.clients.iter_mut().enumerate() {
            if let Err(e) = protocol::write_frame(&mut client.stream, msg) {
                match e.kind() {
                    io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::WriteZero => {
                        dead.push(i);
                    }
                    _ => {
                        eprintln!("[nsbridge] write error to client {i}: {e}");
                        dead.push(i);
                    }
                }
            }
        }
        for i in dead.into_iter().rev() {
            let _ = self.clients.remove(i);
            eprintln!("[nsbridge] client disconnected (after broadcast), total {}", self.clients.len());
        }
    }

    pub fn clients_count(&self) -> usize {
        self.clients.len()
    }

    /// fd всех клиентов (для `poll` в `main.rs`).
    /// Порядок соответствует `clients` (индекс 0..N).
    pub fn client_fds(&self) -> Vec<RawFd> {
        self.clients
            .iter()
            .map(|c| c.stream.as_raw_fd())
            .collect()
    }

    /// Прочитать команды от клиентов. Возвращает список `(index, payload)`.
    /// Если клиент отключился (EOF), он удаляется из списка.
    pub fn read_client_messages(&mut self) -> Vec<(usize, String)> {
        let mut messages = Vec::new();
        let mut dead = Vec::new();

        for (i, client) in self.clients.iter_mut().enumerate() {
            let mut temp = [0u8; 4096];
            let mut is_eof = false;
            loop {
                match client.stream.read(&mut temp) {
                    Ok(0) => {
                        is_eof = true;
                        break;
                    }
                    Ok(n) => client.buffer.extend_from_slice(&temp[..n]),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        eprintln!("[nsbridge] read error client {i}: {e}");
                        is_eof = true;
                        break;
                    }
                }
            }

            if is_eof {
                dead.push(i);
                continue;
            }

            let mut offset = 0;
            while client.buffer.len() - offset >= 4 {
                let len = u32::from_be_bytes([
                    client.buffer[offset],
                    client.buffer[offset + 1],
                    client.buffer[offset + 2],
                    client.buffer[offset + 3],
                ]) as usize;
                if client.buffer.len() - offset - 4 < len {
                    break;
                }
                let payload = client.buffer[offset + 4..offset + 4 + len].to_vec();
                messages.push((i, String::from_utf8_lossy(&payload).to_string()));
                offset += 4 + len;
            }
            if offset > 0 {
                client.buffer.drain(..offset);
            }
        }

        for i in dead.into_iter().rev() {
            let _ = self.clients.remove(i);
            eprintln!("[nsbridge] client disconnected (after read), total {}", self.clients.len());
        }

        messages
    }

    pub fn send_to(&mut self, index: usize, msg: &str) {
        if let Some(client) = self.clients.get_mut(index) {
            if let Err(e) = protocol::write_frame(&mut client.stream, msg) {
                eprintln!("[nsbridge] send_to client {index} error: {e}, removing");
                self.clients.remove(index);
                eprintln!("[nsbridge] client disconnected (after send_to), total {}", self.clients.len());
            }
        }
    }
}

impl AsRawFd for IpcServer {
    fn as_raw_fd(&self) -> RawFd {
        self.listener.as_raw_fd()
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}