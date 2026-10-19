//! An AI tool already on this computer, for people with a plan but no API key: Claude Code
//! (`claude -p`), signed in with the person's own account.
//!
//! The same system prompt and messages as every other provider go in; text comes out. The tool is
//! an agent that could run commands, and site drafts carry text read from web pages, so each call
//! is locked down: no tools at all, restricted mode (no user, project or local settings, so none of
//! KnoEnd's own hooks fire and no plugins load), no MCP servers, no saved session, and an empty
//! folder of its own as the working directory. The reply is only ever read as text.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncWriteExt;

use crate::provider::Message;

const CLAUDE: &str = "claude";

/// Where the `claude` command is: the PATH, then the places its installers use (a background
/// service may start with a short PATH).
pub fn claude_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend([home.join(".local/bin"), home.join(".claude/local"), home.join(".npm-global/bin"),
                 PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin"), PathBuf::from("/usr/bin")]);
    dirs.into_iter().map(|d| d.join(CLAUDE)).find(|p| p.is_file())
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
    let dir = std::env::temp_dir().join(format!("knoend-claude-{}-{}", std::process::id(),
                                                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    std::fs::create_dir(&dir).map_err(|e| format!("could not make a working folder: {e}"))?;
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
}
