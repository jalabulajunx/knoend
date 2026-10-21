//! The configure agent: drafts a layout library for an app that has none.
//!
//! Runs only when the person taps Configure on the board. What leaves the machine (if the model
//! is remote) is the app's class and instance name, nothing else: no window title, no content.
//! Whatever the model returns is treated as untrusted data: the fields that decide where a library
//! applies (id, source, version, match) are overwritten here, and the result must pass the same
//! validator as every other library. One retry is allowed, with the validator's errors.

pub mod assistant;
pub mod cli_tool;
pub mod provider;
pub mod secrets;
pub mod site;

use knoend_core::focus::{Focus, Os};
use knoend_core::library::Library;

pub use provider::{AgentConfig, Provider};

/// A validated draft and the TOML that will be saved.
pub struct Draft {
    pub library: Library,
    pub toml: String,
    pub attempts: u32,
}

const SYSTEM_PROMPT: &str = r#"You design layouts for KnoEnd, a touchscreen shortcut keypad. Given a
desktop application, choose up to 16 of its most useful DEFAULT keyboard shortcuts on the stated
operating system and return them as a TOML layout library.
How many keys decides how big they are (one page): 1 key fills the screen, 2 are two wide keys,
3-4 are four big keys (2x2), 5-9 a 3x3 grid, 10-16 a 4x4 grid of small keys. On 4x4 a key with an
icon shows only the icon, so give every key there a fitting icon or a one-word label. Choose as many
keys as the app really needs; do not pad.

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
- Every action is kind = "shortcut" (a program in a terminal may also use "sequence", below). keys is zero or more modifiers from Primary, Ctrl, Shift, Alt,
  Meta, each followed by "+", then exactly one key from: A-Z, 0-9, F1-F24, Enter, Escape, Tab,
  Space, Backspace, Delete, Insert, Home, End, PageUp, PageDown, Up, Down, Left, Right, Backquote,
  Minus, Equal, BracketLeft, BracketRight, Backslash, Semicolon, Quote, Comma, Period, Slash.
- Primary means Ctrl on Linux and Windows and Cmd on macOS; use it for the common Ctrl/Cmd pair.
- Only shortcuts the application really has by default. If you are not sure a shortcut exists,
  leave that key out: fewer correct keys are better than guesses.
- Several products can share a name (an IDE and an agent desktop app called the same, say). When
  "About the app" is given, draft for the product, version and description it names, not for a
  better-known product with the same name.
- Any key that deletes, sends, submits, closes without saving or buys something must have
  confirm = true. So must every key whose label, speak or action contains one of these words:
  buy, purchase, pay, checkout, submit, send, delete, remove, erase (the library is rejected
  otherwise).
- Put the most used actions first.
- Give each key the icon that best shows its action, from the icon list only; leave icon out if
  none fits.

When the target is a PROGRAM RUNNING IN A TERMINAL (a "Terminal program" line is given), the keys
are typed into that terminal tab while the program runs: choose the keys the program itself reads
(letters, digits, arrows, Enter, Escape, Space, Ctrl+letter, q to quit), not the terminal app's own
shortcuts. Use Ctrl, never Primary: terminal programs read Ctrl on every system, macOS too. A key
name is a physical key, lower case: a program's capital-letter command needs Shift (htop's "P"
sorts by CPU: Shift+P; less's "G" goes to the end: Shift+G; plain "P" types p). A
program that asks questions as it runs (a package manager's "Proceed? [Y/n]") gets keys for its
answers. A program that reads a whole line (pacman's "[Y/n]", rm -i, apt) needs the answer and
then Enter: there use action = { kind = "sequence", keys = ["N", "Enter"] } (2 to 8 keys, each
written like a shortcut's keys); a single key press (less, htop, a "press any key") stays a
shortcut. Stopping it (Ctrl+C) has confirm = true. match is set for you."#;

fn user_prompt(focus: &Focus, about: &[String]) -> String {
    let os = match focus.os {
        Os::Linux => "Linux",
        Os::Macos => "macOS",
        Os::Windows => "Windows",
    };
    let about = if about.is_empty() { String::new() } else { format!("About the app (from its own files): {}\n", about.join("; ")) };
    if let Some(program) = terminal_program(focus) {
        return format!("Terminal program: {program} (running in {})\n{about}Operating system: {os}\nIcon list: {}\n",
                       focus.display_name(), knoend_core::library::icon_names().join(" "));
    }
    format!("Application: {}\nWindow class: {}\nWindow instance: {}\n{about}Operating system: {os}\nIcon list: {}\n",
            focus.display_name(), focus.app, focus.instance, knoend_core::library::icon_names().join(" "))
}

/// The program in a terminal tab that the draft is for, when the focus is a terminal app running
/// one (not a shell at its prompt).
pub fn terminal_program(focus: &Focus) -> Option<&str> {
    focus.terminal.as_ref().and_then(|t| t.program())
}

/// A slug for ids and file names: lowercase letters, digits and dashes ("terminal-pacman" for a
/// program in a terminal).
pub fn slug(focus: &Focus) -> String {
    let owned;
    let raw = match terminal_program(focus) {
        Some(program) => {
            owned = format!("terminal-{program}");
            &owned
        }
        None if focus.instance.is_empty() => &focus.app,
        None => &focus.instance,
    };
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

/// The grid for a draft of `keys` keys (the prompts describe the same rule): big keys for a few
/// actions, 3x3 as before, small keys for many.
pub fn grid_for(keys: usize) -> &'static str {
    match keys {
        0 | 1 => "1x1",
        2 => "1x2",
        3 | 4 => "2x2",
        5..=9 => "3x3",
        _ => "4x4",
    }
}

/// Sets the draft's grid from its key count, in order on one page: the model does not place keys.
/// Keys past 16 run onto a second page. An icon not in the icon list is left out (icons are
/// optional; the label shows instead), so one made-up icon name does not cost the whole draft.
pub(crate) fn fit_grid(value: &mut toml::Table) {
    value.remove("page");
    let icons = knoend_core::library::icon_names();
    let keys = match value.get_mut("key").and_then(toml::Value::as_array_mut) {
        Some(keys) => {
            for key in keys.iter_mut().filter_map(toml::Value::as_table_mut) {
                key.remove("page");
                key.remove("slot");
                if key.get("icon").and_then(toml::Value::as_str).is_some_and(|i| !icons.contains(&i)) {
                    key.remove("icon");
                }
            }
            keys.len()
        }
        None => 0,
    };
    if let Some(pack) = value.get_mut("pack").and_then(toml::Value::as_table_mut) {
        pack.insert("grid".into(), grid_for(keys).into());
    }
}

/// Overwrites the fields that decide where and how the library applies, so a draft can never
/// claim another app, a website, or a non-AI source; sets the grid from the number of keys.
fn constrain(toml_text: &str, focus: &Focus) -> Result<String, String> {
    let mut value: toml::Table = toml::from_str(toml_text).map_err(|e| format!("not valid TOML: {e}"))?;
    let pack = value.entry("pack").or_insert_with(|| toml::Value::Table(Default::default()));
    let pack = pack.as_table_mut().ok_or("[pack] is not a table")?;
    pack.insert("id".into(), format!("ai.{}", slug(focus)).into());
    pack.insert("source".into(), "ai".into());
    pack.insert("version".into(), "0.1.0".into());
    if !pack.contains_key("name") {
        pack.insert("name".into(), terminal_program(focus).unwrap_or(focus.display_name()).into());
    }
    if let Some(program) = terminal_program(focus) {
        // A terminal program's keys apply wherever it runs, in any terminal app, on any OS.
        let mut matches = toml::Table::new();
        matches.insert("terminal".into(), toml::Value::Array(vec![program.into()]));
        value.insert("match".into(), toml::Value::Table(matches));
        fit_grid(&mut value);
        return toml::to_string(&value).map_err(|e| e.to_string());
    }

    let sequence = value.get("key").and_then(toml::Value::as_array).into_iter().flatten()
        .any(|k| k.get("action").and_then(|a| a.get("kind")).and_then(toml::Value::as_str) == Some("sequence"));
    if sequence {
        return Err("kind = \"sequence\" is only for a program in a terminal: use one shortcut per key".into());
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
    fit_grid(&mut value);
    toml::to_string(&value).map_err(|e| e.to_string())
}

/// Asks the model for a library and validates it; one retry with the validator's errors.
/// `about`: what the app's own files say about it (`Package: Antigravity 2.23.1: ...`), so that two
/// products with one name are not confused; may be empty.
pub async fn draft_library(config: &AgentConfig, focus: &Focus, about: &[String]) -> Result<Draft, String> {
    let mut messages = vec![provider::Message::user(user_prompt(focus, about))];
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
        Focus { os: Os::Linux, app: "Xfce4-terminal".into(), instance: "xfce4-terminal".into(), title: "secret title".into(), web: None, pid: None, terminal: None }
    }

    #[test]
    fn unknown_icons_are_left_out() {
        let mut value: toml::Table = toml::from_str(r#"
[pack]
id = "x"
[[key]]
label = "Stats"
icon = "bar_chart"
[[key]]
label = "Find"
icon = "search"
"#).unwrap();
        fit_grid(&mut value);
        let keys = value["key"].as_array().unwrap();
        assert!(keys[0].get("icon").is_none(), "made-up icon dropped");
        assert_eq!(keys[1]["icon"].as_str(), Some("search"));
    }

    #[test]
    fn prompt_names_every_word_that_needs_confirm() {
        for word in knoend_core::library::RISKY_WORDS {
            assert!(SYSTEM_PROMPT.contains(&format!(" {word}")), "{word} missing from the drafting prompt");
        }
    }

    #[test]
    fn prompt_never_contains_the_window_title() {
        assert!(!user_prompt(&focus(), &[]).contains("secret"));
    }

    #[test]
    fn prompt_says_which_product_it_is() {
        let about = ["Package: Antigravity 2.23.1: Antigravity - Agentic Desktop Application".to_string()];
        let prompt = user_prompt(&focus(), &about);
        assert!(prompt.contains("About the app (from its own files): Package: Antigravity 2.23.1"));
        assert!(!user_prompt(&focus(), &[]).contains("About the app"));
        assert!(SYSTEM_PROMPT.contains("Several products can share a name"));
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
        assert!(matches!(library.pack.source, knoend_core::library::Source::Ai));
        assert_eq!(library.pack.grid.as_deref(), Some("1x1"), "one key: one big key");
    }

    #[test]
    fn the_grid_follows_the_number_of_keys() {
        let draft = |n: usize| {
            let keys: String = (0..n).map(|i| format!("[[key]]\nlabel = \"K{i}\"\npage = 3\nslot = 2\naction = {{ kind = \"shortcut\", keys = \"F{}\" }}\n", i + 1)).collect();
            Library::parse(&constrain(&format!("[pack]\nname = \"T\"\n[[page]]\nnumber = 2\ngrid = \"1x1\"\n{keys}"), &focus()).unwrap()).unwrap()
        };
        assert_eq!([2, 4, 5, 9, 10, 16].map(|n| draft(n).pack.grid.unwrap()), ["1x2", "2x2", "3x3", "3x3", "4x4", "4x4"]);
        let many = draft(18);
        assert!(many.pages.is_empty(), "the model's own pages and slots are dropped");
        assert_eq!(many.placed()[16..], [(2, 1), (2, 2)], "past 16 they run onto page 2");
    }

    #[test]
    fn slugs_are_safe() {
        assert_eq!(slug(&focus()), "xfce4-terminal");
        let f = Focus { os: Os::Linux, app: "com.anthropic.Claude".into(), instance: "com.anthropic.claude".into(), title: String::new(), web: None, pid: None, terminal: None };
        assert_eq!(slug(&f), "com-anthropic-claude");
    }

    #[test]
    fn a_program_in_a_terminal_gets_keys_of_its_own() {
        let tab = |programs: &[&str]| Focus { terminal: Some(knoend_core::focus::TerminalTab { tty: "pts/4".into(),
                                              programs: programs.iter().map(|p| p.to_string()).collect() }), ..focus() };
        let pacman = tab(&["sudo", "pacman"]);
        assert_eq!(slug(&pacman), "terminal-pacman");
        let library = Library::parse(&constrain("[pack]\n[[key]]\nlabel = \"Yes\"\naction = { kind = \"shortcut\", keys = \"Y\" }\n\n[match]\nlinux = [\"bash\"]\n", &pacman).unwrap()).unwrap();
        assert_eq!((library.pack.id.as_str(), library.pack.name.as_str()), ("ai.terminal-pacman", "pacman"));
        assert_eq!(library.matches.terminal, vec!["pacman".to_string()]);
        assert!(library.matches.linux.is_empty(), "not the terminal app");
        assert!(user_prompt(&pacman, &[]).starts_with("Terminal program: pacman (running in Xfce4-terminal)"));
        assert!(!user_prompt(&pacman, &[]).contains("secret title"));
        assert!(SYSTEM_PROMPT.contains("Shift+P") && SYSTEM_PROMPT.contains("never Primary"), "capitals and Ctrl in terminals");
        assert!(SYSTEM_PROMPT.contains(r#"kind = "sequence", keys = ["N", "Enter"]"#), "an answer then Enter for programs that read a line");
        let answer = "[pack]\n[[key]]\nlabel = \"No\"\naction = { kind = \"sequence\", keys = [\"N\", \"Enter\"] }\n";
        let library = Library::parse(&constrain(answer, &pacman).unwrap()).unwrap();
        assert_eq!(library.keys[0].action, knoend_core::library::Action::Sequence { keys: vec!["N".into(), "Enter".into()] });
        assert!(constrain(answer, &focus()).is_err(), "not for a desktop app");
        assert!(Library::parse(&constrain(&answer.replace("\"Enter\"", "\"Return\""), &pacman).unwrap()).is_err(), "each key is checked");
        let prompt = tab(&["fish"]);
        assert_eq!(slug(&prompt), "xfce4-terminal", "a shell at its prompt: the terminal app");
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Drafts a real app library through each AI tool on this computer:
    /// `cargo test -p knoend-agents live_draft -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_draft_through_tools_on_this_computer() {
        let focus = Focus { os: Os::Linux, app: "Gnome-calculator".into(), instance: "gnome-calculator".into(), title: String::new(), web: None, pid: None, terminal: None };
        for provider in [Provider::ClaudeCode, Provider::Antigravity] {
            let config = AgentConfig { provider, ..Default::default() };
            let started = std::time::Instant::now();
            match draft_library(&config, &focus, &[]).await {
                Ok(draft) => println!("{provider:?}: {} keys, {} attempt(s), {:?}\n{}", draft.library.keys.len(),
                                      draft.attempts, started.elapsed(), draft.toml),
                Err(e) => println!("{provider:?}: error after {:?}: {e}", started.elapsed()),
            }
        }
    }
}

