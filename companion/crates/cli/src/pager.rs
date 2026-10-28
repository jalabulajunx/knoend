//! Local Windows pager interruptions. Notification text stays in memory and on paired boards.
use std::collections::{VecDeque,HashMap};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use knoend_core::protocol::{Grid, LayoutBody, LayoutKey, Send};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use knoend_transport::pairing::config_dir;
use crate::service::{BoardSender, Shared};

pub const LAYOUT: &str = "system.pager";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub id: String,
    pub app: String,
    pub kind: String,
    #[serde(default)] pub summary: String,
    #[serde(default)] pub body: String,
}
impl Notice {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.app.as_str(), "teams" | "chatgpt" | "codex") {
            return Err("pager app must be teams, chatgpt or codex".into());
        }
        if !matches!(self.kind.as_str(), "message" | "call" | "clear" | "completion") || (self.kind == "call" && self.app != "teams") || (self.kind=="completion"&&self.app!="codex") {
            return Err("pager kind must be message, clear, or a Teams call".into());
        }
        if self.id.is_empty() || self.id.len() > 160 || self.summary.len() > 1024 || self.body.len() > 8192 {
            return Err("pager notice has an empty id or exceeds its size limits".into());
        }
        Ok(())
    }
}

struct Alert { notice: Notice, until: Instant, page: usize }
#[derive(Default)]
struct State { queue: VecDeque<Alert>, revision: u64, completion_keys:HashMap<u64,u64>, completion_seen:VecDeque<String> }
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static WATCHER_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WATCHER_ERROR: OnceLock<Mutex<String>> = OnceLock::new();
static WATCHER_LAST_SCAN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static WATCHER_SCAN: OnceLock<Mutex<Value>> = OnceLock::new();
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
fn fresh_scan(last: u64, now: u64) -> bool { last != 0 && now.saturating_sub(last) < 15_000 }
fn state() -> &'static Mutex<State> { STATE.get_or_init(|| Mutex::new(State::default())) }

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub recorder_launcher: String,
}
pub fn config() -> Config {
    std::fs::read_to_string(config_dir().join("pager.json")).ok()
        .and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}
pub fn configure(enabled: Option<bool>, launcher: Option<&str>) -> Result<(), String> {
    let mut c = config();
    if let Some(path) = launcher {
        if !path.is_empty() && (!std::path::Path::new(path).is_absolute() || !std::path::Path::new(path).is_file()
            || !path.to_ascii_lowercase().ends_with(".vbs")) {
            return Err("Recorder launcher must be an existing absolute .vbs file".into());
        }
        c.recorder_launcher = path.into();
    }
    if let Some(value) = enabled { c.enabled = value; }
    std::fs::create_dir_all(config_dir()).map_err(|e| e.to_string())?;
    let path = config_dir().join("pager.json");
    std::fs::write(path, serde_json::to_vec_pretty(&c).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
pub fn diagnostics() -> Value {
    let c = config();
    let (pending,front_kind,remaining)={let s=state().lock().unwrap();(s.queue.len(),s.queue.front().map(|a|a.notice.kind.clone()),s.queue.front().map(|a|a.until.saturating_duration_since(Instant::now()).as_secs()))};
    let last = WATCHER_LAST_SCAN.load(std::sync::atomic::Ordering::Relaxed);
    json!({"enabled": c.enabled, "windows": cfg!(windows), "watcher_ready": WATCHER_READY.load(std::sync::atomic::Ordering::Relaxed) && fresh_scan(last, now_ms()),
        "last_scan_age_ms": if last == 0 { None } else { Some(now_ms().saturating_sub(last)) },
        "last_scan": WATCHER_SCAN.get_or_init(|| Mutex::new(Value::Null)).lock().unwrap().clone(),
        "watcher_last_error": WATCHER_ERROR.get_or_init(|| Mutex::new(String::new())).lock().unwrap().clone(),
        "recorder_configured": !c.recorder_launcher.is_empty(),
        "recorder_exists": std::path::Path::new(&c.recorder_launcher).is_file(),
        "pending":pending,"front_kind":front_kind,"front_remaining_seconds":remaining,
        "capture": "visible Windows toast notifications and Teams incoming-call windows; suppressed toasts are unavailable"})
}

pub fn receive(n: Notice) {
    if !config().enabled { return; }
    let mut s = state().lock().unwrap();
    s.receive(n);
}
impl State {
    fn receive(&mut self, n: Notice) {
        if n.kind=="completion" {if self.completion_seen.contains(&n.id){return}self.completion_seen.push_back(n.id.clone());if self.completion_seen.len()>128{self.completion_seen.pop_front();}}
        self.queue.retain(|a| !(a.notice.app == n.app && a.notice.id == n.id));
        if n.kind != "clear" {
            let seconds = if n.kind=="completion" {300}else if n.kind == "call" { 90 } else { 45 };
            let a = Alert { notice: n, until: Instant::now() + Duration::from_secs(seconds), page: 0 };
            if a.notice.kind == "call" { self.queue.push_front(a); } else { self.queue.push_back(a); }
            while self.queue.len() > 8 { self.queue.pop_back(); }
        }
        self.revision += 1;
    }
}
pub fn clear_codex_session(session:&str){
    use std::hash::{Hash,Hasher};let mut h=std::collections::hash_map::DefaultHasher::new();session.hash(&mut h);let marker=format!("-complete-{:x}-",h.finish());
    let mut s=state().lock().unwrap();let before=s.queue.len();s.queue.retain(|a|!(a.notice.app=="codex"&&a.notice.kind=="completion"&&a.notice.id.contains(&marker)));if before!=s.queue.len(){s.revision+=1;}
}

pub fn expire() -> bool {
    let mut s = state().lock().unwrap();
    let before = s.queue.len();
    if !config().enabled { s.queue.clear(); }
    s.queue.retain(|a| a.until > Instant::now());
    let changed = before != s.queue.len();
    if changed { s.revision += 1; }
    changed
}

/// ASCII-only, word-bounded pages: firmware's detail buffer is 241 bytes.
fn pages(text: &str) -> Vec<String> {
    let clean: String = text.chars().map(|c| if c.is_ascii_graphic() || c == '\n' { c } else { ' ' }).collect();
    let mut out = Vec::new();
    let mut page = String::new();
    for word in clean.split_whitespace() {
        for chunk in word.as_bytes().chunks(210) {
            let part = std::str::from_utf8(chunk).unwrap();
            if page.len() + part.len() + 1 > 220 { out.push(std::mem::take(&mut page)); }
            if !page.is_empty() { page.push(' '); }
            page.push_str(part);
        }
    }
    if !page.is_empty() || out.is_empty() { out.push(page); }
    out
}
fn key(slot: u8, label: &str, icon: &str, confirm: bool) -> LayoutKey {
    LayoutKey { key: slot, label: label.into(), speak: None, icon: Some(icon.into()), confirm, send: Send::Event(true) }
}
pub async fn show(sender: &mut BoardSender, shown: &mut Option<String>, shared: &Shared,catalog:&knoend_core::catalog::Catalog) -> bool {
    let data = {
        let s = state().lock().unwrap();
        s.queue.front().map(|a| (a.notice.clone(), a.page, s.revision))
    };
    let Some((n, page, revision)) = data else { return false };
    let signature = format!("pager:{revision}:{page}");
    if shown.as_ref() == Some(&signature) { return true; }
    let details = pages(&format!("{} {}", n.summary, n.body));
    let page = page.min(details.len()-1);
    let is_teams = n.app == "teams";
    let mut keys = vec![key(1, if is_teams { "Open Teams" } else { "Open ChatGPT" }, "open_in_new", false),
        key(3, "Dismiss", "close", false), key(7, "Prev text", "chevron_left", false), key(9, "Next text", "chevron_right", false)];
    if is_teams { keys.push(key(2, "Start Steno", "description", false)); }
    let title = format!("{} {}", n.app, if n.kind == "call" { "incoming call" } else { "notification" });
    let mut body = LayoutBody { layout: format!("{LAYOUT}.{revision}"), name: title.clone(), source: "builtin".into(), reviewed: true,
        grid: Some(Grid::DEFAULT), keys, detail: Some(format!("{}/{} {}", page+1, details.len(), details[page])), ..LayoutBody::default() };
    if n.kind=="completion" {
        let library=crate::service::agent_screen_for(catalog,"codex","completion");
        let pages=knoend_core::layout::pages(&library,Grid::DEFAULT);
        if let Ok(mut configured)=knoend_core::layout::build(&library,knoend_core::focus::Os::current(),&pages,0){
            let mut mappings=HashMap::new();
            for (index,slot) in &pages[0].keys {use knoend_core::library::{Action,AgentReply};let action=match library.keys[*index].action {Action::Agent{reply:AgentReply::GoTo}=>1,Action::Agent{reply:AgentReply::Decline}=>3,_=>continue};mappings.insert(u64::from(*slot),action);}
            configured.keys.retain(|k|mappings.contains_key(&u64::from(k.key)));
            for key in &mut configured.keys{key.send=Send::Event(true);}
            configured.layout=body.layout.clone();configured.detail=body.detail.clone();body=configured;
            state().lock().unwrap().completion_keys=mappings;
        }
    }
    match sender.send("knoend.layout", serde_json::to_value(&body).unwrap()).await {
        Ok(_) => {
            let _ = sender.send("knoend.status", json!({"text": title, "level": "alert", "ttl": 45})).await;
            *shown = Some(signature);
            shared.update(|s| s.layout = Some(json!({"id": body.layout, "name": body.name, "source": "builtin", "reviewed": true,
                "grid": body.grid, "keys": body.keys.iter().map(|k| json!({"key": k.key, "label": k.label})).collect::<Vec<_>>()}))).await;
        }
        Err(e) => problem!("pager", "Could not show pager interruption: {e}"),
    }
    true
}

pub fn tap(layout: &str, slot: u64) {
    if !config().enabled { return; }
    let (app,slot,terminal) = {
        let mut s = state().lock().unwrap();
        if layout != format!("{LAYOUT}.{}", s.revision) { return; }
        let Some(a) = s.queue.front_mut() else { return };
        let completion=a.notice.kind=="completion";
        let slot=if completion {let Some(mapped)=s.completion_keys.get(&slot).copied() else{return};mapped}else{slot};
        let a=s.queue.front_mut().unwrap();
        let terminal=a.notice.id.starts_with("codex-terminal-complete-");
        let app = a.notice.app.clone();
        match slot {
            3 => { s.queue.pop_front(); s.revision += 1; return; },
            7 => { a.page = a.page.saturating_sub(1); return; },
            9 => { a.page = (a.page + 1).min(pages(&format!("{} {}", a.notice.summary, a.notice.body)).len()-1); return; },
            _ => {}
        }
        (app,slot,terminal)
    };
    match slot {
        1 => {
            let names: &[&str] = if terminal { &["WindowsTerminal","Code","Terminal","konsole","kitty","alacritty","wezterm","iTerm","ghostty"] }else if app == "teams" { &["ms-teams", "Teams"] } else if app=="codex" { &["Codex", "ChatGPT"] } else { &["ChatGPT", "Codex"] };
            match knoend_focus::bring_to_front(names) {
                Ok(_) => { let mut s = state().lock().unwrap(); s.queue.pop_front(); s.revision += 1; },
                Err(e) => problem!("pager", "Could not open pager app: {e}"),
            }
        }
        2 if app == "teams" => { if let Err(e) = start_steno() { problem!("pager", "Could not open Steno: {e}"); } },
        _ => {}
    }
}

pub fn start_steno() -> Result<(), String> {
    let c = config();
    if c.recorder_launcher.is_empty() || !std::path::Path::new(&c.recorder_launcher).is_file() {
        return Err("Set Recorder launcher in the pager settings to an existing .vbs file".into());
    }
    #[cfg(windows)] {
        // The local config supplies one path, never a shell command or recipe-supplied arguments.
        let windows = std::env::var_os("SystemRoot").ok_or("SystemRoot is unavailable")?;
        let exe = std::path::Path::new(&windows).join("System32/wscript.exe");
        if c.recorder_launcher.contains('"') { return Err("Recorder path cannot contain quotes".into()); }
        knoend_focus::spawn_on_input_desktop(&format!("\"{}\" //nologo \"{}\"", exe.display(), c.recorder_launcher))?;
        say!("Pager: opened the configured Steno launcher");
        Ok(())
    }
    #[cfg(not(windows))] { Err("The configured .vbs recorder launcher is Windows-only".into()) }
}

/// A supervised native child isolates slow UI Automation providers from the service.
pub fn watch(mut each: impl FnMut(Notice) -> bool + std::marker::Send + 'static) {
    #[cfg(windows)] std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        use std::os::windows::process::CommandExt;
        use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}};
        loop {
        let executable = match std::env::current_exe() { Ok(p)=>p, Err(e)=>{problem!("pager-watcher","Could not find native worker: {e}");return;} };
        let mut child = match Command::new(executable).args(["__native-worker","pager"])
            .env("KNOEND_PARENT_PID",std::process::id().to_string())
            .creation_flags(0x08000000).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
            Ok(p)=>p,Err(e)=>{problem!("pager-watcher","Could not start native notification worker: {e}");return;}
        };
        let output = child.stdout.take().unwrap();
        let child = Arc::new(Mutex::new(child));
        let progress = Arc::new(AtomicU64::new(now_ms()));
        let finished = Arc::new(AtomicBool::new(false));
        let (guard_child, guard_progress, guard_finished) = (child.clone(), progress.clone(), finished.clone());
        std::thread::spawn(move || {
            while !guard_finished.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(3));
                if now_ms().saturating_sub(guard_progress.load(Ordering::Relaxed)) > 45_000 {
                    *WATCHER_ERROR.get_or_init(|| Mutex::new(String::new())).lock().unwrap() = "capture scan stalled; restarting worker".into();
                    WATCHER_READY.store(false, Ordering::Relaxed);
                    let _ = guard_child.lock().unwrap().kill();
                    break;
                }
            }
        });
        let mut channel_open = true;
        say!("Pager: Windows visible-notification watcher started");
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if let Some(health) = v["health"].as_str() {
                    if health == "scan" {
                        progress.store(now_ms(), Ordering::Relaxed);
                        WATCHER_LAST_SCAN.store(now_ms(), Ordering::Relaxed);
                        *WATCHER_SCAN.get_or_init(|| Mutex::new(Value::Null)).lock().unwrap() = v.clone();
                        let good = v["errors"].as_u64() == Some(0);
                        WATCHER_READY.store(good, Ordering::Relaxed);
                        if good { WATCHER_ERROR.get_or_init(|| Mutex::new(String::new())).lock().unwrap().clear(); }
                    } else if health != "ready" {
                        *WATCHER_ERROR.get_or_init(|| Mutex::new(String::new())).lock().unwrap() = health.into();
                    }
                    continue;
                }
            }
            if let Ok(n) = serde_json::from_str::<Notice>(&line) {
                if config().enabled && n.validate().is_ok() && !each(n) { channel_open = false; break; }
            }
        }
        finished.store(true, Ordering::Relaxed);
        let _ = child.lock().unwrap().kill(); let _ = child.lock().unwrap().wait();
        WATCHER_READY.store(false, std::sync::atomic::Ordering::Relaxed);
        if !channel_open { return; }
        problem!("pager-watcher", "Windows notification watcher stopped; retrying in 5 seconds");
        std::thread::sleep(Duration::from_secs(5));
        }
    });
    #[cfg(not(windows))] { let _ = &mut each; }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test]fn repeated_completion_is_not_requeued_after_dismissal(){
        let mut state=State::default();let notice=Notice{id:"synthetic-codex-completion".into(),app:"codex".into(),kind:"completion".into(),summary:String::new(),body:String::new()};
        assert!(notice.validate().is_ok());state.receive(notice.clone());state.receive(notice.clone());assert_eq!(state.queue.len(),1);state.queue.clear();state.receive(notice);assert!(state.queue.is_empty());
    }
    #[test] fn readiness_requires_a_recent_completed_scan() {
        assert!(!fresh_scan(0, 1000));
        assert!(fresh_scan(1000, 2000));
        assert!(!fresh_scan(1000, 16000));
    }
    #[test] fn notice_limits_and_sources() {
        let mut n = Notice { id: "one".into(), app: "teams".into(), kind: "call".into(), summary: String::new(), body: String::new() };
        assert!(n.validate().is_ok()); n.app = "browser".into(); assert!(n.validate().is_err());
        n.app = "codex".into(); assert!(n.validate().is_err()); n.kind = "message".into(); assert!(n.validate().is_ok());
        n.body = "a".repeat(8193); assert!(n.validate().is_err());
    }
    #[test] fn detail_pages_fit_firmware_and_preserve_words() {
        let text = "hello world ".repeat(130);
        let p = pages(&text); assert!(p.len() > 1);
        assert!(p.iter().all(|s| s.len() <= 220));
        assert_eq!(p.join(" "), text.trim());
        assert!(pages(&"x".repeat(700)).iter().all(|s| s.len() <= 220));
    }
    #[test] fn calls_preempt_messages_clear_by_id_and_queue_is_bounded() {
        let mut s = State::default();
        let notice = |id: &str, kind: &str| Notice { id: id.into(), app: "teams".into(), kind: kind.into(), summary: String::new(), body: String::new() };
        s.receive(notice("message", "message"));
        s.receive(notice("call", "call"));
        assert_eq!(s.queue[0].notice.id, "call");
        s.receive(notice("call", "call"));
        assert_eq!(s.queue.len(), 2);
        s.receive(notice("call", "clear"));
        assert_eq!(s.queue[0].notice.id, "message");
        for i in 0..20 { s.receive(notice(&i.to_string(), "message")); }
        assert_eq!(s.queue.len(), 8);
        assert_eq!(s.queue[0].notice.id, "message");
    }
}
