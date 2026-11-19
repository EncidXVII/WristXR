//! Local notification endpoint compatible with the public XSOverlay UDP notification
//! protocol (JSON datagram on 127.0.0.1:42069, `messageType` 1 = popup).
use serde::Deserialize;
use std::io;
use std::net::UdpSocket;

pub const PORT: u16 = 42069;
const DEFAULT_TIMEOUT_S: f32 = 3.0;
/// Longest a sender may keep its popup up: an absurd `timeout` must neither pin it there nor overflow a `Duration`.
const MAX_TIMEOUT_S: f32 = 60.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub title: String,
    pub content: String,
    pub source_app: String,
    pub timeout_s: f32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Message {
    message_type: i32,
    title: String,
    content: String,
    source_app: String,
    timeout: f32,
}

pub fn parse(bytes: &[u8]) -> Option<Toast> {
    let m: Message = serde_json::from_slice(bytes).ok()?;
    (m.message_type == 1).then_some(Toast {
        title: m.title,
        content: m.content,
        source_app: m.source_app,
        timeout_s: if m.timeout > 0.0 { m.timeout.min(MAX_TIMEOUT_S) } else { DEFAULT_TIMEOUT_S },
    })
}

pub struct Listener {
    socket: UdpSocket,
    buf: Vec<u8>,
}

impl Listener {
    /// Localhost only. Port 0 picks a free port (tests).
    pub fn bind(port: u16) -> io::Result<Self> {
        let socket = UdpSocket::bind(("127.0.0.1", port))?;
        socket.set_nonblocking(true)?;
        Ok(Self { socket, buf: vec![0; 65_536] })
    }

    pub fn port(&self) -> io::Result<u16> {
        Ok(self.socket.local_addr()?.port())
    }

    /// Non-blocking; returns the next valid toast, skipping malformed packets.
    pub fn poll(&mut self) -> Option<Toast> {
        loop {
            match self.socket.recv(&mut self.buf) {
                Ok(n) => {
                    if let Some(t) = parse(&self.buf[..n]) {
                        return Some(t);
                    }
                    tracing::debug!("ignored notification packet");
                }
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock {
                        tracing::warn!(%e, "notification socket error");
                    }
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_is_defaulted_and_bounded() {
        let timeout = |json: &str| parse(json.as_bytes()).map(|t| t.timeout_s);
        assert_eq!(timeout(r#"{"messageType":1,"timeout":4.0}"#), Some(4.0));
        assert_eq!(timeout(r#"{"messageType":1}"#), Some(DEFAULT_TIMEOUT_S));
        assert_eq!(timeout(r#"{"messageType":1,"timeout":-1}"#), Some(DEFAULT_TIMEOUT_S));
        assert_eq!(timeout(r#"{"messageType":1,"timeout":1e30}"#), Some(MAX_TIMEOUT_S));
        assert_eq!(timeout(r#"{"messageType":2,"timeout":4.0}"#), None);
    }
}
