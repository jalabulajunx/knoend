//! `openpager`: the companion service and its command line.
//!
//!   openpager serve                        web app + service: set up boards, follow the focused app
//!   openpager pair [--port /dev/ttyUSB0]   pair a board over USB (and optionally give it Wi-Fi)
//!   openpager run                          the service without the web app
//!   openpager devices / flash              find, identify and flash boards from a firmware release
//!   openpager draft --class C --instance I  ask the configure agent for a library (no board needed)
//!   openpager browser install              let the browser extension reach openpager
//!   openpager service install              run `serve` in the background, from login
//!   openpager claude-hooks install         Claude Code asks through the board too
//!   openpager hook permission|notification run by Claude Code's hooks
//!
//! A browser that starts this binary as its native messaging host (spec/browser.md) gets the host.

mod agent_hooks;
mod app_facts;
mod background;
mod launcher;
mod browser;
mod diagnostics;
mod firmware;
mod macos_help;
mod provision;
mod service;
mod web;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use openpager_agents::AgentConfig;
use openpager_core::focus::{Focus, Os};
use openpager_core::library::Library;
use openpager_transport::pairing::config_dir;

#[derive(Parser)]
#[command(name = "openpager", version, about = "OpenPager companion service")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
        /// Extra library directories (built-in and ~/.config/openpager/libraries are always read).
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
        #[arg(long)]
        save: bool,
    },
    /// Let the OpenPager browser extension reach this computer's openpager (Chrome, Chromium,
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
    /// Add OpenPager's hooks to ~/.claude/settings.json (a backup is kept next to it).
    Install,
    /// Remove them.
    Uninstall,
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Run `openpager serve` in the background from now on (systemd user service / LaunchAgent).
    Install,
    /// Stop it and remove it.
    Uninstall,
    /// Whether it is installed and running.
    Status,
}

#[derive(Subcommand)]
enum BrowserAction {
    /// Register openpager with every browser found here.
    Install,
    /// Remove those registrations.
    Uninstall,
    /// Act as the native messaging host (browsers start it this way themselves).
    Host,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if browser::started_by_browser(&args) {
        if let Err(message) = browser::run_host() {
            eprintln!("openpager: {message}");
            std::process::exit(1);
        }
        return;
    }
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Pair { port } => pair(&port),
        Command::Serve { port, firmware, no_open, libraries } => tokio::runtime::Runtime::new().expect("tokio").block_on(async move {
            let shared = service::Shared::new(firmware);
            tokio::spawn(service::run(shared.clone(), libraries));
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
            println!("Now add the OpenPager extension to the browser (see extension/README.md).");
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
        Command::Draft { class, instance, save } => tokio::runtime::Runtime::new().expect("tokio")
            .block_on(draft(Focus { os: Os::current(), app: class, instance, title: String::new(), web: None }, save)),
    };
    if let Err(message) = result {
        eprintln!("openpager: {message}");
        std::process::exit(1);
    }
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
    println!("Ready: board {} ({}){}. Now run: openpager serve", done.board, done.profile,
             done.ip.map(|ip| format!(" at {ip}")).unwrap_or_default());
    Ok(())
}

// ---- devices and flashing ----

async fn devices(source: &str) -> Result<(), String> {
    let manifest = firmware::manifest(source).await.map_err(|e| format!("firmware list: {e}"))?;
    let ports = openpager_flash::list_ports();
    if ports.is_empty() {
        println!("No USB serial ports found. Is the board plugged in with a data cable?");
    }
    for port in ports {
        println!("{}  usb {:04x}:{:04x}  {}  {}", port.name, port.vid, port.pid, port.bridge,
                 port.product.as_deref().unwrap_or(""));
        let name = port.name.clone();
        let chip = tokio::task::spawn_blocking(move || openpager_flash::identify(&name)).await.map_err(|e| e.to_string())?;
        match &chip {
            Ok(c) => println!("  chip {} {} flash {} mac {} features {}", c.chip, c.revision.as_deref().unwrap_or(""),
                              c.flash_size, c.mac.as_deref().unwrap_or("?"), c.features.join(", ")),
            Err(e) => println!("  chip: {e}"),
        }
        for c in manifest.candidates(&port, chip.as_ref().ok(), None) {
            println!("  maybe {} (score {}, {}): {}", c.name, c.score, c.status, c.reasons.join("; "));
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
        openpager_flash::flash(&port, &parts, erase, &mut |p| {
            let line = match &p {
                openpager_flash::Progress::Writing { part, parts, done, total, .. } =>
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
    let draft = openpager_agents::draft_library(&config, focus, &about).await?;
    let path = service::ai_libraries().join(format!("{}.toml", openpager_agents::slug(focus)));
    println!("  drafted by {} in {:.1} s ({} attempt{}), {} keys",
             config.describe(), started.elapsed().as_secs_f32(), draft.attempts,
             if draft.attempts == 1 { "" } else { "s" }, draft.library.keys.len());
    if save {
        std::fs::create_dir_all(service::ai_libraries()).map_err(|e| e.to_string())?;
        let header = format!("# Drafted by the OpenPager configure agent ({}). Unreviewed: check the keys,\n\
                              # then change source to \"user\" (and the id prefix to \"user.\") to keep your edits.\n\n",
                             config.describe());
        std::fs::write(&path, header + &draft.toml).map_err(|e| e.to_string())?;
    }
    Ok((path, draft.library))
}

/// Drafts keys for the website in `focus` from the page's controls (the extension lists them; the
/// person must have turned board buttons on for the site), then saves them.
pub async fn configure_site(shared: &service::Shared, focus: &Focus, scope: openpager_agents::site::Scope) -> Result<(PathBuf, Library), String> {
    let browser = &shared.browser;
    let web = focus.web.as_ref().ok_or("no website is focused")?;
    let site = openpager_agents::site::site_of(&web.host);
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
            Some("not-granted") => format!("turn on board buttons for {site} in the OpenPager extension first"),
            Some(other) => format!("the page could not be read ({other})"),
            None => "the page could not be read".into(),
        });
    }
    let items = inventory["items"].as_array().cloned().unwrap_or_default();
    let started = std::time::Instant::now();
    let os = match focus.os {
        openpager_core::focus::Os::Linux => "Linux",
        openpager_core::focus::Os::Macos => "macOS",
        openpager_core::focus::Os::Windows => "Windows",
    };
    let draft = openpager_agents::site::draft_site_library(&config, &web.host, &web.route(), os, &scope, &items).await?;
    println!("  drafted keys for {site}{} by {} from {} controls in {:.1} s ({} attempt{}), {} keys",
             match &scope { openpager_agents::site::Scope::Page { kind, .. } => format!(" (page {kind})"), _ => String::new() },
             config.describe(), items.len(), started.elapsed().as_secs_f32(), draft.attempts,
             if draft.attempts == 1 { "" } else { "s" }, draft.library.keys.len());
    let path = service::ai_libraries().join(format!("{}.toml", draft.library.pack.id.trim_start_matches("ai.")));
    std::fs::create_dir_all(service::ai_libraries()).map_err(|e| e.to_string())?;
    let header = format!("# Drafted by the OpenPager configure agent ({}) from the controls on {site}. Unreviewed:\n\
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

