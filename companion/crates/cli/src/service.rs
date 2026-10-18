//! The companion service: find the paired board on Wi-Fi, follow the focused app, push layouts,
//! and run the configure agent when Configure is tapped. State and events are shared with the
//! web app; window titles never leave this loop (only the app's name is published).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use openpager_core::catalog::Catalog;
use openpager_core::focus::{Focus, WebPage};
use openpager_core::library::{Action, MediaCommand};
use openpager_media::NowPlaying;
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

    // What the OS says is focused, and the browser's active tab; `effective` joins them.
    let mut focus: Option<Focus> = None;
    let mut tab: Option<WebPage> = None;
    let mut players: Vec<NowPlaying> = Vec::new();
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
        if let Some(f) = effective(&focus, &tab) {
            show(&mut sender, &catalog, &f, &players, &mut shown, &shared).await;
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
                    focus = Some(f);
                    let f = effective(&focus, &tab).expect("focus");
                    println!("Focus: {} ({})", f.display_name(), f.instance);
                    let name = f.display_name().to_string();
                    shared.update(|s| s.app = Some(name)).await;
                    show(&mut sender, &catalog, &f, &players, &mut shown, &shared).await;
                }
                Event::Focus(None) => {}
                Event::Players(p) => {
                    players = p;
                    // Re-show only if this changes what the board shows (show() skips a repeat).
                    if let Some(f) = effective(&focus, &tab) {
                        show(&mut sender, &catalog, &f, &players, &mut shown, &shared).await;
                    }
                }
                Event::Tab(t) => {
                    let changed = t.as_ref().map(|p| (&p.host, p.granted)) != tab.as_ref().map(|p| (&p.host, p.granted));
                    tab = t;
                    let browser_focused = focus.as_ref().is_some_and(|f| crate::browser::is_browser(&f.app, &f.instance));
                    if changed && browser_focused {
                        let f = effective(&focus, &tab).expect("focus");
                        println!("Site: {}", f.display_name());
                        let name = f.display_name().to_string();
                        shared.update(|s| s.app = Some(name)).await;
                        show(&mut sender, &catalog, &f, &players, &mut shown, &shared).await;
                    }
                }
                Event::Board(packet) => {
                    let current = effective(&focus, &tab);
                    if let Some(target) = on_board_packet(&packet, current.as_ref(), &catalog, &players, &mut sender, &shared).await {
                        start_configure(target, &events, &shared).await;
                    }
                }
                Event::Status(text) => {
                    let _ = sender.send("openpager.status", json!({"text": board_text(&text, 80), "level": "attention", "ttl": 20})).await;
                }
                Event::ConfigureSite => match effective(&focus, &tab).filter(|f| f.web.is_some()) {
                    Some(target) => {
                        let text = format!("Configuring {}: reading the page...", board_text(target.display_name(), 40));
                        let _ = sender.send("openpager.status", json!({"text": text, "level": "attention"})).await;
                        start_configure(target, &events, &shared).await;
                    }
                    None => shared.browser.notify(json!({"type": "configure-status", "ok": false,
                                                         "text": "Focus the browser window with the site first."})),
                },
                Event::Configured(target, result) => {
                    shared.update(|s| s.configuring = None).await;
                    match result {
                        Ok((path, library)) => {
                            let keys = library.keys.len();
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
                                show(&mut sender, &catalog, &target, &players, &mut shown, &shared).await;
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
              shown: &mut Option<String>, shared: &Shared) {
    let player = openpager_media::for_focus(players, &focus.app, &focus.instance);
    let (mut body, mut status) = match catalog.find(focus) {
        Some(library) => match layout::build(library, focus.os) {
            Ok(mut body) => {
                if let Some(player) = player {
                    for (key, slot) in library.keys.iter().zip(library.slots()) {
                        if let Action::Media { command: MediaCommand::Toggle, .. } = key.action {
                            if let Some(k) = body.keys.iter_mut().find(|k| k.key == slot) {
                                k.label = if player.playing() { "Pause".into() } else { "Play".into() };
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
    body.name = board_text(&body.name, 128);
    let signature = format!("{}|{}", serde_json::to_string(&body).unwrap_or_default(), status);
    if shown.as_deref() == Some(signature.as_str()) {
        return;
    }
    let id = body.layout.clone();
    println!("  -> {} ({}){}", body.name, id, status["text"].as_str().filter(|t| !t.is_empty()).map(|t| format!(" [{t}]")).unwrap_or_default());
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

/// Drafts keys for `target` (an app, or the site in its browser tab) in the background; the
/// result comes back as `Event::Configured`.
async fn start_configure(target: Focus, events: &mpsc::Sender<Event>, shared: &Arc<Shared>) {
    let (events, shared) = (events.clone(), shared.clone());
    let name = target.display_name().to_string();
    shared.update(|s| s.configuring = Some(name)).await;
    tokio::spawn(async move {
        let result = if target.web.is_some() {
            crate::configure_site(&shared, &target).await
        } else {
            crate::configure(&target, true).await
        };
        let _ = events.send(Event::Configured(target, result)).await;
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
        layout: UNSUPPORTED.into(),
        name,
        source: "default".into(),
        reviewed: true,
        keys: vec![LayoutKey { key: CONFIGURE_SLOT, label: "Configure".into(), speak: Some("Configure this app".into()), confirm: false, send: Send::Event(true) }],
    }
}

/// Handles a packet from the board; returns the app to configure when Configure was tapped.
async fn on_board_packet(packet: &Packet, focus: Option<&Focus>, catalog: &Catalog, players: &[NowPlaying],
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
            if let Some(library) = catalog.get(layout) {
                if let Some((library_key, _)) = library.keys.iter().zip(library.slots()).find(|(_, slot)| u64::from(*slot) == key) {
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
