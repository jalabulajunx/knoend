//! `openpager`: the companion service and its command line.
//!
//!   openpager pair [--port /dev/ttyUSB0]   pair a board over USB (and optionally give it Wi-Fi)
//!   openpager run                          follow the focused app and keep the board's keys in step

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use openpager_core::catalog::Catalog;
use openpager_core::focus::Focus;
use openpager_core::layout;
use openpager_core::protocol::{LayoutBody, LayoutKey, Packet, Send};
use openpager_transport::pairing::{config_dir, Pairings};
use openpager_transport::serial::SerialLink;
use openpager_transport::{auth, discover, wifi};
use serde_json::json;
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(name = "openpager", version, about = "OpenPager companion service")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pair a board over USB serial; optionally send it your Wi-Fi name and password.
    Pair {
        #[arg(long, default_value = "/dev/ttyUSB0")]
        port: String,
    },
    /// Find the paired board on Wi-Fi and keep its layout in step with the focused app.
    Run {
        /// Extra library directories (built-in and ~/.config/openpager/libraries are always read).
        #[arg(long)]
        libraries: Vec<PathBuf>,
    },
}

/// Layout id the board shows for an app with no library; key 5 is "Configure".
const UNSUPPORTED: &str = "system.unsupported";
const CONFIGURE_SLOT: u8 = 5;

fn builtin_libraries() -> PathBuf {
    std::env::var_os("OPENPAGER_BUILTIN").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../libraries/builtin")))
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Pair { port } => pair(&port),
        Command::Run { libraries } => tokio::runtime::Runtime::new().expect("tokio").block_on(run(libraries)),
    };
    if let Err(message) = result {
        eprintln!("openpager: {message}");
        std::process::exit(1);
    }
}

// ---- pair ----

fn pair(port: &str) -> Result<(), String> {
    let mut pairings = Pairings::load();
    let mut link = SerialLink::open(port)?;
    link.drain(Duration::from_millis(1500));

    let identity = link.request("openpager.identity", json!({
        "deviceId": pairings.companion, "deviceName": pairings.companion_name, "deviceType": "companion",
        "protocolVersion": 0, "incoming": ["openpager.key"], "outgoing": ["openpager.layout", "openpager.status"],
    }), "openpager.identity", Duration::from_secs(3))?;
    let board = identity.body["deviceId"].as_str().ok_or("board sent no deviceId")?.to_string();
    println!("Board {board} ({}, firmware {})", identity.body["board"]["profile"], identity.body["firmware"]);

    let key = auth::new_key();
    let ack = link.request("openpager.pair", json!({
        "companion": pairings.companion, "companionName": pairings.companion_name, "key": hex_encode(&key),
    }), "openpager.pair.ack", Duration::from_secs(3))?;
    if ack.body["ok"] != true {
        return Err(format!("pairing refused: {}", ack.body));
    }
    pairings.boards.insert(board.clone(), hex_encode(&key));
    pairings.save().map_err(|e| format!("could not save the key: {e}"))?;
    println!("Paired as {:?}; key stored in {} (mode 600)", pairings.companion, config_dir().join("companion.json").display());

    print!("Wi-Fi network name (Enter to skip): ");
    std::io::Write::flush(&mut std::io::stdout()).ok();
    let mut ssid = String::new();
    std::io::stdin().read_line(&mut ssid).map_err(|e| e.to_string())?;
    let ssid = ssid.trim();
    if ssid.is_empty() {
        return Ok(());
    }
    let password = rpassword::prompt_password(format!("Password for {ssid:?} (not shown, not stored here): "))
        .map_err(|e| e.to_string())?;
    let ack = link.request("openpager.wifi", json!({"ssid": ssid, "password": password}),
                           "openpager.wifi.ack", Duration::from_secs(3))?;
    drop(password);
    if ack.body["ok"] != true {
        return Err(format!("Wi-Fi settings refused: {}", ack.body));
    }
    println!("Board accepted the Wi-Fi settings; watching it join (30 s):");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        let Some(line) = link.read_line(left) else { break };
        if line.starts_with("WIFI") {
            println!("  {line}");
            if line.starts_with("WIFI services up") {
                println!("Ready. Now run: openpager run");
                return Ok(());
            }
        }
    }
    Err("the board did not report its Wi-Fi services within 30 s".into())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---- run ----

enum Event {
    Focus(Option<Focus>),
    Board(Packet),
    LinkDown(String),
}

async fn run(extra_libraries: Vec<PathBuf>) -> Result<(), String> {
    let pairings = Pairings::load();
    if pairings.boards.is_empty() {
        return Err("no paired board: connect it by USB and run `openpager pair` first".into());
    }

    let mut dirs = vec![builtin_libraries(), config_dir().join("libraries")];
    dirs.extend(extra_libraries);
    let (catalog, rejected) = Catalog::load(&dirs);
    println!("Libraries: {} loaded from {:?}", catalog.len(), dirs);
    for r in &rejected {
        println!("  skipped {}: {}", r.path.display(), r.reason);
    }

    let (events, mut inbox) = mpsc::channel::<Event>(64);

    // Focus watcher: a plain thread, because X11 waits block.
    let focus_events = events.clone();
    std::thread::spawn(move || {
        let watcher = match openpager_focus::X11Focus::connect() {
            Ok(w) => w,
            Err(e) => {
                eprintln!("focus watcher: {e}");
                return;
            }
        };
        let mut last = watcher.current();
        let _ = focus_events.blocking_send(Event::Focus(last.clone()));
        loop {
            last = watcher.wait_for_change(last.as_ref());
            if focus_events.blocking_send(Event::Focus(last.clone())).is_err() {
                return;
            }
        }
    });

    let mut focus: Option<Focus> = None;
    loop {
        // (Re)connect to the first paired board found on the network.
        let (board, mut sender) = match connect(&pairings, events.clone()).await {
            Ok(link) => link,
            Err(e) => {
                println!("{e}; retrying in 5 s");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        println!("Connected to board {board} over Wi-Fi");
        let mut shown: Option<String> = None; // layout id on the board
        if let Some(f) = &focus {
            show(&mut sender, &catalog, f, &mut shown).await;
        }

        while let Some(event) = inbox.recv().await {
            match event {
                Event::Focus(Some(f)) => {
                    println!("Focus: {} ({})", f.display_name(), f.instance);
                    show(&mut sender, &catalog, &f, &mut shown).await;
                    focus = Some(f);
                }
                Event::Focus(None) => {} // empty desktop: keep the last layout
                Event::Board(packet) => on_board_packet(&packet, focus.as_ref(), &mut sender).await,
                Event::LinkDown(reason) => {
                    println!("Link to board {board} lost: {reason}");
                    break;
                }
            }
        }
    }
}

async fn connect(pairings: &Pairings, events: mpsc::Sender<Event>) -> Result<(String, wifi::WifiSender), String> {
    let wanted: Vec<String> = pairings.boards.keys().cloned().collect();
    let first = wanted.first().cloned();
    let found = tokio::task::spawn_blocking(move || discover::discover(Duration::from_secs(10), first.as_deref()))
        .await.map_err(|e| e.to_string())??;
    let board = found.into_iter().find(|b| wanted.contains(&b.device_id))
        .ok_or("no paired board found on the network")?;
    let key = pairings.key_for(&board.device_id).ok_or("stored key is not valid hex")?;
    let (sender, mut receiver) = wifi::connect((board.address, board.port).into(), &board.device_id, &pairings.companion, key)
        .await.map_err(|e| e.to_string())?;

    let identity = receiver.recv().await.map_err(|e| e.to_string())?;
    println!("Board identity: {} firmware {}", identity.body["board"]["profile"], identity.body["firmware"]);
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
    Ok((board.device_id, sender))
}

/// Pushes the layout for `focus`, or the "unsupported app" screen.
async fn show(sender: &mut wifi::WifiSender, catalog: &Catalog, focus: &Focus, shown: &mut Option<String>) {
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
    let unsupported = id == UNSUPPORTED;
    if shown.as_deref() == Some(id.as_str()) && !unsupported {
        return;
    }
    println!("  -> {} ({})", body.name, id);
    let body = serde_json::to_value(&body).expect("serialisable");
    if sender.send("openpager.layout", body).await.is_ok() {
        let _ = sender.send("openpager.status", status).await;
        *shown = Some(id);
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

async fn on_board_packet(packet: &Packet, focus: Option<&Focus>, sender: &mut wifi::WifiSender) {
    match packet.kind.as_str() {
        "openpager.layout.ack" if packet.body["ok"] != true => println!("  board rejected layout: {}", packet.body),
        "openpager.key" => {
            let layout = packet.body["layout"].as_str().unwrap_or("");
            let key = packet.body["key"].as_u64().unwrap_or(0);
            println!("Key {key} on {layout}");
            if layout == UNSUPPORTED && key == u64::from(CONFIGURE_SLOT) {
                let name = focus.map(Focus::display_name).unwrap_or("this app");
                println!("  Configure requested for {name}: the AI agent is not wired in yet");
                let _ = sender.send("openpager.status", json!({"text": format!("Configure {name}: agent coming next"), "level": "info", "ttl": 5})).await;
            }
        }
        _ => {}
    }
}

