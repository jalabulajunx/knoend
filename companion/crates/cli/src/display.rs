//! How the person wants the board: the key size, the theme and which way it sits, chosen at setup
//! and changeable on the dashboard. Kept in `~/.config/knoend/display.json` (one board per
//! computer for now, #15). The theme and turn are sent to the board (`knoend.display`,
//! spec/protocol.md 3.6d), which keeps them; the key size stays here.
//!
//! Key size caps the grid: a layout page with a bigger grid is split onto pages of the capped size
//! (`layout::pages`), so nothing is lost, only spread out. Agent screens keep their own grid.

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
    Paper,
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

/// Converts RGBA pixels to grey in place (Paper theme), keeping alpha.
pub fn to_grey(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let grey = ((u32::from(pixel[0]) * 299 + u32::from(pixel[1]) * 587 + u32::from(pixel[2]) * 114) / 1000) as u8;
        pixel[..3].fill(grey);
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

/// The grid layouts are shown in: the key size's, within what the board draws.
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
        let paper = Display { theme: Theme::Paper, ..Default::default() };
        assert_eq!(paper.look(), serde_json::json!({"theme": "paper", "turn": 0}));
        let board = |theme: &str| serde_json::json!({"board": {"display": {"theme": theme, "turn": 0}}});
        assert_eq!(paper.shown_by(&board("paper")), Some(true));
        assert_eq!(paper.shown_by(&board("dark")), Some(false));
        assert_eq!(paper.shown_by(&serde_json::json!({"board": {}})), None, "older firmware");
        let mut pixels = vec![255, 0, 0, 200];
        to_grey(&mut pixels);
        assert_eq!(pixels, vec![76, 76, 76, 200]);
    }
}
