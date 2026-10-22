//! Bluetooth LE data link (spec/protocol.md section 2): the KnoEnd data service beside the board's
//! HID keyboard, on the connection the computer already has with the board it is paired with as a
//! keyboard. Packets are `\n`-terminated JSON: written to "to board" in pieces that fit the
//! connection, and read from "from board" notifications. With btleplug (MIT/Apache-2.0): BlueZ on
//! Linux, CoreBluetooth on macOS, WinRT on Windows.
//!
//! The link is as trusted as the Bluetooth pairing: the board's characteristics need an encrypted
//! connection, which only a bonded computer has (spec section 4). A board is never disconnected
//! here: that would also cut off its keyboard.

use std::time::Duration;

use btleplug::api::{Central, Characteristic, Manager as _, Peripheral as _, RetrievePeripheralsOptions, ScanFilter, WriteType};
use btleplug::platform::{Manager, Peripheral};
use futures_util::stream::{Stream, StreamExt};
use knoend_core::protocol::Packet;
use serde_json::{json, Value};
use std::pin::Pin;
use uuid::Uuid;

pub const SERVICE: Uuid = Uuid::from_u128(0xf0e35c51_f991_4cdb_b330_97d0615e0001);
pub const TO_BOARD: Uuid = Uuid::from_u128(0xf0e35c51_f991_4cdb_b330_97d0615e0002);
pub const FROM_BOARD: Uuid = Uuid::from_u128(0xf0e35c51_f991_4cdb_b330_97d0615e0003);

/// The board's Bluetooth name, as it advertises and pairs (firmware DEVICE_NAME).
const BOARD_NAME: &str = "KnoEnd";

pub struct BleSender {
    peripheral: Peripheral,
    to_board: Characteristic,
    next_id: u32,
}

pub struct BleReceiver {
    notifications: Pin<Box<dyn Stream<Item = btleplug::api::ValueNotification> + Send>>,
    buffer: Vec<u8>,
}

/// A board reached over Bluetooth: its id (from its identity), the identity, and the link.
pub struct BleBoard {
    pub device_id: String,
    pub identity: Value,
    pub sender: BleSender,
    pub receiver: BleReceiver,
}

impl BleSender {
    /// Sends one packet; returns its id.
    pub async fn send(&mut self, kind: &str, body: Value) -> Result<u32, String> {
        let id = self.next_id;
        self.next_id += 1;
        let mut text = serde_json::to_vec(&json!({"id": id, "type": kind, "body": body})).map_err(|e| e.to_string())?;
        text.push(b'\n');
        // Each write fits the connection's MTU less the 3-byte header; acknowledged writes, so a
        // piece is never dropped when the board's queue is full.
        let piece = usize::from(self.peripheral.mtu().max(23) - 3).clamp(20, 244);
        for chunk in text.chunks(piece) {
            self.peripheral.write(&self.to_board, chunk, WriteType::WithResponse).await.map_err(|e| e.to_string())?;
        }
        Ok(id)
    }
}

impl BleReceiver {
    /// The next packet from the board; an error when the link has gone.
    pub async fn recv(&mut self) -> Result<Packet, String> {
        loop {
            if let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).collect();
                let text = String::from_utf8_lossy(&line[..line.len() - 1]).trim().to_string();
                if !text.starts_with('{') {
                    continue;
                }
                match serde_json::from_str::<Packet>(&text) {
                    Ok(mut packet) => {
                        packet.kind = knoend_core::protocol::current_name(&packet.kind);
                        return Ok(packet);
                    }
                    Err(_) => continue,
                }
            }
            match self.notifications.next().await {
                Some(n) if n.uuid == FROM_BOARD => self.buffer.extend_from_slice(&n.value),
                Some(_) => {}
                None => return Err("the Bluetooth link closed".into()),
            }
            if self.buffer.len() > 64 * 1024 {
                self.buffer.clear(); // a line that never ends: start again
            }
        }
    }
}

/// Finds KnoEnd boards over Bluetooth and opens the data link to each one in `wanted` (board ids):
/// boards the computer is connected to (paired as a keyboard), and any advertising nearby within
/// `scan`. Boards whose identity names another id are left as they are.
pub async fn connect(wanted: &[String], scan: Duration) -> Result<Vec<BleBoard>, String> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let manager = Manager::new().await.map_err(|e| format!("Bluetooth: {e}"))?;
    let adapter = manager.adapters().await.map_err(|e| format!("Bluetooth: {e}"))?
        .into_iter().next().ok_or("no Bluetooth adapter on this computer")?;
    // Connected boards (a paired keyboard does not advertise), then what a short scan finds.
    let mut candidates: Vec<Peripheral> = adapter
        .retrieve_peripherals(RetrievePeripheralsOptions { services: Some(vec![SERVICE]), ..Default::default() })
        .await.unwrap_or_default();
    if adapter.start_scan(ScanFilter::default()).await.is_ok() {
        tokio::time::sleep(scan).await;
        let _ = adapter.stop_scan().await;
    }
    for p in adapter.peripherals().await.unwrap_or_default() {
        let named = p.properties().await.ok().flatten()
            .is_some_and(|props| props.local_name.as_deref().is_some_and(|n| n.starts_with(BOARD_NAME)) || props.services.contains(&SERVICE));
        if named && !candidates.iter().any(|c| c.id() == p.id()) {
            candidates.push(p);
        }
    }
    let mut boards = Vec::new();
    for peripheral in candidates {
        match open(peripheral).await {
            Ok(board) if wanted.contains(&board.device_id) => boards.push(board),
            Ok(_) | Err(_) => {}
        }
    }
    Ok(boards)
}

/// Opens the data link on one peripheral: connect (a no-op when it is connected already), find the
/// service, subscribe, and wait for the identity the board sends when a session starts.
async fn open(peripheral: Peripheral) -> Result<BleBoard, String> {
    if !peripheral.is_connected().await.unwrap_or(false) {
        tokio::time::timeout(Duration::from_secs(8), peripheral.connect()).await
            .map_err(|_| "connect timed out".to_string())?.map_err(|e| e.to_string())?;
    }
    tokio::time::timeout(Duration::from_secs(8), peripheral.discover_services()).await
        .map_err(|_| "service discovery timed out".to_string())?.map_err(|e| e.to_string())?;
    let characteristics = peripheral.characteristics();
    let find = |uuid: Uuid| characteristics.iter().find(|c| c.uuid == uuid && c.service_uuid == SERVICE).cloned();
    let (Some(to_board), Some(from_board)) = (find(TO_BOARD), find(FROM_BOARD)) else {
        return Err("no KnoEnd data service (firmware without Bluetooth data)".into());
    };
    let notifications = peripheral.notifications().await.map_err(|e| e.to_string())?;
    // Subscribing starts the board's session: it sends its identity.
    peripheral.subscribe(&from_board).await.map_err(|e| e.to_string())?;
    let mut receiver = BleReceiver { notifications, buffer: Vec::new() };
    let mut sender = BleSender { peripheral, to_board, next_id: 0 };
    let identity = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let packet = receiver.recv().await?;
            if packet.kind == "knoend.identity" {
                return Ok::<_, String>(packet.body);
            }
        }
    }).await;
    let identity = match identity {
        Ok(Ok(identity)) => identity,
        // Subscribed after the board's session began (it was subscribed before): ask.
        _ => {
            sender.send("knoend.identity", json!({"deviceType": "companion"})).await?;
            tokio::time::timeout(Duration::from_secs(6), async {
                loop {
                    let packet = receiver.recv().await?;
                    if packet.kind == "knoend.identity" {
                        return Ok::<_, String>(packet.body);
                    }
                }
            }).await.map_err(|_| "the board sent no identity over Bluetooth".to_string())??
        }
    };
    let device_id = identity["deviceId"].as_str().ok_or("identity without deviceId")?.to_string();
    Ok(BleBoard { device_id, identity, sender, receiver })
}
