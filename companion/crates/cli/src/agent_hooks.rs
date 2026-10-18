//! Coding agents asking the person, through the board: Claude Code's hooks run `openpager hook
//! permission` (PermissionRequest) and `openpager hook notification` (Notification); the hook
//! relays to the running service over a Unix socket only this user can open, and the board shows
//! the request as keys. The agent's own prompt stays: whichever answers first applies, and a hook
//! that gets no answer (or finds no service) prints nothing, so the agent asks as usual.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

/// How long a permission request waits for the board before the agent's own prompt decides.
const WAIT_FOR_BOARD: Duration = Duration::from_secs(300);

pub fn socket_path() -> PathBuf {
    crate::browser::socket_path().with_file_name("agents.sock")
}

/// The person's answer from the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Allow, and remember it (the agent's first suggested permission update).
    AlwaysAllow,
    Deny,
    /// Deny and stop the agent.
    Stop,
}

/// A request from an agent, for the service loop.
pub enum AgentEvent {
    Permission { request: Value, answer: oneshot::Sender<Decision> },
    Notification(Value),
}

// ---- the hook, run by the agent ----

/// Runs as the agent's hook: reads the event from stdin, asks the service, prints the decision.
/// Any problem (no service, no answer) prints nothing, which leaves the agent's own prompt.
#[cfg(unix)]
pub fn run_hook(kind: &str) -> Result<(), String> {
    use std::os::unix::net::UnixStream;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).map_err(|e| e.to_string())?;
    let Ok(event) = serde_json::from_str::<Value>(&input) else { return Ok(()) };
    let Ok(mut stream) = UnixStream::connect(socket_path()) else { return Ok(()) };
    let _ = stream.set_read_timeout(Some(WAIT_FOR_BOARD));
    let line = json!({"type": kind, "event": event}).to_string();
    if stream.write_all(format!("{line}\n").as_bytes()).is_err() || kind != "permission" {
        return Ok(());
    }
    let mut reply = String::new();
    if BufReader::new(&stream).read_line(&mut reply).is_err() {
        return Ok(());
    }
    let Ok(reply) = serde_json::from_str::<Value>(&reply) else { return Ok(()) };
    let decision = match reply["decision"].as_str() {
        Some("allow") => json!({"behavior": "allow"}),
        Some("always") => {
            // Echo the agent's own first suggestion (e.g. an allow rule for this command).
            let suggestion = event["permission_suggestions"].as_array().and_then(|s| s.first()).cloned();
            match suggestion {
                Some(s) => json!({"behavior": "allow", "updatedPermissions": [s]}),
                None => json!({"behavior": "allow"}),
            }
        }
        Some("deny") => json!({"behavior": "deny", "message": "Denied on the OpenPager board."}),
        Some("stop") => json!({"behavior": "deny", "message": "Stopped from the OpenPager board.", "interrupt": true}),
        _ => return Ok(()),
    };
    println!("{}", json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": decision}}));
    Ok(())
}

#[cfg(not(unix))]
pub fn run_hook(_kind: &str) -> Result<(), String> {
    Ok(())
}

// ---- the service side ----

/// Listens for hooks and hands their requests to the service loop.
#[cfg(unix)]
pub async fn serve(events: mpsc::Sender<AgentEvent>) {
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let path = socket_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = tokio::net::UnixListener::bind(&path) else {
        println!("Agent hooks off: cannot listen on {}", path.display());
        return;
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let events = events.clone();
        tokio::spawn(async move {
            let (read, mut write) = stream.into_split();
            let mut lines = tokio::io::BufReader::new(read).lines();
            let Ok(Some(line)) = lines.next_line().await else { return };
            let Ok(message) = serde_json::from_str::<Value>(&line) else { return };
            let event = message["event"].clone();
            if message["type"] != "permission" {
                let _ = events.send(AgentEvent::Notification(event)).await;
                return;
            }
            let (answer, decided) = oneshot::channel();
            if events.send(AgentEvent::Permission { request: event, answer }).await.is_err() {
                return;
            }
            // The hook goes away when the agent decided elsewhere (its own prompt): dropping
            // `decided` then tells the service loop to withdraw the request from the board.
            tokio::select! {
                decision = decided => {
                    if let Ok(decision) = decision {
                        let word = match decision {
                            Decision::Allow => "allow",
                            Decision::AlwaysAllow => "always",
                            Decision::Deny => "deny",
                            Decision::Stop => "stop",
                        };
                        let _ = write.write_all(format!("{}\n", json!({"decision": word})).as_bytes()).await;
                    }
                }
                _ = lines.next_line() => {}
            }
        });
    }
}

#[cfg(not(unix))]
pub async fn serve(_events: mpsc::Sender<AgentEvent>) {}

/// A short line for the board: which agent wants what ("Claude: Bash rm -rf build").
pub fn summary(request: &Value) -> String {
    let tool = request["tool_name"].as_str().unwrap_or("a tool");
    let input = &request["tool_input"];
    let detail = ["command", "file_path", "path", "url", "pattern", "description"].iter()
        .find_map(|k| input[*k].as_str())
        .unwrap_or("");
    let detail = detail.rsplit('/').next().filter(|_| input["file_path"].is_string()).unwrap_or(detail);
    format!("Claude: {tool} {detail}").trim().to_string()
}

/// What the board explains under a request: the tool and its full input, what Allow does, and
/// what Always would remember and where (from the agent's own suggestion).
pub fn detail(request: &Value) -> String {
    let tool = request["tool_name"].as_str().unwrap_or("a tool");
    let input = &request["tool_input"];
    let what = ["command", "file_path", "path", "url", "pattern"].iter().find_map(|k| input[*k].as_str()).unwrap_or("");
    let mut text = format!("{tool}: {what}");
    if let Some(why) = input["description"].as_str().filter(|d| !d.is_empty() && *d != what) {
        text += &format!("\n({why})");
    }
    text += "\nAllow = this time only.";
    if let Some(s) = request["permission_suggestions"].as_array().and_then(|s| s.first()) {
        let place = match s["destination"].as_str() {
            Some("session") => "for this session",
            Some("localSettings") => "in this project",
            Some("projectSettings") => "in this project, for everyone",
            Some("userSettings") => "in every project",
            _ => "",
        };
        let remember = match s["type"].as_str() {
            Some("addRules") => {
                let rules: Vec<String> = s["rules"].as_array().into_iter().flatten().map(|r| {
                    let tool = r["toolName"].as_str().unwrap_or("?");
                    match r["ruleContent"].as_str() {
                        Some(content) => format!("{tool}({content})"),
                        None => format!("any {tool}"),
                    }
                }).collect();
                format!("allow {} from now on", rules.join(", "))
            }
            Some("setMode") => format!("switch to {} mode", s["mode"].as_str().unwrap_or("?")),
            Some("addDirectories") => "add these folders".into(),
            _ => "remember this choice".into(),
        };
        text += &format!(" Always = {remember} {place}.");
    }
    text
}

/// Whether a request looks destructive enough to need a second tap to allow.
pub fn destructive(request: &Value) -> bool {
    let command = request["tool_input"]["command"].as_str().unwrap_or("").to_ascii_lowercase();
    ["rm ", "rm\t", "rmdir", "git push", "git reset --hard", "git clean", "drop table", "mkfs", "dd ", "chmod -r", "sudo ", "> /dev/"]
        .iter().any(|w| command.contains(w))
}

// ---- installing the hooks into Claude Code ----

fn claude_settings() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".claude/settings.json")
}

/// Whether a hook entry is one of ours.
fn ours(entry: &Value) -> bool {
    entry["hooks"].as_array().is_some_and(|h| h.iter().any(|x| x["command"].as_str().is_some_and(|c| c.contains("openpager") && c.contains(" hook "))))
}

/// The hooks in `settings`, with ours replaced by entries running `exe`.
pub fn with_hooks(mut settings: Value, exe: &str) -> Value {
    if !settings.is_object() {
        settings = json!({});
    }
    let hooks = settings.as_object_mut().unwrap().entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    for (event, kind, timeout) in [("PermissionRequest", "permission", 600), ("Notification", "notification", 10)] {
        let list = hooks.as_object_mut().unwrap().entry(event).or_insert_with(|| json!([]));
        if !list.is_array() {
            *list = json!([]);
        }
        let list = list.as_array_mut().unwrap();
        list.retain(|e| !ours(e));
        list.push(json!({"hooks": [{"type": "command", "command": format!("\"{exe}\" hook {kind}"), "timeout": timeout}]}));
    }
    settings
}

/// `settings` without our hooks (empty lists and an empty `hooks` removed).
pub fn without_hooks(mut settings: Value) -> Value {
    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        for list in hooks.values_mut() {
            if let Some(list) = list.as_array_mut() {
                list.retain(|e| !ours(e));
            }
        }
        hooks.retain(|_, v| !v.as_array().is_some_and(|a| a.is_empty()));
        if hooks.is_empty() {
            settings.as_object_mut().unwrap().remove("hooks");
        }
    }
    settings
}

/// Adds the hooks to `~/.claude/settings.json` (after a backup next to it).
pub fn install() -> Result<Vec<String>, String> {
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| e.to_string())?;
    let path = claude_settings();
    let current: Value = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON ({e}); not changed", path.display()))?,
        Err(_) => json!({}),
    };
    let mut lines = Vec::new();
    if path.exists() {
        let backup = path.with_extension("json.before-openpager");
        std::fs::copy(&path, &backup).map_err(|e| e.to_string())?;
        lines.push(format!("backup: {}", backup.display()));
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let updated = with_hooks(current, exe.to_str().ok_or("path not UTF-8")?);
    std::fs::write(&path, serde_json::to_string_pretty(&updated).unwrap() + "\n").map_err(|e| e.to_string())?;
    lines.push(format!("{}: PermissionRequest and Notification hooks run openpager", path.display()));
    lines.push("Claude Code sessions started from now on use them (new sessions; restart running ones)".into());
    Ok(lines)
}

/// Removes our hooks from `~/.claude/settings.json`.
pub fn uninstall() -> Result<Vec<String>, String> {
    let path = claude_settings();
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(vec!["no Claude Code settings file".into()]) };
    let current: Value = serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON ({e}); not changed", path.display()))?;
    std::fs::write(&path, serde_json::to_string_pretty(&without_hooks(current)).unwrap() + "\n").map_err(|e| e.to_string())?;
    Ok(vec![format!("{}: OpenPager hooks removed", path.display())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_are_added_once_and_removed_without_touching_others() {
        let mine = json!({"model": "opus", "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "check.sh"}]}]}});
        let once = with_hooks(mine.clone(), "/opt/openpager");
        let twice = with_hooks(once.clone(), "/opt/openpager");
        assert_eq!(once, twice, "installing twice changes nothing");
        assert_eq!(twice["hooks"]["PermissionRequest"][0]["hooks"][0]["command"], "\"/opt/openpager\" hook permission");
        assert_eq!(twice["hooks"]["PreToolUse"], mine["hooks"]["PreToolUse"]);
        assert_eq!(without_hooks(twice), mine);
    }

    #[test]
    fn summaries_and_destructive_commands() {
        let rm = json!({"tool_name": "Bash", "tool_input": {"command": "rm -rf build", "description": "Clean"}});
        assert_eq!(summary(&rm), "Claude: Bash rm -rf build");
        assert!(destructive(&rm));
        let edit = json!({"tool_name": "Edit", "tool_input": {"file_path": "/home/u/p/src/main.rs"}});
        assert_eq!(summary(&edit), "Claude: Edit main.rs");
        assert!(!destructive(&edit));
        let with = json!({"tool_name": "Bash", "tool_input": {"command": "ls /tmp", "description": "List"},
            "permission_suggestions": [{"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "ls:*"}], "behavior": "allow", "destination": "localSettings"}]});
        assert_eq!(detail(&with), "Bash: ls /tmp\n(List)\nAllow = this time only. Always = allow Bash(ls:*) from now on in this project.");
    }
}
