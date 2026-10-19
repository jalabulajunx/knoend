//! The setup assistant: helps a person get a board working, using what the project has learned
//! (`docs/knowledge/setup.md`, built in) and a snapshot of the situation the service collected.
//!
//! What it receives is decided by the service: OS, USB ports, chip details, recent errors and
//! status. Never passwords, API keys or window titles. It may suggest actions from a fixed list;
//! the web app shows them as buttons and nothing runs unless the person clicks.

use serde::Serialize;
use serde_json::Value;

use crate::provider::{self, AgentConfig, Message};

const KNOWLEDGE: &str = include_str!("../../../../docs/knowledge/setup.md");

/// Actions the web app can offer as buttons.
pub const ACTIONS: &[&str] = &["rescan", "identify", "install", "install_erase", "pair", "change_wifi", "dashboard",
                                "open_accessibility_settings", "open_driver_settings", "install_usb_driver"];

#[derive(Debug, Clone, Serialize)]
pub struct Advice {
    pub answer: String,
    pub actions: Vec<String>,
    /// A concrete change the person can apply with one click (checked here; shown in full first).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

/// A change to the AI settings or the board choice. Only these fields, each checked; never a key,
/// a command or a file.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Fix {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// An http(s) address, or "" for the provider's usual one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Request options to set; null = leave that setting out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// A board id from the identify result's candidates (the web app checks it is one).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
}

fn plain_name(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:/@".contains(c))
}

/// Keeps only well-formed fields; None if nothing usable is left.
fn check_fix(value: &Value) -> Option<Fix> {
    let fix = Fix {
        model: value["model"].as_str().map(str::trim).filter(|m| plain_name(m, 120)).map(str::to_string),
        base_url: value["base_url"].as_str().map(str::trim)
            .filter(|u| u.is_empty() || (u.len() <= 200 && (u.starts_with("https://") || u.starts_with("http://"))
                                           && !u.contains(char::is_whitespace)))
            .map(str::to_string),
        options: value["options"].as_object().map(|o| o.iter()
            .filter(|(k, v)| plain_name(k, 60) && !matches!(k.as_str(), "model" | "messages" | "system")
                && match v {
                    Value::Null | Value::Bool(_) | Value::Number(_) => true,
                    Value::String(s) => s.len() <= 100,
                    _ => false,
                })
            .take(10)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<serde_json::Map<_, _>>())
            .filter(|o| !o.is_empty()),
        timeout_secs: value["timeout_secs"].as_u64().filter(|t| (10..=3600).contains(t)),
        board: value["board"].as_str().filter(|b| plain_name(b, 60) && !b.contains(['/', ':', '@'])).map(str::to_string),
    };
    (fix != Fix::default()).then_some(fix)
}

fn system_prompt() -> String {
    format!(r#"You are the KnoEnd setup assistant. You help one person get their ESP32-S3 touchscreen board
working as an KnoEnd keypad: find it over USB, install the firmware, pair it, put it on Wi-Fi,
and make its keys work in their apps. Many people using you will not want to troubleshoot: lead them,
one clear step at a time.

How to work:
- Start from the snapshot you are given (it is real data from their computer) and the knowledge
  below. Name the most likely cause first, then what to check or do next.
- Separate layers (cable and power, USB port and permissions, bootloader, firmware, Wi-Fi, companion,
  the app being typed into); work on the first layer that fails.
- The problem may be one nobody has seen. Reason from the layers and the evidence; say plainly what
  is known, what is a guess, and how to tell which.
- Prefer steps that change nothing before steps that change the board. Never suggest erasing unless
  needed, and say that erasing removes Wi-Fi and pairing.
- Never ask for, repeat or store passwords or API keys. They are typed only into the web app's own
  fields.
- Plain text only: no Markdown (no asterisks, no backticks, no headings); the reply is shown as is.
- Plain language, short sentences, numbered steps when there are several. No more than about 150
  words unless the person asks for detail. Commands go in their own line, exactly as typed.

Reply with ONLY a JSON object, no code fences:
{{"answer": "<your reply to the person>", "actions": [<zero or more of: {actions}>], "fix": <optional, see below>}}
Actions are buttons the person can press in the setup wizard: "rescan" (look for ports again),
"identify" (ask the board what it is; restarts it briefly), "install" (install the firmware),
"install_erase" (erase first, then install), "pair" (pair over USB and give it Wi-Fi), "change_wifi"
(move a board that is already set up to another network over USB, keeping its pairing; for a new
router, a changed password or another place), "dashboard".
On macOS only: "open_accessibility_settings" (opens Privacy & Security > Accessibility, where the
person switches on the app named in system_checks.accessibility.app_to_allow), "open_driver_settings"
(opens Login Items & Extensions, where the person switches on a driver under Driver Extensions), and
"install_usb_driver" (downloads WCH's USB-serial driver, checks Apple has notarized it and WCH signed
it, then opens its installer; the person clicks through and enters their Mac password there).
Offer only actions that are the right next step.

"fix" is a change the person can apply with one click; the web app shows it and, for AI settings,
saves it and runs the test again. Include it only when the evidence points to that exact change.
Fields (all optional): "model" (a name from from_the_web_app.ai_models_available when that list is
given), "base_url" (an http(s) address, or "" for the provider's usual one), "options" (request
settings to add; null leaves a setting out, e.g. {{"temperature": null}}), "timeout_secs" (10 to
3600, e.g. longer for a slow local model), "board" (a board id from identify_result.candidates,
when the person has told you which board they have). Never put keys or passwords in it. Say in
"answer" what the fix would change and why, and that pressing Apply makes the change: nothing has
changed until the person presses it, so never write that you changed or selected anything.

Do the work for the person. Read system_checks before asking them anything: it shows what the
computer itself reports (usb_serial_devices: each USB-serial chip and whether it has a port, on Linux
with the kernel drivers bound to it; on macOS also the WCH driver state, whether knoend may type
keys and which app to allow). When a step needs the person (a
password, an Allow switch, unplugging the board), say exactly what they will see and where to click,
offer the button that takes them there, and say what to tell you or press when it is done.

On Linux, read system_checks.desktop_session first when keys do not change with the app or are not
typed: on a Wayland session say plainly that KnoEnd does not follow apps or type keys there yet
(X11 and macOS only), that the board, Bluetooth typing and Claude questions still work, and that
choosing an X11 (Xorg) session at login makes everything work. Do not send them hunting for a fault.

Never tell the person to edit source code, rebuild or reinstall knoend to get past a problem.
When the provider is claude-code (Claude Code) or antigravity (Antigravity's `agy` command), the
person's own plan is used through a tool on this computer, with no API key. Read
ai_tools_on_this_computer, the tools found on this computer just now (provider, installed, path,
signed_in, sign_in): if the chosen one is not installed, say to install it or pick another provider
(only offer tools that are installed and supported); if signed_in is false, give its sign_in step,
then say to press Save and test again. Codex can be found but KnoEnd does not use it yet; say so if
asked. Never ask them for a key for either.
If the AI provider refuses a request, the fix is in the web app's AI settings (model, key, API
address, or Advanced > request options, where a value of null leaves a setting out); if a fix
truly needs a newer knoend, say that plainly and stop there.
When several boards look the same over USB (native-USB boards do), ask which board they have
before installing.

What the project knows:

{KNOWLEDGE}"#, actions = ACTIONS.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(", "))
}

fn parse(reply: &str) -> Advice {
    let start = reply.find('{');
    let end = reply.rfind('}');
    if let (Some(start), Some(end)) = (start, end) {
        if let Ok(value) = serde_json::from_str::<Value>(&reply[start..=end]) {
            if let Some(answer) = value["answer"].as_str() {
                let actions = value["actions"].as_array().into_iter().flatten()
                    .filter_map(Value::as_str)
                    .filter(|a| ACTIONS.contains(a))
                    .map(str::to_string)
                    .collect();
                return Advice { answer: answer.to_string(), actions, fix: check_fix(&value["fix"]) };
            }
        }
    }
    // Not the JSON we asked for: show the text, offer no buttons.
    Advice { answer: reply.trim().to_string(), actions: Vec::new(), fix: None }
}

/// Asks the model for advice on `question`, given the `snapshot` the service collected.
pub async fn advise(config: &AgentConfig, question: &str, snapshot: &Value) -> Result<Advice, String> {
    let message = format!(
        "Snapshot of the situation (collected automatically):\n{}\n\nThe person asks: {}",
        serde_json::to_string_pretty(snapshot).unwrap_or_default(),
        if question.trim().is_empty() { "What should I do next?" } else { question.trim() }
    );
    let reply = provider::complete(config, &system_prompt(), &[Message::user(message)]).await?;
    Ok(parse(&reply))
}

/// A tiny request to check that a provider, model and key work.
pub async fn check(config: &AgentConfig) -> Result<String, String> {
    let reply = provider::complete(config, "Reply with the single word OK.", &[Message::user("Ready?".into())]).await?;
    Ok(reply.trim().chars().take(40).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_is_built_into_the_prompt() {
        let prompt = system_prompt();
        assert!(prompt.contains("2.4 GHz only"));
        assert!(prompt.contains("CH340K"));
        assert!(prompt.contains("Never ask for, repeat or store passwords"));
        assert!(prompt.contains("install_usb_driver"));
        assert!(prompt.contains("Never tell the person to edit source code"));
        assert!(prompt.contains("never write that you changed or selected anything"));
        assert!(prompt.contains("1a86:7522"));
    }

    #[test]
    fn wayland_is_explained_not_debugged() {
        let prompt = system_prompt();
        assert!(prompt.contains("system_checks.desktop_session"));
        assert!(prompt.contains("X11 (Xorg) session at login"));
        assert!(prompt.contains("GNOME on Xorg"), "the knowledge entry is in the prompt");
    }

    #[test]
    fn claude_code_sign_in_is_read_not_asked() {
        let prompt = system_prompt();
        assert!(prompt.contains("ai_tools_on_this_computer, the tools found"));
        assert!(prompt.contains("give its sign_in step") && prompt.contains("Never ask them for a key for either"));
    }

    #[test]
    fn change_wifi_is_offered_for_a_moved_board() {
        let prompt = system_prompt();
        assert!(prompt.contains("\"change_wifi\""));
        assert!(prompt.contains("router changed"), "the knowledge entry is in the prompt");
        assert_eq!(parse(r#"{"answer": "Plug it in.", "actions": ["change_wifi"]}"#).actions, vec!["change_wifi"]);
    }

    #[test]
    fn parse_keeps_only_allowed_actions() {
        let advice = parse(r#"Sure: {"answer": "Try another cable.", "actions": ["rescan", "format_disk", "identify"]}"#);
        assert_eq!(advice.answer, "Try another cable.");
        assert_eq!(advice.actions, vec!["rescan", "identify"]);
    }

    #[test]
    fn fixes_keep_only_checked_fields() {
        let advice = parse(r#"{"answer": "Switch model.", "actions": [], "fix": {"model": "gpt-6-luna",
            "base_url": "file:///etc/passwd", "options": {"temperature": null, "messages": "x", "nested": {"a": 1},
            "reasoning_effort": "low"}, "timeout_secs": 5, "board": "../../x", "api_key": "sk-123"}}"#);
        let fix = advice.fix.expect("fix");
        assert_eq!(fix.model.as_deref(), Some("gpt-6-luna"));
        assert_eq!(fix.base_url, None);
        let options = fix.options.expect("options");
        assert_eq!(options.len(), 2);
        assert!(options["temperature"].is_null());
        assert_eq!(options["reasoning_effort"], "low");
        assert_eq!(fix.timeout_secs, None);
        assert_eq!(fix.board, None);
        assert!(parse(r#"{"answer": "a", "fix": {"model": "two words"}}"#).fix.is_none());
        assert_eq!(parse(r#"{"answer": "a", "fix": {"base_url": "", "board": "fnk0104b"}}"#).fix,
                   Some(Fix { base_url: Some(String::new()), board: Some("fnk0104b".into()), ..Fix::default() }));
    }

    #[test]
    fn parse_falls_back_to_plain_text() {
        let advice = parse("Plug it in again.");
        assert_eq!(advice.answer, "Plug it in again.");
        assert!(advice.actions.is_empty());
    }
}
