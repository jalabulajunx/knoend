//! The companion service: find the paired board on Wi-Fi, follow the focused app, push layouts,
//! and run the configure agent when Configure is tapped. State and events are shared with the
//! web app; window titles never leave this loop (only the app's name is published).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use openpager_core::catalog::Catalog;
use openpager_core::focus::{Focus, WebPage};
use openpager_core::library::{Action, MediaCommand};
use openpager_agents::site::Scope;
use openpager_media::NowPlaying;
use std::collections::{HashMap, HashSet};
use openpager_core::layout;
use openpager_core::library::Library;
use openpager_core::protocol::{LayoutBody, LayoutKey, Packet, Send};
use openpager_transport::pairing::{config_dir, Pairings};
use openpager_transport::{discover, wifi};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, RwLock};

/// Layout id the board shows for an app with no library; key 5 is "Configure".
pub const UNSUPPORTED: &str = "system.unsupported";
const CONFIGURE_SLOT: u8 = 5;

#[derive(Debug, Clone, Default, Serialize)]
pub struct BoardStatus {
    pub id: String,
    pub profile: String,
    pub firmware: String,
    pub address: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    /// "no-pairing" | "searching" | "connected"
    pub link: String,
    pub board: Option<BoardStatus>,
    /// The focused app's display name (never the window title).
    pub app: Option<String>,
    pub layout: Option<Value>,
    pub configuring: Option<String>,
    pub libraries: usize,
}

pub struct Shared {
    pub status: RwLock<Status>,
    pub events: broadcast::Sender<Value>,
    /// Held while a flash or provisioning job owns the USB serial port.
    pub serial: Arc<tokio::sync::Mutex<()>>,
    pub firmware_source: String,
    /// Types keystrokes for a board that cannot (no Bluetooth host); connected on first use.
    typist: Arc<std::sync::Mutex<Option<openpager_input::Typist>>>,
    /// The last log lines, for the setup assistant's snapshot.
    recent: std::sync::Mutex<std::collections::VecDeque<String>>,
    /// The browser extension link (`spec/browser.md`).
    pub browser: Arc<crate::browser::Browser>,
    /// Media players (MPRIS on Linux); None where there are none to read.
    pub media: Option<Arc<openpager_media::Players>>,
    /// The service loop's inbox, so background jobs can put a line on the board's status bar.
    inbox: std::sync::OnceLock<mpsc::Sender<Event>>,
}

impl Shared {
    pub fn new(firmware_source: String) -> Arc<Shared> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Shared { status: RwLock::new(Status::default()), events, serial: Arc::new(tokio::sync::Mutex::new(())), firmware_source,
                          typist: Arc::new(std::sync::Mutex::new(None)),
                          recent: std::sync::Mutex::new(std::collections::VecDeque::new()),
                          browser: Arc::new(crate::browser::Browser::default()),
                          media: openpager_media::Players::connect().ok().map(Arc::new),
                          inbox: std::sync::OnceLock::new() })
    }

    /// Publishes an event to the web app and prints it for the terminal.
    pub fn emit(&self, kind: &str, data: Value) {
        if let Some(text) = data.get("text").and_then(Value::as_str) {
            println!("{text}");
            if let Ok(mut recent) = self.recent.lock() {
                recent.push_back(text.trim().to_string());
                while recent.len() > 40 {
                    recent.pop_front();
                }
            }
        }
        let _ = self.events.send(json!({"kind": kind, "data": data}));
    }

    /// Shows `text` on the board's status bar (highlighted), from any task.
    pub fn board_status(&self, text: String) {
        if let Some(inbox) = self.inbox.get() {
            let _ = inbox.try_send(Event::Status(text));
        }
    }

    /// Asks the browser to let the board act on the active site (the extension opens a page with
    /// an Allow button; browsers only grant on a click). Waits up to two minutes for the answer.
    pub async fn ask_site_permission(&self, site: &str) -> Result<(), String> {
        self.board_status(format!("Allow {site} in your browser (OpenPager tab)"));
        let answer = self.browser.request(json!({"type": "grant"}), Duration::from_secs(125)).await?;
        if answer["ok"] == true {
            Ok(())
        } else {
            self.board_status(format!("{site} not allowed: the board cannot act there"));
            Err(format!("{site} was not allowed in the browser"))
        }
    }

    pub fn recent(&self) -> Vec<String> {
        self.recent.lock().map(|r| r.iter().cloned().collect()).unwrap_or_default()
    }

    async fn update(&self, change: impl FnOnce(&mut Status)) {
        let mut status = self.status.write().await;
        change(&mut status);
        let snapshot = serde_json::to_value(&*status).expect("serialisable");
        drop(status);
        let _ = self.events.send(json!({"kind": "status", "data": snapshot}));
    }
}

mod builtin {
    include!(concat!(env!("OUT_DIR"), "/builtin.rs"));
}

/// Built-in libraries are compiled in; `OPENPAGER_BUILTIN` (a folder) overrides them, for
/// working on libraries without rebuilding.
fn load_builtin(catalog: &mut Catalog) -> Vec<String> {
    let mut problems = Vec::new();
    if std::env::var_os("OPENPAGER_BUILTIN").is_some() {
        return problems;
    }
    for (name, text) in builtin::BUILTIN {
        match Library::parse(text) {
            Ok(library) => catalog.add(PathBuf::from(format!("builtin:{name}")), library),
            Err(e) => problems.push(format!("built-in {name}: {e}")),
        }
    }
    problems
}

pub fn ai_libraries() -> PathBuf {
    config_dir().join("libraries").join("ai")
}

enum Event {
    Focus(Option<Focus>),
    /// The browser's active tab (None: no browser window focused, or the extension went away).
    Tab(Option<WebPage>),
    /// Every media player and what it plays, when that changed.
    Players(Vec<NowPlaying>),
    /// The extension's popup asked to configure the focused site.
    ConfigureSite,
    /// A line for the board's status bar from a background job.
    Status(String),
    /// A desktop notification from an app the board follows (Claude, Codex, ...).
    Notified(openpager_desktop::Notification),
    /// A coding agent's hook: a permission request or a notification.
    Agent(crate::agent_hooks::AgentEvent),
    /// A site's icon, fetched and shrunk to 32x32 RGBA.
    Favicon(String, Vec<u8>),
    Board(Packet),
    LinkDown(String),
    Configured(Focus, Result<(PathBuf, Library), String>),
}

pub async fn run(shared: Arc<Shared>, extra_libraries: Vec<PathBuf>) {
    let mut dirs: Vec<PathBuf> = std::env::var_os("OPENPAGER_BUILTIN").map(PathBuf::from).into_iter().collect();
    dirs.extend([config_dir().join("libraries"), ai_libraries()]);
    dirs.extend(extra_libraries);
    let (mut catalog, rejected) = Catalog::load(&dirs);
    for problem in load_builtin(&mut catalog) {
        println!("  skipped {problem}");
    }
    println!("Libraries: {} loaded from {:?}", catalog.len(), dirs);
    for r in &rejected {
        println!("  skipped {}: {}", r.path.display(), r.reason);
    }
    let count = catalog.len();
    shared.update(|s| s.libraries = count).await;

    let (events, mut inbox) = mpsc::channel::<Event>(64);
    let _ = shared.inbox.set(events.clone());
    spawn_focus_watcher(events.clone(), shared.clone());
    {
        let (tabs_tx, mut tabs_rx) = mpsc::channel(16);
        tokio::spawn(crate::browser::serve(shared.browser.clone(), tabs_tx));
        let events = events.clone();
        tokio::spawn(async move {
            while let Some(event) = tabs_rx.recv().await {
                let event = match event {
                    crate::browser::BrowserEvent::Tab(tab) => Event::Tab(tab),
                    crate::browser::BrowserEvent::Configure => Event::ConfigureSite,
                };
                if events.send(event).await.is_err() {
                    break;
                }
            }
        });
    }

    if let Some(media) = shared.media.clone() {
        spawn_media_watcher(media, events.clone());
    }
    spawn_notification_watcher(events.clone());
    {
        let (agents_tx, mut agents_rx) = mpsc::channel(16);
        tokio::spawn(crate::agent_hooks::serve(agents_tx));
        let events = events.clone();
        tokio::spawn(async move {
            while let Some(e) = agents_rx.recv().await {
                if events.send(Event::Agent(e)).await.is_err() {
                    break;
                }
            }
        });
    }
    // Permission requests from coding agents, oldest first; the board shows the first.
    let mut approvals: std::collections::VecDeque<Approval> = std::collections::VecDeque::new();

    // What the OS says is focused, and the browser's active tab; `effective` joins them.
    let mut focus: Option<Focus> = None;
    let mut tab: Option<WebPage> = None;
    let mut players: Vec<NowPlaying> = Vec::new();
    // The launcher: usage per app/site, installed apps, site icons, icons already on the board.
    let mut usage = crate::launcher::Usage::load();
    let apps = crate::launcher::installed_apps();
    let mut favicons = load_favicons();
    let mut fetching: HashSet<String> = HashSet::new();
    let mut launcher_targets: HashMap<u8, crate::launcher::Target> = HashMap::new();
    // Kinds of page that have keys of their own, per site (saved), and those tried this run.
    let mut drafted = DraftedPages::load();
    let mut attempted: HashSet<(String, String)> = HashSet::new();
    // What each slot on the board does right now (the layout plus any media row).
    let mut shown_actions: HashMap<u8, KeyRef> = HashMap::new();
    loop {
        let pairings = Pairings::load();
        if pairings.boards.is_empty() {
            shared.update(|s| { s.link = "no-pairing".into(); s.board = None; }).await;
            // Keep following focus while waiting, so the dashboard shows it.
            match tokio::time::timeout(Duration::from_secs(2), inbox.recv()).await {
                Ok(Some(Event::Focus(Some(f)))) => focus = Some(f),
                Ok(Some(Event::Tab(t))) => tab = t,
                Ok(Some(Event::Players(p))) => players = p,
                _ => {}
            }
            if let Some(f) = effective(&focus, &tab) {
                shared.update(|s| s.app = Some(f.display_name().to_string())).await;
            }
            continue;
        }
        shared.update(|s| s.link = "searching".into()).await;
        let (board, mut sender) = match connect(&pairings, events.clone()).await {
            Ok(link) => link,
            Err(e) => {
                println!("{e}; retrying in 5 s");
                drain_focus(&mut inbox, &mut focus, &mut tab, &mut players, &shared).await;
                continue;
            }
        };
        shared.emit("log", json!({"text": format!("Connected to board {} over Wi-Fi", board.id)}));
        let board_status = board.clone();
        shared.update(|s| { s.link = "connected".into(); s.board = Some(board_status); }).await;
        let mut shown: Option<String> = None;
        let mut sent_images: HashSet<String> = HashSet::new();
        if focus.is_none() {
            show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut shown, &shared).await;
        }
        if let Some(f) = effective(&focus, &tab) {
            show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &shared).await;
        }

        // A board that restarts never closes its old connection, so silence is the only sign:
        // ping every 10 s and treat 25 s without any packet as a lost link.
        let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
        heartbeat.tick().await;
        let mut second = tokio::time::interval(Duration::from_secs(1));
        if let Some(a) = approvals.front() {
            show_approval(&mut sender, a, &mut shown, &shared).await;
        }
        let mut last_heard = tokio::time::Instant::now();

        loop {
            let event = tokio::select! {
                event = inbox.recv() => match event { Some(e) => e, None => return },
                _ = second.tick() => {
                    // A request answered elsewhere (the agent's own prompt) leaves the board.
                    let before = approvals.len();
                    approvals.retain(|a| !a.answer.is_closed());
                    if approvals.len() != before {
                        println!("  permission request answered elsewhere; withdrawn from the board");
                        shown = None;
                        match approvals.front() {
                            Some(a) => show_approval(&mut sender, a, &mut shown, &shared).await,
                            None => match effective(&focus, &tab) {
                                Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &shared).await,
                                None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets,
                                                      &mut shown, &shared).await,
                            },
                        }
                    }
                    continue;
                }
                _ = heartbeat.tick() => {
                    if last_heard.elapsed() > Duration::from_secs(25) {
                        Event::LinkDown("no reply from the board for 25 s".into())
                    } else if sender.send("openpager.ping", json!({})).await.is_err() {
                        Event::LinkDown("could not send to the board".into())
                    } else {
                        continue;
                    }
                }
            };
            if matches!(event, Event::Board(_)) {
                last_heard = tokio::time::Instant::now();
            }
            match event {
                Event::Focus(Some(f)) => {
                    focus = Some(f);
                    let f = effective(&focus, &tab).expect("focus");
                    usage.focused(crate::launcher::usage_key(&f, &apps));
                    println!("Focus: {} ({})", f.display_name(), f.instance);
                    let name = f.display_name().to_string();
                    shared.update(|s| s.app = Some(name)).await;
                    show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &shared).await;
                    draft_new_page(&catalog, &f, &drafted, &mut attempted, &events, &shared).await;
                }
                Event::Focus(None) => {
                    // Nothing in front: the launcher.
                    println!("Focus: nothing (launcher)");
                    focus = None;
                    usage.focused(None);
                    shared.update(|s| s.app = None).await;
                    if approvals.is_empty() {
                        show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut shown, &shared).await;
                    }
                }
                Event::Favicon(host, pixels) => {
                    fetching.remove(&host);
                    let _ = std::fs::create_dir_all(crate::launcher::favicon_dir());
                    if let Some(png) = image_png(&pixels) {
                        let _ = std::fs::write(crate::launcher::favicon_dir().join(format!("{host}.png")), png);
                    }
                    favicons.insert(host, pixels);
                }
                Event::Board(packet) if packet.kind == "openpager.key" && packet.body["layout"] == LAUNCHER => {
                    let key = packet.body["key"].as_u64().and_then(|k| u8::try_from(k).ok()).unwrap_or(0);
                    if let Some(target) = launcher_targets.get(&key) {
                        let text = match target.open() {
                            Ok(()) => format!("  launcher: opened {target:?}"),
                            Err(e) => format!("  launcher: could not open {target:?}: {e}"),
                        };
                        shared.emit("launcher", json!({"text": text}));
                    }
                }
                Event::Players(p) => {
                    players = p;
                    // Re-show only if this changes what the board shows (show() skips a repeat).
                    if let Some(f) = effective(&focus, &tab) {
                        show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &shared).await;
                    }
                }
                Event::Tab(t) => {
                    // The site's icon, for the launcher (once per site per run).
                    if let Some(page) = t.as_ref().filter(|p| !p.favicon.is_empty()) {
                        if !favicons.contains_key(&page.host) && fetching.insert(page.host.clone()) {
                            spawn_favicon_fetch(page.host.clone(), page.favicon.clone(), events.clone());
                        }
                    }
                    // A new site, a change of permission, or another kind of page on the same site.
                    let changed = t.as_ref().map(|p| (&p.host, p.granted, p.page_kind(), p.fullscreen))
                        != tab.as_ref().map(|p| (&p.host, p.granted, p.page_kind(), p.fullscreen));
                    tab = t;
                    let browser_focused = focus.as_ref().is_some_and(|f| crate::browser::is_browser(&f.app, &f.instance));
                    if changed && browser_focused {
                        let f = effective(&focus, &tab).expect("focus");
                        println!("Site: {}", f.display_name());
                        usage.focused(crate::launcher::usage_key(&f, &apps));
                        let name = f.display_name().to_string();
                        shared.update(|s| s.app = Some(name)).await;
                        show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &shared).await;
                        draft_new_page(&catalog, &f, &drafted, &mut attempted, &events, &shared).await;
                    }
                }
                Event::Board(packet) if packet.kind == "openpager.key" && packet.body["layout"] == APPROVE => {
                    let key = packet.body["key"].as_u64().unwrap_or(0);
                    if let Some(front) = approvals.front_mut().filter(|a| !a.questions.is_empty()) {
                        match front.tap_question(key as u8) {
                            Some(decision) => {
                                let front = approvals.pop_front().expect("front");
                                println!("  answered on the board: {decision:?}");
                                let _ = front.answer.send(decision);
                                shown = None;
                                match approvals.front() {
                                    Some(a) => show_approval(&mut sender, a, &mut shown, &shared).await,
                                    None => match effective(&focus, &tab) {
                                        Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &shared).await,
                                        None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets,
                                                              &mut shown, &shared).await,
                                    },
                                }
                            }
                            None => show_approval(&mut sender, front, &mut shown, &shared).await,
                        }
                        continue;
                    }
                    let decision = match key {
                        1 => Some(crate::agent_hooks::Decision::Allow),
                        2 if approvals.front().is_some_and(|a| a.can_remember) => Some(crate::agent_hooks::Decision::AlwaysAllow),
                        3 => Some(crate::agent_hooks::Decision::Deny),
                        7 => Some(crate::agent_hooks::Decision::Stop),
                        _ => None,
                    };
                    if let (Some(decision), Some(front)) = (decision, approvals.pop_front()) {
                        println!("  answered on the board: {decision:?} ({})", front.summary);
                        let _ = front.answer.send(decision);
                        shown = None;
                        match approvals.front() {
                            Some(a) => show_approval(&mut sender, a, &mut shown, &shared).await,
                            None => match effective(&focus, &tab) {
                                Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &shared).await,
                                None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets,
                                                      &mut shown, &shared).await,
                            },
                        }
                    }
                }
                Event::Board(packet) => {
                    let current = effective(&focus, &tab);
                    if let Some(target) = on_board_packet(&packet, current.as_ref(), &shown_actions, &players, &mut sender, &shared).await {
                        let scope = site_scope(&catalog, &target);
                        start_configure(target, scope, &events, &shared).await;
                    }
                }
                Event::Agent(crate::agent_hooks::AgentEvent::Permission { request, answer }) => {
                    let summary = crate::agent_hooks::summary(&request);
                    println!("Permission request: {summary}");
                    approvals.push_back(Approval { request: request.clone(), summary, detail: crate::agent_hooks::detail(&request), destructive: crate::agent_hooks::destructive(&request),
                                                   can_remember: request["permission_suggestions"].as_array().is_some_and(|s| !s.is_empty()),
                                                   questions: crate::agent_hooks::questions(&request), step: 0, answers: Vec::new(), ticked: Vec::new(),
                                                   answer });
                    if approvals.len() == 1 {
                        show_approval(&mut sender, approvals.front().unwrap(), &mut shown, &shared).await;
                    }
                }
                Event::Agent(crate::agent_hooks::AgentEvent::Notification(n)) => {
                    // Answered in Claude's own window? Then the request leaves the board.
                    let before = approvals.len();
                    approvals.retain(|a| !crate::agent_hooks::settles(&n, &a.request));
                    if approvals.len() != before {
                        println!("  permission request settled in the app; withdrawn from the board");
                        shown = None;
                        match approvals.front() {
                            Some(a) => show_approval(&mut sender, a, &mut shown, &shared).await,
                            None => match effective(&focus, &tab) {
                                Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &shared).await,
                                None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets,
                                                      &mut shown, &shared).await,
                            },
                        }
                    }
                    if n["hook_event_name"] != "Notification" {
                        continue;
                    }
                    let kind = n["notification_type"].as_str().unwrap_or("");
                    // Permission prompts arrive as PermissionRequest already.
                    if !matches!(kind, "permission_prompt" | "auth_success" | "elicitation_complete" | "elicitation_response") {
                        let message = n["message"].as_str().unwrap_or("needs your attention");
                        println!("Agent notification ({kind}): {message}");
                        let text = board_text(&format!("Claude: {message}"), 80);
                        if approvals.is_empty() {
                            let _ = sender.send("openpager.status", json!({"text": text, "level": "alert", "ttl": 60})).await;
                        }
                    }
                }
                Event::Notified(n) => {
                    println!("Notification from {}: {}", n.app, n.summary);
                    let text = board_text(&format!("{}: {}", n.app, if n.summary.is_empty() { &n.body } else { &n.summary }), 80);
                    // A pending permission request keeps the status line (it is what the notification is about).
                    if approvals.is_empty() {
                        let _ = sender.send("openpager.status", json!({"text": text, "level": "alert", "ttl": 60})).await;
                    }
                    shared.emit("notification", json!({"app": n.app, "summary": n.summary}));
                }
                Event::Status(text) => {
                    let _ = sender.send("openpager.status", json!({"text": board_text(&text, 80), "level": "attention", "ttl": 20})).await;
                }
                Event::ConfigureSite => match effective(&focus, &tab).filter(|f| f.web.is_some()) {
                    Some(target) => {
                        let text = format!("Configuring {}: reading the page...", board_text(target.display_name(), 40));
                        let _ = sender.send("openpager.status", json!({"text": text, "level": "attention"})).await;
                        let scope = site_scope(&catalog, &target);
                        start_configure(target, scope, &events, &shared).await;
                    }
                    None => shared.browser.notify(json!({"type": "configure-status", "ok": false,
                                                         "text": "Focus the browser window with the site first."})),
                },
                Event::Configured(target, result) => {
                    shared.update(|s| s.configuring = None).await;
                    match result {
                        Ok((path, library)) => {
                            let keys = library.keys.len();
                            if let Some(web) = &target.web {
                                drafted.add(&web.host, &web.page_kind());
                            }
                            shared.emit("configured", json!({"text": format!("  saved {} -> {}", library.pack.id, path.display()),
                                                             "app": target.display_name(), "library": library.pack.id}));
                            catalog.add(path, library);
                            let count = catalog.len();
                            shared.update(|s| s.libraries = count).await;
                            if target.web.is_some() {
                                shared.browser.notify(json!({"type": "configure-status", "ok": true,
                                    "text": format!("Made {keys} keys for {}. Look at the board.", target.display_name())}));
                            }
                            let same_site = |f: &Focus| f.web.as_ref().map(|w| &w.host) == target.web.as_ref().map(|w| &w.host);
                            if effective(&focus, &tab).is_some_and(|f| f.app == target.app && f.instance == target.instance && same_site(&f)) {
                                shown = None;
                                show(&mut sender, &catalog, &target, &players, &mut shown, &mut shown_actions, &shared).await;
                            }
                        }
                        Err(error) => {
                            if target.web.is_some() {
                                shared.browser.notify(json!({"type": "configure-status", "ok": false, "text": format!("Could not make keys: {error}")}));
                            }
                            shared.emit("configure-failed", json!({"text": format!("  configure failed for {}: {error}", target.display_name()),
                                                                   "app": target.display_name(), "error": error}));
                            let mut text = format!("Could not configure {}: {error}", target.display_name());
                            text.truncate(80);
                            let _ = sender.send("openpager.status", json!({"text": text, "level": "alert", "ttl": 15})).await;
                        }
                    }
                }
                Event::LinkDown(reason) => {
                    shared.emit("log", json!({"text": format!("Link to board {} lost: {reason}", board.id)}));
                    shared.update(|s| { s.link = "searching".into(); s.layout = None; }).await;
                    break;
                }
            }
        }
    }
}

/// The focused app, with the browser's active site attached when the app is a browser.
fn effective(focus: &Option<Focus>, tab: &Option<WebPage>) -> Option<Focus> {
    let mut f = focus.clone()?;
    if crate::browser::is_browser(&f.app, &f.instance) {
        f.web = tab.clone();
    }
    Some(f)
}

/// While not connected, keep the latest focus without blocking the reconnect for long.
async fn drain_focus(inbox: &mut mpsc::Receiver<Event>, focus: &mut Option<Focus>, tab: &mut Option<WebPage>,
                     players: &mut Vec<NowPlaying>, shared: &Shared) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, inbox.recv()).await {
        match event {
            Event::Focus(Some(f)) => *focus = Some(f),
            Event::Tab(t) => *tab = t,
            Event::Players(p) => { *players = p; continue; }
            _ => continue,
        }
        if let Some(f) = effective(focus, tab) {
            let name = f.display_name().to_string();
            shared.update(|s| s.app = Some(name)).await;
        }
    }
}

fn spawn_focus_watcher(events: mpsc::Sender<Event>, shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let watcher = match openpager_focus::Watcher::connect() {
            Ok(w) => w,
            Err(e) => {
                shared.emit("log", json!({"text": format!("Focus watcher: {e}")}));
                return;
            }
        };
        let mut last = watcher.current();
        let _ = events.blocking_send(Event::Focus(last.clone()));
        loop {
            last = watcher.wait_for_change(last.as_ref());
            if events.blocking_send(Event::Focus(last.clone())).is_err() {
                return;
            }
        }
    });
}

async fn connect(pairings: &Pairings, events: mpsc::Sender<Event>) -> Result<(BoardStatus, wifi::WifiSender), String> {
    let wanted: Vec<String> = pairings.boards.keys().cloned().collect();
    let first = (wanted.len() == 1).then(|| wanted[0].clone());
    let found = tokio::task::spawn_blocking(move || discover::discover(Duration::from_secs(10), first.as_deref()))
        .await.map_err(|e| e.to_string())??;
    let board = found.into_iter().find(|b| wanted.contains(&b.device_id))
        .ok_or("no paired board found on the network")?;
    let key = pairings.key_for(&board.device_id).ok_or("stored key is not valid hex")?;
    let (sender, mut receiver) = wifi::connect((board.address, board.port).into(), &board.device_id, &pairings.companion, key)
        .await.map_err(|e| e.to_string())?;

    let identity = receiver.recv().await.map_err(|e| e.to_string())?;
    let status = BoardStatus {
        id: board.device_id.clone(),
        profile: identity.body["board"]["profile"].as_str().unwrap_or("?").to_string(),
        firmware: identity.body["firmware"].as_str().unwrap_or("?").to_string(),
        address: board.address.to_string(),
    };
    tokio::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(packet) => {
                    if events.send(Event::Board(packet)).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = events.send(Event::LinkDown(e.to_string())).await;
                    return;
                }
            }
        }
    });
    Ok((status, sender))
}

/// Pushes the layout for `focus`, or the "unsupported app" screen. When the focused app has a
/// media player, its play/pause key and the status bar follow what it plays. Sends nothing when the
/// board already shows exactly this.
async fn show(sender: &mut wifi::WifiSender, catalog: &Catalog, focus: &Focus, players: &[NowPlaying],
              shown: &mut Option<String>, shown_actions: &mut HashMap<u8, KeyRef>, shared: &Shared) {
    if shown.as_deref().is_some_and(|s| s.starts_with(APPROVE_SIGNATURE)) {
        return; // a permission request is on the board; it stays until answered
    }
    let player = openpager_media::for_focus(players, &focus.app, &focus.instance);
    let mut actions: HashMap<u8, KeyRef> = HashMap::new();
    let (mut body, mut status) = match catalog.find(focus) {
        Some(library) => match layout::build(library, focus.os) {
            Ok(mut body) => {
                for (key, slot) in library.keys.iter().zip(library.slots()) {
                    actions.insert(slot, KeyRef { label: key.label.clone(), action: key.action.clone() });
                }
                if let Some(player) = player {
                    for (key, slot) in library.keys.iter().zip(library.slots()) {
                        if let Action::Media { command: MediaCommand::Toggle, .. } = key.action {
                            if let Some(k) = body.keys.iter_mut().find(|k| k.key == slot) {
                                k.label = if player.playing() { "Pause".into() } else { "Play".into() };
                                k.icon = Some(if player.playing() { "pause" } else { "play_arrow" }.into());
                            }
                        }
                    }
                }
                (body, json!({"text": ""}))
            }
            Err(e) => {
                println!("  library {} cannot be built: {e}", library.pack.id);
                return;
            }
        },
        None => (unsupported_layout(focus), json!({"text": if focus.web.is_some() { format!("No keys for {} yet", board_text(focus.display_name(), 50)) }
                                         else { format!("Unsupported app: {}", board_text(focus.display_name(), 50)) }, "level": "attention"})),
    };
    if let (Some(player), true) = (player, status["text"] == "") {
        let what = [board_text(&player.title, 50), board_text(&player.artist, 24)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" - ");
        if !what.is_empty() {
            status = json!({"text": format!("{}: {what}", if player.playing() { "Playing" } else { "Paused" }), "level": "info"});
        }
    }
    // While the focused app plays something, the bottom row controls it (as KDE Connect's media
    // controls do), unless the layout has media keys of its own.
    if let Some(player) = player.filter(|p| p.playing() || (p.status == "Paused" && !p.title.is_empty())) {
        if !actions.values().any(|k| matches!(k.action, Action::Media { .. })) {
            let row = [(7u8, player.can_go_previous, "Prev", "Previous track", "skip_previous", MediaCommand::Previous),
                       (8, true, if player.playing() { "Pause" } else { "Play" }, "Play or pause",
                        if player.playing() { "pause" } else { "play_arrow" }, MediaCommand::Toggle),
                       (9, player.can_go_next, "Next", "Next track", "skip_next", MediaCommand::Next)];
            for (slot, offered, label, speak, icon, command) in row {
                if !offered {
                    continue;
                }
                body.keys.retain(|k| k.key != slot);
                body.keys.push(LayoutKey { key: slot, label: label.into(), speak: Some(speak.into()), icon: Some(icon.into()), confirm: false, send: Send::Event(true) });
                actions.insert(slot, KeyRef { label: label.into(), action: Action::Media { command, selector: None } });
            }
            body.keys.sort_by_key(|k| k.key);
        }
    }
    body.name = board_text(&body.name, 128);
    *shown_actions = actions;
    let signature = format!("{}|{}", serde_json::to_string(&body).unwrap_or_default(), status);
    if shown.as_deref() == Some(signature.as_str()) {
        return;
    }
    let id = body.layout.clone();
    println!("  -> {} ({}){} keys: {}", body.name, id, status["text"].as_str().filter(|t| !t.is_empty()).map(|t| format!(" [{t}]")).unwrap_or_default(),
             body.keys.iter().map(|k| format!("{}={}", k.key, k.label)).collect::<Vec<_>>().join(", "));
    let summary = json!({"id": id, "name": body.name, "source": body.source, "reviewed": body.reviewed,
                         "keys": body.keys.iter().map(|k| json!({"key": k.key, "label": k.label})).collect::<Vec<_>>()});
    let body = serde_json::to_value(&body).expect("serialisable");
    if sender.send("openpager.layout", body).await.is_ok() {
        let _ = sender.send("openpager.status", status).await;
        *shown = Some(signature);
        shared.update(|s| s.layout = Some(summary)).await;
    }
}

/// Text the board's font can draw (printable ASCII), on one line, at most `max` characters.
fn board_text(text: &str, max: usize) -> String {
    let ascii: String = text.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { ' ' }).collect();
    let mut out = ascii.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.chars().count() > max {
        out = out.chars().take(max.saturating_sub(3)).collect::<String>() + "...";
    }
    out
}

/// What a site draft should cover: the whole site the first time; once the site has keys, the
/// kind of page the person is on (told the site-wide keys, so it does not repeat them).
fn site_scope(catalog: &Catalog, focus: &Focus) -> Scope {
    let Some(web) = &focus.web else { return Scope::Site };
    match catalog.site_wide(web) {
        Some(site) => Scope::Page { kind: web.page_kind(), site_keys: site.keys.iter().map(|k| k.label.clone()).collect() },
        None => Scope::Site,
    }
}

/// Once the person made keys for a site, a kind of page with no keys of its own gets them drafted
/// in the background (once per kind; remembered across runs).
async fn draft_new_page(catalog: &Catalog, focus: &Focus, drafted: &DraftedPages, attempted: &mut HashSet<(String, String)>,
                        events: &mpsc::Sender<Event>, shared: &Arc<Shared>) {
    // Not while full screen: a presentation or a full-screen video has nothing to draft from.
    let Some(web) = focus.web.as_ref().filter(|w| w.granted && !w.fullscreen) else { return };
    let Some(site) = catalog.site_wide(web) else { return };
    // Only sites the person configured with the AI, and only where nothing more specific applies.
    if site.pack.source != openpager_core::library::Source::Ai || catalog.find(focus).map(|l| &l.pack.id) != Some(&site.pack.id) {
        return;
    }
    let kind = web.page_kind();
    if drafted.has(&web.host, &kind) || !attempted.insert((web.host.clone(), kind.clone())) {
        return;
    }
    if shared.status.read().await.configuring.is_some() {
        attempted.remove(&(web.host.clone(), kind));
        return;
    }
    println!("  new kind of page on {}: {kind}; drafting its keys", web.host);
    shared.board_status("Making keys for this page...".into());
    start_configure(focus.clone(), site_scope(catalog, focus), events, shared).await;
}

/// Layout id of the launcher (nothing focused).
const LAUNCHER: &str = "system.launcher";

/// The launcher: the most used and recent apps and sites, with their own icons (sent to the board
/// first, once per connection), or a hint while there is no history yet.
#[allow(clippy::too_many_arguments)]
async fn show_launcher(sender: &mut wifi::WifiSender, usage: &crate::launcher::Usage, apps: &[crate::launcher::AppInfo],
                       favicons: &HashMap<String, Vec<u8>>, sent_images: &mut HashSet<String>,
                       targets: &mut HashMap<u8, crate::launcher::Target>, shown: &mut Option<String>, shared: &Shared) {
    targets.clear();
    let mut keys = Vec::new();
    for (i, entry) in usage.top(9).into_iter().enumerate() {
        let slot = i as u8 + 1;
        let name = entry.target.image_name();
        let mut icon = Some(match entry.target { crate::launcher::Target::Site { .. } => "public", _ => "apps" }.to_string());
        if sent_images.contains(&name) {
            icon = Some(name.clone());
        } else if let Some(pixels) = crate::launcher::icon_pixels(&entry.target, &entry.name, apps, favicons) {
            if send_image(sender, &name, &pixels).await {
                sent_images.insert(name.clone());
                icon = Some(name.clone());
            }
        }
        keys.push(LayoutKey { key: slot, label: board_text(&entry.name, 24), speak: Some(format!("Open {}", entry.name)),
                              icon, confirm: false, send: Send::Event(true) });
        targets.insert(slot, entry.target.clone());
    }
    let status = if keys.is_empty() { "Nothing open. Apps and sites you use will appear here." } else { "" };
    let body = LayoutBody { detail: None, layout: LAUNCHER.into(), name: "Launcher".into(), source: "default".into(), reviewed: true, keys };
    let signature = format!("launcher|{}", serde_json::to_string(&body).unwrap_or_default());
    if shown.as_deref() == Some(signature.as_str()) {
        return;
    }
    println!("  -> launcher: {}", body.keys.iter().map(|k| k.label.as_str()).collect::<Vec<_>>().join(", "));
    if sender.send("openpager.layout", serde_json::to_value(&body).expect("serialisable")).await.is_ok() {
        let _ = sender.send("openpager.status", json!({"text": status})).await;
        *shown = Some(signature);
        shared.update(|s| s.layout = Some(json!({"id": LAUNCHER, "name": "Launcher"}))).await;
    }
}

/// Sends a 32x32 RGBA picture to the board in slices that fit a packet. False if the board
/// refused the first slice (a board without picture support ignores the rest).
async fn send_image(sender: &mut wifi::WifiSender, name: &str, rgba: &[u8]) -> bool {
    use base64::Engine;
    let pixels = crate::launcher::board_pixels(rgba);
    for (i, chunk) in pixels.chunks(2048).enumerate() {
        let body = json!({"name": name, "w": 32, "h": 32, "offset": i * 2048, "total": pixels.len(),
                          "data": base64::engine::general_purpose::STANDARD.encode(chunk)});
        if sender.send("openpager.image", body).await.is_err() {
            return false;
        }
    }
    true
}

/// Fetches a site's icon (http(s), or a data: URL) and shrinks it to 32x32, off the loop.
fn spawn_favicon_fetch(host: String, url: String, events: mpsc::Sender<Event>) {
    tokio::spawn(async move {
        let bytes: Option<Vec<u8>> = if let Some(data) = url.strip_prefix("data:") {
            use base64::Engine;
            data.split_once(";base64,").and_then(|(_, b)| base64::engine::general_purpose::STANDARD.decode(b).ok())
        } else {
            let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().ok();
            match client {
                Some(c) => match c.get(&url).send().await { Ok(r) if r.status().is_success() => r.bytes().await.ok().map(|b| b.to_vec()), _ => None },
                None => None,
            }
        };
        let Some(bytes) = bytes.filter(|b| b.len() <= 512 * 1024) else { return };
        let svg = url.contains("svg");
        let pixels = tokio::task::spawn_blocking(move || openpager_desktop::picture::render_bytes(&bytes, svg, 32)).await.ok().flatten();
        if let Some(pixels) = pixels {
            let _ = events.send(Event::Favicon(host, pixels)).await;
        }
    });
}

/// Saved site icons from the cache folder.
fn load_favicons() -> HashMap<String, Vec<u8>> {
    let mut out = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(crate::launcher::favicon_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            let host = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if let Some(pixels) = openpager_desktop::picture::render(&path, 32) {
                out.insert(host, pixels);
            }
        }
    }
    out
}

/// 32x32 RGBA as PNG bytes (for the favicon cache).
fn image_png(rgba: &[u8]) -> Option<Vec<u8>> {
    openpager_desktop::picture::png(rgba, 32, 32)
}

/// Layout id of a coding agent's permission request on the board.
const APPROVE: &str = "system.approve";
const APPROVE_SIGNATURE: &str = "approve|";

/// A coding agent waiting for the person's answer.
struct Approval {
    /// The hook's event (session, tool, input), to tell when it was answered elsewhere.
    request: Value,
    summary: String,
    detail: String,
    destructive: bool,
    /// The agent offered a rule to remember ("always allow").
    can_remember: bool,
    /// When the agent asks the person something (AskUserQuestion): its questions, which one is
    /// on the board, the answers so far, and the options ticked on a several-choice question.
    questions: Vec<crate::agent_hooks::Question>,
    step: usize,
    answers: Vec<(String, String)>,
    ticked: Vec<usize>,
    answer: tokio::sync::oneshot::Sender<crate::agent_hooks::Decision>,
}

/// Board slots of a question's options (the middle row holds the question).
const OPTION_SLOTS: [u8; 4] = [1, 2, 3, 7];

impl Approval {
    /// A tap on a question's key: Some(decision) once every question is answered or the person
    /// declined; None while there are more questions (or ticks) to make.
    fn tap_question(&mut self, key: u8) -> Option<crate::agent_hooks::Decision> {
        use crate::agent_hooks::Decision;
        let q = self.questions.get(self.step)?.clone();
        if key == 9 {
            return Some(Decision::Deny);
        }
        let chosen: Vec<usize> = match OPTION_SLOTS.iter().position(|&s| s == key).filter(|&i| i < q.options.len()) {
            Some(i) if q.multi => {
                if let Some(at) = self.ticked.iter().position(|&t| t == i) { self.ticked.remove(at); } else { self.ticked.push(i); }
                return None;
            }
            Some(i) => vec![i],
            None if key == 8 && q.multi && !self.ticked.is_empty() => { let mut t = std::mem::take(&mut self.ticked); t.sort(); t }
            None => return None,
        };
        let text = chosen.iter().map(|&i| q.options[i].0.clone()).collect::<Vec<_>>().join(", ");
        self.answers.push((q.question.clone(), text));
        self.step += 1;
        (self.step >= self.questions.len()).then(|| Decision::Answer(crate::agent_hooks::answered(&self.request, &self.answers)))
    }
}

/// Shows a permission request: Allow, Always (when the agent offers it), Deny, Stop.
async fn show_approval(sender: &mut wifi::WifiSender, approval: &Approval, shown: &mut Option<String>, shared: &Shared) {
    let key = |slot: u8, label: &str, speak: &str, icon: &str, confirm: bool| LayoutKey {
        key: slot, label: label.into(), speak: Some(speak.into()), icon: Some(icon.into()), confirm, send: Send::Event(true) };
    if let Some(q) = approval.questions.get(approval.step) {
        // A question: its options as keys, the question and what each option means in the panel.
        let mut keys: Vec<LayoutKey> = q.options.iter().zip(OPTION_SLOTS).map(|((label, _), slot)| {
            let ticked = approval.ticked.contains(&(OPTION_SLOTS.iter().position(|&s| s == slot).unwrap_or(9)));
            LayoutKey { key: slot, label: board_text(label, 24), speak: Some(board_text(label, 80)),
                        icon: ticked.then(|| "check".to_string()), confirm: false, send: Send::Event(true) }
        }).collect();
        if q.multi {
            keys.push(key(8, "Send", "Send the ticked answers", "send", false));
        }
        keys.push(key(9, "Decline", "Do not answer", "close", false));
        let count = if approval.questions.len() > 1 { format!(" ({}/{})", approval.step + 1, approval.questions.len()) } else { String::new() };
        let mut detail = format!("{}{count}\n{}", q.question, if q.multi { "Tick one or more, then Send." } else { "" });
        for (label, about) in &q.options {
            if !about.is_empty() {
                detail += &format!("\n{label}: {about}");
            }
        }
        let detail: String = detail.chars().map(|c| if c == '\n' || c.is_ascii_graphic() || c == ' ' { c } else { ' ' }).take(240).collect();
        let title = if q.header.is_empty() { "Claude asks".to_string() } else { format!("Claude asks: {}", q.header) };
        let body = LayoutBody { layout: APPROVE.into(), name: board_text(&title, 128), source: "default".into(), reviewed: true, keys, detail: Some(detail) };
        println!("  -> question on the board [{}]", q.question);
        let signature = format!("{APPROVE_SIGNATURE}{}", serde_json::to_string(&body).unwrap_or_default());
        if sender.send("openpager.layout", serde_json::to_value(&body).expect("serialisable")).await.is_ok() {
            let _ = sender.send("openpager.status", json!({"text": board_text(&format!("Claude asks: {}", q.question), 80), "level": "attention"})).await;
            *shown = Some(signature);
            shared.update(|s| s.layout = Some(json!({"id": APPROVE, "name": title, "summary": q.question}))).await;
        }
        return;
    }
    let mut keys = vec![key(1, "Allow", "Allow", "check", approval.destructive)];
    if approval.can_remember {
        keys.push(key(2, "Always", "Always allow this", "done_all", approval.destructive));
    }
    keys.push(key(3, "Deny", "Deny", "close", false));
    keys.push(key(7, "Stop", "Deny and stop Claude", "stop_circle", false));
    let detail: String = approval.detail.chars().map(|c| if c == '\n' || c.is_ascii_graphic() || c == ' ' { c } else { ' ' }).take(240).collect();
    let body = LayoutBody { layout: APPROVE.into(), name: "Claude needs you".into(), source: "default".into(), reviewed: true, keys,
                            detail: Some(detail) };
    println!("  -> permission request on the board [{}]", approval.summary);
    if sender.send("openpager.layout", serde_json::to_value(&body).expect("serialisable")).await.is_ok() {
        let level = if approval.destructive { "alert" } else { "attention" };
        let _ = sender.send("openpager.status", json!({"text": board_text(&approval.summary, 80), "level": level})).await;
        *shown = Some(format!("{APPROVE_SIGNATURE}{}", approval.summary));
        shared.update(|s| s.layout = Some(json!({"id": APPROVE, "name": "Claude needs you", "summary": approval.summary}))).await;
    }
}

/// What one key on the board does now.
#[derive(Debug, Clone)]
pub struct KeyRef {
    pub label: String,
    pub action: Action,
}

/// Kinds of page that have their own drafted keys, per site (`sites.json` in the config folder).
#[derive(Default, Serialize, serde::Deserialize)]
struct DraftedPages {
    sites: HashMap<String, Vec<String>>,
}

impl DraftedPages {
    fn path() -> PathBuf {
        config_dir().join("sites.json")
    }
    fn load() -> DraftedPages {
        std::fs::read_to_string(Self::path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    fn has(&self, host: &str, kind: &str) -> bool {
        self.sites.get(host).is_some_and(|k| k.iter().any(|x| x == kind))
    }
    fn add(&mut self, host: &str, kind: &str) {
        let kinds = self.sites.entry(host.to_string()).or_default();
        if !kinds.iter().any(|k| k == kind) {
            kinds.push(kind.to_string());
        }
        let _ = std::fs::write(Self::path(), serde_json::to_string_pretty(self).unwrap_or_default());
    }
}

/// Drafts keys for `target` (an app, or the site in its browser tab) in the background; the
/// result comes back as `Event::Configured`.
async fn start_configure(target: Focus, scope: Scope, events: &mpsc::Sender<Event>, shared: &Arc<Shared>) {
    let (events, shared) = (events.clone(), shared.clone());
    let name = target.display_name().to_string();
    shared.update(|s| s.configuring = Some(name)).await;
    tokio::spawn(async move {
        let result = if target.web.is_some() {
            crate::configure_site(&shared, &target, scope).await
        } else {
            crate::configure(&target, true).await
        };
        let _ = events.send(Event::Configured(target, result)).await;
    });
}

/// Apps whose desktop notifications reach the board: coding and chat agents that finish work or
/// wait for the person while they look elsewhere.
const NOTIFYING_APPS: &[&str] = &["claude", "codex", "antigravity", "chatgpt", "cursor"];

/// Forwards notifications from `NOTIFYING_APPS` (Linux; nothing elsewhere).
fn spawn_notification_watcher(events: mpsc::Sender<Event>) {
    std::thread::spawn(move || {
        let result = openpager_desktop::watch_notifications(|n| {
            !n.from_any(NOTIFYING_APPS) || events.blocking_send(Event::Notified(n)).is_ok()
        });
        if let Err(e) = result {
            println!("Desktop notifications not followed: {e}");
        }
    });
}

/// Polls the media players every second and reports when anything shown changes.
fn spawn_media_watcher(media: Arc<openpager_media::Players>, events: mpsc::Sender<Event>) {
    std::thread::spawn(move || {
        let mut last: Vec<NowPlaying> = Vec::new();
        loop {
            let now = media.list();
            if now != last {
                if events.blocking_send(Event::Players(now.clone())).is_err() {
                    return;
                }
                last = now;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

fn unsupported_layout(focus: &Focus) -> LayoutBody {
    let mut name: String = format!("Unsupported: {}", focus.display_name());
    name.truncate(128);
    LayoutBody {
        detail: None,
        layout: UNSUPPORTED.into(),
        name,
        source: "default".into(),
        reviewed: true,
        keys: vec![LayoutKey { key: CONFIGURE_SLOT, label: "Configure".into(), speak: Some("Configure this app".into()), icon: Some("auto_awesome".into()), confirm: false, send: Send::Event(true) }],
    }
}

/// Handles a packet from the board; returns the app to configure when Configure was tapped.
async fn on_board_packet(packet: &Packet, focus: Option<&Focus>, shown_actions: &HashMap<u8, KeyRef>, players: &[NowPlaying],
                         sender: &mut wifi::WifiSender, shared: &Arc<Shared>) -> Option<Focus> {
    match packet.kind.as_str() {
        "openpager.layout.ack" if packet.body["ok"] != true => println!("  board rejected layout: {}", packet.body),
        "openpager.key" => {
            let layout = packet.body["layout"].as_str().unwrap_or("");
            let key = packet.body["key"].as_u64().unwrap_or(0);
            shared.emit("key", json!({"text": format!("Key {key} on {layout}"), "key": key, "layout": layout}));
            // A shortcut the board could not type itself (no Bluetooth host): type it here.
            if let (Some(modifiers), Some(usage)) = (packet.body["hid"]["modifiers"].as_u64(), packet.body["hid"]["usage"].as_u64()) {
                let typist = shared.typist.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut slot = typist.lock().map_err(|_| "typist lock".to_string())?;
                    if slot.is_none() {
                        *slot = Some(openpager_input::Typist::connect()?);
                    }
                    slot.as_ref().expect("connected").tap(modifiers as u8, usage as u8)
                }).await.unwrap_or_else(|e| Err(e.to_string()));
                match result {
                    Ok(()) => shared.emit("typed", json!({"text": format!("  typed key {key} on this computer (board has no Bluetooth host)")})),
                    Err(e) => shared.emit("type-failed", json!({"text": format!("  could not type key {key}: {e}")})),
                }
                return None;
            }
            // A key whose action happens on the web page: the browser extension carries it out.
            if layout != UNSUPPORTED || key != u64::from(CONFIGURE_SLOT) {
                if let Some(library_key) = u8::try_from(key).ok().and_then(|k| shown_actions.get(&k)) {
                    // Media keys go to the focused app's media player first (like the keyboard's
                    // media keys): the browser then allows starting playback.
                    let player = focus.and_then(|f| openpager_media::for_focus(players, &f.app, &f.instance));
                    if let (Action::Media { command, .. }, Some(player), Some(media)) = (&library_key.action, player, shared.media.clone()) {
                        let mpris = match command {
                            MediaCommand::Toggle => Some(openpager_media::Command::PlayPause),
                            MediaCommand::Play => Some(openpager_media::Command::Play),
                            MediaCommand::Pause => Some(openpager_media::Command::Pause),
                            MediaCommand::Next if player.can_go_next => Some(openpager_media::Command::Next),
                            MediaCommand::Previous if player.can_go_previous => Some(openpager_media::Command::Previous),
                            MediaCommand::Forward if player.can_seek => Some(openpager_media::Command::Seek(10)),
                            MediaCommand::Back if player.can_seek => Some(openpager_media::Command::Seek(-10)),
                            _ => None,
                        };
                        if let Some(mpris) = mpris {
                            let (bus, label, shared) = (player.bus.clone(), library_key.label.clone(), shared.clone());
                            tokio::task::spawn_blocking(move || {
                                let text = match media.command(&bus, mpris) {
                                    Ok(()) => format!("  {label}: sent to the media player"),
                                    Err(e) => format!("  {label}: the media player refused ({e})"),
                                };
                                shared.emit("media-action", json!({"text": text}));
                            });
                            return None;
                        }
                    }
                    if library_key.action.is_web() {
                        let (shared, label) = (shared.clone(), library_key.label.clone());
                        let action = serde_json::to_value(&library_key.action).unwrap_or_default();
                        let site = focus.and_then(|f| f.web.as_ref()).map(|w| openpager_agents::site::site_of(&w.host)).unwrap_or_default();
                        tokio::spawn(async move {
                            let act = || shared.browser.request(json!({"type": "act", "action": action.clone()}), Duration::from_secs(5));
                            let mut answer = act().await;
                            // Not allowed yet: ask in the browser, then do it.
                            if answer.as_ref().is_ok_and(|r| r["error"] == "not-granted") && shared.ask_site_permission(&site).await.is_ok() {
                                answer = act().await;
                            }
                            let text = match answer {
                                Ok(r) if r["ok"] == true => format!("  {label}: done in the browser"),
                                Ok(r) => format!("  {label}: the browser could not do it ({})", r["error"].as_str().unwrap_or("unknown")),
                                Err(e) => format!("  {label}: {e}"),
                            };
                            shared.emit("web-action", json!({"text": text}));
                        });
                        return None;
                    }
                }
            }
            if layout == UNSUPPORTED && key == u64::from(CONFIGURE_SLOT) {
                let target = focus?.clone();
                println!("  Configure requested for {}", target.display_name());
                let text = format!("Configuring {}: drafting keys...", target.display_name());
                let _ = sender.send("openpager.status", json!({"text": text, "level": "attention"})).await;
                return Some(target);
            }
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_hooks::Decision;

    fn asking(questions: Value) -> (Approval, tokio::sync::oneshot::Receiver<Decision>) {
        let request = json!({"tool_name": "AskUserQuestion", "session_id": "s", "tool_input": {"questions": questions}});
        let (answer, rx) = tokio::sync::oneshot::channel();
        (Approval { questions: crate::agent_hooks::questions(&request), request, summary: String::new(), detail: String::new(),
                    destructive: false, can_remember: false, step: 0, answers: Vec::new(), ticked: Vec::new(), answer }, rx)
    }

    #[test]
    fn questions_are_answered_one_after_another() {
        let (mut a, _rx) = asking(json!([
            {"question": "Which?", "header": "", "multiSelect": false, "options": [{"label": "A"}, {"label": "B"}, {"label": "C"}, {"label": "D"}]},
            {"question": "Also?", "header": "", "multiSelect": true, "options": [{"label": "X"}, {"label": "Y"}]}]));
        assert_eq!(a.tap_question(8), None, "no Send key on a one-choice question");
        assert_eq!(a.tap_question(7), None, "slot 7 = 4th option, answers the first question");
        assert_eq!(a.answers, vec![("Which?".to_string(), "D".to_string())]);
        assert_eq!(a.tap_question(2), None);
        assert_eq!(a.tap_question(1), None);
        assert_eq!(a.tap_question(2), None, "untick");
        assert_eq!(a.tap_question(2), None, "tick again");
        match a.tap_question(8) {
            Some(Decision::Answer(input)) => {
                assert_eq!(input["answers"]["Which?"], "D");
                assert_eq!(input["answers"]["Also?"], "X, Y");
            }
            other => panic!("{other:?}"),
        }
        let (mut b, _rx) = asking(json!([{"question": "Q", "options": [{"label": "A"}, {"label": "B"}]}]));
        assert_eq!(b.tap_question(3), None, "slot 3 has no option here");
        assert_eq!(b.tap_question(9), Some(Decision::Deny));
    }
}
