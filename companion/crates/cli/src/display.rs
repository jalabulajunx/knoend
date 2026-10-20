//! How the person wants the board's keys: the key size, chosen at setup and changeable on the
//! dashboard. Kept in `~/.config/knoend/display.json` (one board per computer for now, #15).
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Display {
    #[serde(default)]
    pub key_size: KeySize,
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
        assert_eq!(serde_json::to_string(&Display { key_size: KeySize::Big }).unwrap(), r#"{"key_size":"big"}"#);
    }
}
