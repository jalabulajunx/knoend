//! How the companion reaches a board: Wi-Fi (mDNS discovery, handshake, authenticated frames)
//! and USB serial (pairing). Wire formats are in spec/protocol.md sections 2 and 4.

pub mod auth;
pub mod discover;
pub mod pairing;
pub mod serial;
pub mod wifi;
