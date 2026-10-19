//! AI tools already on this computer, for people with a plan but no API key, each signed in with
//! the person's own account: Claude Code (`claude -p`) and Antigravity's CLI (`agy -p`).
//!
//! The same system prompt and messages as every other provider go in; text comes out. These tools
//! are agents that could run commands, and site drafts carry text read from web pages, so each call
//! is locked down and runs in an empty folder of its own, removed afterwards. The reply is only
//! ever read as text.
//!   - Claude Code: no tools at all, restricted mode (no user, project or local settings, so none
//!     of KnoEnd's own hooks fire and no plugins load), no MCP servers, no saved session.
//!   - Antigravity: no switch turns its tools off, but print mode cannot ask permission, so running
//!     commands and reading files outside its folder are refused (checked 2026-10-19, agy 1.4.2);
//!     it may only write into the empty folder. It also runs sandboxed, with slash commands off, and
//!     is told to answer as text. It has no system-prompt option and keeps its own conversation
//!     history, so the instructions go first in the prompt.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncWriteExt;

use crate::provider::Message;

const CLAUDE: &str = "claude";
const AGY: &str = "agy";

/// Where a command is: the PATH, then the places installers use (a background service may start
/// with a short PATH).
fn find(command: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend([home.join(".local/bin"), home.join(".claude/local"), home.join(".npm-global/bin"),
                 PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin"), PathBuf::from("/usr/bin")]);
    dirs.into_iter().map(|d| d.join(command)).find(|p| p.is_file())
}

pub fn claude_path() -> Option<PathBuf> {
    find(CLAUDE)
}

pub fn agy_path() -> Option<PathBuf> {
    find(AGY)
}

/// A fresh, empty folder for one call.
fn work_dir(tool: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("knoend-{tool}-{}-{}", std::process::id(),
                                                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    std::fs::create_dir(&dir).map_err(|e| format!("could not make a working folder: {e}"))?;
    Ok(dir)
}

/// What the setup snapshot shows about Claude Code: installed, version, signed in. Facts only.
pub async fn claude_status() -> Value {
    let Some(path) = claude_path() else { return serde_json::json!({"installed": false}) };
    let run = |args: &'static [&'static str]| {
        let path = path.clone();
        async move {
            let out = tokio::time::timeout(Duration::from_secs(10),
                                           tokio::process::Command::new(&path).args(args).env_remove("CLAUDECODE")
                                               .stdin(Stdio::null()).output()).await.ok()?.ok()?;
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        }
    };
    let version = run(&["--version"]).await;
    let auth: Value = run(&["auth", "status"]).await.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    serde_json::json!({
        "installed": true,
        "version": version,
        "signed_in": auth["loggedIn"].as_bool(),
        "sign_in_method": auth["authMethod"].as_str(),
    })
}

/// The conversation as one prompt: the last message alone, or earlier turns labelled before it.
pub fn prompt_text(messages: &[Message]) -> String {
    match messages {
        [] => String::new(),
        [only] => only.content.clone(),
        [earlier @ .., last] => {
            let mut text = String::from("The conversation so far:\n\n");
            for m in earlier {
                text.push_str(&format!("[{}]\n{}\n\n", if m.role == "user" { "User" } else { "You" }, m.content));
            }
            text.push_str(&format!("Now answer this:\n\n{}", last.content));
            text
        }
    }
}

/// The arguments for one locked-down call. The prompt goes on stdin.
pub fn claude_args(system_file: &Path, model: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = ["-p", "--restricted", "--tools", "", "--strict-mcp-config", "--permission-prompts", "none",
                                 "--no-session-persistence", "--disable-slash-commands", "--output-format", "json",
                                 "--system-prompt-file"].iter().map(|s| s.to_string()).collect();
    args.push(system_file.display().to_string());
    if let Some(model) = model.filter(|m| !m.is_empty()) {
        args.extend(["--model".into(), model.into()]);
    }
    args
}

/// A failure, with what the person can do about it.
fn explain(detail: &str) -> String {
    let lower = detail.to_lowercase();
    let todo = if lower.contains("authenticat") || lower.contains("log in") || lower.contains("login") || lower.contains("oauth") {
        "open a terminal, run `claude`, and sign in with /login; then test again here"
    } else if lower.contains("model") {
        "leave the model empty in AI settings (Claude Code's own choice), or pick opus, sonnet or haiku"
    } else if lower.contains("limit") || lower.contains("usage") {
        "your Claude plan's usage limit is reached; wait until it resets, or choose another provider in AI settings"
    } else {
        "run `claude` in a terminal to check it works, or choose another provider in AI settings"
    };
    format!("Claude Code: {}. What to do: {todo}", detail.trim().trim_end_matches('.'))
}

/// One completion through Claude Code.
pub async fn complete_claude(model: Option<&str>, timeout_secs: u64, system: &str, messages: &[Message]) -> Result<String, String> {
    let path = claude_path().ok_or("Claude Code (the `claude` command) is not installed on this computer. What to do: \
                                    install it from claude.com/claude-code and sign in, or choose another provider in AI settings")?;
    // A fresh, empty folder per call: nothing for the tool to read, and nothing left behind.
    let dir = work_dir("claude")?;
    let result = run_claude(&path, &dir, model, timeout_secs, system, messages).await;
    let _ = std::fs::remove_dir_all(&dir);
    result
}

async fn run_claude(path: &Path, dir: &Path, model: Option<&str>, timeout_secs: u64, system: &str, messages: &[Message])
                    -> Result<String, String> {
    // The system prompt as a file: it can be longer than one command-line argument may be.
    let system_file = dir.join("system.txt");
    std::fs::write(&system_file, system).map_err(|e| e.to_string())?;
    let mut child = tokio::process::Command::new(path)
        .args(claude_args(&system_file, model))
        .current_dir(dir)
        .env_remove("CLAUDECODE")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn().map_err(|e| explain(&format!("could not start {}: {e}", path.display())))?;
    let mut stdin = child.stdin.take().expect("piped");
    stdin.write_all(prompt_text(messages).as_bytes()).await.map_err(|e| e.to_string())?;
    drop(stdin);
    let out = tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await
        .map_err(|_| explain(&format!("no reply within {timeout_secs} s; try again, or ask the setup assistant for a longer wait")))?
        .map_err(|e| e.to_string())?;
    let reply: Value = match serde_json::from_slice(&out.stdout) {
        Ok(v) => v,
        Err(_) => {
            let err = String::from_utf8_lossy(&out.stderr);
            let detail = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it stopped without a reply");
            return Err(explain(detail));
        }
    };
    parse_reply(&reply)
}

/// The text of `claude -p --output-format json`, or its error.
pub fn parse_reply(reply: &Value) -> Result<String, String> {
    let text = reply["result"].as_str().unwrap_or_default();
    if reply["is_error"].as_bool().unwrap_or(false) || reply["subtype"].as_str().is_some_and(|s| s != "success") {
        return Err(explain(if text.is_empty() { reply["subtype"].as_str().unwrap_or("error") } else { text }));
    }
    if text.trim().is_empty() {
        return Err("empty reply from the model".into());
    }
    Ok(text.to_string())
}

// ---- Antigravity ----

/// Linux allows one command-line argument up to 128 KiB; the prompt is one argument.
const AGY_MAX_PROMPT: usize = 120 * 1024;

/// Told first, because agy has no system-prompt option.
const AGY_PREAMBLE: &str = "Answer from the instructions and request below alone. Do not use any tools: \
do not run commands, read or write files, or browse. Reply only with the text the instructions ask for.";

/// The whole prompt for agy: our preamble, the instructions, then the request.
pub fn agy_prompt(system: &str, messages: &[Message]) -> String {
    format!("{AGY_PREAMBLE}\n\n<instructions>\n{system}\n</instructions>\n\n<request>\n{}\n</request>", prompt_text(messages))
}

/// The arguments for one call. The prompt is attached to -p, so text starting with "-" is not
/// taken for an option.
pub fn agy_args(prompt: &str, model: Option<&str>, timeout_secs: u64) -> Vec<String> {
    let mut args: Vec<String> = vec!["--sandbox".into(), "--disable-slash-commands".into(), "--output-format".into(), "json".into(),
                                     "--print-timeout".into(), format!("{timeout_secs}s")];
    if let Some(model) = model.filter(|m| !m.is_empty()) {
        args.extend(["--model".into(), model.into()]);
    }
    args.push(format!("-p={prompt}"));
    args
}

fn explain_agy(detail: &str) -> String {
    let lower = detail.to_lowercase();
    let todo = if lower.contains("auth") || lower.contains("sign in") || lower.contains("login") || lower.contains("credential") {
        "open a terminal, run `agy`, and sign in with your Google account; then test again here"
    } else if lower.contains("model") {
        "leave the model empty in AI settings (Antigravity's own choice), or pick one from the list"
    } else if lower.contains("quota") || lower.contains("limit") {
        "your plan's usage limit is reached; wait until it resets, or choose another provider in AI settings"
    } else {
        "run `agy` in a terminal to check it works, or choose another provider in AI settings"
    };
    format!("Antigravity: {}. What to do: {todo}", detail.trim().trim_end_matches('.'))
}

/// The text of `agy -p --output-format json`, or its error.
pub fn parse_agy_reply(reply: &Value) -> Result<String, String> {
    let text = reply["response"].as_str().unwrap_or_default();
    let status = reply["status"].as_str().unwrap_or("no status");
    if status != "SUCCESS" {
        return Err(explain_agy(if text.is_empty() { status } else { text }));
    }
    if text.trim().is_empty() {
        let denied: Vec<&str> = reply["denied_actions"].as_array().into_iter().flatten()
            .filter_map(|a| a["display_name"].as_str().or(a["action"].as_str())).collect();
        return Err(if denied.is_empty() {
            "Antigravity: empty reply from the model. What to do: try again, or pick another model in AI settings".into()
        } else {
            format!("Antigravity: the model tried to use a tool ({}), which KnoEnd does not allow, and gave no answer. \
                     What to do: try again, or pick another model in AI settings", denied.join(", "))
        });
    }
    Ok(text.to_string())
}

/// What the setup snapshot shows about Antigravity's CLI: installed, and signed in when it can
/// list its models. Facts only.
pub async fn agy_status() -> Value {
    let Some(path) = agy_path() else { return serde_json::json!({"installed": false}) };
    let models = agy_models_at(&path).await;
    serde_json::json!({"installed": true, "signed_in": models.as_ref().map(|m| !m.is_empty()).ok(),
                       "error": models.err()})
}

/// The models agy offers (the first column of `agy models`).
pub async fn agy_models() -> Result<Vec<String>, String> {
    let path = agy_path().ok_or("Antigravity's CLI (`agy`) is not installed on this computer")?;
    agy_models_at(&path).await
}

async fn agy_models_at(path: &Path) -> Result<Vec<String>, String> {
    let dir = work_dir("agy")?;
    let out = tokio::time::timeout(Duration::from_secs(30),
                                   tokio::process::Command::new(path).arg("models").current_dir(&dir)
                                       .stdin(Stdio::null()).kill_on_drop(true).output()).await;
    let _ = std::fs::remove_dir_all(&dir);
    let out = out.map_err(|_| "agy models took too long".to_string())?.map_err(|e| e.to_string())?;
    let mut models: Vec<String> = String::from_utf8_lossy(&out.stdout).lines()
        .filter_map(|l| l.split('\t').next()).map(str::trim)
        .filter(|m| !m.is_empty() && !m.contains(' ')).map(str::to_string).collect();
    if !out.status.success() || models.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it listed no models").trim().to_string());
    }
    models.sort();
    Ok(models)
}

/// One completion through Antigravity's CLI.
pub async fn complete_agy(model: Option<&str>, timeout_secs: u64, system: &str, messages: &[Message]) -> Result<String, String> {
    let path = agy_path().ok_or("Antigravity's CLI (the `agy` command) is not installed on this computer. What to do: \
                                 install antigravity-cli and sign in, or choose another provider in AI settings")?;
    let prompt = agy_prompt(system, messages);
    if prompt.len() > AGY_MAX_PROMPT {
        return Err(format!("Antigravity: the request is {} KB, more than the {} KB one command can take. What to do: \
                            choose another provider in AI settings for this one", prompt.len() / 1024, AGY_MAX_PROMPT / 1024));
    }
    let dir = work_dir("agy")?;
    let run = tokio::process::Command::new(&path)
        .args(agy_args(&prompt, model, timeout_secs))
        .current_dir(&dir)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(Duration::from_secs(timeout_secs + 15), run).await;
    let _ = std::fs::remove_dir_all(&dir);
    let out = out.map_err(|_| explain_agy(&format!("no reply within {timeout_secs} s; try again, or ask the setup assistant for a longer wait")))?
        .map_err(|e| explain_agy(&format!("could not start {}: {e}", path.display())))?;
    match serde_json::from_slice::<Value>(&out.stdout) {
        Ok(reply) => parse_agy_reply(&reply),
        Err(_) => {
            let err = String::from_utf8_lossy(&out.stderr);
            Err(explain_agy(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it stopped without a reply")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_call_is_locked_down() {
        let args = claude_args(Path::new("/tmp/x/system.txt"), None);
        let joined = args.join(" ");
        for flag in ["-p", "--restricted", "--strict-mcp-config", "--no-session-persistence", "--disable-slash-commands"] {
            assert!(args.iter().any(|a| a == flag), "{flag} missing from {joined}");
        }
        let tools = args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(args[tools + 1], "", "no tools at all");
        let prompts = args.iter().position(|a| a == "--permission-prompts").unwrap();
        assert_eq!(args[prompts + 1], "none");
        assert!(!args.iter().any(|a| a.contains("dangerously") || a == "--bare"), "never bypass permissions; --bare would need an API key");
        assert!(!args.contains(&"--model".to_string()), "no model: Claude Code's own choice");
        assert!(claude_args(Path::new("s"), Some("sonnet")).ends_with(&["--model".into(), "sonnet".into()]));
    }

    #[test]
    fn agy_gets_the_instructions_first_and_no_permission_bypass() {
        let prompt = agy_prompt("SYSTEM", &[Message::user("-p --dangerously-skip-permissions".into())]);
        assert!(prompt.starts_with(AGY_PREAMBLE) && prompt.find("SYSTEM") < prompt.find("<request>"));
        let args = agy_args(&prompt, None, 300);
        assert!(args.iter().any(|a| a == "--sandbox") && args.iter().any(|a| a == "--disable-slash-commands"));
        assert!(!args.iter().any(|a| a.starts_with("--dangerously") || a == "--mode"), "{args:?}");
        assert!(args.last().unwrap().starts_with("-p="), "the prompt is attached to -p, never an option of its own");
        assert!(agy_args("x", Some("gemini-3.8-flash-low"), 60).windows(2).any(|w| w == ["--model", "gemini-3.8-flash-low"]));
    }

    #[test]
    fn agy_replies_and_refused_tools() {
        assert_eq!(parse_agy_reply(&json!({"status": "SUCCESS", "response": "{\"ok\":true}\n"})).unwrap(), "{\"ok\":true}\n");
        let err = parse_agy_reply(&json!({"status": "SUCCESS", "response": "",
                                          "denied_actions": [{"action": "command", "display_name": "RunCommand"}]})).unwrap_err();
        assert!(err.contains("RunCommand") && err.contains("does not allow"), "{err}");
        assert!(parse_agy_reply(&json!({"status": "ERROR", "response": "not authenticated"})).unwrap_err().contains("sign in"));
    }

    #[test]
    fn earlier_turns_are_labelled() {
        assert_eq!(prompt_text(&[Message::user("hi".into())]), "hi");
        let text = prompt_text(&[Message::user("draft".into()), Message::assistant("bad".into()), Message::user("fix it".into())]);
        assert!(text.contains("[User]\ndraft") && text.contains("[You]\nbad") && text.ends_with("fix it"));
    }

    #[test]
    fn replies_and_errors() {
        assert_eq!(parse_reply(&json!({"type": "result", "subtype": "success", "is_error": false, "result": "OK"})).unwrap(), "OK");
        let err = parse_reply(&json!({"subtype": "success", "is_error": true,
                                      "result": "Failed to authenticate: OAuth session expired"})).unwrap_err();
        assert!(err.contains("/login"), "{err}");
        assert!(parse_reply(&json!({"subtype": "error_max_turns", "is_error": false, "result": ""})).is_err());
    }
}

#[cfg(test)]
mod live {
    /// Runs the real `claude` command: `cargo test -p knoend-agents live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn real_claude_code() {
        let status = super::claude_status().await;
        println!("status: {status}");
        let reply = super::complete_claude(None, 120, "You answer with JSON only.",
                                           &[crate::provider::Message::user("Reply with exactly {\"ok\":true}".into())]).await;
        println!("reply: {reply:?}");
    }

    #[tokio::test]
    #[ignore]
    async fn real_antigravity() {
        println!("status: {}", super::agy_status().await);
        let reply = super::complete_agy(None, 120, "You answer with JSON only.",
                                        &[crate::provider::Message::user("Reply with exactly {\"ok\":true}".into())]).await;
        println!("reply: {reply:?}");
    }
}
