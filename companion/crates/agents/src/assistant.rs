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

/// Buttons the web app can offer, and what each does: the prompt is written from this list, so a
/// button added here (and in the web app's ACTIONS) is one the assistant knows.
pub const ACTIONS: &[(&str, &str)] = &[
    ("presentation_settings","open recipe notes-source and exact saved PPTX binding settings"),
    ("recipes_reload","validate and reload recipes from disk without a rebuild; invalid recipes leave the current catalog unchanged"),
    ("pager_settings", "open Windows pager settings: enable interruptions or set the existing local recorder .vbs shortcut"),
    ("pager_test", "put a synthetic incoming Teams call on the board; never makes a real call or starts recording"),
    ("rescan", "look for USB ports again"),
    ("identify", "ask the board on the chosen port what it is; restarts it briefly"),
    ("install", "install the firmware, keeping the board's Wi-Fi, pairing and settings"),
    ("install_erase", "erase the board first, then install: removes Wi-Fi, pairing and settings"),
    ("pair", "pair the board with this computer over USB and optionally give it 2.4 GHz Wi-Fi"),
    ("look_settings", "open the Keys and look step to configure key size, look (theme), orientation and fonts over USB or Wi-Fi"),
    ("change_wifi", "move a board that is already set up to another network over USB, keeping its pairing: for a new \
                     router, a changed password or another place"),
    ("open_bluetooth_settings", "open this computer's Bluetooth settings to pair the board as the \"KnoEnd\" Bluetooth \
                                 Low Energy keyboard (optional; the companion also types shortcuts over USB or Wi-Fi)"),
    ("app_setup", "open the Your apps step: the apps installed on this computer, which already have keys, and the AI \
                   drafting keys for the ones the person picks (up to 12 at once) before they open them; \
                   it takes no request, so not for changing an app's keys (that is a fix's \"redraft\")"),
    ("dashboard", "go to the dashboard"),
    ("open_accessibility_settings", "macOS only: opens Privacy & Security > Accessibility, where the person switches on \
                                     the app named in system_checks.accessibility.app_to_allow"),
    ("open_driver_settings", "macOS only: opens Login Items & Extensions, where the person switches on a driver under \
                              Driver Extensions"),
    ("install_usb_driver", "macOS only: downloads WCH's USB-serial driver, checks Apple has notarized it and WCH signed \
                            it, then opens its installer; the person clicks through and enters their Mac password there"),
];

fn is_action(name: &str) -> bool {
    ACTIONS.iter().any(|(a, _)| *a == name)
}

/// Rounds of checks the assistant may ask for before it must answer.
pub const CHECK_ROUNDS: usize = 2;

#[derive(Debug, Clone, Serialize)]
pub struct Advice {
    pub answer: String,
    pub actions: Vec<String>,
    /// Checks the assistant asked for (ids from the snapshot's checks_available): run them and ask
    /// again with the results (`advise`, `earlier`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<String>,
    /// The model's reply as it came, to carry the conversation into the next round.
    #[serde(skip)]
    pub raw: String,
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
    /// Settings to change, by id, from the snapshot's settings list (the service checks each value
    /// against that setting's choices before anything changes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Map<String, Value>>,
    /// Rework an AI-drafted library's keys as the person asked (the service checks the library).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redraft: Option<Redraft>,
}

/// An AI-drafted library to draft again, and what the person asked for, in their words.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Redraft {
    pub library: String,
    pub request: String,
}

/// A paired board's id (its MAC address in hex), not an identify candidate.
fn paired_board_id(s: &str) -> bool {
    s.len() == 12 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn plain_name(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:/@".contains(c))
}

/// Keeps only well-formed fields; None if nothing usable is left.
fn check_fix(value: &Value) -> Option<Fix> {
    let mut fix = Fix {
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
        settings: value["settings"].as_object().map(|o| o.iter()
            .filter(|(k, v)| plain_name(k, 40) && match v {
                Value::Bool(_) | Value::Number(_) => true,
                Value::String(s) => s.len() <= 60,
                _ => false,
            })
            .take(10)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<serde_json::Map<_, _>>())
            .filter(|o| !o.is_empty()),
        redraft: value["redraft"]["library"].as_str()
            .filter(|l| l.starts_with("ai.") && plain_name(l, 60) && !l.contains(['/', ':', '@']))
            .zip(value["redraft"]["request"].as_str().map(str::trim)
                .filter(|r| !r.is_empty() && r.chars().count() <= crate::REDRAFT_REQUEST_MAX && !r.contains(char::is_control)))
            .map(|(library, request)| Redraft { library: library.into(), request: request.into() }),
    };
    // A paired board's id put where an install's board goes (seen 2026-10-24: a key size fix for
    // one board was refused as "not installable"): it names the board whose settings change.
    if let Some(board) = fix.board.take_if(|b| paired_board_id(b)) {
        if let Some(settings) = fix.settings.as_mut() {
            settings.entry("board").or_insert(board.into());
        }
    }
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
- Local file settings marked private expose configured/existing status, never the path. Do not
  infer or request those paths in chat or replace a configured value because current is null.
  Offer presentation_settings or pager_settings so the person can edit paths in the local UI.
- Plain text only: no Markdown (no asterisks, no backticks, no headings); the reply is shown as is.
- Plain language, short sentences, numbered steps when there are several. No more than about 150
  words unless the person asks for detail. Commands go in their own line, exactly as typed.

Reply with ONLY a JSON object, no code fences:
{{"answer": "<your reply to the person>", "actions": [<zero or more button ids>], "fix": <optional>,
 "checks": [<optional: check ids, see below>]}}

Buttons ("actions") the person can press in the web app; offer only the right next step:
{actions}

"fix" is a change the person can apply with one click: the web app shows it in full, and nothing
changes until they press Apply. Include it only when the evidence points to that exact change, say in
"answer" what it would change and why, and never write that you changed or selected anything. Fields
(all optional):
- "settings": {{"<id>": <value>}} for any setting in the snapshot's "settings" list, with a value from
  that setting's "choices". That list is everything the person can change and says what each one
  means, what it is now and when a change shows; read it rather than assuming what exists. For
  example keys too small or too hard to read: "key_size"; a screen hard to read in daylight: "theme".
  Settings marked "per_board" are one board's own (each board keeps its own look; board_looks has
  every paired board's): add "board": "<board id>" to "settings" to change that board's, else they
  change the board the list names. Never change one board's look to fix another.
- For the AI settings: "model" (a name from from_the_web_app.ai_models_available when that list is
  given), "base_url" (an http(s) address, or "" for the provider's usual one), "options" (request
  settings to add; null leaves a setting out, e.g. {{"temperature": null}}), "timeout_secs" (10 to
  3600, e.g. longer for a slow local model).
- "board": a board id from identify_result.candidates, when the person has told you which board they
  have (for installing; a paired board's settings take "board" inside "settings" instead).
- "redraft": {{"library": "<id>", "request": "<what the person asked, in their words>"}} when they
  want an app's keys changed (other keys, rearranged, a layout like the real thing): the configure
  agent drafts that app's keys again from the current ones and the request, in about 15 seconds,
  and the board shows them when that app is in front. Only for a library in ai_drafted_libraries
  (id, name and key labels in order); keys fill the grid row by row in that order. 10 to 16 keys
  are a 4x4 grid, which a board shows only with key_size "small" (bigger keys split the library
  into pages): check that board's key_size and add it to the same fix when needed. This is the
  only way you change keys: whenever the person asks for other, more, fewer or rearranged keys for
  an app in ai_drafted_libraries, the fix must have "redraft". Do not offer app_setup for that (it
  drafts apps from scratch and cannot take their request), and never offer a key_size change alone
  as a new layout: it only shows the same keys smaller. For keys not in that list (built-in or the
  person's own) send them to the Edit keys tab, and never say you will change keys yourself.
Never put keys or passwords in a fix.

You have no tools and cannot call functions. The only way to run a check is the "checks" field of
your JSON reply; never write a function call, and never write a check's result you have not been
given: the person must be able to trust that what you say you checked was checked.
"checks": when the snapshot does not settle what is wrong, ask the service to run checks from the
snapshot's "checks_available" (each says what it finds out; all are read-only and touch no board).
Reply then with "checks" and a short "answer" saying what you are checking; you get the results and
answer again. At most {rounds} rounds: after that, answer with what you know.

Do the work for the person. Read system_checks before asking them anything: it shows what the
computer itself reports (usb_serial_devices: each USB-serial chip and whether it has a port, on Linux
and Windows with the drivers bound to it; on macOS also the WCH driver state, whether knoend may type
keys and which app to allow). When a step needs the person (a
password, an Allow switch, unplugging the board), say exactly what they will see and where to click,
offer the button that takes them there, and say what to tell you or press when it is done.

On Linux, read system_checks.desktop_session first when keys do not change with the app or are not
typed: on a Wayland session say plainly that KnoEnd does not follow apps or type keys there yet
(X11, macOS and Windows only), that the board, Bluetooth typing and Claude questions still work, and that
choosing an X11 (Xorg) session at login makes everything work. Do not send them hunting for a fault.

Resolve, not only explain. If the snapshot or a check can settle a question, settle it before you
answer: never write that you "can check" something later, and never ask the person to retry a step
whose outcome a check can show. service_status.link "connected" with a service_status.board means the
board is paired with this computer and on the network right now. Use the checks to find out instead
of asking the person, offer the
button or fix that makes the change, and afterwards check again (a check, or ask them to press the
step) to confirm it worked. Before sending the person back to a step, find out whether it is needed:
a board flashed without erasing keeps its pairing and Wi-Fi, so if it comes back on the network
(find_boards_on_network: paired_with_this_computer) there is nothing to pair or join again. A step
that fails seconds after the board restarted (after a flash or identify) is often only early: the
board may still be joining Wi-Fi; say to wait a few seconds and try once more before anything else.
The settings list, the checks and the service log grow as KnoEnd grows: read them each time rather
than relying on what this prompt names.

Never tell the person to edit source code, rebuild or reinstall knoend to get past a problem.
When the provider is claude-code (Claude Code), antigravity (Antigravity's `agy` command) or codex
(OpenAI's `codex` command), the person's own plan is used through a tool on this computer, with no
API key. Read ai_tools_on_this_computer, the tools found on this computer just now (provider,
installed, path, signed_in, sign_in): if the chosen one is not installed, say to install it or pick
another provider (only offer tools that are installed and supported); if signed_in is false,
give its sign_in step, then say to press Save and test again. Never ask them for a key for either.
If the AI provider refuses a request, the fix is in the web app's AI settings (model, key, API
address, or Advanced > request options, where a value of null leaves a setting out); if a fix
truly needs a newer knoend, say that plainly and stop there.
When several boards look the same over USB (native-USB boards do), ask which board they have
before installing.

Which board is it: read last_board_identified (or run identify_board when it is missing or old).
When known is false, KnoEnd does not list this board. Name the board it most likely is from the
evidence: firmware_on_board's components name the vendor's board support package and its display
and touch drivers ("waveshare__esp32_s3_touch_amoled_2_06", "esp_lcd_sh8601") and outweigh an app's
project name, which is often a demo's ("phone_s3_box_3" on a board that is not an ESP32-S3-BOX-3);
flash size and USB help. Say how sure you are and why. Then say whether KnoEnd can run on it:
knoend_firmware_drives lists the display and touch chips KnoEnd's firmware can drive; a board whose
display is another kind (AMOLED, QSPI, RGB, MIPI) or chip cannot run KnoEnd yet, and installing it
would leave a blank screen. Then say plainly not to install KnoEnd on it, that its own firmware is
untouched, and that it can be backed up first (esptool read_flash) before trying anything. Never
suggest one of the listed boards just because its chip matches.

What the project knows:

{KNOWLEDGE}"#, actions = ACTIONS.iter().map(|(a, what)| format!("- \"{a}\": {what}")).collect::<Vec<_>>().join("\n"),
            rounds = CHECK_ROUNDS)
}

/// Check ids in a made-up function call ("<invoke name=...>", "<parameter ...>find_boards...</...>"):
/// some models write one instead of the JSON asked for, and then invent its result.
fn invented_calls(reply: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for piece in reply.split(['<', '>', '"']) {
        let piece = piece.trim();
        if plain_name(piece, 40) && piece.contains('_') && !piece.contains(['/', ':', '.', '@']) && !ids.iter().any(|i| i == piece) {
            ids.push(piece.to_string());
        }
    }
    ids
}

fn parse(reply: &str) -> Advice {
    // A made-up call: its "result" is invented, so the answer is dropped and the named checks (the
    // service keeps only real ones) are run; the model is asked again with what they find.
    if reply.contains("<invoke") || reply.contains("<function_calls") {
        return Advice { answer: String::new(), actions: Vec::new(), checks: invented_calls(reply), raw: reply.to_string(), fix: None };
    }
    let start = reply.find('{');
    let end = reply.rfind('}');
    if let (Some(start), Some(end)) = (start, end) {
        if let Ok(value) = serde_json::from_str::<Value>(&reply[start..=end]) {
            if let Some(answer) = value["answer"].as_str() {
                let actions = value["actions"].as_array().into_iter().flatten()
                    .filter_map(Value::as_str)
                    .filter(|a| is_action(a))
                    .map(str::to_string)
                    .collect();
                // Ids only; the service runs only those it lists.
                let checks = value["checks"].as_array().into_iter().flatten()
                    .filter_map(Value::as_str)
                    .filter(|c| plain_name(c, 40))
                    .take(4)
                    .map(str::to_string)
                    .collect();
                return Advice { answer: answer.to_string(), actions, checks, raw: reply.to_string(), fix: check_fix(&value["fix"]) };
            }
        }
    }
    // Not the JSON we asked for: show the text, offer no buttons.
    Advice { answer: reply.trim().to_string(), actions: Vec::new(), checks: Vec::new(), raw: reply.to_string(), fix: None }
}

/// Asks the model for advice on `question`, given the `snapshot` the service collected. `earlier`:
/// the rounds so far, each the model's reply and the results of the checks it asked for.
pub async fn advise(config: &AgentConfig, question: &str, snapshot: &Value, earlier: &[(String, Value)]) -> Result<Advice, String> {
    let message = format!(
        "Snapshot of the situation (collected automatically):\n{}\n\nThe person asks: {}",
        serde_json::to_string_pretty(snapshot).unwrap_or_default(),
        if question.trim().is_empty() { "What should I do next?" } else { question.trim() }
    );
    let mut messages = vec![Message::user(message)];
    for (i, (reply, results)) in earlier.iter().enumerate() {
        messages.push(Message::assistant(reply.clone()));
        let last = i + 1 >= CHECK_ROUNDS;
        messages.push(Message::user(format!(
            "Results of the checks you asked for:\n{}\n\n{}",
            serde_json::to_string_pretty(results).unwrap_or_default(),
            if last { "No more checks now: answer the person with what you know." } else { "Answer the person, or ask for more checks." })));
    }
    let reply = provider::complete(config, &system_prompt(), &messages).await?;
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
        assert!(prompt.contains("Local file settings marked private"));
        assert!(prompt.contains("replace a configured value because current is null"));
    }

    #[test]
    fn wayland_is_explained_not_debugged() {
        let prompt = system_prompt();
        assert!(prompt.contains("system_checks.desktop_session"));
        assert!(prompt.contains("X11 (Xorg) session at login"));
        assert!(prompt.contains("GNOME on Xorg"), "the knowledge entry is in the prompt");
        assert!(prompt.contains("X11, macOS and Windows only"));
        assert!(prompt.contains("Windows SetupAPI"), "the Windows knowledge entry is in the prompt");
    }

    #[test]
    fn an_unknown_board_is_named_from_its_firmware_not_guessed() {
        let prompt = system_prompt();
        assert!(prompt.contains("last_board_identified") && prompt.contains("identify_board"));
        assert!(prompt.contains("outweigh an app's") && prompt.contains("knoend_firmware_drives"));
        assert!(prompt.contains("not to install KnoEnd on it"));
    }

    #[test]
    fn a_look_fix_names_its_board() {
        let prompt = system_prompt();
        assert!(prompt.contains("\"per_board\"") && prompt.contains("board_looks") && prompt.contains("\"board\": \"<board id>\""));
    }

    #[test]
    fn keys_are_changed_by_a_redraft_of_an_ai_library_only() {
        let prompt = system_prompt();
        assert!(prompt.contains("\"redraft\": {\"library\": \"<id>\"") && prompt.contains("ai_drafted_libraries"));
        assert!(prompt.contains("key_size \"small\"") && prompt.contains("Edit keys tab"));
        assert!(prompt.contains("the fix must have \"redraft\"") && prompt.contains("Do not offer app_setup for that"));
        let fix = parse(r#"{"answer": "a", "fix": {"redraft": {"library": "ai.calculator", "request": " numbers like a real calculator "}}}"#).fix;
        assert_eq!(fix.and_then(|f| f.redraft), Some(Redraft { library: "ai.calculator".into(), request: "numbers like a real calculator".into() }));
        for bad in [r#"{"library": "builtin.vscode", "request": "x"}"#, r#"{"library": "ai.calculator", "request": ""}"#,
                    r#"{"library": "ai.calculator", "request": "a\nb"}"#, r#"{"library": "ai.calculator"}"#] {
            assert!(parse(&format!(r#"{{"answer": "a", "fix": {{"redraft": {bad}}}}}"#)).fix.is_none(), "{bad}");
        }
    }

    #[test]
    fn a_paired_board_in_the_install_field_names_the_settings_board() {
        // Synthetic reply: a board-specific key-size fix keeps its target board.
        let fix = parse(r#"{"answer": "a", "fix": {"settings": {"key_size": "small"}, "board": "a1b2c3d4e5f6"}}"#).fix.expect("fix");
        assert_eq!(fix.board, None);
        assert_eq!(fix.settings.expect("settings")["board"], "a1b2c3d4e5f6");
        assert_eq!(parse(r#"{"answer": "a", "fix": {"board": "onx2432g028"}}"#).fix.and_then(|f| f.board).as_deref(), Some("onx2432g028"));
        assert!(parse(r#"{"answer": "a", "fix": {"board": "a1b2c3d4e5f6"}}"#).fix.is_none(), "nothing left to apply");
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
        assert!(prompt.contains("\"open_bluetooth_settings\""));
        assert!(prompt.contains("\"look_settings\""));
        assert!(prompt.contains("router changed"), "the knowledge entry is in the prompt");
        assert_eq!(parse(r#"{"answer": "Plug it in.", "actions": ["change_wifi", "open_bluetooth_settings", "look_settings"]}"#).actions,
                   vec!["change_wifi", "open_bluetooth_settings", "look_settings"]);
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
    fn settings_and_checks_come_from_the_snapshot_not_the_prompt() {
        let prompt = system_prompt();
        assert!(prompt.contains("\"settings\": {\"<id>\": <value>}") && prompt.contains("checks_available"));
        assert!(prompt.contains("read them each time rather"), "the lists grow; the prompt says to read them");
        assert!(prompt.contains("- \"install_erase\": erase the board first"), "buttons are listed with what they do");
        assert!(prompt.contains("paired_with_this_computer"), "a flash that kept settings needs no pairing again");
        assert!(prompt.contains("never write that you \"can check\" something later"));
        let advice = parse(r#"{"answer": "Bigger keys.", "fix": {"settings": {"key_size": "big", "bad name": 1, "deep": {"x": 1}}},
                              "checks": ["find_boards_on_network", "rm -rf /"]}"#);
        let settings = advice.fix.expect("fix").settings.expect("settings");
        assert_eq!(settings.len(), 1);
        assert_eq!(settings["key_size"], "big");
        assert_eq!(advice.checks, vec!["find_boards_on_network"]);
    }

    #[test]
    fn an_invented_function_call_becomes_a_real_check() {
        let reply = "I will check.\n<function_calls>\n<invoke name=\"call_service\">\n<parameter name=\"request\">find_boards_on_network</parameter>\n</invoke>\n</function_calls>\nYour board is on the network.";
        let advice = parse(reply);
        assert!(advice.answer.is_empty(), "the invented result is not shown");
        assert!(advice.checks.contains(&"find_boards_on_network".to_string()));
        assert!(system_prompt().contains("You have no tools and cannot call functions"));
    }

    #[test]
    fn parse_falls_back_to_plain_text() {
        let advice = parse("Plug it in again.");
        assert_eq!(advice.answer, "Plug it in again.");
        assert!(advice.actions.is_empty());
    }
}
