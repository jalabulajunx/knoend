//! The companion service: find the paired board on Wi-Fi, follow the focused app, push layouts,
//! and run the configure agent when Configure is tapped. State and events are shared with the
//! web app; window titles never leave this loop (only the app's name is published).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use knoend_core::catalog::Catalog;
use knoend_core::focus::{Focus, WebPage};
use knoend_core::library::{Action, AgentReply, LibraryKey, MediaCommand};
use knoend_agents::site::Scope;
use knoend_media::NowPlaying;
use std::collections::{HashMap, HashSet};
use knoend_core::layout;
use knoend_core::library::Library;
use knoend_core::protocol::{Grid, LayoutBody, LayoutKey, Packet, Send};
use knoend_transport::pairing::{config_dir, Pairings};
use knoend_transport::{discover, wifi};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, RwLock};

/// Layout id the board shows for an app with no library; its middle key is "Configure".
pub const UNSUPPORTED: &str = "system.unsupported";
/// Pages of apps and sites the launcher offers.
const LAUNCHER_PAGES: usize = 3;

#[derive(Debug, Clone, Default, Serialize)]
pub struct BoardStatus {
    pub id: String,
    pub profile: String,
    pub firmware: String,
    pub address: String,
    /// The largest grid the board draws.
    pub grid: Grid,
    /// What the identity says about its look (`board.display`); None on firmware without
    /// `knoend.display`.
    pub display: Option<Value>,
    /// The fonts it holds (`board.fonts`: capacity, and per slot name and CRC-32); None on firmware
    /// without fonts.
    pub fonts: Option<Value>,
    /// Why the board last started (identity `lastStart`): "crash", "watchdog" and "brownout" mean it
    /// stopped on its own.
    pub last_start: Option<String>,
}

/// Which page of the layout on the board is shown: the companion turns pages (spec/protocol.md
/// 3.4a) and drops taps from a page that is no longer shown.
struct Paging {
    /// The largest grid the board draws (edit mode shows layouts at this size).
    board: Grid,
    /// The largest grid layouts are shown in: the person's key size, within `board` (display.rs).
    max: Grid,
    /// The layout whose pages these are, the page shown (0-based), how many it has, and the grid of
    /// the page shown.
    layout: String,
    page: usize,
    pages: usize,
    grid: Grid,
}

impl Paging {
    fn new(board: Grid, key_size: crate::display::KeySize) -> Paging {
        Paging { board, max: crate::display::shown_grid(board, key_size), layout: String::new(), page: 0, pages: 1, grid: Grid::DEFAULT }
    }

    /// Shows `layout`, which has `pages` pages: another layout starts on its first page. Returns
    /// the page to show.
    fn enter(&mut self, layout: &str, pages: usize) -> usize {
        if self.layout != layout {
            self.layout = layout.to_string();
            self.page = 0;
        }
        self.pages = pages.max(1);
        self.page = self.page.min(self.pages - 1);
        self.page
    }

    /// The next (or with `prev`, the previous) page, wrapping round. False when there is one page.
    fn turn(&mut self, to: &str) -> bool {
        if self.pages < 2 {
            return false;
        }
        self.page = if to == "prev" { (self.page + self.pages - 1) % self.pages } else { (self.page + 1) % self.pages };
        true
    }

    /// Whether a tap was on a page of this layout other than the one shown (it crossed a page turn).
    fn stale(&self, packet: &Packet) -> bool {
        packet.body["layout"] == self.layout.as_str() && packet.body["page"].as_u64().unwrap_or(1) != self.page as u64 + 1
    }
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
    typist: Arc<std::sync::Mutex<Option<knoend_input::Typist>>>,
    /// The browser extension link (`spec/browser.md`).
    pub browser: Arc<crate::browser::Browser>,
    /// Media players (MPRIS on Linux); None where there are none to read.
    pub media: Option<Arc<knoend_media::Players>>,
    /// The service loop's inbox, so background jobs can put a line on the board's status bar.
    inbox: std::sync::OnceLock<mpsc::Sender<Event>>,
    /// Edit mode (#29): the library the web app is editing, shown on the board instead of focus.
    pub edit: std::sync::Mutex<Option<crate::edit::Edit>>,
    /// Open web app event streams; when the last one closes (its tab closed), edit mode ends.
    viewers: std::sync::atomic::AtomicUsize,
}

impl Shared {
    pub fn new(firmware_source: String) -> Arc<Shared> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Shared { status: RwLock::new(Status::default()), events, serial: Arc::new(tokio::sync::Mutex::new(())), firmware_source,
                          typist: Arc::new(std::sync::Mutex::new(None)),
                          browser: Arc::new(crate::browser::Browser::default()),
                          media: knoend_media::Players::connect().ok().map(Arc::new),
                          inbox: std::sync::OnceLock::new(), edit: std::sync::Mutex::new(None),
                          viewers: std::sync::atomic::AtomicUsize::new(0) })
    }

    /// Publishes an event to the web app and prints it for the terminal.
    pub fn emit(&self, kind: &str, data: Value) {
        if let Some(text) = data.get("text").and_then(Value::as_str) {
            say!("{text}");
        }
        let _ = self.events.send(json!({"kind": kind, "data": data}));
    }

    /// Shows `text` on the board's status bar (highlighted), from any task.
    pub fn board_status(&self, text: String) {
        if let Some(inbox) = self.inbox.get() {
            let _ = inbox.try_send(Event::Status(text));
        }
    }

    /// Sends an edit-mode command to the service loop and waits for its answer.
    pub async fn edit(&self, command: crate::edit::Command) -> Result<Value, String> {
        let inbox = self.inbox.get().ok_or("the service is still starting")?;
        let (answer, reply) = tokio::sync::oneshot::channel();
        inbox.send(Event::Edit(command, Some(answer))).await.map_err(|_| "the service stopped".to_string())?;
        reply.await.map_err(|_| "no answer from the service".to_string())?
    }

    /// Tells the service loop the key size changed (it is saved already); the next connection
    /// reads it from the file anyway.
    pub fn look_changed(&self, display: crate::display::Display) {
        if let Some(inbox) = self.inbox.get() {
            let _ = inbox.try_send(Event::Look(display));
        }
    }

    pub fn key_size_changed(&self, key_size: crate::display::KeySize) {
        if let Some(inbox) = self.inbox.get() {
            let _ = inbox.try_send(Event::KeySize(key_size));
        }
    }

    pub fn editing(&self) -> bool {
        self.edit.lock().is_ok_and(|e| e.is_some())
    }

    pub fn viewer_joined(&self) {
        self.viewers.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// A web app tab closed its event stream; the last one to go ends edit mode.
    pub fn viewer_left(&self) {
        if self.viewers.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 && self.editing() {
            if let Some(inbox) = self.inbox.get() {
                let _ = inbox.try_send(Event::Edit(crate::edit::Command::Stop, None));
            }
        }
    }

    /// Asks the browser to let the board act on the active site (the extension opens a page with
    /// an Allow button; browsers only grant on a click). Waits up to two minutes for the answer.
    pub async fn ask_site_permission(&self, site: &str) -> Result<(), String> {
        self.board_status(format!("Allow {site} in your browser (KnoEnd tab)"));
        let answer = self.browser.request(json!({"type": "grant"}), Duration::from_secs(125)).await?;
        if answer["ok"] == true {
            Ok(())
        } else {
            self.board_status(format!("{site} not allowed: the board cannot act there"));
            Err(format!("{site} was not allowed in the browser"))
        }
    }

    /// The last lines the service said (journal.rs).
    pub fn recent(&self) -> Vec<String> {
        crate::journal::recent(60)
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

/// A folder of recipes to use instead of the compiled-in ones (`KNOEND_BUILTIN`, or
/// `OPENPAGER_BUILTIN` from before the rename).
fn builtin_override() -> Option<std::ffi::OsString> {
    std::env::var_os("KNOEND_BUILTIN").or_else(|| std::env::var_os("OPENPAGER_BUILTIN"))
}

/// Built-in libraries are compiled in; `KNOEND_BUILTIN` (a folder) overrides them, for
/// working on libraries without rebuilding.
fn load_builtin(catalog: &mut Catalog) -> Vec<String> {
    let mut problems = Vec::new();
    if builtin_override().is_some() {
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
    /// Turn the page of the layout on the board: "next" or "prev" (a swipe or tap on the board).
    Page(String),
    /// A desktop notification from an app the board follows (Claude, Codex, ...).
    Notified(knoend_desktop::Notification),
    /// A coding agent's hook: a permission request or a notification.
    Agent(crate::agent_hooks::AgentEvent),
    /// A site's icon, fetched and shrunk to 32x32 RGBA.
    Favicon(String, Vec<u8>),
    Board(Packet),
    LinkDown(String),
    Configured(Focus, Result<(PathBuf, Library), String>),
    /// The person chose another theme or turn in the web app (already saved).
    Look(crate::display::Display),
    /// A font fetched for a slot ("keys", "start"): its family (empty: back to the built-in font)
    /// and the TrueType bytes.
    Font(String, String, Result<Vec<u8>, String>),
    /// The person chose another key size in the web app (already saved).
    KeySize(crate::display::KeySize),
    /// From the web app's editor, with where to send the answer.
    Edit(crate::edit::Command, Option<tokio::sync::oneshot::Sender<Result<Value, String>>>),
}

pub async fn run(shared: Arc<Shared>, extra_libraries: Vec<PathBuf>) {
    // KNOEND_BUILTIN is a recipes tree (apps/, sites/<host>/, agents/); the person's own
    // folder is read flat, since its ai/ subfolder is listed on its own.
    let mut dirs: Vec<PathBuf> = builtin_override().map(|d| Catalog::folders(Path::new(&d))).unwrap_or_default();
    dirs.extend([config_dir().join("libraries"), ai_libraries()]);
    dirs.extend(extra_libraries);
    let (mut catalog, rejected) = Catalog::load(&dirs);
    for problem in load_builtin(&mut catalog) {
        say!("  skipped {problem}");
    }
    say!("Libraries: {} loaded from {:?}", catalog.len(), dirs);
    for r in &rejected {
        say!("  skipped {}: {}", r.path.display(), r.reason);
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
    // Claude notifications that no local hook explains (a cloud session, which runs its hooks on
    // Anthropic's machines): the board cannot answer those, so it offers a key to show Claude.
    let mut last_hook: Option<std::time::Instant> = None;
    let mut unexplained: Option<(knoend_desktop::Notification, std::time::Instant)> = None;
    let mut calling: Option<Calling> = None;

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
    // A paired board missing from the network for minutes is a problem worth raising (watch.rs).
    let mut missing_since: Option<tokio::time::Instant> = None;
    loop {
        let pairings = Pairings::load();
        if pairings.boards.is_empty() {
            shared.update(|s| { s.link = "no-pairing".into(); s.board = None; }).await;
            // Keep following focus while waiting, so the dashboard shows it.
            match tokio::time::timeout(Duration::from_secs(2), inbox.recv()).await {
                Ok(Some(Event::Focus(Some(f)))) => {
                    tab = shared.browser.focus_changed(&f.app, &f.instance);
                    focus = Some(f);
                }
                Ok(Some(Event::Tab(t))) => tab = t,
                Ok(Some(Event::Players(p))) => players = p,
                Ok(Some(Event::Edit(command, answer))) => edit_while_away(command, answer, &shared),
                _ => {}
            }
            if let Some(f) = effective(&focus, &tab) {
                shared.update(|s| s.app = Some(f.display_name().to_string())).await;
            }
            continue;
        }
        shared.update(|s| s.link = "searching".into()).await;
        let (mut board, mut sender) = match connect(&pairings, events.clone()).await {
            Ok(link) => link,
            Err(e) => {
                say!("{e}; retrying in 5 s");
                // Not while installing or pairing over USB: the board is restarting on purpose then.
                if shared.serial.try_lock().is_err() {
                    missing_since = None;
                }
                let since = *missing_since.get_or_insert_with(tokio::time::Instant::now);
                if since.elapsed() >= Duration::from_secs(120) {
                    problem!("board-missing", "The paired board has not been found on the network for {} minutes ({e})",
                             since.elapsed().as_secs() / 60);
                    missing_since = Some(tokio::time::Instant::now()); // again in two minutes if still missing
                }
                drain_focus(&mut inbox, &mut focus, &mut tab, &mut players, &shared).await;
                continue;
            }
        };
        missing_since = None;
        shared.emit("log", json!({"text": format!("Connected to board {} over Wi-Fi", board.id)}));
        let board_status = board.clone();
        shared.update(|s| { s.link = "connected".into(); s.board = Some(board_status); }).await;
        let mut shown: Option<String> = None;
        let mut sent_images: HashSet<String> = HashSet::new();
        if let Some(reason) = board.last_start.as_deref().filter(|r| ["crash", "watchdog", "brownout"].contains(r)) {
            problem!("board-crash", "The board started again after a {reason} (it stopped on its own; firmware {})", board.firmware);
        }
        let wanted = crate::display::load();
        let mut paging = Paging::new(board.grid, wanted.key_size);
        set_picture_theme(board.display.as_ref().and_then(|d| d["theme"].as_str()).unwrap_or("dark"));
        let mut asked_look = send_look(&mut sender, &board, &wanted).await;
        fetch_fonts(&board, &wanted, &events, false);
        if focus.is_none() {
            show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut paging, &mut shown, calling.as_ref(), &shared).await;
        }
        if let Some(f) = effective(&focus, &tab) {
            show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await;
        }

        // A board that restarts never closes its old connection, so silence is the only sign:
        // ping every 10 s and treat 25 s without any packet as a lost link.
        let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
        heartbeat.tick().await;
        let mut second = tokio::time::interval(Duration::from_secs(1));
        if let Some(a) = approvals.front() {
            show_approval(&mut sender, a, &catalog, &mut shown, &shared).await;
        }
        let mut last_heard = tokio::time::Instant::now();

        loop {
            let event = tokio::select! {
                event = inbox.recv() => match event { Some(e) => e, None => return },
                _ = second.tick() => {
                    // A Claude notification with no hook event near it: offer "Go to Claude".
                    let decided = unexplained.as_ref().is_some_and(|(_, at)| at.elapsed() >= HOOK_GRACE);
                    let expired = calling.as_ref().is_some_and(|c| c.since.elapsed() >= CALLING_FOR);
                    if decided || expired {
                        if expired {
                            calling = None;
                        }
                        if let Some((n, at)) = unexplained.take().filter(|_| decided) {
                            let local = last_hook.is_some_and(|h| h.max(at) - h.min(at) <= HOOK_GRACE * 3);
                            let in_front = effective(&focus, &tab).is_some_and(|f| Calling::matches(&f, &n));
                            if !local && !in_front && approvals.is_empty() {
                                say!("  {} notification with no local session behind it (cloud?): Go to key on the board", n.app);
                                calling = Some(Calling::new(&n, &catalog));
                            }
                        }
                        if approvals.is_empty() {
                            match effective(&focus, &tab) {
                                Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await,
                                None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut paging,
                                                      &mut shown, calling.as_ref(), &shared).await,
                            }
                        }
                    }
                    // A request answered elsewhere (the agent's own prompt) leaves the board.
                    let before = approvals.len();
                    approvals.retain(|a| !a.answer.is_closed());
                    if approvals.len() != before {
                        say!("  permission request answered elsewhere; withdrawn from the board");
                        shown = None;
                        match approvals.front() {
                            Some(a) => show_approval(&mut sender, a, &catalog, &mut shown, &shared).await,
                            None => match effective(&focus, &tab) {
                                Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await,
                                None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut paging,
                                                      &mut shown, calling.as_ref(), &shared).await,
                            },
                        }
                    }
                    continue;
                }
                _ = heartbeat.tick() => {
                    if last_heard.elapsed() > Duration::from_secs(25) {
                        Event::LinkDown("no reply from the board for 25 s".into())
                    } else if sender.send("knoend.ping", json!({})).await.is_err() {
                        Event::LinkDown("could not send to the board".into())
                    } else {
                        continue;
                    }
                }
            };
            if matches!(event, Event::Board(_)) {
                last_heard = tokio::time::Instant::now();
            }
            if matches!(event, Event::Agent(_)) {
                last_hook = Some(std::time::Instant::now());
            }
            // A swipe on the board turns the page it was made on; a late one for another layout does nothing.
            let event = match event {
                Event::Board(p) if p.kind == "knoend.page" => {
                    if p.body["layout"] != paging.layout.as_str() {
                        continue;
                    }
                    Event::Page(p.body["to"].as_str().unwrap_or("next").to_string())
                }
                event => event,
            };
            match event {
                Event::Focus(Some(f)) => {
                    tab = shared.browser.focus_changed(&f.app, &f.instance);
                    focus = Some(f);
                    let f = effective(&focus, &tab).expect("focus");
                    if calling.as_ref().is_some_and(|c| c.is(&f)) {
                        calling = None;
                    }
                    usage.focused(crate::launcher::usage_key(&f, &apps));
                    say!("Focus: {} ({})", f.display_name(), f.instance);
                    let name = f.display_name().to_string();
                    shared.update(|s| s.app = Some(name)).await;
                    // Questions waiting while the person goes to the app the agent runs in: they are
                    // most likely answering there, so the board says so (and back when they leave).
                    if let Some(front) = approvals.front_mut().filter(|a| a.questions.len() > 1 && !a.here) {
                        let elsewhere = hosts_agent(&f);
                        if elsewhere != front.elsewhere {
                            front.elsewhere = elsewhere;
                            shown = None;
                            if elsewhere {
                                show_answering(&mut sender, front, &f, &mut shown, &shared).await;
                            } else {
                                show_approval(&mut sender, front, &catalog, &mut shown, &shared).await;
                            }
                        }
                    }
                    show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await;
                    draft_new_page(&catalog, &f, &drafted, &mut attempted, &events, &shared).await;
                }
                Event::Focus(None) => {
                    // Nothing in front: the launcher.
                    say!("Focus: nothing (launcher)");
                    tab = shared.browser.focus_changed("", "");
                    focus = None;
                    usage.focused(None);
                    shared.update(|s| s.app = None).await;
                    if approvals.is_empty() {
                        show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut paging, &mut shown, calling.as_ref(), &shared).await;
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
                Event::Board(packet) if packet.kind == "knoend.key" && paging.stale(&packet) => {
                    say!("  tap on page {} of {}, which is no longer shown; ignored", packet.body["page"], paging.layout);
                }
                Event::Page(to) => {
                    if approvals.is_empty() && paging.turn(&to) {
                        say!("  page {}/{} of {}", paging.page + 1, paging.pages, paging.layout);
                        match effective(&focus, &tab) {
                            Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await,
                            None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut paging,
                                                  &mut shown, calling.as_ref(), &shared).await,
                        }
                        if shared.editing() {
                            shared.emit("edit", edit_state(&shared, &paging));
                        }
                    }
                }
                Event::Board(packet) if packet.kind == "knoend.key" && packet.body["layout"] != APPROVE && shared.editing() => {
                    // Edit mode: a tap selects the key in the web app; nothing runs.
                    shared.emit("edit.tap", json!({"page": paging.page, "slot": packet.body["key"]}));
                }
                Event::Look(wanted) => {
                    asked_look = send_look(&mut sender, &board, &wanted).await;
                    fetch_fonts(&board, &wanted, &events, true);
                }
                Event::Font(slot, family, font) => match font {
                    Ok(bytes) if family.is_empty() => {
                        say!("Font {slot}: back to the built-in font");
                        let _ = sender.send("knoend.font", json!({"slot": slot, "clear": true})).await;
                        let _ = bytes;
                    }
                    Ok(bytes) => {
                        say!("Font {slot}: sending {family} ({} bytes) to the board", bytes.len());
                        send_font(&mut sender, &slot, &family, &bytes).await;
                    }
                    Err(e) => {
                        raise!("font", "Could not get the font {family}: {e}");
                        shared.emit("display", json!({"text": format!("Could not get the font {family}: {e}")}));
                    }
                },
                Event::Board(packet) if packet.kind == "knoend.font.ack" => {
                    let slot = packet.body["slot"].as_str().unwrap_or("?").to_string();
                    if packet.body["ok"] == true {
                        let name = packet.body["name"].as_str().unwrap_or_default();
                        shared.emit("display", json!({"text": if name.is_empty() { format!("The board uses its built-in font for {slot} again") }
                                                              else { format!("The board now uses {name} for {slot}") }}));
                        if let Some(fonts) = board.fonts.as_mut() {
                            fonts[slot.as_str()] = if name.is_empty() { Value::Null } else { json!({"name": name}) };
                        }
                        let status = board.clone();
                        shared.update(|s| s.board = Some(status)).await;
                        // The keys again, now in the new font.
                        shown = None;
                        if approvals.is_empty() {
                            if let Some(f) = effective(&focus, &tab) {
                                show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await;
                            }
                        }
                    } else {
                        let reason = packet.body["error"].as_str().unwrap_or("it refused it");
                        raise!("font", "The board could not keep the {slot} font {}: {reason}", packet.body["name"].as_str().unwrap_or_default());
                        shared.emit("display", json!({"text": format!("The board could not keep the {slot} font: {reason}")}));
                    }
                }
                Event::Board(packet) if packet.kind == "knoend.error" && asked_look => {
                    // The board cannot show that look (older firmware): the choice goes back to
                    // what it shows, and the web app says why.
                    asked_look = false;
                    let reason = packet.body["message"].as_str().unwrap_or("the board refused it").to_string();
                    problem!("display", "The board refused the display settings: {reason}");
                    let mut saved = crate::display::load();
                    if let Some(shown) = &board.display {
                        saved.theme = crate::display::Theme::named(shown["theme"].as_str().unwrap_or("dark"));
                        saved.turn = shown["turn"].as_u64().and_then(|t| u8::try_from(t).ok()).unwrap_or(0);
                    }
                    let _ = crate::display::save(&saved);
                    shared.emit("display", json!({"text": format!("The board could not show that: {reason}. Its firmware may need updating."),
                                                  "theme": saved.theme, "turn": saved.turn}));
                }
                Event::KeySize(key_size) => {
                    paging.max = crate::display::shown_grid(paging.board, key_size);
                    say!("Key size: {key_size:?}, pages up to {}x{}", paging.max.cols, paging.max.rows);
                    // Shown again at the new size.
                    shown = None;
                    if approvals.is_empty() {
                        match effective(&focus, &tab) {
                            Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await,
                            None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut paging,
                                                  &mut shown, calling.as_ref(), &shared).await,
                        }
                    }
                }
                Event::Edit(command, answer) => {
                    let list = matches!(command, crate::edit::Command::List);
                    let result = edit_command(command, &mut catalog, &mut paging, &shared);
                    if result.is_ok() {
                        let count = catalog.len();
                        shared.update(|s| s.libraries = count).await;
                        // The board shows the edited library, or again what is focused.
                        if approvals.is_empty() {
                            match effective(&focus, &tab) {
                                Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await,
                                None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut paging,
                                                      &mut shown, calling.as_ref(), &shared).await,
                            }
                        }
                    }
                    let result = result.map(|value| value.unwrap_or_else(|| edit_state(&shared, &paging)));
                    // Every tab follows the session, not only the one that asked.
                    if result.is_ok() && !list {
                        shared.emit("edit", edit_state(&shared, &paging));
                    }
                    if let Some(answer) = answer {
                        let _ = answer.send(result);
                    }
                }
                Event::Board(packet) if packet.kind == "knoend.key" && packet.body["layout"] != APPROVE
                                        && calling.as_ref().and_then(|c| c.key(paging.grid)).is_some_and(|k| packet.body["key"] == k.key) => {
                    let c = calling.take().expect("calling");
                    let names = c.names.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        let names: Vec<&str> = names.iter().map(String::as_str).collect();
                        knoend_focus::bring_to_front(&names)
                    }).await.unwrap_or_else(|e| Err(e.to_string()));
                    match result {
                        Ok(()) => say!("  brought {} to the front", c.app),
                        Err(e) => say!("  could not bring {} to the front: {e}", c.app),
                    }
                    match effective(&focus, &tab) {
                        Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await,
                        None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut paging,
                                              &mut shown, calling.as_ref(), &shared).await,
                    }
                }
                Event::Board(packet) if packet.kind == "knoend.key" && packet.body["layout"] == LAUNCHER => {
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
                        show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await;
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
                        say!("Site: {}", f.display_name());
                        usage.focused(crate::launcher::usage_key(&f, &apps));
                        let name = f.display_name().to_string();
                        shared.update(|s| s.app = Some(name)).await;
                        show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await;
                        draft_new_page(&catalog, &f, &drafted, &mut attempted, &events, &shared).await;
                    }
                }
                Event::Board(packet) if packet.kind == "knoend.key" && packet.body["layout"] == ANSWERING => {
                    if let Some(front) = approvals.front_mut().filter(|_| packet.body["key"] == u64::from(ANSWER_HERE_SLOT)) {
                        say!("  answering on the board instead");
                        front.here = true;
                        front.elsewhere = false;
                        shown = None;
                        show_approval(&mut sender, front, &catalog, &mut shown, &shared).await;
                    }
                }
                Event::Board(packet) if packet.kind == "knoend.key" && packet.body["layout"] == APPROVE => {
                    let key = packet.body["key"].as_u64().unwrap_or(0);
                    let decision = match approvals.front_mut() {
                        Some(front) => {
                            let screen = front.screen(&catalog);
                            let tapped = agent_keys(&screen, front).into_iter().find(|k| u64::from(k.slot) == key).map(|k| (k.reply, k.option));
                            match tapped.and_then(|(reply, option)| front.tap(reply, option)) {
                                Some(decision) => Some(decision),
                                None => {
                                    if tapped.is_some() {
                                        // A tick, or the next question.
                                        show_approval(&mut sender, front, &catalog, &mut shown, &shared).await;
                                    }
                                    None
                                }
                            }
                        }
                        None => None,
                    };
                    // Only a tap that answers takes the request off the queue.
                    if let Some((decision, front)) = decision.and_then(|d| approvals.pop_front().map(|f| (d, f))) {
                        say!("  answered on the board: {decision:?} ({})", front.summary);
                        let _ = front.answer.send(decision);
                        shown = None;
                        match approvals.front() {
                            Some(a) => show_approval(&mut sender, a, &catalog, &mut shown, &shared).await,
                            None => match effective(&focus, &tab) {
                                Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await,
                                None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut paging,
                                                      &mut shown, calling.as_ref(), &shared).await,
                            },
                        }
                    }
                }
                Event::Board(packet) if packet.kind == "knoend.display.ack" => {
                    asked_look = false;
                    // A new theme or turn is shown at once: pictures are sent again in the theme's
                    // colours, and the layout in a grid that fits the screen as it now sits.
                    say!("  board display: {}", packet.body);
                    board.display = Some(json!({"theme": packet.body["theme"], "turn": packet.body["turn"]}));
                    if packet.body["grid"].is_object() {
                        board.grid = identity_grid(&json!({"board": {"grid": packet.body["grid"]}}));
                    }
                    paging.board = board.grid;
                    paging.max = crate::display::shown_grid(board.grid, crate::display::load().key_size);
                    set_picture_theme(packet.body["theme"].as_str().unwrap_or("dark"));
                    let status = board.clone();
                    shared.update(|s| s.board = Some(status)).await;
                    sent_images.clear();
                    shown = None;
                    if approvals.is_empty() {
                        match effective(&focus, &tab) {
                            Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await,
                            None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut paging,
                                                  &mut shown, calling.as_ref(), &shared).await,
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
                    say!("Permission request: {summary}");
                    approvals.push_back(Approval { request: request.clone(), summary, detail: crate::agent_hooks::detail(&request), destructive: crate::agent_hooks::destructive(&request),
                                                   can_remember: request["permission_suggestions"].as_array().is_some_and(|s| !s.is_empty()),
                                                   questions: crate::agent_hooks::questions(&request), step: 0, answers: Vec::new(), ticked: Vec::new(), elsewhere: false, here: false,
                                                   answer });
                    if approvals.len() == 1 {
                        // Several questions while the person is in the app the agent runs in: they
                        // will most likely answer there, and the board cannot follow each answer.
                        let front = approvals.front_mut().unwrap();
                        match effective(&focus, &tab).filter(|f| front.questions.len() > 1 && hosts_agent(f)) {
                            Some(f) => {
                                front.elsewhere = true;
                                show_answering(&mut sender, front, &f, &mut shown, &shared).await;
                            }
                            None => show_approval(&mut sender, front, &catalog, &mut shown, &shared).await,
                        }
                    }
                }
                Event::Agent(crate::agent_hooks::AgentEvent::Notification(n)) => {
                    // Answered in Claude's own window? Then the request leaves the board.
                    if n["hook_event_name"] == "PostToolUse" && n["tool_name"] == "AskUserQuestion" {
                        let fields: Vec<&String> = n["tool_input"].as_object().map(|o| o.keys().collect()).unwrap_or_default();
                        say!("Agent: AskUserQuestion finished (input fields: {fields:?})");
                    }
                    let before = approvals.len();
                    approvals.retain(|a| !crate::agent_hooks::settles(&n, &a.request));
                    if approvals.len() != before {
                        say!("  permission request settled in the app; withdrawn from the board");
                        shown = None;
                        match approvals.front() {
                            Some(a) => show_approval(&mut sender, a, &catalog, &mut shown, &shared).await,
                            None => match effective(&focus, &tab) {
                                Some(f) => show(&mut sender, &catalog, &f, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await,
                                None => show_launcher(&mut sender, &usage, &apps, &favicons, &mut sent_images, &mut launcher_targets, &mut paging,
                                                      &mut shown, calling.as_ref(), &shared).await,
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
                        say!("Agent notification ({kind}): {message}");
                        let text = board_text(&format!("Claude: {message}"), 80);
                        if approvals.is_empty() {
                            let _ = sender.send("knoend.status", json!({"text": text, "level": "alert", "ttl": 60})).await;
                        }
                    }
                }
                Event::Notified(n) => {
                    say!("Notification from {}: {}", n.app, n.summary);
                    if n.from_any(&["claude"]) {
                        unexplained = Some((n.clone(), std::time::Instant::now()));
                    }
                    let text = board_text(&format!("{}: {}", n.app, if n.summary.is_empty() { &n.body } else { &n.summary }), 80);
                    // A pending permission request keeps the status line (it is what the notification is about).
                    if approvals.is_empty() {
                        let _ = sender.send("knoend.status", json!({"text": text, "level": "alert", "ttl": 60})).await;
                    }
                    shared.emit("notification", json!({"app": n.app, "summary": n.summary}));
                }
                Event::Status(text) => {
                    let _ = sender.send("knoend.status", json!({"text": board_text(&text, 80), "level": "attention", "ttl": 20})).await;
                }
                Event::ConfigureSite => match effective(&focus, &tab).filter(|f| f.web.is_some()) {
                    Some(target) => {
                        let text = format!("Configuring {}: reading the page...", board_text(target.display_name(), 40));
                        let _ = sender.send("knoend.status", json!({"text": text, "level": "attention"})).await;
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
                            let page = library.matches.web.iter().any(|p| p.contains('/'));
                            if let Some(web) = &target.web {
                                drafted.add(&web.host, &web.page_kind());
                            }
                            shared.emit("configured", json!({"text": format!("  saved {} -> {}", library.pack.id, path.display()),
                                                             "app": target.display_name(), "library": library.pack.id}));
                            catalog.add(path, library);
                            let count = catalog.len();
                            shared.update(|s| s.libraries = count).await;
                            if let Some(web) = &target.web {
                                // Say what the keys cover: the whole site, or one kind of page (whose
                                // keys a second "Make keys" there replaces; the site's own keys stay).
                                let what = if page { format!("{} pages like {}", web.host, web.page_kind()) } else { web.host.clone() };
                                shared.browser.notify(json!({"type": "configure-status", "ok": true,
                                    "text": format!("Made {keys} keys for {what}{}. Look at the board.",
                                                    if page { " (the rest of the site keeps its keys)" } else { "" })}));
                            }
                            let same_site = |f: &Focus| f.web.as_ref().map(|w| &w.host) == target.web.as_ref().map(|w| &w.host);
                            if effective(&focus, &tab).is_some_and(|f| f.app == target.app && f.instance == target.instance && same_site(&f)) {
                                shown = None;
                                show(&mut sender, &catalog, &target, &players, &mut shown, &mut shown_actions, &mut paging, calling.as_ref(), &shared).await;
                            }
                        }
                        Err(error) => {
                            if target.web.is_some() {
                                shared.browser.notify(json!({"type": "configure-status", "ok": false, "text": format!("Could not make keys: {error}")}));
                            }
                            raise!("configure", "Drafting keys failed for {}: {error}", target.display_name());
                            shared.emit("configure-failed", json!({"text": format!("  configure failed for {}: {error}", target.display_name()),
                                                                   "app": target.display_name(), "error": error}));
                            let mut text = format!("Could not configure {}: {error}", target.display_name());
                            text.truncate(80);
                            let _ = sender.send("knoend.status", json!({"text": text, "level": "alert", "ttl": 15})).await;
                        }
                    }
                }
                Event::LinkDown(reason) => {
                    // Not while the board is being installed or paired over USB: it restarts then.
                    if !board_restart_expected(&shared) {
                        raise!("link-lost", "Link to board {} lost: {reason}", board.id);
                    }
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
            Event::Focus(Some(f)) => {
                *tab = shared.browser.focus_changed(&f.app, &f.instance);
                *focus = Some(f);
            }
            Event::Tab(t) => *tab = t,
            Event::Players(p) => { *players = p; continue; }
            Event::Edit(command, answer) => { edit_while_away(command, answer, shared); continue; }
            _ => continue,
        }
        if let Some(f) = effective(focus, tab) {
            let name = f.display_name().to_string();
            shared.update(|s| s.app = Some(name)).await;
        }
    }
}

fn spawn_focus_watcher(events: mpsc::Sender<Event>, shared: Arc<Shared>) {
    #[cfg(target_os = "linux")]
    if let Some(note) = crate::diagnostics::desktop_session()["note"].as_str().filter(|n| !n.is_empty()) {
        say!("{note}");
        shared.emit("log", json!({"text": note}));
    }
    std::thread::spawn(move || {
        let watcher = match knoend_focus::Watcher::connect() {
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
    let (sender, mut receiver) = wifi::connect((board.address, board.port).into(), &board.device_id, &pairings.companion, key, board.old_names)
        .await.map_err(|e| e.to_string())?;

    let identity = receiver.recv().await.map_err(|e| e.to_string())?;
    let status = BoardStatus {
        id: board.device_id.clone(),
        profile: identity.body["board"]["profile"].as_str().unwrap_or("?").to_string(),
        firmware: identity.body["firmware"].as_str().unwrap_or("?").to_string(),
        address: board.address.to_string(),
        grid: identity_grid(&identity.body),
        display: identity.body["board"].get("display").cloned(),
        fonts: identity.body["board"].get("fonts").cloned(),
        last_start: identity.body["lastStart"].as_str().map(str::to_string),
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

/// The largest grid a board says it draws (`board.grid` in its identity), 3x3 if it says nothing usable.
fn identity_grid(identity: &Value) -> Grid {
    let grid = &identity["board"]["grid"];
    let part = |name: &str| grid[name].as_u64().and_then(|n| u8::try_from(n).ok()).filter(|n| (1..=4).contains(n));
    match (part("cols"), part("rows")) {
        (Some(cols), Some(rows)) => Grid { cols, rows },
        _ => Grid::DEFAULT,
    }
}

/// Pushes the layout for `focus`, or the "unsupported app" screen. When the focused app has a
/// media player, its play/pause key and the status bar follow what it plays. Sends nothing when the
/// board already shows exactly this.
#[allow(clippy::too_many_arguments)]
async fn show(sender: &mut wifi::WifiSender, catalog: &Catalog, focus: &Focus, players: &[NowPlaying],
              shown: &mut Option<String>, shown_actions: &mut HashMap<u8, KeyRef>, paging: &mut Paging, calling: Option<&Calling>, shared: &Shared) {
    if shown.as_deref().is_some_and(|s| s.starts_with(APPROVE_SIGNATURE)) {
        return; // a permission request is on the board; it stays until answered
    }
    if show_edit(sender, paging, shown, shared).await {
        shown_actions.clear();
        return;
    }
    let player = knoend_media::for_focus(players, &focus.app, &focus.instance);
    let mut actions: HashMap<u8, KeyRef> = HashMap::new();
    let found = catalog.layout_for(focus).map(|library| {
        let pages = layout::packed(&library, paging.max);
        let page = paging.enter(&library.pack.id, pages.len());
        let built = layout::build(&library, focus.os, &pages, page);
        (library, built, pages.into_iter().nth(page).map(|p| p.keys).unwrap_or_default())
    });
    let (mut body, mut status) = match found {
        Some((library, built, placed)) => match built {
            Ok(mut body) => {
                for &(i, slot) in &placed {
                    let key = &library.keys[i];
                    actions.insert(slot, KeyRef { label: key.label.clone(), action: key.action.clone() });
                }
                if let Some(player) = player {
                    for &(i, slot) in &placed {
                        if let Action::Media { command: MediaCommand::Toggle, .. } = library.keys[i].action {
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
                say!("  library {} cannot be built: {e}", library.pack.id);
                return;
            }
        },
        None => (unsupported_layout(focus, paging), json!({"text": if focus.web.is_some() { format!("No keys for {} yet", board_text(focus.display_name(), 50)) }
                                         else { format!("Unsupported app: {}", board_text(focus.display_name(), 50)) }, "level": "attention"})),
    };
    if let (Some(player), true) = (player, status["text"] == "") {
        let what = [board_text(&player.title, 50), board_text(&player.artist, 24)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" - ");
        if !what.is_empty() {
            status = json!({"text": format!("{}: {what}", if player.playing() { "Playing" } else { "Paused" }), "level": "info"});
        }
    }
    let grid = body.grid.unwrap_or_default();
    paging.grid = grid;
    // While the focused app plays something, the bottom row controls it (as KDE Connect's media
    // controls do), unless the layout has media keys of its own. Not on grids of one or two rows:
    // the bottom row would be half the keys or more.
    if let Some(player) = player.filter(|p| (p.playing() || (p.status == "Paused" && !p.title.is_empty())) && grid.rows >= 3) {
        if !actions.values().any(|k| matches!(k.action, Action::Media { .. })) {
            let row = [(player.can_go_previous, "Prev", "Previous track", "skip_previous", MediaCommand::Previous),
                       (true, if player.playing() { "Pause" } else { "Play" }, "Play or pause",
                        if player.playing() { "pause" } else { "play_arrow" }, MediaCommand::Toggle),
                       (player.can_go_next, "Next", "Next track", "skip_next", MediaCommand::Next)];
            for ((offered, label, speak, icon, command), slot) in row.into_iter().zip(media_slots(grid)) {
                let Some(slot) = slot.filter(|_| offered) else { continue };
                body.keys.retain(|k| k.key != slot);
                body.keys.push(LayoutKey { key: slot, label: label.into(), speak: Some(speak.into()), icon: Some(icon.into()), confirm: false, send: Send::Event(true) });
                actions.insert(slot, KeyRef { label: label.into(), action: Action::Media { command, selector: None } });
            }
            body.keys.sort_by_key(|k| k.key);
        }
    }
    if let Some(calling) = calling {
        match calling.key(grid) {
            Some(key) => {
                body.keys.retain(|k| k.key != key.key);
                actions.remove(&key.key);
                body.keys.push(key);
                body.keys.sort_by_key(|k| k.key);
            }
            None => status = json!({"text": calling.status(), "level": "attention"}),
        }
    }
    body.name = board_text(&body.name, 128);
    fit_packet(&mut body);
    *shown_actions = actions;
    let signature = format!("{}|{}", serde_json::to_string(&body).unwrap_or_default(), status);
    if shown.as_deref() == Some(signature.as_str()) {
        return;
    }
    let id = body.layout.clone();
    say!("  -> {} ({}){} keys: {}", body.name, id, status["text"].as_str().filter(|t| !t.is_empty()).map(|t| format!(" [{t}]")).unwrap_or_default(),
             body.keys.iter().map(|k| format!("{}={}", k.key, k.label)).collect::<Vec<_>>().join(", "));
    let summary = json!({"id": id, "name": body.name, "source": body.source, "reviewed": body.reviewed, "grid": grid, "page": body.page, "pages": body.pages,
                         "keys": body.keys.iter().map(|k| json!({"key": k.key, "label": k.label})).collect::<Vec<_>>()});
    let body = serde_json::to_value(&body).expect("serialisable");
    if sender.send("knoend.layout", body).await.is_ok() {
        let _ = sender.send("knoend.status", status).await;
        *shown = Some(signature);
        shared.update(|s| s.layout = Some(summary)).await;
    }
}

/// Where the media row goes on `grid`'s bottom row: Prev, Play/Pause, Next with three columns or
/// more; Play/Pause and Next with two; Play/Pause alone with one.
fn media_slots(grid: Grid) -> [Option<u8>; 3] {
    let row = grid.bottom_row();
    let (first, last) = (row[0], row[row.len() - 1]);
    match row.len() {
        1 => [None, Some(first), None],
        2 => [None, Some(first), Some(last)],
        n => [Some(first), Some(row[(n - 1) / 2]), Some(last)],
    }
}

/// Packets are at most 4096 bytes (spec/protocol.md); sixteen keys with long spoken names can pass
/// that. The board does not use `speak`, so it goes first.
fn fit_packet(body: &mut LayoutBody) {
    if serde_json::to_string(body).map_or(0, |s| s.len()) > 3800 {
        for key in &mut body.keys {
            key.speak = None;
        }
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
        // The site's front page ("/") is the site: its keys are the site-wide ones.
        Some(site) if web.page_kind() != "/" => Scope::Page { kind: web.page_kind(), site_keys: site.keys.iter().map(|k| k.label.clone()).collect() },
        _ => Scope::Site,
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
    if site.pack.source != knoend_core::library::Source::Ai || catalog.find(focus).map(|l| &l.pack.id) != Some(&site.pack.id) {
        return;
    }
    let kind = web.page_kind();
    // The front page uses the site-wide keys; drafting it as a "page" would replace them.
    if kind == "/" || drafted.has(&web.host, &kind) || !attempted.insert((web.host.clone(), kind.clone())) {
        return;
    }
    if shared.status.read().await.configuring.is_some() {
        attempted.remove(&(web.host.clone(), kind));
        return;
    }
    say!("  new kind of page on {}: {kind}; drafting its keys", web.host);
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
                       targets: &mut HashMap<u8, crate::launcher::Target>, paging: &mut Paging, shown: &mut Option<String>,
                       calling: Option<&Calling>, shared: &Shared) {
    targets.clear();
    if show_edit(sender, paging, shown, shared).await {
        return;
    }
    // Pages at the person's key size (3x3 unless they chose otherwise).
    let grid = paging.max;
    let per_page = usize::from(grid.size());
    let entries = usage.top(per_page * LAUNCHER_PAGES);
    let pages = entries.len().div_ceil(per_page);
    let page = paging.enter(LAUNCHER, pages);
    paging.grid = grid;
    let mut keys = Vec::new();
    for (i, entry) in entries.into_iter().skip(page * per_page).take(per_page).enumerate() {
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
    if let Some(key) = calling.and_then(|c| c.key(grid)) {
        keys.retain(|k| k.key != key.key);
        targets.remove(&key.key);
        keys.push(key);
    }
    let status = match calling {
        Some(c) if c.key(grid).is_none() => c.status(),
        _ if keys.is_empty() => "Nothing open. Apps and sites you use will appear here.".into(),
        _ => String::new(),
    };
    let paged = pages > 1;
    let mut body = LayoutBody { layout: LAUNCHER.into(), name: "Launcher".into(), source: "default".into(), reviewed: true, keys, grid: Some(grid),
                                page: paged.then(|| page as u8 + 1), pages: paged.then(|| pages as u8), ..LayoutBody::default() };
    fit_packet(&mut body);
    let signature = format!("launcher|{}|{status}", serde_json::to_string(&body).unwrap_or_default());
    if shown.as_deref() == Some(signature.as_str()) {
        return;
    }
    say!("  -> launcher: {}", body.keys.iter().map(|k| k.label.as_str()).collect::<Vec<_>>().join(", "));
    if sender.send("knoend.layout", serde_json::to_value(&body).expect("serialisable")).await.is_ok() {
        let _ = sender.send("knoend.status", json!({"text": status})).await;
        *shown = Some(signature);
        shared.update(|s| s.layout = Some(json!({"id": LAUNCHER, "name": "Launcher"}))).await;
    }
}

/// Edit mode: the board shows the library being edited, whatever is focused. Every key only
/// reports its tap (it selects the key in the web app; nothing is typed or asked twice). False when
/// not editing.
async fn show_edit(sender: &mut wifi::WifiSender, paging: &mut Paging, shown: &mut Option<String>, shared: &Shared) -> bool {
    let Some((library, dirty)) = shared.edit.lock().ok().and_then(|e| e.as_ref().map(|e| (e.library.clone(), e.dirty()))) else {
        return false;
    };
    let pages = layout::pages(&library, paging.board);
    let page = paging.enter(&library.pack.id, pages.len());
    let mut body = match layout::build(&library, knoend_core::focus::Os::current(), &pages, page) {
        Ok(body) => body,
        Err(e) => {
            say!("  edited library {} cannot be shown: {e}", library.pack.id);
            return true;
        }
    };
    for key in &mut body.keys {
        key.send = Send::Event(true);
        key.confirm = false;
    }
    paging.grid = body.grid.unwrap_or_default();
    let status = json!({"text": board_text(&format!("Editing {}{}", library.pack.name, if dirty { " (not saved)" } else { "" }), 80),
                        "level": "attention"});
    fit_packet(&mut body);
    let signature = format!("edit|{}|{}", serde_json::to_string(&body).unwrap_or_default(), status);
    if shown.as_deref() == Some(signature.as_str()) {
        return true;
    }
    say!("  -> editing {} page {}: {}", library.pack.id, page + 1,
             body.keys.iter().map(|k| format!("{}={}", k.key, k.label.replace('\n', "/"))).collect::<Vec<_>>().join(", "));
    let summary = json!({"id": body.layout, "name": body.name, "source": body.source, "reviewed": body.reviewed, "grid": body.grid,
                         "page": body.page, "pages": body.pages, "editing": true,
                         "keys": body.keys.iter().map(|k| json!({"key": k.key, "label": k.label})).collect::<Vec<_>>()});
    let body = serde_json::to_value(&body).expect("serialisable");
    if sender.send("knoend.layout", body).await.is_ok() {
        let _ = sender.send("knoend.status", status).await;
        *shown = Some(signature);
        shared.update(|s| s.layout = Some(summary)).await;
    }
    true
}

/// Edit mode shows changes on the board; while it is away, Stop still ends it and everything else
/// says why it cannot be done.
fn edit_while_away(command: crate::edit::Command, answer: Option<tokio::sync::oneshot::Sender<Result<Value, String>>>, shared: &Shared) {
    let result = match command {
        crate::edit::Command::Stop => {
            if let Ok(mut session) = shared.edit.lock() {
                *session = None;
            }
            shared.emit("edit", json!({"editing": false}));
            Ok(json!({"editing": false}))
        }
        _ => Err("the board is not connected; edit mode needs it to show your changes".to_string()),
    };
    if let Some(answer) = answer {
        let _ = answer.send(result);
    }
}

/// The edit session as the web app shows it, or `{"editing": false}`.
fn edit_state(shared: &Shared, paging: &Paging) -> Value {
    match shared.edit.lock().ok().as_ref().and_then(|e| e.as_ref()) {
        Some(edit) => {
            let mut state = edit.state(paging.page, paging.board);
            state["editing"] = json!(true);
            state
        }
        None => json!({"editing": false}),
    }
}

/// Carries out an edit-mode command. `Ok(Some(answer))` is the answer itself; `Ok(None)` means
/// "answer with the edit state", once the board shows it.
fn edit_command(command: crate::edit::Command, catalog: &mut Catalog, paging: &mut Paging, shared: &Shared) -> Result<Option<Value>, String> {
    use crate::edit::{Command, Edit};
    use knoend_core::library::Source;
    let mut session = shared.edit.lock().map_err(|e| e.to_string())?;
    match command {
        Command::List => {
            // Agent screens (Claude's questions and permission requests) are not edited here yet:
            // their keys answer a waiting request and their middle rows hold its text.
            let libraries: Vec<Value> = catalog.all().filter(|(_, l)| l.matches.agent.is_empty()).map(|(path, l)| {
                let kind = if !l.matches.web.is_empty() { "site" } else { "app" };
                json!({"id": l.pack.id, "name": l.pack.name, "source": l.pack.source, "kind": kind, "keys": l.keys.len(),
                       "overrides": l.pack.overrides, "own": l.pack.source == Source::User && path.is_file()})
            }).collect();
            let edit = session.as_ref().map(|e| {
                let mut state = e.state(paging.page, paging.board);
                state["editing"] = json!(true);
                state
            }).unwrap_or_else(|| json!({"editing": false}));
            Ok(Some(json!({"libraries": libraries, "shown": paging.layout, "icons": knoend_core::library::icon_names(), "edit": edit})))
        }
        Command::Start(id) => {
            let id = id.unwrap_or_else(|| paging.layout.clone());
            let (path, library) = catalog.by_id(&id).ok_or_else(|| format!("{id} has no keys to edit here; pick one from the list"))?;
            // The person's own version, when there is one, is what they edit.
            let (path, library) = catalog.all().find(|(_, l)| l.pack.overrides.as_deref() == Some(id.as_str())).unwrap_or((path, library));
            if !library.matches.agent.is_empty() {
                return Err("Claude's screens cannot be edited here yet".into());
            }
            crate::edit::check(serde_json::to_value(library).map_err(|e| e.to_string())?, paging.board)?;
            say!("Edit mode: {}", library.pack.id);
            *session = Some(Edit::start(library, path));
            Ok(None)
        }
        Command::Change(json) => {
            let edit = session.as_mut().ok_or("edit mode is off")?;
            let library = crate::edit::check(json, paging.board)?;
            if library.pack.id != edit.library.pack.id {
                return Err("the library's id cannot change while editing".into());
            }
            // Every shortcut must be one the board can type.
            let pages = layout::pages(&library, paging.board);
            for page in 0..pages.len() {
                layout::build(&library, knoend_core::focus::Os::current(), &pages, page)?;
            }
            edit.library = library;
            Ok(None)
        }
        Command::Page(page) => {
            session.as_ref().ok_or("edit mode is off")?;
            paging.page = page; // show_edit keeps it within the pages there are
            Ok(None)
        }
        Command::Save => {
            let edit = session.as_mut().ok_or("edit mode is off")?;
            let before = edit.library.pack.id.clone();
            let (path, library) = edit.save(&config_dir().join("libraries"))?;
            say!("  saved {} to {}", library.pack.id, path.display());
            if paging.layout == before {
                paging.layout = library.pack.id.clone(); // a draft saved as the person's own stays on its page
            }
            catalog.add(path, library);
            Ok(None)
        }
        Command::Revert => {
            let edit = session.as_mut().ok_or("edit mode is off")?;
            edit.library = edit.saved.clone();
            Ok(None)
        }
        Command::Remove => {
            let edit = session.as_ref().ok_or("edit mode is off")?;
            let path = edit.path.clone().ok_or("there is no version of your own to remove")?;
            std::fs::remove_file(&path).map_err(|e| format!("could not remove {}: {e}", path.display()))?;
            say!("  removed {} ({})", edit.library.pack.id, path.display());
            catalog.remove(&edit.library.pack.id);
            // Back to the recipe or draft it was layered on; nothing left to edit ends edit mode.
            let under = edit.library.pack.overrides.as_deref().and_then(|id| catalog.by_id(id)).map(|(p, l)| Edit::start(l, p));
            *session = under;
            Ok(None)
        }
        Command::Stop => {
            if session.take().is_some() {
                say!("Edit mode off");
            }
            Ok(None)
        }
    }
}

/// When the last USB job (installing, pairing, changing Wi-Fi) ended: the board restarts during and
/// after one, so its link drops then are expected, not problems.
static USB_JOB_ENDED: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

pub fn usb_job_ended() {
    if let Ok(mut ended) = USB_JOB_ENDED.lock() {
        *ended = Some(std::time::Instant::now());
    }
}

/// A USB job is running, or ended less than 90 s ago.
fn board_restart_expected(shared: &Shared) -> bool {
    shared.serial.try_lock().is_err()
        || USB_JOB_ENDED.lock().ok().and_then(|e| *e).is_some_and(|at| at.elapsed() < Duration::from_secs(90))
}

/// The connected board's theme, for the tones pictures are sent in.
static PICTURE_THEME: std::sync::Mutex<crate::display::Theme> = std::sync::Mutex::new(crate::display::Theme::Dark);

fn set_picture_theme(name: &str) {
    if let Ok(mut theme) = PICTURE_THEME.lock() {
        *theme = crate::display::Theme::named(name);
    }
}

/// Sends the person's theme and turn when the board shows something else; the board stores and
/// shows them, and answers with its new size (spec/protocol.md 3.6d).
/// Fetches (in the background) each font the person chose that the board does not hold, or asks
/// for the built-in one back; the loop then sends it (`Event::Font`).
fn fetch_fonts(board: &BoardStatus, wanted: &crate::display::Display, events: &mpsc::Sender<Event>, chosen_now: bool) {
    let Some(held) = board.fonts.clone() else {
        if !wanted.key_font.is_empty() || !wanted.start_font.is_empty() {
            say!("Fonts: this board's firmware ({}) has the built-in font only; update its firmware to use others", board.firmware);
        }
        return;
    };
    let refused = held["refused"].as_str().unwrap_or_default();
    for (slot, family) in [("keys", wanted.key_font.clone()), ("start", wanted.start_font.clone())] {
        let holds = held[slot]["name"].as_str().unwrap_or_default();
        if holds == family {
            continue;
        }
        // The board dropped it after a start it did not get through: not sent again by itself,
        // only when the person (or the assistant's fix) chooses fonts again.
        if !chosen_now && !family.is_empty() && refused.split(", ").any(|r| r == family) {
            problem!("font", "The board dropped the font {family} ({slot}) after it could not start with it: it is not sent again until it is chosen again");
            continue;
        }
        let events = events.clone();
        tokio::spawn(async move {
            let font = if family.is_empty() { Ok(Vec::new()) } else { crate::fonts::fetch(&family).await };
            let _ = events.send(Event::Font(slot.to_string(), family, font)).await;
        });
    }
}

/// Sends a font to the board in slices that fit a packet (spec/protocol.md 3.6e).
async fn send_font(sender: &mut wifi::WifiSender, slot: &str, family: &str, bytes: &[u8]) {
    use base64::Engine;
    let crc = crate::fonts::crc32(bytes);
    for (i, chunk) in bytes.chunks(2048).enumerate() {
        let body = json!({"slot": slot, "name": family, "total": bytes.len(), "offset": i * 2048, "crc": crc,
                          "data": base64::engine::general_purpose::STANDARD.encode(chunk)});
        if let Err(e) = sender.send("knoend.font", body).await {
            say!("  could not send the font: {e}");
            return;
        }
    }
}

/// True when it asked (an answer or an error follows).
async fn send_look(sender: &mut wifi::WifiSender, board: &BoardStatus, wanted: &crate::display::Display) -> bool {
    let identity = json!({"board": {"display": board.display}});
    match wanted.shown_by(&identity) {
        Some(true) => false,
        Some(false) => {
            say!("Display: asking the board for {}", wanted.look());
            match sender.send("knoend.display", wanted.look()).await {
                Ok(_) => true,
                Err(e) => {
                    say!("  could not send the display settings: {e}");
                    false
                }
            }
        }
        None => {
            if *wanted != (crate::display::Display { key_size: wanted.key_size, ..Default::default() }) {
                say!("Display: this board's firmware ({}) has one look only; update its firmware for themes and turning", board.firmware);
            }
            false
        }
    }
}

/// Sends a 32x32 RGBA picture to the board in slices that fit a packet. False if the board
/// refused the first slice (a board without picture support ignores the rest).
async fn send_image(sender: &mut wifi::WifiSender, name: &str, rgba: &[u8]) -> bool {
    use base64::Engine;
    let mut rgba = rgba.to_vec();
    if let Ok(theme) = PICTURE_THEME.lock() {
        theme.tone(&mut rgba);
    }
    let pixels = crate::launcher::board_pixels(&rgba);
    for (i, chunk) in pixels.chunks(2048).enumerate() {
        let body = json!({"name": name, "w": 32, "h": 32, "offset": i * 2048, "total": pixels.len(),
                          "data": base64::engine::general_purpose::STANDARD.encode(chunk)});
        if sender.send("knoend.image", body).await.is_err() {
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
        let pixels = tokio::task::spawn_blocking(move || knoend_desktop::picture::render_bytes(&bytes, svg, 32)).await.ok().flatten();
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
            if let Some(pixels) = knoend_desktop::picture::render(&path, 32) {
                out.insert(host, pixels);
            }
        }
    }
    out
}

/// 32x32 RGBA as PNG bytes (for the favicon cache).
fn image_png(rgba: &[u8]) -> Option<Vec<u8>> {
    knoend_desktop::picture::png(rgba, 32, 32)
}

/// Layout id of a coding agent's permission request on the board.
const APPROVE: &str = "system.approve";
const APPROVE_SIGNATURE: &str = "approve|";
/// The board while the person answers the agent's questions on the computer.
const ANSWERING: &str = "system.answering";
/// The "Answer here" key on that screen (the bottom row's middle on 3x3).
const ANSWER_HERE_SLOT: u8 = 8;

/// Whether the focused app is one an agent like Claude Code runs in: its desktop app, a terminal
/// or an editor. While a question waits there, the person is likely answering it on the computer.
fn hosts_agent(focus: &Focus) -> bool {
    const HOSTS: &[&str] = &["claude", "codex", "antigravity", "terminal", "konsole", "kitty", "alacritty", "wezterm",
                             "foot", "xterm", "urxvt", "ghostty", "tilix", "terminator", "iterm", "warp", "code", "cursor",
                             "windsurf", "zed"];
    let names = format!("{} {}", focus.app, focus.instance).to_lowercase();
    HOSTS.iter().any(|h| names.contains(h))
}

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
    /// The person is in the app where the agent runs, answering there: the board says so instead
    /// of showing a question it cannot follow (Claude Code reports no answer until the last).
    elsewhere: bool,
    /// The person chose "Answer here": the questions stay on the board wherever they look.
    here: bool,
    answer: tokio::sync::oneshot::Sender<crate::agent_hooks::Decision>,
}

/// How long after a Claude notification a hook event may still arrive (and the window around it
/// that counts as "explained by a local session").
const HOOK_GRACE: Duration = Duration::from_secs(3);
/// How long the "Go to" key stays when the person does not use it.
const CALLING_FOR: Duration = Duration::from_secs(30 * 60);

/// An app that wants the person and that the board cannot answer for (a Claude cloud session):
/// the last key becomes "Go to <app>", which brings its window to the front.
struct Calling {
    app: String,
    /// Names its window may have (app name, desktop entry).
    names: Vec<String>,
    since: std::time::Instant,
    /// The "Go to" key, from the recipe `claude-code/elsewhere`.
    key: LayoutKey,
}

impl Calling {
    fn new(n: &knoend_desktop::Notification, catalog: &Catalog) -> Calling {
        let names = [n.app.clone(), n.desktop_entry.clone()].into_iter().filter(|s| !s.is_empty()).collect();
        let screen = agent_screen(catalog, "elsewhere");
        let key = screen.keys.iter().zip(screen.placed().into_iter().map(|(_, slot)| slot))
            .find(|(k, _)| matches!(k.action, Action::Agent { reply: AgentReply::GoTo }))
            .map(|(k, slot)| board_key(k, slot, None))
            .unwrap_or_else(|| LayoutKey { key: 9, label: "Go to Claude".into(), speak: None, icon: Some("open_in_new".into()), confirm: false, send: Send::Event(true) });
        Calling { app: n.app.clone(), names, since: std::time::Instant::now(), key }
    }

    fn named(focus: &Focus, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        !name.is_empty() && [&focus.app, &focus.instance].iter().any(|part| {
            let part = part.to_ascii_lowercase();
            part == name || part.ends_with(&format!(".{name}"))
        })
    }

    /// Whether `focus` is this app.
    fn is(&self, focus: &Focus) -> bool {
        self.names.iter().any(|n| Calling::named(focus, n))
    }

    /// Whether `focus` is the app that posted `n`.
    fn matches(focus: &Focus, n: &knoend_desktop::Notification) -> bool {
        Calling::named(focus, &n.app) || Calling::named(focus, &n.desktop_entry)
    }

    /// The "Go to" key on the last key of `grid`, or none on a grid of fewer than four keys, where
    /// it would take the app's own keys (on 1x1, the only one): the status line says it instead.
    fn key(&self, grid: Grid) -> Option<LayoutKey> {
        (grid.size() >= 4).then(|| LayoutKey { key: grid.size(), ..self.key.clone() })
    }

    /// The status line in place of the key ("Claude needs you").
    fn status(&self) -> String {
        board_text(&format!("{} needs you", if self.app.is_empty() { "An app" } else { &self.app }), 80)
    }
}

/// Coding-agent screens are recipes (`recipes/agents`): `claude-code/permission`, `/question` and
/// `/elsewhere` say which key sits where and what it answers.
const AGENT: &str = "claude-code";

/// The recipe for one screen of the agent; the compiled-in one when the catalog has none (e.g.
/// `KNOEND_BUILTIN` points at a folder without agent recipes).
fn agent_screen(catalog: &Catalog, state: &str) -> Library {
    let name = format!("{AGENT}/{state}");
    catalog.agent(&name).cloned().unwrap_or_else(|| {
        builtin::BUILTIN.iter().filter_map(|(_, text)| Library::parse(text).ok())
            .find(|l| l.matches.agent.iter().any(|a| a.eq_ignore_ascii_case(&name)))
            .expect("agent recipes are compiled in")
    })
}

/// A key on an agent screen as the board shows it now.
struct AgentKey<'a> {
    slot: u8,
    reply: AgentReply,
    /// For an option key: which of the agent's options it carries.
    option: Option<usize>,
    key: &'a LibraryKey,
}

/// The keys of `screen` that apply to this request: option keys take the question's options in
/// order (spare ones are left out), Done only for several-choice questions, Always only when the
/// agent offers a rule to remember.
fn agent_keys<'a>(screen: &'a Library, approval: &Approval) -> Vec<AgentKey<'a>> {
    let question = approval.questions.get(approval.step);
    let mut next_option = 0;
    let mut out = Vec::new();
    for (key, slot) in screen.keys.iter().zip(screen.placed().into_iter().map(|(_, slot)| slot)) {
        let Action::Agent { reply } = key.action else { continue };
        let option = match reply {
            AgentReply::Option => {
                next_option += 1;
                match question {
                    Some(q) if next_option - 1 < q.options.len() => Some(next_option - 1),
                    _ => continue,
                }
            }
            AgentReply::Done if !question.is_some_and(|q| q.multi) => continue,
            AgentReply::Always if !approval.can_remember => continue,
            _ => None,
        };
        out.push(AgentKey { slot, reply, option, key });
    }
    out
}

/// A recipe key as a board key (an option key shows the option's label instead of its own).
fn board_key(key: &LibraryKey, slot: u8, option_label: Option<&str>) -> LayoutKey {
    let label = option_label.unwrap_or(&key.label);
    LayoutKey { key: slot, label: board_text(label, 24), speak: Some(board_text(option_label.or(key.speak.as_deref()).unwrap_or(label), 80)),
                icon: key.icon.clone(), confirm: key.confirm, send: Send::Event(true) }
}

impl Approval {
    /// A tap on a key of this request's screen: Some(decision) once it is answered (every question
    /// answered, or declined, or a permission given or refused); None while there is more to do.
    fn tap(&mut self, reply: AgentReply, option: Option<usize>) -> Option<crate::agent_hooks::Decision> {
        use crate::agent_hooks::Decision;
        let Some(q) = self.questions.get(self.step).cloned() else {
            return match reply {
                AgentReply::Allow => Some(Decision::Allow),
                AgentReply::Always if self.can_remember => Some(Decision::AlwaysAllow),
                AgentReply::Deny => Some(Decision::Deny),
                AgentReply::Stop => Some(Decision::Stop),
                _ => None,
            };
        };
        let chosen: Vec<usize> = match (reply, option.filter(|&i| i < q.options.len())) {
            (AgentReply::Decline | AgentReply::Deny | AgentReply::Stop, _) => return Some(if reply == AgentReply::Stop { Decision::Stop } else { Decision::Deny }),
            (AgentReply::Option, Some(i)) if q.multi => {
                if let Some(at) = self.ticked.iter().position(|&t| t == i) { self.ticked.remove(at); } else { self.ticked.push(i); }
                return None;
            }
            (AgentReply::Option, Some(i)) => vec![i],
            (AgentReply::Done, _) if q.multi && !self.ticked.is_empty() => { let mut t = std::mem::take(&mut self.ticked); t.sort(); t }
            _ => return None,
        };
        let text = chosen.iter().map(|&i| q.options[i].0.clone()).collect::<Vec<_>>().join(", ");
        self.answers.push((q.question.clone(), text));
        self.step += 1;
        (self.step >= self.questions.len()).then(|| Decision::Answer(crate::agent_hooks::answered(&self.request, &self.answers)))
    }

    /// The recipe for what is on the board for this request now.
    fn screen(&self, catalog: &Catalog) -> Library {
        agent_screen(catalog, if self.questions.is_empty() { "permission" } else { "question" })
    }
}

/// Shows a request on the board from its recipe: a permission request (Allow, Always when the
/// agent offers it, Deny, Stop) or a question (its options, Done for several-choice, Decline),
/// with the explanation in the panel between the rows.
async fn show_approval(sender: &mut wifi::WifiSender, approval: &Approval, catalog: &Catalog, shown: &mut Option<String>, shared: &Shared) {
    let screen = approval.screen(catalog);
    let question = approval.questions.get(approval.step);
    let keys: Vec<LayoutKey> = agent_keys(&screen, approval).into_iter().map(|k| {
        let option = k.option.and_then(|i| question.map(|q| q.options[i].0.as_str()));
        let mut key = board_key(k.key, k.slot, option);
        if let Some(i) = k.option {
            key.icon = approval.ticked.contains(&i).then(|| "check".to_string());
        }
        if matches!(k.reply, AgentReply::Allow | AgentReply::Always) && approval.destructive {
            key.confirm = true;
        }
        key
    }).collect();
    let clean = |text: &str| -> String { text.chars().map(|c| if c == '\n' || c.is_ascii_graphic() || c == ' ' { c } else { ' ' }).take(240).collect() };
    let (title, detail, status, level) = match question {
        Some(q) => {
            let count = if approval.questions.len() > 1 { format!(" ({}/{})", approval.step + 1, approval.questions.len()) } else { String::new() };
            let mut detail = format!("{}{count}\n{}", q.question, if q.multi { "Tick one or more, then Done." } else { "" });
            if approval.questions.len() > 1 {
                detail += "\nOr answer in Claude on the computer: the board clears when Claude has every answer.";
            }
            for (label, about) in &q.options {
                if !about.is_empty() {
                    detail += &format!("\n{label}: {about}");
                }
            }
            let title = if q.header.is_empty() { screen.pack.name.clone() } else { format!("{}: {}", screen.pack.name, q.header) };
            say!("  -> question on the board [{}]", q.question);
            (title, clean(&detail), format!("{}: {}", screen.pack.name, q.question), "attention")
        }
        None => {
            say!("  -> permission request on the board [{}]", approval.summary);
            (screen.pack.name.clone(), clean(&approval.detail), approval.summary.clone(), if approval.destructive { "alert" } else { "attention" })
        }
    };
    let body = LayoutBody { layout: APPROVE.into(), name: board_text(&title, 128), source: "builtin".into(), reviewed: true, keys, detail: Some(detail),
                            grid: Some(screen.grid(1)), ..LayoutBody::default() };
    let signature = format!("{APPROVE_SIGNATURE}{}", serde_json::to_string(&body).unwrap_or_default());
    if sender.send("knoend.layout", serde_json::to_value(&body).expect("serialisable")).await.is_ok() {
        let _ = sender.send("knoend.status", json!({"text": board_text(&status, 80), "level": level})).await;
        *shown = Some(signature);
        shared.update(|s| s.layout = Some(json!({"id": APPROVE, "name": title, "summary": status}))).await;
    }
}

/// The panel while the person answers on the computer: the questions, short, then what happens.
/// The board's panel takes 240 characters and its built-in font is ASCII, so each question gets
/// an equal share, cut with "...".
fn answering_text(app: &str, questions: &[crate::agent_hooks::Question]) -> String {
    const PANEL: usize = 240;
    let head = format!("Answering in {app}:");
    let tail = "Clears when Claude has every answer. Tap below to answer here.";
    let room = PANEL.saturating_sub(head.len() + tail.len() + 2);
    let each = (room / questions.len().max(1)).saturating_sub(4).max(12);
    let ascii = |text: &str| -> String { text.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { ' ' }).collect() };
    let mut lines = vec![head];
    for (i, q) in questions.iter().enumerate() {
        let text = ascii(q.question.trim());
        let short = if text.len() > each { format!("{}...", text[..each - 3].trim_end()) } else { text };
        lines.push(format!("{}. {short}", i + 1));
    }
    lines.push(tail.to_string());
    let mut text = lines.join("\n");
    text.truncate(PANEL);
    text
}

/// The board while the person answers on the computer: what is happening, and "Answer here".
async fn show_answering(sender: &mut wifi::WifiSender, approval: &Approval, focus: &Focus, shown: &mut Option<String>, shared: &Shared) {
    let app = focus.display_name().to_string();
    let detail = answering_text(&app, &approval.questions);
    let keys = vec![LayoutKey { key: ANSWER_HERE_SLOT, label: "Answer here".into(), speak: Some("Answer the questions here".into()),
                                icon: Some("reply".into()), confirm: false, send: Send::Event(true) }];
    let title = format!("Answering in {app}");
    let body = LayoutBody { layout: ANSWERING.into(), name: board_text(&title, 128), source: "builtin".into(), reviewed: true, keys,
                            detail: Some(detail), grid: Some(Grid::DEFAULT), ..LayoutBody::default() };
    // Starts like the approval screen's signature, so following focus does not replace it.
    let signature = format!("{APPROVE_SIGNATURE}answering|{}", serde_json::to_string(&body).unwrap_or_default());
    if shown.as_deref() == Some(signature.as_str()) {
        return;
    }
    say!("  -> answering on the computer ({app}); the board waits");
    if sender.send("knoend.layout", serde_json::to_value(&body).expect("serialisable")).await.is_ok() {
        let _ = sender.send("knoend.status", json!({"text": board_text(&format!("Claude: answering in {app}"), 80), "level": "info"})).await;
        *shown = Some(signature);
        shared.update(|s| s.layout = Some(json!({"id": ANSWERING, "name": title}))).await;
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
        let result = knoend_desktop::watch_notifications(|n| {
            !n.from_any(NOTIFYING_APPS) || events.blocking_send(Event::Notified(n)).is_ok()
        });
        if let Err(e) = result {
            say!("Desktop notifications not followed: {e}");
        }
    });
}

/// Polls the media players every second and reports when anything shown changes.
fn spawn_media_watcher(media: Arc<knoend_media::Players>, events: mpsc::Sender<Event>) {
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

/// One "Configure" key in the middle of a 3x3 grid (or of the board's grid, if smaller).
fn unsupported_layout(focus: &Focus, paging: &mut Paging) -> LayoutBody {
    let mut name: String = format!("Unsupported: {}", focus.display_name());
    name.truncate(128);
    paging.enter(UNSUPPORTED, 1);
    // One key: a 3x3 places it in the middle, whatever the key size.
    let grid = Grid::DEFAULT.within(paging.board);
    LayoutBody {
        layout: UNSUPPORTED.into(),
        name,
        source: "default".into(),
        reviewed: true,
        grid: Some(grid),
        keys: vec![LayoutKey { key: grid.center(), label: "Configure".into(), speak: Some("Configure this app".into()), icon: Some("auto_awesome".into()), confirm: false, send: Send::Event(true) }],
        ..LayoutBody::default()
    }
}

/// Handles a packet from the board; returns the app to configure when Configure was tapped.
async fn on_board_packet(packet: &Packet, focus: Option<&Focus>, shown_actions: &HashMap<u8, KeyRef>, players: &[NowPlaying],
                         sender: &mut wifi::WifiSender, shared: &Arc<Shared>) -> Option<Focus> {
    match packet.kind.as_str() {
        "knoend.layout.ack" if packet.body["ok"] != true => say!("  board rejected layout: {}", packet.body),
        "knoend.error" => problem!("board-error", "The board answered with an error: {}", packet.body),
        "knoend.key" => {
            let layout = packet.body["layout"].as_str().unwrap_or("");
            let key = packet.body["key"].as_u64().unwrap_or(0);
            shared.emit("key", json!({"text": format!("Key {key} on {layout}"), "key": key, "layout": layout}));
            // A shortcut the board could not type itself (no Bluetooth host): type it here.
            if let (Some(modifiers), Some(usage)) = (packet.body["hid"]["modifiers"].as_u64(), packet.body["hid"]["usage"].as_u64()) {
                let typist = shared.typist.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut slot = typist.lock().map_err(|_| "typist lock".to_string())?;
                    if slot.is_none() {
                        *slot = Some(knoend_input::Typist::connect()?);
                    }
                    slot.as_ref().expect("connected").tap(modifiers as u8, usage as u8)
                }).await.unwrap_or_else(|e| Err(e.to_string()));
                match result {
                    Ok(()) => shared.emit("typed", json!({"text": format!("  typed key {key} on this computer (board has no Bluetooth host)")})),
                    Err(e) => {
                        raise!("typing", "Could not type key {key} on this computer: {e}");
                        shared.emit("type-failed", json!({"text": format!("  could not type key {key}: {e}")}));
                    }
                }
                return None;
            }
            // A key whose action happens on the web page: the browser extension carries it out.
            let acting = u8::try_from(key).ok().and_then(|k| shown_actions.get(&k));
            if let Some(library_key) = acting {
                // Media keys go to the focused app's media player first (like the keyboard's
                // media keys): the browser then allows starting playback.
                let player = focus.and_then(|f| knoend_media::for_focus(players, &f.app, &f.instance));
                if let (Action::Media { command, .. }, Some(player), Some(media)) = (&library_key.action, player, shared.media.clone()) {
                    let mpris = match command {
                        MediaCommand::Toggle => Some(knoend_media::Command::PlayPause),
                        MediaCommand::Play => Some(knoend_media::Command::Play),
                        MediaCommand::Pause => Some(knoend_media::Command::Pause),
                        MediaCommand::Next if player.can_go_next => Some(knoend_media::Command::Next),
                        MediaCommand::Previous if player.can_go_previous => Some(knoend_media::Command::Previous),
                        MediaCommand::Forward if player.can_seek => Some(knoend_media::Command::Seek(10)),
                        MediaCommand::Back if player.can_seek => Some(knoend_media::Command::Seek(-10)),
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
                    let site = focus.and_then(|f| f.web.as_ref()).map(|w| knoend_agents::site::site_of(&w.host)).unwrap_or_default();
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
            // Configure is the unsupported screen's only key of its own (a media row may share it).
            if layout == UNSUPPORTED && acting.is_none() {
                let target = focus?.clone();
                say!("  Configure requested for {}", target.display_name());
                let text = format!("Configuring {}: drafting keys...", target.display_name());
                let _ = sender.send("knoend.status", json!({"text": text, "level": "attention"})).await;
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

    fn recipes() -> Catalog {
        let mut catalog = Catalog::default();
        let problems = load_builtin(&mut catalog);
        assert!(problems.is_empty(), "every recipe loads: {problems:?}");
        catalog
    }

    fn request(tool_input: Value, tool: &str, suggestions: bool) -> (Approval, tokio::sync::oneshot::Receiver<Decision>) {
        let mut request = json!({"tool_name": tool, "session_id": "s", "tool_input": tool_input});
        if suggestions {
            request["permission_suggestions"] = json!([{"type": "addRules"}]);
        }
        let (answer, rx) = tokio::sync::oneshot::channel();
        (Approval { questions: crate::agent_hooks::questions(&request), destructive: crate::agent_hooks::destructive(&request),
                    request, summary: String::new(), detail: String::new(), can_remember: suggestions,
                    step: 0, answers: Vec::new(), ticked: Vec::new(), elsewhere: false, here: false, answer }, rx)
    }

    fn asking(questions: Value) -> (Approval, tokio::sync::oneshot::Receiver<Decision>) {
        request(json!({"questions": questions}), "AskUserQuestion", false)
    }

    /// The keys the board would show, as (slot, label, icon, confirm).
    fn shown(approval: &Approval, catalog: &Catalog) -> Vec<(u8, String, Option<String>, bool)> {
        let screen = approval.screen(catalog);
        let question = approval.questions.get(approval.step);
        agent_keys(&screen, approval).into_iter().map(|k| {
            let key = board_key(k.key, k.slot, k.option.and_then(|i| question.map(|q| q.options[i].0.as_str())));
            let confirm = key.confirm || (matches!(k.reply, AgentReply::Allow | AgentReply::Always) && approval.destructive);
            (key.key, key.label, key.icon, confirm)
        }).collect()
    }

    /// Taps a slot as the service does: the recipe says what the slot answers.
    fn tap(approval: &mut Approval, catalog: &Catalog, slot: u8) -> Option<Decision> {
        let screen = approval.screen(catalog);
        let found = agent_keys(&screen, approval).into_iter().find(|k| k.slot == slot).map(|k| (k.reply, k.option));
        found.and_then(|(reply, option)| approval.tap(reply, option))
    }

    #[test]
    fn permission_screen_is_the_same_as_before_recipes() {
        let catalog = recipes();
        let (plain, _rx) = request(json!({"command": "ls"}), "Bash", false);
        let s = |v: &str| v.to_string();
        assert_eq!(shown(&plain, &catalog), vec![
            (1, s("Allow"), Some(s("check")), false), (3, s("Deny"), Some(s("close")), false), (7, s("Stop"), Some(s("stop_circle")), false)]);
        let (risky, _rx) = request(json!({"command": "rm -rf build"}), "Bash", true);
        assert_eq!(shown(&risky, &catalog), vec![
            (1, s("Allow"), Some(s("check")), true), (2, s("Always"), Some(s("done_all")), true),
            (3, s("Deny"), Some(s("close")), false), (7, s("Stop"), Some(s("stop_circle")), false)]);
        assert_eq!(plain.screen(&catalog).pack.name, "Claude needs you");
    }

    #[test]
    fn taps_on_unused_keys_answer_nothing() {
        let catalog = recipes();
        let (mut plain, _rx) = request(json!({"command": "ls"}), "Bash", false);
        for slot in [2, 4, 5, 6, 8, 9] {
            assert_eq!(tap(&mut plain, &catalog, slot), None, "slot {slot}");
        }
        assert_eq!(tap(&mut plain, &catalog, 7), Some(Decision::Stop));
        let (mut always, _rx) = request(json!({"command": "ls"}), "Bash", true);
        assert_eq!(tap(&mut always, &catalog, 2), Some(Decision::AlwaysAllow));
    }

    #[test]
    fn go_to_key_comes_from_its_recipe_and_knows_the_window() {
        let catalog = recipes();
        let n = knoend_desktop::Notification { app: "Claude".into(), desktop_entry: String::new(), summary: "Claude needs your attention".into(), body: String::new() };
        let calling = Calling::new(&n, &catalog);
        let claude = Focus { os: knoend_core::focus::Os::Linux, app: "com.anthropic.Claude".into(), instance: "com.anthropic.claude".into(), title: String::new(), web: None };
        let terminal = Focus { app: "Xfce4-terminal".into(), instance: "xfce4-terminal".into(), ..claude.clone() };
        assert!(calling.is(&claude) && Calling::matches(&claude, &n));
        assert!(!calling.is(&terminal));
        let key = calling.key(Grid::DEFAULT).unwrap();
        assert_eq!((key.key, key.label.as_str(), key.icon.as_deref()), (9, "Go to Claude", Some("open_in_new")));
        assert_eq!(calling.key(Grid { cols: 2, rows: 2 }).unwrap().key, 4, "the last key of any grid");
        for tiny in [Grid { cols: 1, rows: 1 }, Grid { cols: 2, rows: 1 }, Grid { cols: 1, rows: 3 }] {
            assert!(calling.key(tiny).is_none(), "{tiny:?} keeps its keys");
        }
        assert_eq!(calling.status(), "Claude needs you");
    }

    #[test]
    fn media_row_sits_on_the_bottom_row_of_any_grid() {
        assert_eq!(media_slots(Grid::DEFAULT), [Some(7), Some(8), Some(9)]);
        assert_eq!(media_slots(Grid::LARGEST), [Some(13), Some(14), Some(16)]);
        assert_eq!(media_slots(Grid { cols: 2, rows: 1 }), [None, Some(1), Some(2)]);
        assert_eq!(media_slots(Grid { cols: 1, rows: 2 }), [None, Some(2), None]);
    }

    #[test]
    fn pages_turn_wrap_and_drop_taps_from_another_page() {
        let mut paging = Paging::new(Grid::LARGEST, crate::display::KeySize::Small);
        assert_eq!(paging.enter("builtin.a", 3), 0);
        assert!(paging.turn("prev"));
        assert_eq!(paging.page, 2, "back from the first page wraps to the last");
        assert_eq!(paging.enter("builtin.a", 3), 2, "the same layout keeps its page");
        let tap = |page: u64| Packet { id: 1, re: None, kind: "knoend.key".into(), body: json!({"key": 1, "layout": "builtin.a", "page": page, "seq": 1}) };
        assert!(paging.stale(&tap(1)) && !paging.stale(&tap(3)));
        assert_eq!(paging.enter("builtin.a", 2), 1, "fewer pages than before: the last one");
        assert_eq!(paging.enter("builtin.b", 1), 0, "another layout starts on its first page");
        assert!(!paging.turn("next"), "one page does not turn");
        assert!(!paging.stale(&Packet { body: json!({"key": 1, "layout": "builtin.b", "seq": 1}), ..tap(1) }), "no page means page 1");
    }

    #[test]
    fn key_size_caps_pages_but_edit_mode_sees_the_board() {
        let medium = Paging::new(Grid::LARGEST, crate::display::KeySize::Medium);
        assert_eq!((medium.max, medium.board), (Grid::DEFAULT, Grid::LARGEST));
        let big = Paging::new(Grid::DEFAULT, crate::display::KeySize::Big);
        assert_eq!(big.max, Grid { cols: 2, rows: 2 });
    }

    #[test]
    fn board_grid_comes_from_its_identity() {
        assert_eq!(identity_grid(&json!({"board": {"grid": {"cols": 4, "rows": 2}}})), Grid { cols: 4, rows: 2 });
        assert_eq!(identity_grid(&json!({"board": {"grid": {"cols": 9, "rows": 2}}})), Grid::DEFAULT);
        assert_eq!(identity_grid(&json!({})), Grid::DEFAULT);
    }

    #[test]
    fn the_answering_panel_lists_the_questions_within_240_ascii_characters() {
        let q = |text: &str| crate::agent_hooks::Question { question: text.into(), header: String::new(), options: Vec::new(), multi: false };
        let long = "What should the default key size be on a small 2.8 inch screen for people who find it hard to read? ".repeat(2);
        let text = answering_text("Claude", &[q("Which way round?"), q(&long), q("Dark or paper — and why?")]);
        assert!(text.len() <= 240 && text.is_ascii(), "{text}");
        assert!(text.starts_with("Answering in Claude:\n1. Which way round?\n2. What should"));
        assert!(text.contains("...\n3. Dark or paper"));
        assert!(text.ends_with("Tap below to answer here."));
    }

    #[test]
    fn agents_run_in_their_app_a_terminal_or_an_editor() {
        let focus = |app: &str, instance: &str| Focus { os: knoend_core::focus::Os::Linux, app: app.into(), instance: instance.into(), title: String::new(), web: None };
        assert!(hosts_agent(&focus("Claude", "com.anthropic.claude")));
        assert!(hosts_agent(&focus("Xfce4-terminal", "xfce4-terminal")));
        assert!(hosts_agent(&focus("Code", "code")));
        assert!(!hosts_agent(&focus("firefox", "Navigator")));
        assert!(!hosts_agent(&focus("okular", "okular")));
    }

    #[test]
    fn configure_sits_in_the_middle_of_the_grid() {
        let focus = Focus { os: knoend_core::focus::Os::Linux, app: "x".into(), instance: "x".into(), title: String::new(), web: None };
        assert_eq!(unsupported_layout(&focus, &mut Paging::new(Grid::LARGEST, crate::display::KeySize::Small)).keys[0].key, 5);
        assert_eq!(unsupported_layout(&focus, &mut Paging::new(Grid { cols: 2, rows: 2 }, crate::display::KeySize::Small)).keys[0].key, 1);
    }

    #[test]
    fn questions_are_answered_one_after_another() {
        let catalog = recipes();
        let (mut a, _rx) = asking(json!([
            {"question": "Which?", "header": "", "multiSelect": false, "options": [{"label": "A"}, {"label": "B"}, {"label": "C"}, {"label": "D"}]},
            {"question": "Also?", "header": "", "multiSelect": true, "options": [{"label": "X"}, {"label": "Y"}]}]));
        let labels = |a: &Approval| shown(a, &catalog).into_iter().map(|(slot, label, _, _)| (slot, label)).collect::<Vec<_>>();
        let s = |v: &str| v.to_string();
        assert_eq!(labels(&a), vec![(1, s("A")), (2, s("B")), (3, s("C")), (7, s("D")), (9, s("Decline"))]);
        assert_eq!(tap(&mut a, &catalog, 8), None, "no Done key on a one-choice question");
        assert_eq!(tap(&mut a, &catalog, 7), None, "slot 7 = 4th option, answers the first question");
        assert_eq!(a.answers, vec![("Which?".to_string(), "D".to_string())]);
        assert_eq!(labels(&a), vec![(1, s("X")), (2, s("Y")), (8, s("Done")), (9, s("Decline"))], "spare option keys hidden");
        assert_eq!(tap(&mut a, &catalog, 2), None);
        assert_eq!(tap(&mut a, &catalog, 1), None);
        assert_eq!(tap(&mut a, &catalog, 2), None, "untick");
        assert_eq!(tap(&mut a, &catalog, 2), None, "tick again");
        match tap(&mut a, &catalog, 8) {
            Some(Decision::Answer(input)) => {
                assert_eq!(input["answers"]["Which?"], "D");
                assert_eq!(input["answers"]["Also?"], "X, Y");
            }
            other => panic!("{other:?}"),
        }
        let (mut b, _rx) = asking(json!([{"question": "Q", "options": [{"label": "A"}, {"label": "B"}]}]));
        assert_eq!(tap(&mut b, &catalog, 3), None, "slot 3 has no option here");
        assert_eq!(tap(&mut b, &catalog, 9), Some(Decision::Deny));
    }

    #[test]
    fn a_person_s_own_recipe_replaces_the_built_in_screen() {
        let mut catalog = recipes();
        let mine = r#"
[pack]
id = "user.my-claude"
name = "Claude?"
version = "0.1.0"
source = "user"
[match]
agent = ["claude-code/permission"]
[[key]]
slot = 5
label = "Yes"
action = { kind = "agent", reply = "allow" }
"#;
        catalog.add(PathBuf::from("mine.toml"), Library::parse(mine).unwrap());
        let (mut plain, _rx) = request(json!({"command": "ls"}), "Bash", false);
        assert_eq!(shown(&plain, &catalog), vec![(5, "Yes".to_string(), None, false)]);
        assert_eq!(tap(&mut plain, &catalog, 5), Some(Decision::Allow));
    }
}
