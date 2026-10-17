//! The companion service: find the paired board on Wi-Fi, follow the focused app, push layouts,
//! and run the configure agent when Configure is tapped. State and events are shared with the
//! web app; window titles never leave this loop (only the app's name is published).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use openpager_core::catalog::Catalog;
use openpager_core::focus::Focus;
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
}

impl Shared {
    pub fn new(firmware_source: String) -> Arc<Shared> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Shared { status: RwLock::new(Status::default()), events, serial: Arc::new(tokio::sync::Mutex::new(())), firmware_source,
                          typist: Arc::new(std::sync::Mutex::new(None)),
                          recent: std::sync::Mutex::new(std::collections::VecDeque::new()) })
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
    spawn_focus_watcher(events.clone(), shared.clone());

    let mut focus: Option<Focus> = None;
    loop {
        let pairings = Pairings::load();
        if pairings.boards.is_empty() {
            shared.update(|s| { s.link = "no-pairing".into(); s.board = None; }).await;
            // Keep following focus while waiting, so the dashboard shows it.
            if let Ok(Some(Event::Focus(Some(f)))) = tokio::time::timeout(Duration::from_secs(2), inbox.recv()).await {
                shared.update(|s| s.app = Some(f.display_name().to_string())).await;
                focus = Some(f);
            }
            continue;
        }
        shared.update(|s| s.link = "searching".into()).await;
        let (board, mut sender) = match connect(&pairings, events.clone()).await {
            Ok(link) => link,
            Err(e) => {
                println!("{e}; retrying in 5 s");
                drain_focus(&mut inbox, &mut focus, &shared).await;
                continue;
            }
        };
        shared.emit("log", json!({"text": format!("Connected to board {} over Wi-Fi", board.id)}));
        let board_status = board.clone();
        shared.update(|s| { s.link = "connected".into(); s.board = Some(board_status); }).await;
        let mut shown: Option<String> = None;
        if let Some(f) = &focus {
            show(&mut sender, &catalog, f, &mut shown, &shared).await;
        }

        // A board that restarts never closes its old connection, so silence is the only sign:
        // ping every 10 s and treat 25 s without any packet as a lost link.
        let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
        heartbeat.tick().await;
        let mut last_heard = tokio::time::Instant::now();

        loop {
            let event = tokio::select! {
                event = inbox.recv() => match event { Some(e) => e, None => return },
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
                    println!("Focus: {} ({})", f.display_name(), f.instance);
                    let name = f.display_name().to_string();
                    shared.update(|s| s.app = Some(name)).await;
                    show(&mut sender, &catalog, &f, &mut shown, &shared).await;
                    focus = Some(f);
                }
                Event::Focus(None) => {}
                Event::Board(packet) => {
                    if let Some(target) = on_board_packet(&packet, focus.as_ref(), &mut sender, &shared).await {
                        let events = events.clone();
                        let name = target.display_name().to_string();
                        shared.update(|s| s.configuring = Some(name)).await;
                        tokio::spawn(async move {
                            let result = crate::configure(&target, true).await;
                            let _ = events.send(Event::Configured(target, result)).await;
                        });
                    }
                }
                Event::Configured(target, result) => {
                    shared.update(|s| s.configuring = None).await;
                    match result {
                        Ok((path, library)) => {
                            shared.emit("configured", json!({"text": format!("  saved {} -> {}", library.pack.id, path.display()),
                                                             "app": target.display_name(), "library": library.pack.id}));
                            catalog.add(path, library);
                            let count = catalog.len();
                            shared.update(|s| s.libraries = count).await;
                            if focus.as_ref().is_some_and(|f| f.app == target.app && f.instance == target.instance) {
                                shown = None;
                                show(&mut sender, &catalog, &target, &mut shown, &shared).await;
                            }
                        }
                        Err(error) => {
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

/// While not connected, keep the latest focus without blocking the reconnect for long.
async fn drain_focus(inbox: &mut mpsc::Receiver<Event>, focus: &mut Option<Focus>, shared: &Shared) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, inbox.recv()).await {
        if let Event::Focus(Some(f)) = event {
            let name = f.display_name().to_string();
            shared.update(|s| s.app = Some(name)).await;
            *focus = Some(f);
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

/// Pushes the layout for `focus`, or the "unsupported app" screen.
async fn show(sender: &mut wifi::WifiSender, catalog: &Catalog, focus: &Focus, shown: &mut Option<String>, shared: &Shared) {
    let (body, status) = match catalog.find(focus) {
        Some(library) => match layout::build(library, focus.os) {
            Ok(body) => (body, json!({"text": ""})),
            Err(e) => {
                println!("  library {} cannot be built: {e}", library.pack.id);
                return;
            }
        },
        None => (unsupported_layout(focus), json!({"text": format!("Unsupported app: {}", focus.display_name()), "level": "attention"})),
    };
    let id = body.layout.clone();
    if shown.as_deref() == Some(id.as_str()) && id != UNSUPPORTED {
        return;
    }
    println!("  -> {} ({})", body.name, id);
    let summary = json!({"id": id, "name": body.name, "source": body.source, "reviewed": body.reviewed,
                         "keys": body.keys.iter().map(|k| json!({"key": k.key, "label": k.label})).collect::<Vec<_>>()});
    let body = serde_json::to_value(&body).expect("serialisable");
    if sender.send("openpager.layout", body).await.is_ok() {
        let _ = sender.send("openpager.status", status).await;
        *shown = Some(id);
        shared.update(|s| s.layout = Some(summary)).await;
    }
}

fn unsupported_layout(focus: &Focus) -> LayoutBody {
    let mut name: String = format!("Unsupported: {}", focus.display_name());
    name.truncate(128);
    LayoutBody {
        layout: UNSUPPORTED.into(),
        name,
        source: "default".into(),
        reviewed: true,
        keys: vec![LayoutKey { key: CONFIGURE_SLOT, label: "Configure".into(), speak: Some("Configure this app".into()), confirm: false, send: Send::Event(true) }],
    }
}

/// Handles a packet from the board; returns the app to configure when Configure was tapped.
async fn on_board_packet(packet: &Packet, focus: Option<&Focus>, sender: &mut wifi::WifiSender, shared: &Shared) -> Option<Focus> {
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
