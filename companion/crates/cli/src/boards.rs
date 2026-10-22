//! What the service makes of a board plugged in by USB (issue #63): whether it is one KnoEnd knows,
//! and when it is not, the evidence for the setup assistant (chip, flash, the firmware it carries
//! and that firmware's components). The last identification is kept for the assistant's snapshot,
//! and an unknown board is raised as a problem, so the always-listening assistant looks at it
//! without the person asking (watch.rs).

use std::sync::Mutex;

use serde_json::{json, Value};

/// What KnoEnd's universal firmware can drive (firmware/components/board_generic/board_generic.c):
/// a board needs one of each.
pub const DISPLAYS: &[&str] = &["st7789 (SPI LCD)", "ili9341 (SPI LCD)"];
pub const TOUCH: &[&str] = &["gt911", "cst816", "cst826", "ft6336"];

static LAST: Mutex<Option<Value>> = Mutex::new(None);

/// The last board identified over USB and what the service made of it.
pub fn last() -> Option<Value> {
    LAST.lock().ok().and_then(|l| l.clone())
}

/// Whether the suggestions name the board: KnoEnd firmware on it says so, or flash and USB bridge
/// match a board we list. Chip alone (every board with that chip) is not knowing it.
pub fn known(candidates: &[knoend_flash::manifest::Candidate]) -> bool {
    candidates.iter().any(|c| c.confident)
}

/// One line on the firmware a board carries ("factory: phone_s3_box_3 v0.4.2 (ESP-IDF v5.5.1),
/// components waveshare__esp32_s3_touch_amoled_2_06, ...").
fn firmware_line(chip: &knoend_flash::Chip) -> String {
    let Some(firmware) = &chip.firmware else { return "its firmware could not be read".into() };
    if firmware.apps.is_empty() {
        return "no app in its flash (erased, or not ESP-IDF)".into();
    }
    firmware.apps.iter().map(|a| {
        let components = if a.components.is_empty() { String::new() } else { format!(", components {}", a.components.join(", ")) };
        format!("{}: {} {} (ESP-IDF {}){components}", a.partition, a.project, a.version, a.idf)
    }).collect::<Vec<_>>().join("; ")
}

/// Records an identification and returns what it means, for the web app and `knoend devices`:
/// `{"known", "summary", "evidence"}`. With `raise`, an unknown board is raised for the setup
/// assistant (watch.rs); without, the caller asks it (the web app's AI-guided set-up) or is it.
pub fn record(port: &knoend_flash::Port, chip: Option<&knoend_flash::Chip>, candidates: &[knoend_flash::manifest::Candidate],
              raise: bool, running: Option<&Value>) -> Value {
    // KnoEnd firmware on it names its board: known, even when this firmware release does not list
    // that board yet (a build newer than the release). Nothing needs installing.
    let profile = running.and_then(|r| r["board"]["profile"].as_str()).filter(|p| !p.is_empty() && *p != "unknown");
    let listed = profile.is_some_and(|p| candidates.iter().any(|c| c.board == p));
    let known = known(candidates) || profile.is_some();
    let evidence = json!({
        "port": port.name, "usb": format!("{:04x}:{:04x}", port.vid, port.pid), "bridge": port.bridge, "product": port.product,
        "chip": chip.map(|c| &c.chip), "flash": chip.map(|c| &c.flash_size), "features": chip.map(|c| &c.features),
        "firmware_on_board": chip.and_then(|c| c.firmware.as_ref()),
        "suggestions": candidates,
        "knoend_firmware_drives": {"displays": DISPLAYS, "touch": TOUCH},
    });
    let summary = match chip {
        _ if profile.is_some() && !listed => format!(
            "{} on {}, already running KnoEnd {}: nothing to install (skip install). The firmware release this computer installs \
             from does not list this board, so installing from it would replace the firmware with one that cannot drive it",
            profile.unwrap_or_default(), port.name, running.and_then(|r| r["firmware"].as_str()).unwrap_or("?")),
        _ if known && candidates.is_empty() => format!("{} on {}", profile.unwrap_or("?"), port.name),
        _ if known => format!("{} on {}", candidates[0].name, port.name),
        None => format!("A board on {} whose chip did not answer", port.name),
        Some(c) => format!("A board KnoEnd does not know on {}: {} with {} flash; {}", port.name, c.chip, c.flash_size, firmware_line(c)),
    };
    let verdict = json!({"known": known, "summary": summary, "evidence": evidence, "running_unlisted": profile.is_some() && !listed});
    if let Ok(mut last) = LAST.lock() {
        *last = Some(verdict.clone());
    }
    if raise && !known && chip.is_some() {
        problem!("unknown-board", "{summary}. Not one KnoEnd lists; flashing KnoEnd on it is not safe to suggest until it is identified.");
    }
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;
    use knoend_flash::on_board::{App, OnBoard};

    #[test]
    fn an_unknown_board_is_said_plainly_with_its_firmware() {
        let port = knoend_flash::Port { name: "/dev/ttyACM0".into(), vid: 0x303a, pid: 0x1001, manufacturer: None, product: None,
                                         serial: Some("28:84:85:90:BE:B8".into()), bridge: "espressif-native".into() };
        assert_eq!(port.mac_id().as_deref(), Some("28848590beb8"));
        let firmware = OnBoard { partitions: vec![], apps: vec![App { partition: "factory".into(), project: "phone_s3_box_3".into(), version: "v0.4.2".into(),
            idf: "v5.5.1".into(), built: String::new(), components: vec!["waveshare__esp32_s3_touch_amoled_2_06".into(), "waveshare__esp_lcd_sh8601".into()] }] };
        let chip = knoend_flash::Chip { chip: "esp32s3".into(), revision: None, flash_size: "32MB".into(), features: vec![], mac: None, firmware: Some(firmware) };
        let verdict = record(&port, Some(&chip), &[], false, None);
        assert_eq!(verdict["known"], false);
        let summary = verdict["summary"].as_str().unwrap();
        assert!(summary.starts_with("A board KnoEnd does not know on /dev/ttyACM0: esp32s3 with 32MB flash"), "{summary}");
        assert!(summary.contains("factory: phone_s3_box_3 v0.4.2 (ESP-IDF v5.5.1), components waveshare__esp32_s3_touch_amoled_2_06"));
        assert_eq!(verdict["evidence"]["knoend_firmware_drives"]["touch"][0], "gt911");
        assert_eq!(last().unwrap()["summary"], verdict["summary"]);
        // The same board once it runs KnoEnd from a build newer than the release.
        let running = json!({"firmware": "v0.2.0-38", "board": {"profile": "waveshare_s3_touch_amoled_2_06"}});
        let verdict = record(&port, Some(&chip), &[], false, Some(&running));
        assert_eq!((verdict["known"].as_bool(), verdict["running_unlisted"].as_bool()), (Some(true), Some(true)));
        assert!(verdict["summary"].as_str().unwrap().contains("already running KnoEnd v0.2.0-38: nothing to install (skip install)"));
    }
}
