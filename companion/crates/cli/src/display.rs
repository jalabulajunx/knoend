//! How the person wants the board: the key size, the theme and which way it sits, chosen at setup
//! and changeable on the dashboard. Kept in `~/.config/knoend/display.json` (one board per
//! computer for now, #15). The theme and turn are sent to the board (`knoend.display`,
//! spec/protocol.md 3.6d), which keeps them; the key size stays here.
//!
//! Key size is the grid every page is shown in: a layout's keys are packed into pages of that grid,
//! in order (`layout::packed`), so a 3x3 layout fills a 4x4 page and a 4x4 one spreads over 3x3
//! pages. Edit mode shows layouts as they were made; agent screens keep their own grid.

use serde::{Deserialize, Serialize};

use knoend_core::protocol::Grid;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeySize {
    /// Up to 2x2: four big keys.
    Big,
    /// Up to 3x3. The default: on a 2.8-inch screen 4x4 keys are about 80x60 pixels.
    #[default]
    Medium,
    /// Up to 4x4: as many keys as the layout asks for.
    Small,
}

impl KeySize {
    pub fn largest(self) -> Grid {
        match self {
            KeySize::Big => Grid { cols: 2, rows: 2 },
            KeySize::Medium => Grid::DEFAULT,
            KeySize::Small => Grid::LARGEST,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Light text on dark keys, in colour.
    #[default]
    Dark,
    /// An e-ink look: black on white, high contrast; pictures are sent in grey.
    #[serde(alias = "paper")]
    White,
    /// An e-ink look on warm paper: dark brown ink; pictures are sent in sepia tones.
    Sepia,
}

impl Theme {
    /// The theme a board's identity or answer names (`board.display.theme`).
    pub fn named(name: &str) -> Theme {
        match name {
            "white" | "paper" => Theme::White,
            "sepia" => Theme::Sepia,
            _ => Theme::Dark,
        }
    }

    /// Puts pictures in this theme's tones, in place, keeping alpha: grey for White, sepia for
    /// Sepia, as they are for Dark.
    pub fn tone(self, rgba: &mut [u8]) {
        if self == Theme::Dark {
            return;
        }
        for pixel in rgba.chunks_exact_mut(4) {
            let grey = (u32::from(pixel[0]) * 299 + u32::from(pixel[1]) * 587 + u32::from(pixel[2]) * 114) / 1000;
            let [r, g, b] = match self {
                // Ink to paper: 0x2E241B at black, 0xF4ECD8 at white.
                Theme::Sepia => [(0x2E, 0xF4), (0x24, 0xEC), (0x1B, 0xD8)].map(|(dark, light): (u32, u32)| (dark + (light - dark) * grey / 255) as u8),
                _ => [grey as u8; 3],
            };
            pixel[..3].copy_from_slice(&[r, g, b]);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Display {
    #[serde(default)]
    pub key_size: KeySize,
    #[serde(default)]
    pub theme: Theme,
    /// Quarter turns clockwise from the board's usual landscape (0-3).
    #[serde(default)]
    pub turn: u8,
}

impl Display {
    /// The `knoend.display` body for the board.
    pub fn look(&self) -> serde_json::Value {
        serde_json::json!({"theme": self.theme, "turn": self.turn})
    }

    /// Whether the board's identity (`board.display`) already shows this look. None: the board's
    /// firmware does not know `knoend.display`.
    pub fn shown_by(&self, identity: &serde_json::Value) -> Option<bool> {
        let shown = identity["board"]["display"].as_object()?;
        Some(shown.get("theme").and_then(|t| t.as_str()) == self.look()["theme"].as_str()
             && shown.get("turn").and_then(|t| t.as_u64()) == Some(u64::from(self.turn)))
    }
}

fn path() -> std::path::PathBuf {
    knoend_transport::pairing::config_dir().join("display.json")
}

/// The saved choice, or the defaults.
pub fn load() -> Display {
    std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save(display: &Display) -> Result<(), String> {
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(display).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())
}

/// The grid every page is shown in: the key size's, within what the board draws.
pub fn shown_grid(board: Grid, key_size: KeySize) -> Grid {
    key_size.largest().within(board)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_size_caps_the_grid_within_the_board() {
        assert_eq!(shown_grid(Grid::LARGEST, KeySize::default()), Grid::DEFAULT, "medium by default");
        assert_eq!(shown_grid(Grid::LARGEST, KeySize::Small), Grid::LARGEST);
        assert_eq!(shown_grid(Grid::DEFAULT, KeySize::Small), Grid::DEFAULT, "never more than the board draws");
        assert_eq!(shown_grid(Grid::LARGEST, KeySize::Big), Grid { cols: 2, rows: 2 });
        assert_eq!(serde_json::from_str::<Display>("{}").unwrap().key_size, KeySize::Medium);
        assert_eq!(serde_json::to_value(Display { key_size: KeySize::Big, ..Default::default() }).unwrap(),
                   serde_json::json!({"key_size": "big", "theme": "dark", "turn": 0}));
    }

    #[test]
    fn the_look_is_compared_with_what_the_board_says() {
        let sepia = Display { theme: Theme::Sepia, ..Default::default() };
        assert_eq!(sepia.look(), serde_json::json!({"theme": "sepia", "turn": 0}));
        let board = |theme: &str| serde_json::json!({"board": {"display": {"theme": theme, "turn": 0}}});
        assert_eq!(sepia.shown_by(&board("sepia")), Some(true));
        assert_eq!(sepia.shown_by(&board("dark")), Some(false));
        assert_eq!(sepia.shown_by(&serde_json::json!({"board": {}})), None, "older firmware");
        assert_eq!(serde_json::from_str::<Display>(r#"{"theme": "paper"}"#).unwrap().theme, Theme::White, "saved before White had its name");
    }

    #[test]
    fn pictures_take_the_theme_tones() {
        let tone = |theme: Theme, rgba: [u8; 4]| { let mut p = rgba.to_vec(); theme.tone(&mut p); p };
        assert_eq!(tone(Theme::Dark, [255, 0, 0, 200]), vec![255, 0, 0, 200]);
        assert_eq!(tone(Theme::White, [255, 0, 0, 200]), vec![76, 76, 76, 200]);
        assert_eq!(tone(Theme::Sepia, [255, 255, 255, 255]), vec![0xF4, 0xEC, 0xD8, 255], "white is the paper");
        assert_eq!(tone(Theme::Sepia, [0, 0, 0, 9]), vec![0x2E, 0x24, 0x1B, 9], "black is the ink");
    }
}
