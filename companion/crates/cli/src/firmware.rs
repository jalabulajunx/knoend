//! Where firmware comes from: a release manifest and its parts, from a local folder or a URL
//! (default: the latest GitHub release). Every part is checked against its SHA-256 before use.

use openpager_flash::manifest::{Manifest, Part};

/// Size of the firmware's "board" partition (firmware/partitions.csv).
const DEFINITION_PARTITION_SIZE: usize = 0x1000;

pub const DEFAULT_SOURCE: &str = "https://github.com/jalabulajunx/openpager/releases/latest/download";

async fn fetch(source: &str, file: &str) -> Result<Vec<u8>, String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let url = format!("{}/{file}", source.trim_end_matches('/'));
        let response = reqwest::get(&url).await.map_err(|e| format!("{url}: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("{url}: {}", response.status()));
        }
        Ok(response.bytes().await.map_err(|e| e.to_string())?.to_vec())
    } else {
        std::fs::read(std::path::Path::new(source).join(file)).map_err(|e| format!("{source}/{file}: {e}"))
    }
}

pub async fn manifest(source: &str) -> Result<Manifest, String> {
    let bytes = fetch(source, "manifest.json").await?;
    serde_json::from_slice(&bytes).map_err(|e| format!("manifest.json: {e}"))
}

/// Downloads and verifies every part of a board's image: (offset, bytes) ready to flash.
pub async fn parts_for(source: &str, manifest: &Manifest, board: &str) -> Result<Vec<(u32, Vec<u8>)>, String> {
    let entry = manifest.boards.get(board).ok_or_else(|| format!("board {board} is not in the manifest"))?;
    if entry.status != "supported" {
        return Err(format!("{} has no firmware yet", entry.name));
    }
    let image = manifest.images.get(&entry.image).ok_or_else(|| format!("image {} missing", entry.image))?;
    let mut out = Vec::new();
    for part in &image.parts {
        out.push(load_part(source, part).await?);
    }
    // The board definition tells the universal image what hardware it is running on.
    if let (Some(definition), Some(offset)) = (&entry.definition, &image.definition_offset) {
        let (offset, mut bytes) = load_part(source, &definition.as_part(offset)).await?;
        // Fill the whole 4 KB partition (0xFF = erased; the firmware stops reading there). Written
        // as a short part, the tail of the definition did not reach the flash on the ONX (the last
        // 5 of 517 bytes were missing and verification failed).
        bytes.resize(DEFINITION_PARTITION_SIZE.max(bytes.len()), 0xFF);
        out.push((offset, bytes));
    } else if image.definition_offset.is_some() {
        return Err(format!("{} has no board definition in this release", entry.name));
    }
    Ok(out)
}

async fn load_part(source: &str, part: &Part) -> Result<(u32, Vec<u8>), String> {
    let bytes = fetch(source, &part.file).await?;
    part.verify(&bytes)?;
    Ok((part.offset()?, bytes))
}
