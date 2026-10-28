//! `knoend`: the companion service and its command line.
//!
//!   knoend serve                        web app + service: set up boards, follow the focused app
//!   knoend pair [--port /dev/ttyUSB0]   pair a board over USB (and optionally give it Wi-Fi)
//!   knoend run                          the service without the web app
//!   knoend devices / flash              find, identify and flash boards from a firmware release
//!   knoend draft --class C --instance I  ask the configure agent for a library (no board needed)
//!   knoend browser install              let the browser extension reach knoend
//!   knoend service install              run `serve` in the background, from login
//!   knoend claude-hooks install         Claude Code asks through the board too
//!   knoend hook permission|notification run by Claude Code's hooks
//!
//! A browser that starts this binary as its native messaging host (spec/browser.md) gets the host.

#[macro_use]
mod journal;
mod pager;
mod agent_hooks;
mod app_facts;
mod app_setup;
mod background;
mod boards;
mod launcher;
mod browser;
mod diagnostics;
mod edit;
mod checks;
mod consult;
mod watch;
mod display;
mod fonts;
mod settings;
mod firmware;
mod macos_help;
mod midi;
mod powerpoint;
mod presentation;
mod provision;
mod service;
mod web;

use std::path::PathBuf;

#[cfg(windows)]
mod windows_native;

use clap::{Parser, Subcommand};
use knoend_agents::AgentConfig;
use knoend_core::focus::{Focus, Os};
use knoend_core::library::Library;
use knoend_transport::pairing::config_dir;

#[derive(Parser)]
#[command(name = "knoend", version, about = "KnoEnd companion service")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the companion using local runtime storage and editable recipes.
    Local {
        #[arg(long)] runtime: PathBuf,
        #[arg(long)] recipes: PathBuf,
        #[arg(long)] no_open: bool,
    },
    /// Start the web app (set up and flash boards) and the service (layouts follow the focused app).
    Serve {
        #[arg(long, default_value_t = 7342)]
        port: u16,
        /// Firmware release (folder or URL) the web app flashes from.
        #[arg(long, default_value = firmware::DEFAULT_SOURCE)]
        firmware: String,
        /// Do not open the browser.
        #[arg(long)]
        no_open: bool,
        #[arg(long)]
        libraries: Vec<PathBuf>,
    },
    /// Pair a board over USB serial; optionally send it your Wi-Fi name and password.
    Pair {
        #[arg(long, default_value = "/dev/ttyUSB0")]
        port: String,
    },
    /// Find the paired board on Wi-Fi and keep its layout in step with the focused app.
    Run {
        /// Extra library directories (built-in and ~/.config/knoend/libraries are always read).
        #[arg(long)]
        libraries: Vec<PathBuf>,
    },
    /// List USB serial ports, identify the chip on each, and suggest which board it is.
    Devices {
        /// Firmware release (folder or URL) whose board list is used for suggestions.
        #[arg(long, default_value = firmware::DEFAULT_SOURCE)]
        firmware: String,
    },
    /// Flash a board's firmware from a release (folder or URL), verifying every part first.
    Flash {
        #[arg(long)]
        board: String,
        #[arg(long, default_value = "/dev/ttyUSB0")]
        port: String,
        #[arg(long, default_value = firmware::DEFAULT_SOURCE)]
        firmware: String,
        /// Erase the whole flash first (also erases pairing keys and Wi-Fi settings).
        #[arg(long)]
        erase: bool,
    },
    /// Ask the configure agent to draft a library for an app, print it, and optionally save it.
    Draft {
        /// WM_CLASS class, e.g. "Xfce4-terminal"
        #[arg(long)]
        class: String,
        /// WM_CLASS instance, e.g. "xfce4-terminal"
        #[arg(long)]
        instance: String,
        /// A program running in that terminal app, e.g. "pacman": drafts the program's keys
        #[arg(long)]
        terminal: Option<String>,
        #[arg(long)]
        save: bool,
    },
    /// Let the KnoEnd browser extension reach this computer's knoend (Chrome, Chromium,
    /// Edge, Brave, Firefox).
    Browser {
        #[command(subcommand)]
        action: BrowserAction,
    },
    /// Run the service in the background, started at login and restarted if it stops.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Let Claude Code ask for permission (and say it needs you) on the board as well.
    ClaudeHooks {
        #[command(subcommand)]
        action: HooksAction,
    },
    /// Run by a coding agent's hook (reads the event on stdin): `permission` or `notification`.
    Hook { kind: String },
}

#[derive(Subcommand)]
enum HooksAction {
    /// Add KnoEnd's hooks to ~/.claude/settings.json (a backup is kept next to it).
    Install,
    /// Remove them.
    Uninstall,
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Run `knoend serve` in the background from now on (systemd user service / LaunchAgent).
    Install,
    /// Stop it and remove it.
    Uninstall,
    /// Whether it is installed and running.
    Status,
}

#[derive(Subcommand)]
enum BrowserAction {
    /// Register knoend with every browser found here.
    Install,
    /// Remove those registrations.
    Uninstall,
    /// Act as the native messaging host (browsers start it this way themselves).
    Host,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    #[cfg(windows)]
    if args.get(1).is_some_and(|s| s == "__native-worker") {
        if let Err(e) = windows_native::run(args.get(2).map(String::as_str).unwrap_or(""), args.iter().any(|s| s == "--once")) {
            eprintln!("native worker: {e}"); std::process::exit(1);
        }
        return;
    }
    if browser::started_by_browser(&args) {
        if let Err(message) = browser::run_host() {
            eprintln!("knoend: {message}");
            std::process::exit(1);
        }
        return;
    }
    let cli = Cli::parse();
    if let Some(moved) = knoend_transport::pairing::move_old_settings() {
        println!("{moved}");
    }
    let result = match cli.command {
        Command::Local { runtime, recipes, no_open } => start_local(&runtime,&recipes,no_open),
        Command::Pair { port } => pair(&port),
        Command::Serve { port, firmware, no_open, libraries } => tokio::runtime::Runtime::new().expect("tokio").block_on(async move {
            // Already running (the background service): open its web app instead (the Start menu
            // and app launchers run `knoend serve` either way).
            if web::already_serving(port).await {
                println!("KnoEnd is already running; its web app is at http://127.0.0.1:{port}/");
                if !no_open {
                    web::open_in_browser(&format!("http://127.0.0.1:{port}/"));
                }
                return Ok(());
            }
            let shared = service::Shared::new(firmware);
            tokio::spawn(service::run(shared.clone(), libraries));
            watch::start(shared.clone());
            web::serve(shared, port, !no_open).await
        }),
        Command::Run { libraries } => tokio::runtime::Runtime::new().expect("tokio").block_on(async move {
            service::run(service::Shared::new(firmware::DEFAULT_SOURCE.into()), libraries).await;
            Ok(())
        }),
        Command::Devices { firmware } => tokio::runtime::Runtime::new().expect("tokio").block_on(devices(&firmware)),
        Command::Flash { board, port, firmware, erase } =>
            tokio::runtime::Runtime::new().expect("tokio").block_on(flash(&board, &port, &firmware, erase)),
        Command::Browser { action: BrowserAction::Install } => browser::install().map(|done| {
            for line in done {
                println!("{line}");
            }
            println!("Now add the KnoEnd extension to the browser (see extension/README.md).");
        }),
        Command::Browser { action: BrowserAction::Uninstall } => {
            for line in browser::uninstall() {
                println!("{line}");
            }
            Ok(())
        }
        Command::Browser { action: BrowserAction::Host } => browser::run_host(),
        Command::Hook { kind } => agent_hooks::run_hook(&kind),
        Command::ClaudeHooks { action } => match action {
            HooksAction::Install => agent_hooks::install(),
            HooksAction::Uninstall => agent_hooks::uninstall(),
        }.map(|lines| lines.into_iter().for_each(|l| println!("{l}"))),
        Command::Service { action } => {
            let lines = match action {
                ServiceAction::Install => background::install(),
                ServiceAction::Uninstall => background::uninstall(),
                ServiceAction::Status => Ok(vec![background::status()]),
            };
            lines.map(|lines| lines.into_iter().for_each(|l| println!("{l}")))
        }
        Command::Draft { class, instance, terminal, save } => tokio::runtime::Runtime::new().expect("tokio")
            .block_on(draft(Focus { os: Os::current(), app: class, instance, title: String::new(), web: None, pid: None,
                                    terminal: terminal.map(|p| knoend_core::focus::TerminalTab { tty: String::new(), programs: vec![p] }) }, save)),
    };
    if let Err(message) = result {
        eprintln!("knoend: {message}");
        std::process::exit(1);
    }
}

fn start_local(runtime:&std::path::Path,recipes:&std::path::Path,no_open:bool)->Result<(),String>{
    if !runtime.is_absolute() || !recipes.is_absolute() || !recipes.is_dir(){return Err("Use absolute runtime and existing recipe directory paths".into())}
    let runtime=runtime.to_path_buf();
    for name in ["OneDrive","OneDriveCommercial","OneDriveConsumer"] {
        if let Some(root)=std::env::var_os(name) {
            let root=std::path::PathBuf::from(root);
            if runtime.to_string_lossy().to_ascii_lowercase().starts_with(&format!("{}\\",root.to_string_lossy().trim_end_matches('\\').to_ascii_lowercase())) || runtime==root {
                return Err("Choose runtime storage outside OneDrive".into());
            }
        }
    }
    for folder in ["config","temp","logs"]{std::fs::create_dir_all(runtime.join(folder)).map_err(|e|e.to_string())?;}
    let mut command=std::process::Command::new(std::env::current_exe().map_err(|e|e.to_string())?);
    command.args(["serve","--no-open"]).current_dir(&runtime)
        .env("XDG_CONFIG_HOME",runtime.join("config")).env("TEMP",runtime.join("temp")).env("TMP",runtime.join("temp"))
        .env("KNOEND_BUILTIN",recipes).stdin(std::process::Stdio::null())
        .stdout(std::fs::OpenOptions::new().create(true).append(true).open(runtime.join("logs/companion.stdout.log")).map_err(|e|e.to_string())?)
        .stderr(std::fs::OpenOptions::new().create(true).append(true).open(runtime.join("logs/companion.stderr.log")).map_err(|e|e.to_string())?);
    #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
    command.spawn().map_err(|e|e.to_string())?;
    if !no_open{web::open_in_browser("http://127.0.0.1:7342/");}
    Ok(())
}

// ---- pair ----

fn pair(port: &str) -> Result<(), String> {
    print!("Wi-Fi network name (Enter to skip): ");
    std::io::Write::flush(&mut std::io::stdout()).ok();
    let mut ssid = String::new();
    std::io::stdin().read_line(&mut ssid).map_err(|e| e.to_string())?;
    let ssid = ssid.trim().to_string();
    let password = if ssid.is_empty() {
        String::new()
    } else {
        rpassword::prompt_password(format!("Password for {ssid:?} (not shown, not stored here): ")).map_err(|e| e.to_string())?
    };
    let wifi = (!ssid.is_empty()).then_some((ssid.as_str(), password.as_str()));
    let done = provision::provision(port, wifi, &mut |line| println!("{line}"))?;
    println!("Ready: board {} ({}){}. Now run: knoend serve", done.board, done.profile,
             done.ip.map(|ip| format!(" at {ip}")).unwrap_or_default());
    Ok(())
}

// ---- devices and flashing ----

async fn devices(source: &str) -> Result<(), String> {
    let manifest = firmware::manifest(source).await.map_err(|e| format!("firmware list: {e}"))?;
    let ports = knoend_flash::list_ports();
    if ports.is_empty() {
        println!("No USB serial ports found. Is the board plugged in with a data cable?");
    }
    for port in ports {
        println!("{}  usb {:04x}:{:04x}  {}  {}", port.name, port.vid, port.pid, port.bridge,
                 port.product.as_deref().unwrap_or(""));
        let name = port.name.clone();
        let chip = tokio::task::spawn_blocking(move || knoend_flash::identify(&name)).await.map_err(|e| e.to_string())?;
        match &chip {
            Ok(c) => println!("  chip {} {} flash {} mac {} features {}", c.chip, c.revision.as_deref().unwrap_or(""),
                              c.flash_size, c.mac.as_deref().unwrap_or("?"), c.features.join(", ")),
            Err(e) => println!("  chip: {e}"),
        }
        if let Ok(firmware) = chip.as_ref().map(|c| &c.firmware) {
            for app in firmware.iter().flat_map(|f| &f.apps) {
                println!("  firmware on it, {}: {} {} (ESP-IDF {}){}", app.partition, app.project, app.version, app.idf,
                         if app.components.is_empty() { String::new() } else { format!("; components {}", app.components.join(", ")) });
            }
        }
        let candidates = manifest.candidates(&port, chip.as_ref().ok(), None);
        if !boards::known(&candidates) && chip.is_ok() {
            println!("  a board KnoEnd does not know (no board we list has this chip, flash and USB)");
        }
        for c in &candidates {
            println!("  {} {} (score {}, {}): {}", if c.confident { "likely" } else { "maybe" }, c.name, c.score, c.status, c.reasons.join("; "));
        }
    }
    Ok(())
}

async fn flash(board: &str, port: &str, source: &str, erase: bool) -> Result<(), String> {
    let manifest = firmware::manifest(source).await?;
    let parts = firmware::parts_for(source, &manifest, board).await?;
    println!("Firmware {} for {board}: {} parts verified against the manifest", manifest.version, parts.len());
    let port = port.to_string();
    tokio::task::spawn_blocking(move || {
        let mut last = String::new();
        knoend_flash::flash(&port, &parts, erase, &mut |p| {
            let line = match &p {
                knoend_flash::Progress::Writing { part, parts, done, total, .. } =>
                    format!("writing part {part}/{parts}: {}%", if *total == 0 { 100 } else { done * 100 / total }),
                other => format!("{other:?}").to_lowercase(),
            };
            if line != last {
                println!("  {line}");
                last = line;
            }
        })
    }).await.map_err(|e| e.to_string())??;
    println!("Flashed and reset.");
    Ok(())
}

// ---- the configure agent ----

pub fn agent_config() -> Result<AgentConfig, String> {
    AgentConfig::load(&config_dir().join("agent.toml"))
}

/// Drafts, validates and (optionally) saves a library for `focus`. Returns it with its path.
pub async fn configure(focus: &Focus, save: bool) -> Result<(PathBuf, Library), String> {
    let config = agent_config()?;
    let started = std::time::Instant::now();
    let target = focus.clone();
    let about = tokio::task::spawn_blocking(move || app_facts::about(&target)).await.unwrap_or_default();
    let draft = knoend_agents::draft_library(&config, focus, &about).await?;
    let path = service::ai_libraries().join(format!("{}.toml", knoend_agents::slug(focus)));
    println!("  drafted by {} in {:.1} s ({} attempt{}), {} keys",
             config.describe(), started.elapsed().as_secs_f32(), draft.attempts,
             if draft.attempts == 1 { "" } else { "s" }, draft.library.keys.len());
    if save {
        std::fs::create_dir_all(service::ai_libraries()).map_err(|e| e.to_string())?;
        let header = format!("# Drafted by the KnoEnd configure agent ({}). Unreviewed: check the keys,\n\
                              # then change source to \"user\" (and the id prefix to \"user.\") to keep your edits.\n\n",
                             config.describe());
        std::fs::write(&path, header + &draft.toml).map_err(|e| e.to_string())?;
    }
    Ok((path, draft.library))
}

/// The AI-drafted libraries on this computer: file, TOML and library, for redrafts and the lists.
pub fn ai_drafts() -> Vec<(PathBuf, String, Library)> {
    let Ok(entries) = std::fs::read_dir(service::ai_libraries()) else { return Vec::new() };
    let mut out: Vec<_> = entries.flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let library = Library::parse(&text).ok()?;
            Some((p, text, library))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The AI drafts a redraft can change: still the AI's (not adopted as the person's own), for an
/// app or a program in a terminal (a website's are made from its page), and the ones shown (no
/// library of the person's own or recipe for the same app above them).
pub fn redraftable() -> Vec<(PathBuf, String, Library)> {
    let catalog = service::load_catalog();
    ai_drafts().into_iter()
        .filter(|(_, _, l)| l.pack.source == knoend_core::library::Source::Ai && l.matches.web.is_empty()
                && !catalog.shadowed(l, Os::current()))
        .collect()
}

/// Checks a redraft can run (see `redraftable`, and the AI set up) and returns its file, its TOML
/// and the app it is for (by name, for messages); the drafting itself is `redraft`.
pub fn redraft_target(id: &str) -> Result<(PathBuf, String, Focus), String> {
    agent_config().map_err(|e| format!("set up the AI first (AI page): {e}"))?;
    let (path, text, library) = redraftable().into_iter().find(|(_, _, l)| l.pack.id == id)
        .ok_or_else(|| format!("{id} is not an AI draft that can be redrafted (an AI-drafted app's keys that are in use); \
                                change other keys in Edit keys"))?;
    let name = library.pack.name.clone();
    let focus = Focus { os: Os::current(), app: name.clone(), instance: name, title: String::new(), web: None, pid: None, terminal: None };
    Ok((path, text, focus))
}

/// Drafts an AI library again as the person asked and saves it over the old one, which is kept
/// beside it as `<name>.toml.before-redraft`.
pub async fn redraft(path: &std::path::Path, current: &str, request: &str) -> Result<(PathBuf, Library), String> {
    let config = agent_config()?;
    let started = std::time::Instant::now();
    let draft = knoend_agents::redraft_library(&config, current, request, Os::current()).await?;
    say!("  redrafted by {} in {:.1} s ({} attempt{}), {} keys",
         config.describe(), started.elapsed().as_secs_f32(), draft.attempts,
         if draft.attempts == 1 { "" } else { "s" }, draft.library.keys.len());
    std::fs::write(path.with_extension("toml.before-redraft"), current).map_err(|e| e.to_string())?;
    let header = format!("# Redrafted by the KnoEnd configure agent ({}) as asked: {}\n\
                          # Unreviewed: check the keys. The version before is in the .before-redraft file.\n\n",
                         config.describe(), request.trim().replace(['\r', '\n'], " "));
    std::fs::write(path, header + &draft.toml).map_err(|e| e.to_string())?;
    Ok((path.to_path_buf(), draft.library))
}

/// Drafts keys for the website in `focus` from the page's controls (the extension lists them; the
/// person must have turned board buttons on for the site), then saves them.
pub async fn configure_site(shared: &service::Shared, focus: &Focus, scope: knoend_agents::site::Scope) -> Result<(PathBuf, Library), String> {
    let browser = &shared.browser;
    let web = focus.web.as_ref().ok_or("no website is focused")?;
    let site = knoend_agents::site::site_of(&web.host);
    let config = agent_config()?;
    let read = || browser.request(serde_json::json!({"type": "inventory"}), std::time::Duration::from_secs(10));
    let mut inventory = read().await?;
    if inventory["error"] == "not-granted" {
        shared.ask_site_permission(&site).await?;
        shared.board_status(format!("Configuring {site}: reading the page..."));
        inventory = read().await?;
    }
    if inventory["ok"] != true {
        return Err(match inventory["error"].as_str() {
            Some("not-granted") => format!("turn on board buttons for {site} in the KnoEnd extension first"),
            Some(other) => format!("the page could not be read ({other})"),
            None => "the page could not be read".into(),
        });
    }
    let items = inventory["items"].as_array().cloned().unwrap_or_default();
    let started = std::time::Instant::now();
    let os = match focus.os {
        knoend_core::focus::Os::Linux => "Linux",
        knoend_core::focus::Os::Macos => "macOS",
        knoend_core::focus::Os::Windows => "Windows",
    };
    let draft = knoend_agents::site::draft_site_library(&config, &web.host, &web.page_kind(), os, &scope, &items).await?;
    println!("  drafted keys for {site}{} by {} from {} controls in {:.1} s ({} attempt{}), {} keys",
             match &scope { knoend_agents::site::Scope::Page { kind, .. } => format!(" (page {kind})"), _ => String::new() },
             config.describe(), items.len(), started.elapsed().as_secs_f32(), draft.attempts,
             if draft.attempts == 1 { "" } else { "s" }, draft.library.keys.len());
    let path = service::ai_libraries().join(format!("{}.toml", draft.library.pack.id.trim_start_matches("ai.")));
    std::fs::create_dir_all(service::ai_libraries()).map_err(|e| e.to_string())?;
    let header = format!("# Drafted by the KnoEnd configure agent ({}) from the controls on {site}. Unreviewed:\n\
                          # check the keys, then change source to \"user\" (and the id prefix to \"user.\") to keep your edits.\n\n",
                         config.describe());
    std::fs::write(&path, header + &draft.toml).map_err(|e| e.to_string())?;
    Ok((path, draft.library))
}

async fn draft(focus: Focus, save: bool) -> Result<(), String> {
    println!("Drafting a library for {} ({}) with {}", focus.display_name(), focus.instance, agent_config()?.describe());
    let (path, library) = configure(&focus, save).await?;
    println!("{}", toml::to_string(&library).map_err(|e| e.to_string())?);
    if save {
        println!("Saved to {}", path.display());
    }
    Ok(())
}

