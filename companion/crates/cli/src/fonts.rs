//! Fonts the person chooses from Google Fonts, for the board's key labels and its start-up name
//! (spec/protocol.md 3.6e). The service finds the family in Google's public catalogue, asks Google
//! for it trimmed to the characters the board shows (`text=`), as TrueType, and sends it to the
//! board, which keeps it and draws with it. Nothing needs a Google account or key.
//!
//! Nothing is stored here: the catalogue is fetched live (and held in memory a few minutes, so a
//! search does not refetch it), and a font is downloaded when the person picks it and sent straight
//! to the board, which keeps it.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Google's catalogue of families (public; about 2.7 MB).
const CATALOGUE: &str = "https://fonts.google.com/metadata/fonts";
/// How long the catalogue is held in memory.
const CATALOGUE_AGE: Duration = Duration::from_secs(10 * 60);

/// What the board may draw: printable ASCII and a few marks used in labels and the start-up name.
pub fn characters() -> String {
    let mut text: String = (0x20u8..0x7f).map(char::from).collect();
    text.push_str("°…–—‘’“”•×");
    text
}

fn client() -> Result<reqwest::Client, String> {
    // No browser user agent: Google then answers with TrueType, which the board draws.
    reqwest::Client::builder().timeout(Duration::from_secs(30)).user_agent("knoend").build().map_err(|e| e.to_string())
}

/// The families with Latin letters, `[{"family", "category"}]`, live from Google (held in memory
/// for a few minutes).
pub async fn catalogue() -> Result<Vec<Value>, String> {
    static HELD: Mutex<Option<(Instant, Vec<Value>)>> = Mutex::new(None);
    if let Some((at, list)) = HELD.lock().ok().and_then(|h| h.clone()) {
        if at.elapsed() < CATALOGUE_AGE {
            return Ok(list);
        }
    }
    let text = client()?.get(CATALOGUE).send().await.map_err(|e| format!("could not reach Google Fonts: {e}"))?
        .text().await.map_err(|e| e.to_string())?;
    let start = text.find('{').ok_or("Google Fonts sent no catalogue")?;
    let data: Value = serde_json::from_str(&text[start..]).map_err(|e| format!("Google Fonts catalogue: {e}"))?;
    let list: Vec<Value> = data["familyMetadataList"].as_array().into_iter().flatten()
        .filter(|f| f["subsets"].as_array().is_some_and(|s| s.iter().any(|s| s == "latin")))
        .filter_map(|f| Some(json!({"family": f["family"].as_str()?, "category": f["category"]})))
        .collect();
    if let Ok(mut held) = HELD.lock() {
        *held = Some((Instant::now(), list.clone()));
    }
    Ok(list)
}

/// A family name as Google writes it: letters, digits, spaces.
pub fn valid_family(family: &str) -> bool {
    !family.trim().is_empty() && family.len() <= 64 && family.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ')
}

/// The TrueType address in Google's stylesheet answer.
fn truetype_url(css: &str) -> Option<String> {
    let start = css.find("url(")? + 4;
    let end = start + css[start..].find(')')?;
    let url = css[start..end].trim_matches(['\'', '"']);
    (css[end..].contains("truetype") && url.starts_with("https://fonts.gstatic.com/")).then(|| url.to_string())
}

/// The family's font, trimmed to `characters()`, as TrueType, downloaded from Google now.
pub async fn fetch(family: &str) -> Result<Vec<u8>, String> {
    if !valid_family(family) {
        return Err(format!("\"{family}\" is not a Google Fonts family name"));
    }
    let client = client()?;
    let url = reqwest::Url::parse_with_params("https://fonts.googleapis.com/css2",
                                              &[("family", family), ("text", characters().as_str())]).map_err(|e| e.to_string())?;
    let css = client.get(url)
        .send().await.map_err(|e| format!("could not reach Google Fonts: {e}"))?;
    if !css.status().is_success() {
        return Err(format!("Google Fonts has no family called \"{family}\" ({})", css.status()));
    }
    let css = css.text().await.map_err(|e| e.to_string())?;
    let url = truetype_url(&css).ok_or("Google Fonts did not offer this family as TrueType")?;
    let bytes = client.get(&url).send().await.map_err(|e| format!("could not download the font: {e}"))?
        .bytes().await.map_err(|e| e.to_string())?.to_vec();
    if bytes.len() < 12 || !(bytes.starts_with(&[0, 1, 0, 0]) || bytes.starts_with(b"true")) {
        return Err("the download is not a TrueType font".into());
    }
    Ok(bytes)
}

/// CRC-32 (IEEE), as the board checks a font it received.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_truetype_address_is_read_from_googles_answer() {
        let css = "@font-face {\n  font-family: 'X';\n  src: url(https://fonts.gstatic.com/l/font?kit=abc&v=v4) format('truetype');\n}";
        assert_eq!(truetype_url(css).as_deref(), Some("https://fonts.gstatic.com/l/font?kit=abc&v=v4"));
        assert_eq!(truetype_url("src: url(https://fonts.gstatic.com/x.woff2) format('woff2');"), None);
        assert_eq!(truetype_url("src: url(https://evil.example/x.ttf) format('truetype');"), None);
    }

    #[test]
    fn names_and_checksums() {
        assert!(valid_family("Edu AU VIC WA NT Dots"));
        assert!(!valid_family("x&text=y") && !valid_family(""));
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926, "the standard check value");
        assert!(characters().contains('°') && characters().contains('~'));
    }
}
