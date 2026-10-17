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
pub const ACTIONS: &[&str] = &["rescan", "identify", "install", "install_erase", "pair", "dashboard"];

#[derive(Debug, Clone, Serialize)]
pub struct Advice {
    pub answer: String,
    pub actions: Vec<String>,
}

fn system_prompt() -> String {
    format!(r#"You are the OpenPager setup assistant. You help one person get their ESP32-S3 touchscreen board
working as an OpenPager keypad: find it over USB, install the firmware, pair it, put it on Wi-Fi,
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
{{"answer": "<your reply to the person>", "actions": [<zero or more of: {actions}>]}}
Actions are buttons the person can press in the setup wizard: "rescan" (look for ports again),
"identify" (ask the board what it is; restarts it briefly), "install" (install the firmware),
"install_erase" (erase first, then install), "pair" (pair over USB and give it Wi-Fi), "dashboard".
Offer only actions that are the right next step.

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
                return Advice { answer: answer.to_string(), actions };
            }
        }
    }
    // Not the JSON we asked for: show the text, offer no buttons.
    Advice { answer: reply.trim().to_string(), actions: Vec::new() }
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
    }

    #[test]
    fn parse_keeps_only_allowed_actions() {
        let advice = parse(r#"Sure: {"answer": "Try another cable.", "actions": ["rescan", "format_disk", "identify"]}"#);
        assert_eq!(advice.answer, "Try another cable.");
        assert_eq!(advice.actions, vec!["rescan", "identify"]);
    }

    #[test]
    fn parse_falls_back_to_plain_text() {
        let advice = parse("Plug it in again.");
        assert_eq!(advice.answer, "Plug it in again.");
        assert!(advice.actions.is_empty());
    }
}
