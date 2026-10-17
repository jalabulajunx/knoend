//! Layout libraries: TOML files, data only (spec/protocol.md section 5).
//!
//! Validation uses the same JSON Schema as `tools/validate_spec.py`
//! (`spec/schemas/library.schema.json`, compiled in), plus the checks a schema cannot express:
//! key slots must not collide, and risky-looking keys must ask for a second tap.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const SCHEMA: &str = include_str!("../../../../spec/schemas/library.schema.json");

/// Words that make a key "risky" unless it has `confirm = true` (same list as validate_spec.py).
const RISKY_WORDS: &[&str] = &["buy", "purchase", "pay", "checkout", "submit", "send", "delete", "remove", "erase"];

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
    Command { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryKey {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u8>,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speak: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirm: bool,
    pub action: Action,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub pack: Pack,
    #[serde(rename = "match")]
    pub matches: Match,
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

    /// The slot (1-9) each key lands in: explicit `slot`, otherwise the next free one.
    pub fn slots(&self) -> Vec<u8> {
        let mut used: Vec<u8> = Vec::new();
        let mut next_free = 1u8;
        for key in &self.keys {
            let slot = match key.slot {
                Some(slot) => slot,
                None => {
                    while used.contains(&next_free) {
                        next_free += 1;
                    }
                    next_free
                }
            };
            used.push(slot);
        }
        used
    }

    fn semantic_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let slots = self.slots();
        let mut sorted = slots.clone();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() != slots.len() || slots.iter().any(|&s| s > 9) {
            problems.push("key slots collide or run past 9".to_string());
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
        for name in ["library-vscode.toml", "library-amazon.toml"] {
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
        ];
        for (name, expected) in cases {
            let text = fs::read_to_string(examples().join("invalid").join(name)).unwrap();
            let error = Library::parse(&text).expect_err(name).to_string();
            assert!(error.contains(expected), "{name}: {error}");
        }
    }

    #[test]
    fn risky_words_are_whole_words() {
        assert!(looks_risky("Buy now"));
        assert!(looks_risky(r##"{"selector":"#buy-now-button"}"##));
        assert!(!looks_risky("Your orders"));
        assert!(!looks_risky("Sendmail settings")); // "sendmail" is not the word "send"
    }
}
