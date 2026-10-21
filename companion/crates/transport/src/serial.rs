//! USB serial link (spec section 2): one JSON packet per line; other lines are console output.
//! Used for pairing, which is accepted over serial only.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use knoend_core::protocol::{current_name, old_name, Packet};
use serde_json::Value;

pub struct SerialLink {
    port: Box<dyn serialport::SerialPort>,
    buffer: Vec<u8>,
    next_id: u32,
    /// The board runs firmware from before the rename: send `openpager.*` names.
    pub old_names: bool,
}

impl SerialLink {
    pub fn open(path: &str) -> Result<SerialLink, String> {
        let port = serialport::new(path, 115_200).timeout(Duration::from_millis(100)).open()
            .map_err(|e| format!("cannot open {path}: {e}"))?;
        Ok(SerialLink { port, buffer: Vec::new(), next_id: 0, old_names: false })
    }

    /// Next complete line within `timeout`, or None.
    pub fn read_line(&mut self, timeout: Duration) -> Option<String> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(end) = self.buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).collect();
                return Some(String::from_utf8_lossy(&line).trim_end().to_string());
            }
            if Instant::now() >= deadline {
                return None;
            }
            let mut chunk = [0u8; 1024];
            match self.port.read(&mut chunk) {
                Ok(n) => self.buffer.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => return None,
            }
        }
    }

    pub fn drain(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline && self.read_line(Duration::from_millis(100)).is_some() {}
    }

    /// Sends a packet without waiting for a reply.
    pub fn send(&mut self, kind: &str, body: Value) -> Result<u32, String> {
        let id = self.next_id;
        self.next_id += 1;
        let kind = if self.old_names { old_name(kind) } else { kind.to_string() };
        let packet = Packet { id, re: None, kind, body };
        let mut text = serde_json::to_string(&packet).expect("serialisable");
        text.push('\n');
        self.port.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Sends a packet and waits for the reply of `reply_type` (or an error) that answers it.
    pub fn request(&mut self, kind: &str, body: Value, reply_type: &str, timeout: Duration) -> Result<Packet, String> {
        let id = self.send(kind, body)?;

        let deadline = Instant::now() + timeout;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Some(line) = self.read_line(left) else { break };
            if !line.starts_with('{') {
                continue;
            }
            if let Ok(mut reply) = serde_json::from_str::<Packet>(&line) {
                reply.kind = current_name(&reply.kind);
                if reply.re == Some(id) && (reply.kind == reply_type || reply.kind == "knoend.error") {
                    return Ok(reply);
                }
            }
        }
        Err(format!("no {reply_type} from the board within {timeout:?}"))
    }
}
