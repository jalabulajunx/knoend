//! The browser link (`spec/browser.md`): the extension talks to `openpager` over native
//! messaging (the browser starts this binary as its "host"), and the host relays to the running
//! service over a Unix socket only this user can open. The service learns the active site and
//! asks the extension to act on the page.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use openpager_core::focus::WebPage;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

/// The native messaging host's name, as the extension and the host manifests use it.
pub const HOST_NAME: &str = "io.github.jalabulajunx.openpager";
/// The extension's id in Chrome (fixed by the public key in its manifest) and in Firefox.
pub const CHROME_EXTENSION_ID: &str = "echibclfkpicikodohehemgjefbpakpj";
pub const FIREFOX_EXTENSION_ID: &str = "openpager@jalabulajunx.github.io";

/// Where the service listens for hosts.
pub fn socket_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        return home.join("Library/Application Support/openpager/browser.sock");
    }
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("openpager/browser.sock"),
        None => openpager_transport::pairing::config_dir().join("browser.sock"),
    }
}

/// Whether the browser started this binary as its native messaging host: Chrome passes the
/// extension's origin, Firefox the host manifest's path and the extension's id.
pub fn started_by_browser(args: &[String]) -> bool {
    args.get(1).is_some_and(|a| a.starts_with("chrome-extension://"))
        || (args.len() >= 3 && args[1].ends_with(".json") && args[2] == FIREFOX_EXTENSION_ID)
}

// ---- the service side ----

/// What the service knows about the connected extensions, and how it asks them to act.
#[derive(Default)]
pub struct Browser {
    /// Each connected host, by connection number.
    links: Mutex<HashMap<u64, mpsc::UnboundedSender<String>>>,
    /// The connection whose window was focused last: the one to act in.
    active: Mutex<Option<u64>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    next: AtomicU64,
}

impl Browser {
    /// Sends `message` (with a fresh `request`) to the active extension and waits for its answer.
    pub async fn request(&self, mut message: Value, timeout: Duration) -> Result<Value, String> {
        let link = {
            let active = *self.active.lock().unwrap();
            let links = self.links.lock().unwrap();
            active.and_then(|id| links.get(&id).cloned())
                .ok_or("the browser extension is not connected (install it, then open the browser)")?
        };
        let request = format!("r{}", self.next.fetch_add(1, Ordering::Relaxed));
        message["request"] = json!(request);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(request.clone(), tx);
        link.send(message.to_string()).map_err(|_| "the browser extension disconnected")?;
        let answer = tokio::time::timeout(timeout, rx).await;
        self.pending.lock().unwrap().remove(&request);
        match answer {
            Ok(Ok(value)) => Ok(value),
            _ => Err("the browser did not answer".into()),
        }
    }

    /// Tells the active extension something it does not answer (e.g. configure progress).
    pub fn notify(&self, message: Value) {
        let active = *self.active.lock().unwrap();
        if let Some(link) = active.and_then(|id| self.links.lock().unwrap().get(&id).cloned()) {
            let _ = link.send(message.to_string());
        }
    }

    pub fn connected(&self) -> bool {
        !self.links.lock().unwrap().is_empty()
    }
}

/// What the extension tells the service.
pub enum BrowserEvent {
    /// The active tab; `None` when no browser window is focused.
    Tab(Option<WebPage>),
    /// "Configure keys for this site" was pressed in the popup.
    Configure,
}

/// Listens for hosts; tab changes go to `tabs`.
#[cfg(unix)]
pub async fn serve(browser: Arc<Browser>, tabs: mpsc::Sender<BrowserEvent>) {
    use std::os::unix::fs::PermissionsExt;
    let path = socket_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let _ = std::fs::remove_file(&path);
    let listener = match tokio::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            println!("Browser link off: {}: {e}", path.display());
            return;
        }
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    let mut number = 0u64;
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        number += 1;
        tokio::spawn(connection(number, stream, browser.clone(), tabs.clone()));
    }
}

#[cfg(not(unix))]
pub async fn serve(_browser: Arc<Browser>, _tabs: mpsc::Sender<BrowserEvent>) {}

#[cfg(unix)]
async fn connection(id: u64, stream: tokio::net::UnixStream, browser: Arc<Browser>, tabs: mpsc::Sender<BrowserEvent>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (read, mut write) = stream.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    browser.links.lock().unwrap().insert(id, tx);
    let writer = tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if write.write_all(format!("{line}\n").as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let mut lines = tokio::io::BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        match message["type"].as_str() {
            Some("hello") => println!("Browser extension connected ({} {})",
                                      message["browser"].as_str().unwrap_or("?"), message["version"].as_str().unwrap_or("?")),
            Some("tab") => {
                let focused = message["windowFocused"] == true;
                if focused {
                    *browser.active.lock().unwrap() = Some(id);
                } else if *browser.active.lock().unwrap() != Some(id) {
                    continue;
                }
                let page = focused.then(|| page_from(&message)).flatten();
                let _ = tabs.send(BrowserEvent::Tab(page)).await;
            }
            Some("configure") => {
                *browser.active.lock().unwrap() = Some(id);
                let _ = tabs.send(BrowserEvent::Configure).await;
            }
            Some("result") | Some("inventory") => {
                if let Some(request) = message["request"].as_str() {
                    if let Some(waiting) = browser.pending.lock().unwrap().remove(request) {
                        let _ = waiting.send(message);
                    }
                }
            }
            _ => {}
        }
    }
    browser.links.lock().unwrap().remove(&id);
    let was_active = {
        let mut active = browser.active.lock().unwrap();
        let was = *active == Some(id);
        if was {
            *active = None;
        }
        was
    };
    if was_active {
        let _ = tabs.send(BrowserEvent::Tab(None)).await;
    }
    writer.abort();
}

fn page_from(message: &Value) -> Option<WebPage> {
    let url = message["url"].as_str()?;
    let host = url.split("://").nth(1)?.split(['/', '?', '#']).next()?.rsplit('@').next()?
        .split(':').next()?.to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    Some(WebPage {
        host,
        url: url.chars().take(2048).collect(),
        title: message["title"].as_str().unwrap_or("").chars().take(200).collect(),
        granted: message["granted"] == true,
        fullscreen: message["fullscreen"] == true,
    })
}

// ---- the host: started by the browser ----

/// Native messaging framing: 32-bit native-endian length, then UTF-8 JSON.
fn read_frame(input: &mut impl Read) -> Option<Vec<u8>> {
    let mut len = [0u8; 4];
    input.read_exact(&mut len).ok()?;
    let len = u32::from_ne_bytes(len) as usize;
    if len > 8 * 1024 * 1024 {
        return None;
    }
    let mut body = vec![0u8; len];
    input.read_exact(&mut body).ok()?;
    Some(body)
}

fn write_frame(output: &mut impl Write, body: &[u8]) -> std::io::Result<()> {
    output.write_all(&(body.len() as u32).to_ne_bytes())?;
    output.write_all(body)?;
    output.flush()
}

/// Relays between the browser (stdin/stdout) and the service socket until the browser closes
/// stdin. Never prints anything else to stdout: the browser reads it as messages.
#[cfg(unix)]
pub fn run_host() -> Result<(), String> {
    use std::os::unix::net::UnixStream;
    let stdout = Arc::new(Mutex::new(std::io::stdout()));
    let socket: Arc<Mutex<Option<UnixStream>>> = Arc::new(Mutex::new(None));
    // What to replay to a service that (re)starts: the extension's hello and its last tab.
    let replay: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // Service -> browser, (re)connecting as needed.
    {
        let (stdout, socket, replay) = (stdout.clone(), socket.clone(), replay.clone());
        std::thread::spawn(move || {
            let mut told_down = false;
            loop {
                match UnixStream::connect(socket_path()) {
                    Ok(stream) => {
                        let mut writer = stream.try_clone().expect("socket clone");
                        for line in replay.lock().unwrap().iter() {
                            let _ = writer.write_all(format!("{line}\n").as_bytes());
                        }
                        *socket.lock().unwrap() = Some(writer);
                        let _ = write_frame(&mut *stdout.lock().unwrap(), br#"{"type":"service","running":true}"#);
                        told_down = false;
                        for line in BufReader::new(stream).lines() {
                            let Ok(line) = line else { break };
                            if write_frame(&mut *stdout.lock().unwrap(), line.as_bytes()).is_err() {
                                std::process::exit(0);
                            }
                        }
                        *socket.lock().unwrap() = None;
                    }
                    Err(_) => {}
                }
                if !told_down {
                    let _ = write_frame(&mut *stdout.lock().unwrap(), br#"{"type":"service","running":false}"#);
                    told_down = true;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    }

    // Browser -> service.
    let mut stdin = std::io::stdin().lock();
    while let Some(body) = read_frame(&mut stdin) {
        let Ok(message) = serde_json::from_slice::<Value>(&body) else { continue };
        let line = message.to_string();
        match message["type"].as_str() {
            Some("hello") => *replay.lock().unwrap() = vec![line.clone()],
            Some("tab") => {
                let mut replay = replay.lock().unwrap();
                replay.retain(|l| !l.contains(r#""type":"tab""#));
                replay.push(line.clone());
            }
            _ => {}
        }
        if let Some(writer) = socket.lock().unwrap().as_mut() {
            let _ = writer.write_all(format!("{line}\n").as_bytes());
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn run_host() -> Result<(), String> {
    Err("the browser link is not available on this OS yet".into())
}

// ---- registering the host with each browser ----

/// Native messaging host folders for the browsers on this computer (only browsers whose own
/// settings folder exists), with whether each is Firefox.
fn host_dirs() -> Vec<(&'static str, PathBuf, bool)> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let candidates: Vec<(&str, PathBuf, PathBuf, bool)> = if cfg!(target_os = "macos") {
        let support = home.join("Library/Application Support");
        vec![
            ("Chrome", support.join("Google/Chrome"), support.join("Google/Chrome/NativeMessagingHosts"), false),
            ("Chromium", support.join("Chromium"), support.join("Chromium/NativeMessagingHosts"), false),
            ("Edge", support.join("Microsoft Edge"), support.join("Microsoft Edge/NativeMessagingHosts"), false),
            ("Brave", support.join("BraveSoftware/Brave-Browser"), support.join("BraveSoftware/Brave-Browser/NativeMessagingHosts"), false),
            ("Firefox", support.join("Mozilla"), support.join("Mozilla/NativeMessagingHosts"), true),
        ]
    } else {
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
        vec![
            ("Chrome", config.join("google-chrome"), config.join("google-chrome/NativeMessagingHosts"), false),
            ("Chromium", config.join("chromium"), config.join("chromium/NativeMessagingHosts"), false),
            ("Edge", config.join("microsoft-edge"), config.join("microsoft-edge/NativeMessagingHosts"), false),
            ("Brave", config.join("BraveSoftware/Brave-Browser"), config.join("BraveSoftware/Brave-Browser/NativeMessagingHosts"), false),
            ("Firefox", home.join(".mozilla"), home.join(".mozilla/native-messaging-hosts"), true),
        ]
    };
    candidates.into_iter().filter(|(_, browser, _, _)| browser.is_dir()).map(|(n, _, dir, f)| (n, dir, f)).collect()
}

/// Writes the host manifest for every browser found; returns one line per browser.
pub fn install() -> Result<Vec<String>, String> {
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| e.to_string())?;
    let mut done = Vec::new();
    for (name, dir, firefox) in host_dirs() {
        let mut manifest = json!({
            "name": HOST_NAME,
            "description": "OpenPager: lets the board follow and act on the active website",
            "path": exe,
            "type": "stdio",
        });
        if firefox {
            manifest["allowed_extensions"] = json!([FIREFOX_EXTENSION_ID]);
        } else {
            manifest["allowed_origins"] = json!([format!("chrome-extension://{CHROME_EXTENSION_ID}/")]);
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(format!("{HOST_NAME}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap() + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
        done.push(format!("{name}: {}", path.display()));
    }
    if done.is_empty() {
        return Err("no Chrome, Chromium, Edge, Brave or Firefox settings folder found".into());
    }
    Ok(done)
}

/// Removes the host manifests `install` wrote.
pub fn uninstall() -> Vec<String> {
    host_dirs().into_iter().filter_map(|(name, dir, _)| {
        let path = dir.join(format!("{HOST_NAME}.json"));
        std::fs::remove_file(&path).ok().map(|_| format!("{name}: removed {}", path.display()))
    }).collect()
}

/// Whether a focused app is a web browser the extension can run in.
pub fn is_browser(app: &str, instance: &str) -> bool {
    let names = [app.to_ascii_lowercase(), instance.to_ascii_lowercase()];
    const BROWSERS: &[&str] = &["firefox", "navigator", "google-chrome", "chromium", "microsoft-edge", "msedge",
                                "brave-browser", "org.mozilla.firefox", "com.google.chrome", "org.chromium.chromium",
                                "com.microsoft.edgemac", "com.brave.browser"];
    names.iter().any(|n| BROWSERS.contains(&n.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, br#"{"type":"hello"}"#).unwrap();
        assert_eq!(&buffer[..4], &16u32.to_ne_bytes());
        assert_eq!(read_frame(&mut buffer.as_slice()).unwrap(), br#"{"type":"hello"}"#);
    }

    #[test]
    fn browser_arguments_are_recognised() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(started_by_browser(&args(&["openpager", "chrome-extension://echibclfkpicikodohehemgjefbpakpj/"])));
        assert!(started_by_browser(&args(&["openpager", "/home/u/.mozilla/native-messaging-hosts/x.json", FIREFOX_EXTENSION_ID])));
        assert!(!started_by_browser(&args(&["openpager", "serve"])));
        assert!(!started_by_browser(&args(&["openpager", "/tmp/x.json", "other@example.com"])));
    }

    #[test]
    fn pages_keep_only_the_host_and_bounded_text() {
        let page = page_from(&json!({"url": "https://user:pw@WWW.YouTube.com:443/watch?v=1", "title": "t", "granted": true})).unwrap();
        assert_eq!(page.host, "www.youtube.com");
        assert!(page.granted);
        assert!(page_from(&json!({"url": null})).is_none());
    }

    #[test]
    fn browsers_are_recognised() {
        assert!(is_browser("firefox", "Navigator"));
        assert!(is_browser("Google-chrome", "google-chrome"));
        assert!(is_browser("org.mozilla.firefox", "Firefox"));
        assert!(!is_browser("Code", "code"));
    }
}
