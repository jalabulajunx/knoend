//! Drafting keys for a website from its page inventory (`spec/browser.md` section 3).
//!
//! Runs only when the person asks ("Configure this site" in the extension, or Configure on the
//! board), on a site where they turned board buttons on. What is sent to the model: the site's host,
//! the page's path (no query) and its controls (role, name, selector, declared shortcut, same-site
//! link path). Never page text or field values. The draft is untrusted: where it applies is set
//! here, and every selector must be one the inventory listed.

use std::collections::HashSet;

use openpager_core::library::{Action, Library};
use serde_json::Value;

use crate::provider::{self, AgentConfig, Message};
use crate::{extract_toml, Draft};

const SYSTEM_PROMPT: &str = r#"You design website layouts for OpenPager, a 3x3 touchscreen keypad that
sits next to the computer. You are given one website and the list of controls on the page the person
is looking at. Choose up to 9 keys that make this site faster to use and return them as a TOML layout
library.

Return ONLY the TOML document, no prose and no code fences. Use exactly this shape:

[pack]
id = "ai.site"
name = "Example"            # the site's human name, at most 32 characters
version = "0.1.0"
source = "ai"
description = "One sentence on what the keys cover."

[match]
web = ["example.com"]

[[key]]
label = "Compose"           # at most 14 characters so it fits on the key
speak = "Write a message"   # optional, spoken aloud, at most 80 characters
action = { kind = "element", selector = "[data-testid=\"compose\"]" }

Action kinds:
- element: click a control. selector MUST be copied exactly from the list.
- focus: put the cursor in a text field or search box. selector MUST be copied exactly from the list.
- open: go to a page on this site. url is a path starting with "/" (from a listed link, or a
  well-known page of this site you are sure exists).
- media: control the page's video or audio: command is toggle, play, pause, forward, back, mute,
  next or previous (next/previous need a selector of the player's own button from the list).
- shortcut: a keyboard shortcut this site itself defines (listed as shortcut=..., or a documented
  shortcut of this site you are sure of). keys is optional modifiers (Primary, Ctrl, Shift, Alt,
  Meta, each followed by "+") then one key: A-Z, 0-9, F1-F24, Enter, Escape, Tab, Space, Slash,
  Period, Comma, Up, Down, Left, Right, Home, End, PageUp, PageDown.

Rules:
- Prefer controls with clear names that stay on the page (navigation, search, compose, play). Avoid
  controls whose names are content (an article or video title, a person's name): they change.
- Never use a selector that is not in the list. Fewer correct keys are better than nine guesses.
- Any key that deletes, sends, submits, posts, pays or buys must have confirm = true.
- Put the most used actions first."#;

/// The inventory as compact lines for the prompt (at most 150 controls).
fn describe(items: &[Value]) -> String {
    items.iter().take(150).enumerate().map(|(i, item)| {
        let mut line = format!("{}. {} \"{}\" selector={}", i + 1, item["role"].as_str().unwrap_or("?"),
                               item["name"].as_str().unwrap_or(""), item["selector"].as_str().unwrap_or(""));
        if let Some(s) = item["shortcut"].as_str() {
            line += &format!(" shortcut={s}");
        }
        if let Some(h) = item["href"].as_str() {
            line += &format!(" href={}", h.split('?').next().unwrap_or(h));
        }
        line
    }).collect::<Vec<_>>().join("\n")
}

/// "youtube.com" from "www.youtube.com".
pub fn site_of(host: &str) -> String {
    host.strip_prefix("www.").unwrap_or(host).to_ascii_lowercase()
}

/// Library id for a site: "ai.site-youtube-com".
pub fn site_id(host: &str) -> String {
    let mut slug: String = site_of(host).chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    format!("ai.site-{}", slug.trim_matches('-'))
}

/// Sets what the model may not choose: id, source, version, and where the library applies.
fn constrain(toml_text: &str, host: &str) -> Result<String, String> {
    let mut value: toml::Table = toml::from_str(toml_text).map_err(|e| format!("not valid TOML: {e}"))?;
    let pack = value.entry("pack").or_insert_with(|| toml::Value::Table(Default::default()));
    let pack = pack.as_table_mut().ok_or("[pack] is not a table")?;
    pack.insert("id".into(), site_id(host).into());
    pack.insert("source".into(), "ai".into());
    pack.insert("version".into(), "0.1.0".into());
    if !pack.contains_key("name") {
        pack.insert("name".into(), site_of(host).into());
    }
    let mut matches = toml::Table::new();
    matches.insert("web".into(), toml::Value::Array(vec![site_of(host).into()]));
    value.insert("match".into(), toml::Value::Table(matches));
    toml::to_string(&value).map_err(|e| e.to_string())
}

/// Selectors the draft uses that the page did not list.
fn unknown_selectors(library: &Library, items: &[Value]) -> Vec<String> {
    let known: HashSet<&str> = items.iter().filter_map(|i| i["selector"].as_str()).collect();
    library.keys.iter().filter_map(|k| match &k.action {
        Action::Element { selector } | Action::Focus { selector } => Some(selector.as_str()),
        Action::Media { selector: Some(selector), .. } => Some(selector.as_str()),
        _ => None,
    }).filter(|s| !known.contains(s)).map(str::to_string).collect()
}

/// Drafts and validates a site library; one retry with the validator's errors.
pub async fn draft_site_library(config: &AgentConfig, host: &str, path: &str, items: &[Value]) -> Result<Draft, String> {
    if items.is_empty() {
        return Err("the page listed no controls to build keys from".into());
    }
    let prompt = format!("Website: {}\nPage path: {}\nControls on this page:\n{}\n", site_of(host),
                         path.split(['?', '#']).next().unwrap_or("/"), describe(items));
    let mut messages = vec![Message::user(prompt)];
    let mut last_error = String::new();
    for attempt in 1..=2 {
        let reply = provider::complete(config, SYSTEM_PROMPT, &messages).await?;
        let candidate = extract_toml(&reply);
        let checked = constrain(&candidate, host)
            .and_then(|t| Library::parse(&t).map(|l| (l, t)).map_err(|e| e.to_string()))
            .and_then(|(library, toml)| {
                let unknown = unknown_selectors(&library, items);
                if unknown.is_empty() { Ok((library, toml)) } else { Err(format!("selectors not in the list: {}", unknown.join(", "))) }
            });
        match checked {
            Ok((library, toml)) => return Ok(Draft { library, toml, attempts: attempt }),
            Err(error) => {
                last_error = error.clone();
                messages.push(Message::assistant(reply));
                messages.push(Message::user(format!("That library was rejected: {error}\nReturn the corrected TOML only.")));
            }
        }
    }
    Err(format!("the model's draft did not pass validation twice: {last_error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn items() -> Vec<Value> {
        vec![json!({"role": "button", "name": "Compose", "selector": "[data-testid=\"compose\"]", "shortcut": "c"}),
             json!({"role": "searchbox", "name": "Search", "selector": "input[name=\"q\"]"}),
             json!({"role": "link", "name": "Inbox", "selector": "#inbox", "href": "/inbox?tab=1"})]
    }

    #[test]
    fn prompt_lines_carry_controls_but_not_queries() {
        let text = describe(&items());
        assert!(text.contains("1. button \"Compose\" selector=[data-testid=\"compose\"] shortcut=c"));
        assert!(text.contains("href=/inbox"));
        assert!(!text.contains("tab=1"));
    }

    #[test]
    fn drafts_are_pinned_to_the_site_and_the_listed_selectors() {
        let hostile = r##"
[pack]
id = "builtin.bank"
name = "Mail"
version = "9"
source = "builtin"
[match]
web = ["bank.example"]
[[key]]
label = "Compose"
action = { kind = "element", selector = "[data-testid=\"compose\"]" }
[[key]]
label = "Steal"
action = { kind = "element", selector = "#transfer-all" }
"##;
        let fixed = constrain(hostile, "www.Mail.Example").unwrap();
        let library = Library::parse(&fixed).unwrap();
        assert_eq!(library.pack.id, "ai.site-mail-example");
        assert_eq!(library.matches.web, vec!["mail.example".to_string()]);
        assert_eq!(unknown_selectors(&library, &items()), vec!["#transfer-all".to_string()]);
    }
}
