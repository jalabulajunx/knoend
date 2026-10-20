//! Authenticated Wi-Fi link to one board (spec section 4): WebSocket, handshake over fresh
//! nonces, then `{ctr, mac, packet}` frames bound to the session. Any failure ends the link.

use std::net::SocketAddr;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use knoend_core::protocol::Packet;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::auth::{self, Key};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("connection: {0}")]
    Connection(String),
    #[error("handshake: {0}")]
    Handshake(String),
    #[error("frame rejected: {0}")]
    Frame(String),
    #[error("board closed the connection")]
    Closed,
}

pub struct WifiSender {
    /// The board runs firmware from before the rename: send `openpager.*` names.
    old_names: bool,
    sink: SplitSink<Socket, Message>,
    key: Key,
    session_id: String,
    next_ctr: u64,
    next_id: u32,
}

pub struct WifiReceiver {
    stream: SplitStream<Socket>,
    key: Key,
    session_id: String,
    last_ctr: Option<u64>,
}

async fn next_text(stream: &mut SplitStream<Socket>) -> Result<String, LinkError> {
    loop {
        match stream.next().await {
            Some(Ok(Message::Text(text))) => return Ok(text.to_string()),
            Some(Ok(Message::Close(_))) | None => return Err(LinkError::Closed),
            Some(Ok(_)) => continue, // ping/pong/binary: not part of the protocol
            Some(Err(e)) => return Err(LinkError::Connection(e.to_string())),
        }
    }
}

/// Connects and runs the handshake. Fails unless the board proves it holds `key` and is `board_id`.
/// `old_names`: the board is from before the rename (it advertised only `_openpager._tcp`).
pub async fn connect(address: SocketAddr, board_id: &str, companion: &str, key: Key, old_names: bool)
    -> Result<(WifiSender, WifiReceiver), LinkError> {
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await
        .map_err(|e| LinkError::Connection(e.to_string()))?;
    let (mut sink, mut stream) = socket.split();

    let companion_nonce = auth::nonce();
    let hello = json!({"hello": {"companion": companion, "nonce": companion_nonce}});
    sink.send(Message::text(hello.to_string())).await.map_err(|e| LinkError::Connection(e.to_string()))?;

    let reply: Value = serde_json::from_str(&next_text(&mut stream).await?)
        .map_err(|e| LinkError::Handshake(e.to_string()))?;
    let hello = &reply["hello"];
    let board_nonce = hello["nonce"].as_str().ok_or_else(|| LinkError::Handshake("no board nonce".into()))?;
    if hello["board"] != board_id {
        return Err(LinkError::Handshake(format!("expected board {board_id}, got {}", hello["board"])));
    }
    let proof = hello["proof"].as_str().unwrap_or("");
    if !auth::equal(proof, &auth::board_proof(&key, &companion_nonce, board_nonce)) {
        return Err(LinkError::Handshake("board proof does not verify".into()));
    }
    let ours = auth::companion_proof(&key, board_nonce, &companion_nonce);
    sink.send(Message::text(json!({"proof": ours}).to_string())).await
        .map_err(|e| LinkError::Connection(e.to_string()))?;

    let session_id = format!("{board_nonce}{companion_nonce}");
    Ok((
        WifiSender { old_names, sink, key, session_id: session_id.clone(), next_ctr: 0, next_id: 0 },
        WifiReceiver { stream, key, session_id, last_ctr: None },
    ))
}

impl WifiSender {
    /// Sends one packet; returns its id.
    pub async fn send(&mut self, kind: &str, body: Value) -> Result<u32, LinkError> {
        let id = self.next_id;
        self.next_id += 1;
        let kind = if self.old_names { knoend_core::protocol::old_name(kind) } else { kind.to_string() };
        let packet = serde_json::to_string(&Packet { id, re: None, kind, body }).expect("serialisable");
        let ctr = self.next_ctr;
        self.next_ctr += 1;
        let frame = json!({"ctr": ctr, "mac": auth::frame_mac(&self.key, &self.session_id, ctr, &packet), "packet": packet});
        self.sink.send(Message::text(frame.to_string())).await.map_err(|e| LinkError::Connection(e.to_string()))?;
        Ok(id)
    }
}

impl WifiReceiver {
    /// Next verified packet from the board.
    pub async fn recv(&mut self) -> Result<Packet, LinkError> {
        let frame: Value = serde_json::from_str(&next_text(&mut self.stream).await?)
            .map_err(|e| LinkError::Frame(e.to_string()))?;
        let ctr = frame["ctr"].as_u64().ok_or_else(|| LinkError::Frame("no ctr".into()))?;
        let packet = frame["packet"].as_str().ok_or_else(|| LinkError::Frame("no packet".into()))?;
        let mac = frame["mac"].as_str().unwrap_or("");
        if !auth::equal(mac, &auth::frame_mac(&self.key, &self.session_id, ctr, packet)) {
            return Err(LinkError::Frame("mac does not verify".into()));
        }
        if self.last_ctr.is_some_and(|last| ctr <= last) {
            return Err(LinkError::Frame(format!("counter {ctr} did not increase")));
        }
        self.last_ctr = Some(ctr);
        let mut packet: Packet = serde_json::from_str(packet).map_err(|e| LinkError::Frame(e.to_string()))?;
        packet.kind = knoend_core::protocol::current_name(&packet.kind);
        Ok(packet)
    }
}
