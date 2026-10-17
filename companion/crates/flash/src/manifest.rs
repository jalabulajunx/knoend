//! The firmware release manifest (`manifest.json`), published next to the images.
//!
//! ```json
//! { "version": "0.2.0",
//!   "images": { "universal-esp32s3": { "chip": "esp32s3", "definition_offset": "0x310000", "parts": [
//!       { "offset": "0x0", "file": "universal-esp32s3-bootloader.bin", "sha256": "...", "size": 20928 } ] } },
//!   "boards": { "onx2432g028": { "name": "ONX2432G028", "image": "universal-esp32s3",
//!       "flash_size": "16MB", "bridges": ["ch340"], "status": "supported",
//!       "definition": { "file": "board-onx2432g028.json", "sha256": "...", "size": 517 } } } }
//! ```
//! Boards share one universal image; what makes each board work is its definition.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Chip, Port};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub images: BTreeMap<String, Image>,
    pub boards: BTreeMap<String, Board>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    pub chip: String,
    pub parts: Vec<Part>,
    /// Where the board definition goes (the "board" partition), as hex.
    #[serde(default)]
    pub definition_offset: Option<String>,
}

/// A board definition file (JSON), written into the image's "board" partition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Definition {
    pub file: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Part {
    /// Flash offset as hex ("0x10000").
    pub offset: String,
    pub file: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Board {
    pub name: String,
    pub image: String,
    #[serde(default)]
    pub flash_size: Option<String>,
    /// USB bridges this board is known to use ("ch340", "espressif-native", ...).
    #[serde(default)]
    pub bridges: Vec<String>,
    /// "supported" (run and confirmed), "unverified" (definition from documentation, not yet run on
    /// the board), or "coming" (no firmware or definition yet).
    pub status: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub definition: Option<Definition>,
}

/// A board the plugged-in hardware might be, best first.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub board: String,
    pub name: String,
    pub score: u32,
    pub reasons: Vec<String>,
    pub status: String,
}

impl Definition {
    /// The definition as a flashable part at `offset`.
    pub fn as_part(&self, offset: &str) -> Part {
        Part { offset: offset.to_string(), file: self.file.clone(), sha256: self.sha256.clone(), size: self.size }
    }
}

impl Part {
    pub fn offset(&self) -> Result<u32, String> {
        u32::from_str_radix(self.offset.trim_start_matches("0x"), 16).map_err(|_| format!("bad offset {}", self.offset))
    }

    /// Checks size and SHA-256 of downloaded bytes against the manifest.
    pub fn verify(&self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() as u64 != self.size {
            return Err(format!("{}: {} bytes, manifest says {}", self.file, bytes.len(), self.size));
        }
        let digest = hex::encode(Sha256::digest(bytes));
        if digest != self.sha256 {
            return Err(format!("{}: SHA-256 {digest} does not match the manifest", self.file));
        }
        Ok(())
    }
}

impl Manifest {
    /// Ranks the boards this hardware could be. An exact identity from running OpenPager firmware
    /// beats everything; otherwise chip, flash size and USB bridge decide, and the person confirms.
    pub fn candidates(&self, port: &Port, chip: Option<&Chip>, running_profile: Option<&str>) -> Vec<Candidate> {
        let mut out: Vec<Candidate> = Vec::new();
        for (id, board) in &self.boards {
            let image = self.images.get(&board.image);
            let mut score = 0;
            let mut reasons = Vec::new();
            if running_profile == Some(id.as_str()) {
                score += 100;
                reasons.push("OpenPager firmware on it says so".to_string());
            }
            if let (Some(chip), Some(image)) = (chip, image) {
                if chip.chip.replace('-', "").eq_ignore_ascii_case(&image.chip) {
                    score += 10;
                    reasons.push(format!("chip is {}", chip.chip));
                } else {
                    continue; // wrong chip: never offer it
                }
                if board.flash_size.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(&chip.flash_size)) {
                    score += 5;
                    reasons.push(format!("{} flash", chip.flash_size));
                }
            }
            if board.bridges.iter().any(|b| b == &port.bridge) {
                score += 5;
                reasons.push(format!("USB through {}", port.bridge));
            }
            if score > 0 {
                out.push(Candidate { board: id.clone(), name: board.name.clone(), score, reasons, status: board.status.clone() });
            }
        }
        // Best match first; on equal evidence, a board someone has run beats an unverified one.
        let rank = |status: &str| match status { "supported" => 0, "unverified" => 1, _ => 2 };
        out.sort_by(|a, b| b.score.cmp(&a.score).then(rank(&a.status).cmp(&rank(&b.status))).then(a.board.cmp(&b.board)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        serde_json::from_str(r#"{ "version": "0.2.0",
          "images": { "onx2432g028": { "chip": "esp32s3", "parts": [] },
                      "fnk0104b": { "chip": "esp32s3", "parts": [] },
                      "c3": { "chip": "esp32c3", "parts": [] } },
          "boards": { "onx2432g028": { "name": "ONX2432G028", "image": "onx2432g028", "flash_size": "16MB", "bridges": ["ch340"], "status": "supported" },
                      "fnk0104b": { "name": "Freenove FNK0104B", "image": "fnk0104b", "flash_size": "16MB", "bridges": ["espressif-native"], "status": "coming" },
                      "c3board": { "name": "Some C3", "image": "c3", "bridges": ["ch340"], "status": "supported" } } }"#).unwrap()
    }

    fn port(bridge: &str) -> Port {
        Port { name: "/dev/ttyUSB0".into(), vid: 0, pid: 0, manufacturer: None, product: None, bridge: bridge.into() }
    }

    fn s3() -> Chip {
        Chip { chip: "esp32s3".into(), revision: None, flash_size: "16MB".into(), features: vec![], mac: None }
    }

    #[test]
    fn ch340_s3_ranks_the_onx_first_and_never_offers_a_wrong_chip() {
        let c = manifest().candidates(&port("ch340"), Some(&s3()), None);
        assert_eq!(c[0].board, "onx2432g028");
        assert!(c.iter().all(|c| c.board != "c3board"), "an ESP32-C3 image must never be offered for an S3");
    }

    #[test]
    fn native_usb_ranks_the_fnk_first_and_running_identity_wins() {
        assert_eq!(manifest().candidates(&port("espressif-native"), Some(&s3()), None)[0].board, "fnk0104b");
        assert_eq!(manifest().candidates(&port("espressif-native"), Some(&s3()), Some("onx2432g028"))[0].board, "onx2432g028");
    }

    #[test]
    fn verify_checks_size_and_hash() {
        let bytes = b"firmware";
        let part = Part { offset: "0x10000".into(), file: "app.bin".into(), sha256: hex::encode(Sha256::digest(bytes)), size: 8 };
        assert!(part.verify(bytes).is_ok());
        assert!(part.verify(b"firmwarf").is_err());
        assert!(part.verify(b"firm").is_err());
        assert_eq!(part.offset().unwrap(), 0x10000);
    }
}
