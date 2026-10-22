//! What firmware a board already carries, read without writing anything: the partition table, each
//! app's descriptor (project name, version, ESP-IDF version, build date) and the component names in
//! the first part of each app. A vendor's firmware is built with the board's support package and
//! its display and touch drivers ("waveshare__esp32_s3_touch_amoled_2_06", "esp_lcd_sh8601"), so
//! those names say which board it is when the chip alone cannot (issue #63).
//!
//! Only the partition table (3 KB) and the first `APP_HEAD` bytes of each app are read: a few
//! seconds over USB. Data partitions (Wi-Fi settings in NVS, files) are never read.

use serde::Serialize;

/// Where ESP-IDF keeps the partition table.
pub const TABLE_OFFSET: u32 = 0x8000;
pub const TABLE_SIZE: u32 = 0xC00;
/// How much of each app is read for its descriptor and component names.
pub const APP_HEAD: u32 = 0x20000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Partition {
    pub name: String,
    /// "app" or "data".
    pub kind: String,
    /// "factory", "ota_0", "nvs", "spiffs", ... or the number.
    pub subtype: String,
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct App {
    pub partition: String,
    pub project: String,
    pub version: String,
    pub idf: String,
    pub built: String,
    /// Components named in the app: the board support package and drivers first.
    pub components: Vec<String>,
}

/// The firmware a board carries now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OnBoard {
    pub partitions: Vec<Partition>,
    pub apps: Vec<App>,
}

/// The partition table: 32-byte entries, magic 0xAA 0x50, ended by 0xFF.
pub fn parse_table(bytes: &[u8]) -> Vec<Partition> {
    bytes.chunks_exact(32).take_while(|e| e[0] == 0xAA && e[1] == 0x50).map(|e| {
        let (kind, subtype) = (e[2], e[3]);
        let subtype = match (kind, subtype) {
            (0, 0x00) => "factory".to_string(),
            (0, 0x10..=0x1F) => format!("ota_{}", subtype - 0x10),
            (0, 0x20) => "test".to_string(),
            (1, 0x00) => "ota".to_string(),
            (1, 0x01) => "phy".to_string(),
            (1, 0x02) => "nvs".to_string(),
            (1, 0x03) => "coredump".to_string(),
            (1, 0x81) => "fat".to_string(),
            (1, 0x82) => "spiffs".to_string(),
            (1, 0x83) => "littlefs".to_string(),
            _ => format!("{subtype:#04x}"),
        };
        Partition {
            name: String::from_utf8_lossy(&e[12..28]).trim_end_matches('\0').to_string(),
            kind: if kind == 0 { "app".into() } else { "data".into() },
            subtype,
            offset: u32::from_le_bytes([e[4], e[5], e[6], e[7]]),
            size: u32::from_le_bytes([e[8], e[9], e[10], e[11]]),
        }
    }).collect()
}

/// Whether `bytes` start an ESP app image (an empty slot reads 0xFF).
pub fn is_app_image(bytes: &[u8]) -> bool {
    bytes.first() == Some(&0xE9)
}

/// An app's descriptor and component names from the first part of its image.
pub fn parse_app(partition: &str, bytes: &[u8]) -> Option<App> {
    if !is_app_image(bytes) {
        return None;
    }
    // Image header (24 bytes), first segment header (8), then esp_app_desc_t.
    let desc = bytes.get(32..32 + 256)?;
    if desc[..4] != 0xABCD_5432u32.to_le_bytes() {
        return None;
    }
    let text = |from: usize, to: usize| String::from_utf8_lossy(&desc[from..to]).split('\0').next().unwrap_or("").trim().to_string();
    Some(App {
        partition: partition.to_string(),
        version: text(16, 48),
        project: text(48, 80),
        built: format!("{} {}", text(96, 112), text(80, 96)).trim().to_string(),
        idf: text(112, 144),
        components: components(bytes),
    })
}

/// Component names in an app: packages from the component registry ("waveshare__esp_lcd_sh8601",
/// from `managed_components/<name>/` in assert messages) and display and touch drivers
/// ("esp_lcd_st7789", "esp_lcd_touch_ft5x06"). At most 40, registry packages first.
pub fn components(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let name_at = |s: &str| -> String { s.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '.').collect() };
    let mut registry: Vec<String> = Vec::new();
    for (i, _) in text.match_indices("managed_components/") {
        let name = name_at(&text[i + "managed_components/".len()..]);
        if name.len() > 2 && !registry.contains(&name) {
            registry.push(name);
        }
    }
    // A driver is `esp_lcd_<chip>` or `esp_lcd_touch_<chip>` ("esp_lcd_touch_ft5x06_read_data" ->
    // "esp_lcd_touch_ft5x06"); the generic LCD layer's own words are not chips.
    const GENERIC: &[&str] = &["panel", "new", "rgb", "i80", "io", "spi", "i2c", "touch", "common", "dsi", "dpi", "register", "read", "get",
                               "set", "exit", "enter", "del", "init", "isr", "handle", "dev", "color", "cmd", "lock", "unlock", "ctrl"];
    let mut drivers: Vec<String> = Vec::new();
    for (i, _) in text.match_indices("esp_lcd_") {
        let name = name_at(&text[i..]);
        let mut words = name.trim_start_matches("esp_lcd_").split(['_', '.']);
        let driver = match words.next() {
            Some("touch") => words.next().filter(|w| !GENERIC.contains(w) && w.len() >= 3).map(|w| format!("esp_lcd_touch_{w}")),
            Some(w) if !GENERIC.contains(&w) && w.len() >= 3 => Some(format!("esp_lcd_{w}")),
            _ => None,
        };
        if let Some(d) = driver.filter(|d| !drivers.contains(d) && !registry.iter().any(|r| r.ends_with(d.as_str()))) {
            drivers.push(d);
        }
    }
    registry.into_iter().chain(drivers).take(40).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: u8, subtype: u8, offset: u32, size: u32, name: &str) -> Vec<u8> {
        let mut e = vec![0xAA, 0x50, kind, subtype];
        e.extend(offset.to_le_bytes());
        e.extend(size.to_le_bytes());
        let mut n = name.as_bytes().to_vec();
        n.resize(16, 0);
        e.extend(n);
        e.extend([0u8; 4]);
        e
    }

    #[test]
    fn the_partition_table_reads_like_gen_esp32part() {
        // The Waveshare ESP32-S3-Touch-AMOLED-2.06 as it came (from a backup, 2026-10-21).
        let mut table = [entry(1, 2, 0x9000, 0x6000, "nvs"), entry(1, 0x82, 0x12000, 0xEE000, "model"),
                         entry(0, 0, 0x100000, 0x900000, "factory"), entry(0, 0x11, 0x1000000, 0x600000, "ota_1")].concat();
        table.extend([0xFF; 64]);
        let parts = parse_table(&table);
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[1], Partition { name: "model".into(), kind: "data".into(), subtype: "spiffs".into(), offset: 0x12000, size: 0xEE000 });
        assert_eq!((parts[2].kind.as_str(), parts[2].subtype.as_str()), ("app", "factory"));
        assert_eq!(parts[3].subtype, "ota_1");
    }

    #[test]
    fn an_apps_descriptor_and_components() {
        let mut image = vec![0xE9];
        image.resize(32, 0);
        let mut desc = 0xABCD_5432u32.to_le_bytes().to_vec();
        desc.resize(256, 0);
        let put = |d: &mut Vec<u8>, at: usize, s: &str| d[at..at + s.len()].copy_from_slice(s.as_bytes());
        put(&mut desc, 16, "1.8.5");
        put(&mut desc, 48, "xiaozhi");
        put(&mut desc, 80, "10:25:46");
        put(&mut desc, 96, "Oct 31 2025");
        put(&mut desc, 112, "v5.5.1-dirty");
        image.extend(desc);
        image.extend(b"\0./managed_components/waveshare__esp32_s3_touch_amoled_2_06/esp32_s3_touch_amoled_2_06.c\0\
                       /./managed_components/waveshare__esp_lcd_sh8601/esp_lcd_sh8601.c\0esp_lcd_touch_new_i2c_ft5x06\0\
                       esp_lcd_touch_ft5x06_read_data\0esp_lcd_panel_reset\0esp_lcd_new_panel_io_spi\0");
        let app = parse_app("ota_0", &image).unwrap();
        assert_eq!((app.project.as_str(), app.version.as_str(), app.idf.as_str()), ("xiaozhi", "1.8.5", "v5.5.1-dirty"));
        assert_eq!(app.built, "Oct 31 2025 10:25:46");
        assert_eq!(app.components[..2], ["waveshare__esp32_s3_touch_amoled_2_06".to_string(), "waveshare__esp_lcd_sh8601".to_string()]);
        assert_eq!(app.components[2..], ["esp_lcd_touch_ft5x06".to_string()], "the touch driver once; the generic LCD layer and sh8601 (a package already) left out");
        assert!(parse_app("ota_1", &[0xFF; 64]).is_none(), "an empty slot");
    }
}
