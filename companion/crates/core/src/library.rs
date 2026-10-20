//! Layout libraries: TOML files, data only (spec/protocol.md section 5).
//!
//! Validation uses the same JSON Schema as `tools/validate_spec.py`
//! (`spec/schemas/library.schema.json`, compiled in), plus the checks a schema cannot express:
//! key slots must not collide or run past their page's grid, and risky-looking keys must ask for a
//! second tap.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::protocol::Grid;

/// Pages a library may have (`page` 1-8).
pub const MAX_PAGES: u8 = 8;

const SCHEMA: &str = include_str!("../../../../spec/schemas/library.schema.json");

/// Words that make a key "risky" unless it has `confirm = true` (same list as validate_spec.py).
/// Words that make a key need `confirm = true` (the drafting prompts list them too).
pub const RISKY_WORDS: &[&str] = &["buy", "purchase", "pay", "checkout", "submit", "send", "delete", "remove", "erase"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Builtin,
    Community,
    Ai,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub version: String,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<String>,
    /// Columns x rows of every page ("2x2"); 3x3 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<String>,
    /// Who saw it work, on which OS and when; an OS not listed is untested there.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tested: Vec<Tested>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tested {
    pub os: String,
    pub by: String,
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Match {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub web: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linux: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macos: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Action {
    Shortcut { keys: String },
    Open { url: String },
    Element { selector: String },
    /// Focus a field on the page, ready to type (browser extension).
    Focus { selector: String },
    /// Control the page's main video or audio (browser extension); `selector` names the player's
    /// own button for next/previous.
    Media {
        command: MediaCommand,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
    },
    Command { name: String },
    /// A key on a coding agent's screen (libraries matched by `match.agent`).
    Agent { reply: AgentReply },
}

/// What a key on a coding agent's screen answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentReply {
    Allow,
    /// Allow and remember (the agent's own suggested rule); shown only when the agent offers one.
    Always,
    Deny,
    /// Deny and stop the agent.
    Stop,
    /// One of the agent's options; these keys take the options in order, unused ones are hidden.
    Option,
    /// Send the ticked options (several-choice questions only).
    Done,
    /// Do not answer the question.
    Decline,
    /// Bring the agent's app to the front.
    GoTo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaCommand {
    Toggle,
    Play,
    Pause,
    Next,
    Previous,
    Mute,
    Forward,
    Back,
}

impl Action {
    /// Whether the browser extension carries this action out (on the active tab).
    pub fn is_web(&self) -> bool {
        matches!(self, Action::Open { .. } | Action::Element { .. } | Action::Focus { .. } | Action::Media { .. })
    }
}

/// A page with a grid of its own (`[[page]]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageSettings {
    pub number: u8,
    pub grid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryKey {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u8>,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speak: Option<String>,
    /// A Material Icons name from `spec/icons.txt`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirm: bool,
    pub action: Action,
}

const ICONS: &str = include_str!("../../../../spec/icons.txt");

/// The icon names a key may use (`spec/icons.txt`).
pub fn icon_names() -> Vec<&'static str> {
    ICONS.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub pack: Pack,
    #[serde(rename = "match")]
    pub matches: Match,
    #[serde(rename = "page", default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<PageSettings>,
    #[serde(rename = "key")]
    pub keys: Vec<LibraryKey>,
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("not valid TOML: {0}")]
    Toml(String),
    #[error("{}", .0.join("; "))]
    Invalid(Vec<String>),
}

fn schema() -> &'static jsonschema::Validator {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).expect("library schema is valid JSON");
        jsonschema::validator_for(&schema).expect("library schema compiles")
    })
}

impl Library {
    /// Parses and validates a library. Anything that fails is rejected whole.
    pub fn parse(text: &str) -> Result<Library, LibraryError> {
        let value: toml::Value = toml::from_str(text).map_err(|e| LibraryError::Toml(e.to_string()))?;
        let json = serde_json::to_value(&value).map_err(|e| LibraryError::Toml(e.to_string()))?;

        let errors: Vec<String> = schema().iter_errors(&json).map(|e| format!("{} at {}", e, e.instance_path())).collect();
        if !errors.is_empty() {
            return Err(LibraryError::Invalid(errors));
        }
        let library: Library = serde_json::from_value(json).map_err(|e| LibraryError::Invalid(vec![e.to_string()]))?;
        let problems = library.semantic_problems();
        if problems.is_empty() {
            Ok(library)
        } else {
            Err(LibraryError::Invalid(problems))
        }
    }

    /// The grid of page `page` (1-based): its `[[page]]` entry, else `pack.grid`, else 3x3.
    pub fn grid(&self, page: u8) -> Grid {
        self.pages.iter().find(|p| p.number == page).map(|p| p.grid.as_str()).or(self.pack.grid.as_deref())
            .and_then(Grid::parse).unwrap_or(Grid::DEFAULT)
    }

    /// The (page, slot) each key lands in, or None where it does not fit (spec/protocol.md section
    /// 5, the same rule as `tools/validate_spec.py`): explicit slots first; a key with only `page`
    /// takes that page's next free slot; a key with neither takes the next free slot, running onto
    /// the next page when one is full.
    pub fn placements(&self) -> Vec<Option<(u8, u8)>> {
        let mut used: Vec<(u8, u8)> = self.keys.iter().filter_map(|k| k.slot.map(|s| (k.page.unwrap_or(1), s))).collect();
        let mut cursor = 1u8;
        let mut out = Vec::new();
        for key in &self.keys {
            if let Some(slot) = key.slot {
                let page = key.page.unwrap_or(1);
                out.push((slot <= self.grid(page).size()).then_some((page, slot)));
                continue;
            }
            let mut page = key.page.unwrap_or(cursor);
            let slot = loop {
                let free = (1..=self.grid(page).size()).find(|s| !used.contains(&(page, *s)));
                if free.is_some() || key.page.is_some() || page >= MAX_PAGES {
                    break free;
                }
                page += 1;
            };
            if key.page.is_none() {
                cursor = page;
            }
            if let Some(slot) = slot {
                used.push((page, slot));
            }
            out.push(slot.map(|s| (page, s)));
        }
        out
    }

    /// Where each key lands, for a library that passed `parse` (every key fits).
    pub fn placed(&self) -> Vec<(u8, u8)> {
        self.placements().into_iter().map(|p| p.unwrap_or((MAX_PAGES + 1, 0))).collect()
    }

    fn semantic_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let icons = icon_names();
        for key in &self.keys {
            if let Some(icon) = &key.icon {
                if !icons.contains(&icon.as_str()) {
                    problems.push(format!("key {:?}: unknown icon {icon:?} (see spec/icons.txt)", key.label));
                }
            }
        }
        let mut numbers: Vec<u8> = self.pages.iter().map(|p| p.number).collect();
        numbers.sort_unstable();
        if numbers.windows(2).any(|w| w[0] == w[1]) {
            problems.push("a page number has two [[page]] entries".to_string());
        }
        let placed = self.placements();
        // Agent screens are shown as one layout (the request and its answers).
        if !self.matches.agent.is_empty() && placed.iter().flatten().any(|(page, _)| *page != 1) {
            problems.push("an agent screen has one page: every key on page 1".to_string());
        }
        let mut sorted = placed.clone();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() != placed.len() || placed.contains(&None) {
            problems.push("key slots collide or run past the grid".to_string());
        }
        for key in &self.keys {
            let text = format!("{} {} {}", key.label, key.speak.as_deref().unwrap_or(""),
                               serde_json::to_string(&key.action).unwrap_or_default());
            if looks_risky(&text) && !key.confirm {
                problems.push(format!("key '{}' looks risky but has no confirm", key.label));
            }
        }
        problems
    }
}

fn looks_risky(text: &str) -> bool {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    words.iter().any(|w| RISKY_WORDS.contains(w)) || words.windows(2).any(|pair| pair == ["order", "now"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn examples() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../spec/examples")
    }

    #[test]
    fn spec_example_libraries_parse() {
        for name in ["library-vscode.toml", "library-amazon.toml", "library-pages.toml"] {
            let text = fs::read_to_string(examples().join(name)).unwrap();
            Library::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn spec_invalid_libraries_are_rejected_for_the_intended_reason() {
        let cases = [
            ("library-raw-command.toml", "rm -rf"),
            ("library-offsite-url.toml", "https://evil.example/"),
            ("library-risky-without-confirm.toml", "looks risky"),
            ("library-agent-key-in-app-layout.toml", "at /key/0/action/kind"),
            ("library-slot-past-grid.toml", "run past the grid"),
            ("library-duplicate-page.toml", "two [[page]] entries"),
            ("library-agent-two-pages.toml", "agent screen has one page"),
        ];
        for (name, expected) in cases {
            let text = fs::read_to_string(examples().join("invalid").join(name)).unwrap();
            let error = Library::parse(&text).expect_err(name).to_string();
            assert!(error.contains(expected), "{name}: {error}");
        }
    }

    #[test]
    fn keys_fill_pages_in_order() {
        let text = fs::read_to_string(examples().join("library-pages.toml")).unwrap();
        let library = Library::parse(&text).unwrap();
        assert_eq!(library.grid(1), Grid { cols: 2, rows: 2 });
        assert_eq!(library.grid(2), Grid::LARGEST);
        // Four fill the 2x2 page 1, the fifth starts page 2; explicit page 2 slot 16; page 3 slot 1.
        assert_eq!(library.placed(), vec![(1, 1), (1, 2), (1, 3), (1, 4), (2, 1), (2, 16), (3, 1)]);
    }

    #[test]
    fn explicit_slots_are_kept_free_for_their_keys() {
        let library = Library::parse(r#"
            [pack]
            id = "user.t"
            name = "T"
            version = "0.1.0"
            source = "user"
            grid = "1x2"
            [match]
            linux = ["t"]
            [[key]]
            label = "A"
            action = { kind = "shortcut", keys = "A" }
            [[key]]
            slot = 1
            label = "B"
            action = { kind = "shortcut", keys = "B" }
            [[key]]
            label = "C"
            action = { kind = "shortcut", keys = "C" }
        "#).unwrap();
        assert_eq!(library.placed(), vec![(1, 2), (1, 1), (2, 1)]);
    }

    #[test]
    fn risky_words_are_whole_words() {
        assert!(looks_risky("Buy now"));
        assert!(looks_risky(r##"{"selector":"#buy-now-button"}"##));
        assert!(!looks_risky("Your orders"));
        assert!(!looks_risky("Sendmail settings")); // "sendmail" is not the word "send"
    }
}
