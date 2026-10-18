//! The configure agent: drafts a layout library for an app that has none.
//!
//! Runs only when the person taps Configure on the board. What leaves the machine (if the model
//! is remote) is the app's class and instance name, nothing else: no window title, no content.
//! Whatever the model returns is treated as untrusted data: the fields that decide where a library
//! applies (id, source, version, match) are overwritten here, and the result must pass the same
//! validator as every other library. One retry is allowed, with the validator's errors.

pub mod assistant;
pub mod provider;
pub mod secrets;
pub mod site;

use openpager_core::focus::{Focus, Os};
use openpager_core::library::Library;

pub use provider::{AgentConfig, Provider};

/// A validated draft and the TOML that will be saved.
pub struct Draft {
    pub library: Library,
    pub toml: String,
    pub attempts: u32,
}

const SYSTEM_PROMPT: &str = r#"You design layouts for OpenPager, a 3x3 touchscreen shortcut keypad.
Given a desktop application, choose up to 9 of its most useful DEFAULT keyboard shortcuts on the
stated operating system and return them as a TOML layout library.

Return ONLY the TOML document, no prose and no code fences. Use exactly this shape:

[pack]
id = "ai.example"
name = "Example"            # the app's human name, at most 32 characters
version = "0.1.0"
source = "ai"
description = "One sentence on what the keys cover."

[match]
linux = ["example"]

[[key]]
label = "Save"              # at most 14 characters so it fits on the key
speak = "Save file"         # optional, spoken aloud, at most 80 characters
icon = "save"               # optional: one name from the icon list given with the app
action = { kind = "shortcut", keys = "Primary+S" }

Rules:
- Every action is kind = "shortcut". keys is zero or more modifiers from Primary, Ctrl, Shift, Alt,
  Meta, each followed by "+", then exactly one key from: A-Z, 0-9, F1-F24, Enter, Escape, Tab,
  Space, Backspace, Delete, Insert, Home, End, PageUp, PageDown, Up, Down, Left, Right, Backquote,
  Minus, Equal, BracketLeft, BracketRight, Backslash, Semicolon, Quote, Comma, Period, Slash.
- Primary means Ctrl on Linux and Windows and Cmd on macOS; use it for the common Ctrl/Cmd pair.
- Only shortcuts the application really has by default. If you are not sure a shortcut exists,
  leave that key out: fewer correct keys are better than nine guesses.
- Any key that deletes, sends, submits, closes without saving or buys something must have
  confirm = true.
- Put the most used actions first.
- Give each key the icon that best shows its action, from the icon list only; leave icon out if
  none fits."#;

fn user_prompt(focus: &Focus) -> String {
    let os = match focus.os {
        Os::Linux => "Linux",
        Os::Macos => "macOS",
        Os::Windows => "Windows",
    };
    format!("Application: {}\nWindow class: {}\nWindow instance: {}\nOperating system: {os}\nIcon list: {}\n",
            focus.display_name(), focus.app, focus.instance, openpager_core::library::icon_names().join(" "))
}

/// A slug for ids and file names: lowercase letters, digits and dashes.
pub fn slug(focus: &Focus) -> String {
    let raw = if focus.instance.is_empty() { &focus.app } else { &focus.instance };
    let mut slug: String = raw.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() { "app".into() } else { slug }
}

/// Strips code fences or prose around the TOML, if the model added any.
pub(crate) fn extract_toml(reply: &str) -> String {
    let text = reply.trim();
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        let after = after.strip_prefix("toml").unwrap_or(after);
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    match text.find("[pack]") {
        Some(start) => text[start..].trim().to_string(),
        None => text.to_string(),
    }
}

/// Overwrites the fields that decide where and how the library applies, so a draft can never
/// claim another app, a website, or a non-AI source.
fn constrain(toml_text: &str, focus: &Focus) -> Result<String, String> {
    let mut value: toml::Table = toml::from_str(toml_text).map_err(|e| format!("not valid TOML: {e}"))?;
    let pack = value.entry("pack").or_insert_with(|| toml::Value::Table(Default::default()));
    let pack = pack.as_table_mut().ok_or("[pack] is not a table")?;
    pack.insert("id".into(), format!("ai.{}", slug(focus)).into());
    pack.insert("source".into(), "ai".into());
    pack.insert("version".into(), "0.1.0".into());
    if !pack.contains_key("name") {
        pack.insert("name".into(), focus.display_name().into());
    }

    let mut patterns = vec![toml::Value::String(focus.instance.clone())];
    if !focus.app.is_empty() && !focus.app.eq_ignore_ascii_case(&focus.instance) {
        patterns.push(focus.app.clone().into());
    }
    patterns.retain(|p| p.as_str().is_some_and(|s| !s.is_empty()));
    let os_key = match focus.os {
        Os::Linux => "linux",
        Os::Macos => "macos",
        Os::Windows => "windows",
    };
    let mut matches = toml::Table::new();
    matches.insert(os_key.into(), toml::Value::Array(patterns));
    value.insert("match".into(), toml::Value::Table(matches));
    toml::to_string(&value).map_err(|e| e.to_string())
}

/// Asks the model for a library and validates it; one retry with the validator's errors.
pub async fn draft_library(config: &AgentConfig, focus: &Focus) -> Result<Draft, String> {
    let mut messages = vec![provider::Message::user(user_prompt(focus))];
    let mut last_error = String::new();
    for attempt in 1..=2 {
        let reply = provider::complete(config, SYSTEM_PROMPT, &messages).await?;
        let candidate = extract_toml(&reply);
        match constrain(&candidate, focus).and_then(|t| Library::parse(&t).map(|l| (l, t)).map_err(|e| e.to_string())) {
            Ok((library, toml)) => return Ok(Draft { library, toml, attempts: attempt }),
            Err(error) => {
                last_error = error.clone();
                messages.push(provider::Message::assistant(reply));
                messages.push(provider::Message::user(format!(
                    "That library was rejected by the validator: {error}\nReturn the corrected TOML only.")));
            }
        }
    }
    Err(format!("the model's draft did not pass validation twice: {last_error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus() -> Focus {
        Focus { os: Os::Linux, app: "Xfce4-terminal".into(), instance: "xfce4-terminal".into(), title: "secret title".into(), web: None }
    }

    #[test]
    fn prompt_never_contains_the_window_title() {
        assert!(!user_prompt(&focus()).contains("secret"));
    }

    #[test]
    fn extracts_toml_from_fences_and_prose() {
        assert_eq!(extract_toml("```toml\n[pack]\nid = 1\n```"), "[pack]\nid = 1");
        assert_eq!(extract_toml("Here you go:\n[pack]\nid = 1"), "[pack]\nid = 1");
    }

    #[test]
    fn constrain_overrides_what_the_model_cannot_choose() {
        let hostile = r#"
[pack]
id = "builtin.vscode"
name = "Terminal"
version = "9.9.9"
source = "builtin"
[match]
web = ["bank.example"]
linux = ["code"]
[[key]]
label = "Copy"
action = { kind = "shortcut", keys = "Ctrl+Shift+C" }
"#;
        let fixed = constrain(hostile, &focus()).unwrap();
        let library = Library::parse(&fixed).unwrap();
        assert_eq!(library.pack.id, "ai.xfce4-terminal");
        assert_eq!(library.pack.version, "0.1.0");
        assert!(library.matches.web.is_empty(), "web match removed");
        assert_eq!(library.matches.linux, vec!["xfce4-terminal".to_string()], "class equals instance ignoring case");
        assert!(matches!(library.pack.source, openpager_core::library::Source::Ai));
    }

    #[test]
    fn slugs_are_safe() {
        assert_eq!(slug(&focus()), "xfce4-terminal");
        let f = Focus { os: Os::Linux, app: "com.anthropic.Claude".into(), instance: "com.anthropic.claude".into(), title: String::new(), web: None };
        assert_eq!(slug(&f), "com-anthropic-claude");
    }
}
