//! How the person wants their boards: each board's key size, theme, which way it sits, fonts and
//! start-up size (`BoardLook`, one per board: an AMOLED watch and a landscape LCD want different
//! ones, #15), and what belongs to the computer (the home screen, the assistant watching). Kept in
//! `~/.config/knoend/display.json`. The theme and turn are sent to each board (`knoend.display`,
//! spec/protocol.md 3.6d), which keeps them; the key size stays here.
//!
//! `Display` is read for one board with `for_board`: its look over the computer's settings. A
//! board with no look of its own yet starts from the defaults. The look fields at the top level of
//! the file are from before looks were per board: on first load they become the look of every board
//! paired then.
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
                // Ink to paper (the firmware's key colours): 0x3A2410 at black, 0xEEDDB8 at white.
                Theme::Sepia => [(0x3A, 0xEE), (0x24, 0xDD), (0x10, 0xB8)].map(|(dark, light): (u32, u32)| (dark + (light - dark) * grey / 255) as u8),
                _ => [grey as u8; 3],
            };
            pixel[..3].copy_from_slice(&[r, g, b]);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Display {
    #[serde(default)]
    pub key_size: KeySize,
    #[serde(default)]
    pub theme: Theme,
    /// Quarter turns clockwise from the board's usual landscape (0-3).
    #[serde(default)]
    pub turn: u8,
    /// Google Fonts family for key labels; empty: the built-in font.
    #[serde(default)]
    pub key_font: String,
    /// Google Fonts family for the start-up name; empty: the built-in font.
    #[serde(default)]
    pub start_font: String,
    /// Pixels of the name shown at start-up (24-96), kept on the board.
    #[serde(default = "start_size")]
    pub start_size: u8,
    /// The setup assistant looks at problems the service notices without being asked (watch.rs).
    #[serde(default = "yes")]
    pub assistant_watches: bool,
    /// The board's home screen (the launcher, shown when no app is in front): these first, in this
    /// order ("app:firefox", "site:github.com"), ...
    #[serde(default)]
    pub home_apps: Vec<String>,
    /// ... then, when true, the apps and sites used most.
    #[serde(default = "yes")]
    pub home_fill: bool,
    /// While the board listens for notes, sounds quieter than this (dB below full scale) are silence.
    #[serde(default = "listen_quiet")]
    pub listen_quiet: i8,
    /// Each board's look, by board id.
    #[serde(default)]
    pub boards: std::collections::BTreeMap<String, BoardLook>,
}

/// One board's look: what differs from board to board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardLook {
    #[serde(default)]
    pub key_size: KeySize,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub turn: u8,
    #[serde(default)]
    pub key_font: String,
    #[serde(default)]
    pub start_font: String,
    #[serde(default = "start_size")]
    pub start_size: u8,
}

impl Default for BoardLook {
    fn default() -> Self {
        BoardLook { key_size: KeySize::default(), theme: Theme::default(), turn: 0, key_font: String::new(),
                    start_font: String::new(), start_size: start_size() }
    }
}

impl BoardLook {
    fn of(d: &Display) -> BoardLook {
        BoardLook { key_size: d.key_size, theme: d.theme, turn: d.turn, key_font: d.key_font.clone(),
                    start_font: d.start_font.clone(), start_size: d.start_size }
    }
}

fn listen_quiet() -> i8 {
    -60
}

fn yes() -> bool {
    true
}

fn start_size() -> u8 {
    64
}

impl Default for Display {
    fn default() -> Self {
        Display { key_size: KeySize::default(), theme: Theme::default(), turn: 0, key_font: String::new(),
                  start_font: String::new(), start_size: start_size(), assistant_watches: true, home_apps: Vec::new(), home_fill: true,
                  listen_quiet: listen_quiet(), boards: Default::default() }
    }
}

impl Display {
    /// The settings as board `id` sees them: its own look (the defaults for a board with none
    /// yet) over the computer's settings.
    pub fn for_board(&self, id: &str) -> Display {
        let look = self.boards.get(id).cloned().unwrap_or_default();
        Display { key_size: look.key_size, theme: look.theme, turn: look.turn, key_font: look.key_font,
                  start_font: look.start_font, start_size: look.start_size, ..self.clone() }
    }

    /// Stores the look fields of `seen` (a `for_board` copy, changed) as board `id`'s look.
    pub fn set_board(&mut self, id: &str, seen: &Display) {
        self.boards.insert(id.to_string(), BoardLook::of(seen));
    }

    /// The `knoend.display` body for the board.
    pub fn look(&self) -> serde_json::Value {
        serde_json::json!({"theme": self.theme, "turn": self.turn, "startSize": self.start_size})
    }

    /// Whether the board's identity (`board.display`) already shows this look. None: the board's
    /// firmware does not know `knoend.display`.
    pub fn shown_by(&self, identity: &serde_json::Value) -> Option<bool> {
        let shown = identity["board"]["display"].as_object()?;
        Some(shown.get("theme").and_then(|t| t.as_str()) == self.look()["theme"].as_str()
             && shown.get("turn").and_then(|t| t.as_u64()) == Some(u64::from(self.turn))
             // Firmware without a start-up size says nothing about it.
             && shown.get("startSize").and_then(|t| t.as_u64()).is_none_or(|t| t == u64::from(self.start_size)))
    }
}

fn path() -> std::path::PathBuf {
    knoend_transport::pairing::config_dir().join("display.json")
}

/// The saved choice, or the defaults.
pub fn load() -> Display {
    let text = std::fs::read_to_string(path()).ok();
    let mut display: Display = text.as_deref().and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
    // Saved before looks were per board: the one look becomes that of every board paired now.
    if display.boards.is_empty() && text.is_some() {
        let paired: Vec<String> = knoend_transport::pairing::Pairings::load().boards.keys().cloned().collect();
        if migrate(&mut display, &paired) {
            let _ = save(&display);
        }
    }
    display
}

/// Gives every board in `paired` the look saved at the top level, before looks were per board.
fn migrate(display: &mut Display, paired: &[String]) -> bool {
    let look = BoardLook::of(display);
    for id in paired {
        display.boards.insert(id.clone(), look.clone());
    }
    !paired.is_empty()
}

/// Board `id`'s settings (see `Display::for_board`).
pub fn for_board(id: &str) -> Display {
    load().for_board(id)
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
                   serde_json::json!({"key_size": "big", "theme": "dark", "turn": 0, "key_font": "", "start_font": "",
                                      "start_size": 64, "assistant_watches": true, "home_apps": [], "home_fill": true, "listen_quiet": -60,
                                      "boards": {}}));
    }

    #[test]
    fn each_board_has_its_own_look_and_a_new_one_starts_from_the_defaults() {
        // Saved before looks were per board, for the ONX.
        let mut d: Display = serde_json::from_str(r#"{"theme": "sepia", "turn": 1, "key_font": "Silkscreen", "home_fill": false}"#).unwrap();
        assert!(migrate(&mut d, &["80b54edbcdb8".to_string()]));
        let onx = d.for_board("80b54edbcdb8");
        assert_eq!((onx.theme, onx.turn, onx.key_font.as_str()), (Theme::Sepia, 1, "Silkscreen"));
        let amoled = d.for_board("28848590beb8");
        assert_eq!((amoled.theme, amoled.turn, amoled.key_font.as_str(), amoled.start_size), (Theme::Dark, 0, "", 64), "a new board: defaults");
        assert!(!amoled.home_fill, "the computer's settings are shared");
        let mut changed = amoled.clone();
        changed.key_size = KeySize::Big;
        d.set_board("28848590beb8", &changed);
        assert_eq!(d.for_board("28848590beb8").key_size, KeySize::Big);
        assert_eq!(d.for_board("80b54edbcdb8").key_size, KeySize::Medium, "the other board keeps its own");
    }

    #[test]
    fn the_look_is_compared_with_what_the_board_says() {
        let sepia = Display { theme: Theme::Sepia, ..Default::default() };
        assert_eq!(sepia.look(), serde_json::json!({"theme": "sepia", "turn": 0, "startSize": 64}));
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
        assert_eq!(tone(Theme::Sepia, [255, 255, 255, 255]), vec![0xEE, 0xDD, 0xB8, 255], "white is the paper");
        assert_eq!(tone(Theme::Sepia, [0, 0, 0, 9]), vec![0x3A, 0x24, 0x10, 9], "black is the ink");
    }
}
