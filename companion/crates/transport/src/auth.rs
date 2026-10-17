//! The Wi-Fi handshake proofs and frame MACs (spec/protocol.md sections 4.2 and 4.3):
//! HMAC-SHA256 over newline-joined fields, written as lowercase hex.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

pub const KEY_BYTES: usize = 32;
pub type Key = [u8; KEY_BYTES];

fn hmac_hex(key: &Key, parts: &[&str]) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(parts.join("\n").as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// What the board sends to prove it holds `key`: HMAC(K, "board\n" + Nc + "\n" + Nb).
pub fn board_proof(key: &Key, companion_nonce: &str, board_nonce: &str) -> String {
    hmac_hex(key, &["board", companion_nonce, board_nonce])
}

/// What the companion sends back: HMAC(K, "companion\n" + Nb + "\n" + Nc).
pub fn companion_proof(key: &Key, board_nonce: &str, companion_nonce: &str) -> String {
    hmac_hex(key, &["companion", board_nonce, companion_nonce])
}

/// MAC for one frame: HMAC(K, S + "\n" + ctr + "\n" + packet), S = Nb + Nc.
pub fn frame_mac(key: &Key, session_id: &str, ctr: u64, packet: &str) -> String {
    hmac_hex(key, &[session_id, &ctr.to_string(), packet])
}

/// Constant-time comparison of two hex strings.
pub fn equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// 16 random bytes as 32 hex digits.
pub fn nonce() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS random source");
    hex::encode(bytes)
}

/// A fresh pairing key from the OS random source.
pub fn new_key() -> Key {
    let mut key = [0u8; KEY_BYTES];
    getrandom::fill(&mut key).expect("OS random source");
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// spec/examples/wifi-session.json was produced by the Python tools; this implementation must
    /// reproduce every proof and MAC in it exactly.
    #[test]
    fn reproduces_the_spec_test_vector() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../spec/examples/wifi-session.json");
        let session: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let key: Key = hex::decode(session["key"].as_str().unwrap()).unwrap().try_into().unwrap();
        let messages = session["messages"].as_array().unwrap();
        let nc = messages[0]["message"]["hello"]["nonce"].as_str().unwrap();
        let nb = messages[1]["message"]["hello"]["nonce"].as_str().unwrap();

        assert_eq!(board_proof(&key, nc, nb), messages[1]["message"]["hello"]["proof"]);
        assert_eq!(companion_proof(&key, nb, nc), messages[2]["message"]["proof"]);
        let session_id = format!("{nb}{nc}");
        let mut frames = 0;
        for item in &messages[3..] {
            let frame = &item["message"];
            let mac = frame_mac(&key, &session_id, frame["ctr"].as_u64().unwrap(), frame["packet"].as_str().unwrap());
            assert_eq!(mac, frame["mac"]);
            frames += 1;
        }
        assert_eq!(frames, 3);
    }

    #[test]
    fn compares_and_generates() {
        assert!(equal("abc", "abc"));
        assert!(!equal("abc", "abd"));
        assert!(!equal("abc", "ab"));
        assert_eq!(nonce().len(), 32);
        assert_ne!(new_key(), new_key());
    }
}
