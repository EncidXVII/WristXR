//! VRChat OSC chatbox (public OSC docs: `/chatbox/input s b b`, `/chatbox/typing b`).
use rosc::{OscMessage, OscPacket, OscType, encoder};
use std::io;
use std::net::{SocketAddr, UdpSocket};

pub const VRCHAT_ADDR: &str = "127.0.0.1:9000";
pub const CHATBOX_MAX_CHARS: usize = 144;

pub struct Chatbox {
    socket: UdpSocket,
    target: SocketAddr,
}

impl Chatbox {
    pub fn new(target: SocketAddr) -> io::Result<Self> {
        Ok(Self { socket: UdpSocket::bind("127.0.0.1:0")?, target })
    }

    /// Sends immediately (no keyboard UI in VRChat), with the notification sound.
    pub fn send(&self, text: &str) -> io::Result<()> {
        let text: String = text.chars().take(CHATBOX_MAX_CHARS).collect();
        self.emit("/chatbox/input", vec![OscType::String(text), OscType::Bool(true), OscType::Bool(true)])
    }

    pub fn typing(&self, on: bool) -> io::Result<()> {
        self.emit("/chatbox/typing", vec![OscType::Bool(on)])
    }

    fn emit(&self, addr: &str, args: Vec<OscType>) -> io::Result<()> {
        let packet = OscPacket::Message(OscMessage { addr: addr.to_owned(), args });
        let bytes = encoder::encode(&packet).map_err(|e| io::Error::other(format!("{e:?}")))?;
        self.socket.send_to(&bytes, self.target).map(|_| ())
    }
}
