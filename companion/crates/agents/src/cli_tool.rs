//! AI tools already on this computer, for people with a plan but no API key, each signed in with
//! the person's own account: Claude Code (`claude -p`) and Antigravity's CLI (`agy -p`).
//!
//! The same system prompt and messages as every other provider go in; text comes out. These tools
//! are agents that could run commands, and site drafts carry text read from web pages, so each call
//! is locked down and runs in an empty folder of its own, removed afterwards. The reply is only
//! ever read as text.
//!   - Claude Code: no tools at all, restricted mode (no user, project or local settings, so none
//!     of KnoEnd's own hooks fire and no plugins load), no MCP servers, no saved session, and no
//!     extended thinking (as with the API providers).
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
const CODEX: &str = "codex";

/// An AI tool KnoEnd can look for on this computer.
pub struct Tool {
    /// The provider name in agent.toml and the web app.
    pub provider: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    /// Whether KnoEnd can use it yet (Codex is only reported: not run here so far).
    pub supported: bool,
    pub sign_in: &'static str,
}

pub const TOOLS: &[Tool] = &[
    Tool { provider: "claude-code", name: "Claude Code", command: CLAUDE, supported: true,
           sign_in: "run `claude` in a terminal and type /login" },
    Tool { provider: "antigravity", name: "Antigravity CLI", command: AGY, supported: true,
           sign_in: "run `agy` in a terminal and sign in with your Google account" },
    Tool { provider: "codex", name: "Codex CLI", command: CODEX, supported: false,
           sign_in: "run `codex login` in a terminal" },
];

/// The PATH a shell gives the person, read at run time: `<shell> -lic env`. A service started by
/// systemd, launchd or a desktop launcher has a much shorter PATH than the person's terminal, and
/// installers (`agy install`, npm, Homebrew, pnpm) add their folders in shell start-up files.
#[cfg(unix)]
fn shell_paths() -> Vec<PathBuf> {
    let mut shells: Vec<PathBuf> = std::env::var_os("SHELL").map(PathBuf::from).into_iter().collect();
    // The terminal may run another shell than the account's (bash account, fish terminal).
    for name in ["zsh", "bash", "fish"] {
        for dir in ["/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"] {
            let path = Path::new(dir).join(name);
            if path.is_file() && !shells.iter().any(|s| s.file_name() == path.file_name()) {
                shells.push(path);
                break;
            }
        }
    }
    let mut dirs = Vec::new();
    for shell in shells {
        let Ok(mut child) = std::process::Command::new(&shell).args(["-l", "-i", "-c", "env"])
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { continue };
        // A shell whose start-up waits on something is given 3 s, then left out.
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while child.try_wait().ok().flatten().is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
            let _ = child.wait();
            continue;
        }
        let mut out = String::new();
        let _ = std::io::Read::read_to_string(child.stdout.as_mut().expect("piped"), &mut out);
        if let Some(path) = out.lines().find_map(|l| l.strip_prefix("PATH=")) {
            dirs.extend(std::env::split_paths(path));
        }
    }
    dirs
}

#[cfg(not(unix))]
fn shell_paths() -> Vec<PathBuf> {
    Vec::new()
}

/// Per-user folders installers use on this OS, for when no shell names them.
fn usual_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    let mut dirs = vec![home.join(".local/bin"), home.join(".claude/local"), home.join(".npm-global/bin")];
    if cfg!(target_os = "macos") {
        dirs.extend([PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")]);
    } else if cfg!(windows) {
        for var in ["APPDATA", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(var).map(PathBuf::from) {
                dirs.extend([base.join("npm"), base.join("Programs")]);
            }
        }
    } else {
        dirs.extend([PathBuf::from("/usr/local/bin"), PathBuf::from("/usr/bin")]);
    }
    dirs
}

/// Every folder to look in, without repeats: this process's PATH, the shells' PATHs, then the
/// usual per-user folders.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend(shell_paths());
    dirs.extend(usual_dirs());
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| !d.as_os_str().is_empty() && seen.insert(d.clone()));
    dirs
}

/// The file names a command can have here: `claude`, or on Windows also `claude.exe`, `.cmd`...
fn file_names(command: &str) -> Vec<String> {
    if cfg!(windows) {
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into());
        exts.split(';').filter(|e| !e.is_empty()).map(|e| format!("{command}{}", e.to_lowercase())).collect()
    } else {
        vec![command.to_string()]
    }
}

/// Looks for a command now (nothing is assumed in advance).
fn find_in(command: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let names = file_names(command);
    dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// Where a command is, looked up afresh and remembered once found, so each request does not ask
/// the shells again. A tool installed or moved later is found by the next miss.
fn find(command: &str) -> Option<PathBuf> {
    static FOUND: std::sync::Mutex<Option<std::collections::HashMap<String, PathBuf>>> = std::sync::Mutex::new(None);
    if let Some(path) = FOUND.lock().ok()?.get_or_insert_with(Default::default).get(command).filter(|p| p.is_file()).cloned() {
        return Some(path);
    }
    let path = find_in(command, &search_dirs())?;
    FOUND.lock().ok()?.get_or_insert_with(Default::default).insert(command.to_string(), path.clone());
    Some(path)
}

/// `find` off the async threads (it may start the person's shells).
async fn find_async(command: &'static str) -> Option<PathBuf> {
    tokio::task::spawn_blocking(move || find(command)).await.ok().flatten()
}

/// One short command's output (10 s at most): stdout and whether it succeeded.
async fn quick(path: &Path, args: &[&str]) -> Option<(String, bool)> {
    let dir = work_dir("check").ok()?;
    let out = tokio::time::timeout(Duration::from_secs(10),
                                   tokio::process::Command::new(path).args(args).current_dir(&dir).env_remove("CLAUDECODE")
                                       .stdin(Stdio::null()).kill_on_drop(true).output()).await;
    let _ = std::fs::remove_dir_all(&dir);
    let out = out.ok()?.ok()?;
    Some((String::from_utf8_lossy(&out.stdout).trim().to_string(), out.status.success()))
}

/// What is on this computer right now, for onboarding and the setup snapshot: for each tool,
/// whether it is installed, where, its version, and whether it is signed in. Looked up on every
/// call, in this OS's places. Facts only: no account names.
pub async fn tools_on_this_computer() -> Value {
    let dirs = tokio::task::spawn_blocking(search_dirs).await.unwrap_or_default();
    let mut found = Vec::new();
    for tool in TOOLS {
        let Some(path) = find_in(tool.command, &dirs) else {
            found.push(serde_json::json!({"provider": tool.provider, "name": tool.name, "installed": false, "supported": tool.supported}));
            continue;
        };
        let version = quick(&path, &["--version"]).await.map(|(v, _)| v.lines().next().unwrap_or_default().to_string());
        let mut check_error = None;
        let signed_in = match tool.command {
            CLAUDE => quick(&path, &["auth", "status"]).await
                .and_then(|(o, _)| serde_json::from_str::<Value>(&o).ok()).and_then(|v| v["loggedIn"].as_bool()),
            // agy has no sign-in status command: listing its models needs the account.
            AGY => match agy_models_at(&path).await {
                Ok(models) => Some(!models.is_empty()),
                Err(e) => {
                    check_error = Some(e);
                    None
                }
            },
            CODEX => quick(&path, &["login", "status"]).await.map(|(_, ok)| ok),
            _ => None,
        };
        found.push(serde_json::json!({
            "provider": tool.provider, "name": tool.name, "installed": true, "supported": tool.supported,
            "path": path.display().to_string(), "version": version, "signed_in": signed_in,
            "sign_in": tool.sign_in, "check_error": check_error,
        }));
    }
    Value::Array(found)
}

/// A fresh, empty folder for one call.
fn work_dir(tool: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("knoend-{tool}-{}-{}", std::process::id(),
                                                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    std::fs::create_dir(&dir).map_err(|e| format!("could not make a working folder: {e}"))?;
    Ok(dir)
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
    let path = find_async(CLAUDE).await.ok_or("Claude Code (the `claude` command) is not installed on this computer. What to do: \
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
        // No extended thinking, as with the API providers: Claude Code thinks by default, and a
        // Haiku draft took 104 s (13,923 output tokens) with it and 5 s (532) without.
        .env("MAX_THINKING_TOKENS", "0")
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

/// The models agy offers (the first column of `agy models`).
pub async fn agy_models() -> Result<Vec<String>, String> {
    let path = find_async(AGY).await.ok_or("Antigravity's CLI (`agy`) is not installed on this computer")?;
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
    let path = find_async(AGY).await.ok_or("Antigravity's CLI (the `agy` command) is not installed on this computer. What to do: \
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
    fn commands_are_looked_up_in_the_given_folders() {
        let dir = std::env::temp_dir().join(format!("knoend-find-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let name = file_names("knoend-test-tool").remove(0);
        std::fs::write(dir.join(&name), "").unwrap();
        assert_eq!(find_in("knoend-test-tool", &[PathBuf::from("/nonexistent"), dir.clone()]), Some(dir.join(&name)));
        assert_eq!(find_in("knoend-no-such-tool", &[dir.clone()]), None);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(TOOLS.iter().any(|t| t.command == "codex" && !t.supported), "Codex is reported, not run");
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
        println!("tools: {:#}", super::tools_on_this_computer().await);
        let reply = super::complete_claude(None, 120, "You answer with JSON only.",
                                           &[crate::provider::Message::user("Reply with exactly {\"ok\":true}".into())]).await;
        println!("reply: {reply:?}");
    }

    #[tokio::test]
    #[ignore]
    async fn real_antigravity() {
        let reply = super::complete_agy(None, 120, "You answer with JSON only.",
                                        &[crate::provider::Message::user("Reply with exactly {\"ok\":true}".into())]).await;
        println!("reply: {reply:?}");
    }
}
