//! Packets (spec/protocol.md sections 1 and 3). Bodies the companion builds are typed; anything
//! received is kept as JSON and inspected by type.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Message names travel as `knoend.<name>`. Boards and companions from before the rename (v0.1.x,
/// "OpenPager") use `openpager.<name>`; both are accepted (spec/protocol.md section 2).
pub const PREFIX: &str = "knoend.";
/// The prefix from before the rename.
pub const OLD_PREFIX: &str = "openpager.";

/// A received message name in the current form: `openpager.key` -> `knoend.key`.
pub fn current_name(kind: &str) -> String {
    match kind.strip_prefix(OLD_PREFIX) {
        Some(name) => format!("{PREFIX}{name}"),
        None => kind.to_string(),
    }
}

/// A message name for a board from before the rename: `knoend.layout` -> `openpager.layout`.
pub fn old_name(kind: &str) -> String {
    match kind.strip_prefix(PREFIX) {
        Some(name) => format!("{OLD_PREFIX}{name}"),
        None => kind.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Packet {
    pub id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub re: Option<u32>,
    #[serde(rename = "type")]
    pub kind: String,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Send {
    Hid { modifiers: u8, usage: u8 },
    Event(bool),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutKey {
    pub key: u8,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speak: Option<String>,
    /// A Material Icons name from `spec/icons.txt`, drawn above a small label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirm: bool,
    pub send: Send,
}

/// Columns and rows of keys (1-4 each). Slots run 1 to cols x rows, left to right, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    pub cols: u8,
    pub rows: u8,
}

impl Grid {
    /// What a layout without a grid gets.
    pub const DEFAULT: Grid = Grid { cols: 3, rows: 3 };
    /// The largest grid the protocol allows.
    pub const LARGEST: Grid = Grid { cols: 4, rows: 4 };

    /// "2x3" (cols x rows), each 1-4.
    pub fn parse(text: &str) -> Option<Grid> {
        let (cols, rows) = text.split_once('x')?;
        let grid = Grid { cols: cols.parse().ok()?, rows: rows.parse().ok()? };
        ((1..=4).contains(&grid.cols) && (1..=4).contains(&grid.rows)).then_some(grid)
    }

    pub fn size(self) -> u8 {
        self.cols * self.rows
    }

    pub fn fits(self, max: Grid) -> bool {
        self.cols <= max.cols && self.rows <= max.rows
    }

    /// This grid cut down to fit `max`.
    pub fn within(self, max: Grid) -> Grid {
        Grid { cols: self.cols.min(max.cols), rows: self.rows.min(max.rows) }
    }

    /// The slots of the bottom row, left to right.
    pub fn bottom_row(self) -> Vec<u8> {
        let first = (self.rows - 1) * self.cols + 1;
        (first..first + self.cols).collect()
    }

    /// The key in the middle (upper left of the middle when there is none).
    pub fn center(self) -> u8 {
        (self.rows - 1) / 2 * self.cols + (self.cols - 1) / 2 + 1
    }
}

impl Default for Grid {
    fn default() -> Grid {
        Grid::DEFAULT
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutBody {
    /// A relative touch surface above the bottom row; send knoend.pointer deltas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touchpad: Option<bool>,
    /// Text the board shows in place of the middle rows (e.g. what a permission request is for).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub layout: String,
    pub name: String,
    pub source: String,
    pub reviewed: bool,
    /// The layout's grid; 3x3 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<Grid>,
    /// Page `page` of `pages`, when there is more than one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<u8>,
    pub keys: Vec<LayoutKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusBody {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u32>,
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn old_and_new_message_names() {
        assert_eq!(current_name("openpager.key"), "knoend.key");
        assert_eq!(current_name("knoend.key"), "knoend.key");
        assert_eq!(old_name("knoend.wifi.scan"), "openpager.wifi.scan");
        assert_eq!(old_name("other.thing"), "other.thing");
    }
}
