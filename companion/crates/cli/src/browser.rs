//! The browser link (`spec/browser.md`): the extension talks to `knoend` over native
//! messaging (the browser starts this binary as its "host"), and the host relays to the running
//! service over a Unix socket only this user can open. The service learns the active site and
//! asks the extension to act on the page.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use knoend_core::focus::WebPage;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

/// The native messaging host's name, as the extension and the host manifests use it.
pub const HOST_NAME: &str = "io.github.jalabulajunx.knoend";
/// The host's name before the rename: still registered, so an extension built before it connects.
pub const OLD_HOST_NAME: &str = "io.github.jalabulajunx.openpager";
/// The extension's id in Chrome (fixed by the public key in its manifest, unchanged by the
/// rename) and in Firefox (with the id from before the rename, still allowed).
pub const CHROME_EXTENSION_ID: &str = "echibclfkpicikodohehemgjefbpakpj";
pub const FIREFOX_EXTENSION_ID: &str = "knoend@jalabulajunx.github.io";
pub const OLD_FIREFOX_EXTENSION_ID: &str = "openpager@jalabulajunx.github.io";

/// Where the service listens for hosts.
pub fn socket_path() -> PathBuf {
    socket_in("knoend")
}

/// Where the service listened before the rename. It listens there too, so hooks and browser hosts
/// registered before the rename (running the old program) still reach it.
pub fn old_socket_path() -> PathBuf {
    socket_in("openpager")
}

fn socket_in(folder: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        return home.join("Library/Application Support").join(folder).join("browser.sock");
    }
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join(folder).join("browser.sock"),
        None => knoend_transport::pairing::config_dir().join(if folder == "knoend" { "browser.sock" } else { "browser-old.sock" }),
    }
}

/// The current binary's canonical path, without Windows's `\\?\` verbatim prefix.
pub fn clean_exe_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    if let Some(s) = exe.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        return Ok(PathBuf::from(s));
    }
    Ok(exe)
}

#[cfg(unix)]
pub type PrivateStream = std::os::unix::net::UnixStream;
#[cfg(windows)]
pub type PrivateStream = std::net::TcpStream;

#[cfg(unix)]
pub struct PrivateListener(tokio::net::UnixListener);

#[cfg(windows)]
pub struct PrivateListener {
    listener: tokio::net::TcpListener,
    token: String,
}

/// Binds a local socket only this user can open (Unix: 0600 Unix socket; Windows: 127.0.0.1 TCP
/// listener whose port and per-start random auth token are stored in `path`).
#[cfg(unix)]
pub fn bind_private(path: &std::path::Path) -> std::io::Result<PrivateListener> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let _ = std::fs::remove_file(path);
    let listener = tokio::net::UnixListener::bind(path)?;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    Ok(PrivateListener(listener))
}

#[cfg(windows)]
pub fn bind_private(path: &std::path::Path) -> std::io::Result<PrivateListener> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    std_listener.set_nonblocking(true)?;
    let addr = std_listener.local_addr()?;
    let token = knoend_transport::auth::nonce();
    std::fs::write(path, format!("{addr} {token}\n"))?;
    let listener = tokio::net::TcpListener::from_std(std_listener)?;
    Ok(PrivateListener { listener, token })
}

#[cfg(unix)]
impl PrivateListener {
    pub async fn accept(&self) -> std::io::Result<(tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>, tokio::net::unix::OwnedWriteHalf)> {
        let (stream, _) = self.0.accept().await?;
        let (r, w) = stream.into_split();
        Ok((tokio::io::BufReader::new(r), w))
    }
}

#[cfg(windows)]
impl PrivateListener {
    pub async fn accept(&self) -> std::io::Result<(tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>, tokio::net::tcp::OwnedWriteHalf)> {
        use tokio::io::AsyncBufReadExt;
        loop {
            let (stream, _) = self.listener.accept().await?;
            let (r, w) = stream.into_split();
            let mut reader = tokio::io::BufReader::new(r);
            let mut auth_line = String::new();
            let ok = tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut auth_line))
                .await
                .ok()
                .and_then(|r| r.ok())
                .is_some_and(|_| auth_line.trim() == format!("AUTH {}", self.token));
            if ok {
                return Ok((reader, w));
            }
        }
    }
}

/// Connects to the local socket bound by `bind_private`.
#[cfg(unix)]
pub fn connect_private(path: &std::path::Path) -> std::io::Result<PrivateStream> {
    std::os::unix::net::UnixStream::connect(path)
}

#[cfg(windows)]
pub fn connect_private(path: &std::path::Path) -> std::io::Result<PrivateStream> {
    let info = std::fs::read_to_string(path)?;
    let (addr, token) = info
        .trim()
        .split_once(' ')
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid socket file"))?;
    let mut stream = std::net::TcpStream::connect(addr)?;
    stream.write_all(format!("AUTH {token}\n").as_bytes())?;
    Ok(stream)
}

/// Whether the browser started this binary as its native messaging host: Chrome passes the
/// extension's origin, Firefox the host manifest's path and the extension's id.
pub fn started_by_browser(args: &[String]) -> bool {
    args.get(1).is_some_and(|a| a.starts_with("chrome-extension://"))
        || (args.len() >= 3 && args[1].ends_with(".json") && args[2] == FIREFOX_EXTENSION_ID)
}

// ---- the service side ----

/// A connection's browser kind, its last reported page, and when (a counter).
type Reported = (String, Option<WebPage>, u64);

/// What the service knows about the connected extensions, and how it asks them to act.
#[derive(Default)]
pub struct Browser {
    /// Each connected host, by connection number.
    links: Mutex<HashMap<u64, mpsc::UnboundedSender<String>>>,
    /// The connection whose window was focused last: the one to act in.
    active: Mutex<Option<u64>>,
    /// Each connection's browser ("chrome", "firefox") and the active tab it reported last, in or
    /// out of focus, with when (a counter): the service picks the page of the browser the window
    /// system says is in front, rather than trusting each browser's own focus reports (Chrome's
    /// are not reliable on Linux, e.g. while its popup is open or after switching apps).
    pages: Mutex<HashMap<u64, Reported>>,
    /// The kind of browser in front ("chrome", "firefox"), as the service last told us.
    front: Mutex<Option<String>>,
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

    /// The focused app changed: remembers which browser (if any) is in front, makes its extension
    /// the one to act in, and returns its active page.
    pub fn focus_changed(&self, app: &str, instance: &str) -> Option<WebPage> {
        let kind = browser_kind(app, instance).map(str::to_string);
        *self.front.lock().unwrap() = kind.clone();
        let kind = kind?;
        let pages = self.pages.lock().unwrap();
        let (id, (_, page, _)) = pages.iter().filter(|(_, (k, _, _))| *k == kind).max_by_key(|(_, (_, _, seq))| *seq)?;
        *self.active.lock().unwrap() = Some(*id);
        page.clone()
    }

    /// Records a connection's tab; Some(page) when that browser is the one in front (the service
    /// should show it), None when it is not.
    fn tab_reported(&self, id: u64, page: Option<WebPage>) -> Option<Option<WebPage>> {
        let seq = self.next.fetch_add(1, Ordering::Relaxed);
        let mut pages = self.pages.lock().unwrap();
        let entry = pages.entry(id).or_insert_with(|| (String::new(), None, 0));
        entry.1 = page.clone();
        entry.2 = seq;
        let in_front = self.front.lock().unwrap().as_deref() == Some(entry.0.as_str());
        if in_front {
            *self.active.lock().unwrap() = Some(id);
        }
        in_front.then_some(page)
    }
}

/// Which extension serves a focused browser window: "firefox" or "chrome" (every Chromium browser).
pub fn browser_kind(app: &str, instance: &str) -> Option<&'static str> {
    if !is_browser(app, instance) {
        return None;
    }
    let names = format!("{} {}", app, instance).to_ascii_lowercase();
    Some(if names.contains("firefox") || names.contains("navigator") { "firefox" } else { "chrome" })
}

/// What the extension tells the service.
pub enum BrowserEvent {
    /// The active tab; `None` when no browser window is focused.
    Tab(Option<WebPage>),
    /// "Configure keys for this site" was pressed in the popup.
    Configure,
}

/// Listens for hosts; tab changes go to `tabs`.
#[cfg(any(unix, windows))]
pub async fn serve(browser: Arc<Browser>, tabs: mpsc::Sender<BrowserEvent>) {
    let listener = match bind_private(&socket_path()) {
        Ok(l) => l,
        Err(e) => {
            problem!("browser", "The browser extension cannot connect: {}: {e}", socket_path().display());
            return;
        }
    };
    let old = bind_private(&old_socket_path()).ok();
    loop {
        let stream = match &old {
            Some(old) => tokio::select! { s = listener.accept() => s, s = old.accept() => s },
            None => listener.accept().await,
        };
        let Ok((read, write)) = stream else { continue };
        let number = browser.next.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(connection(number, read, write, browser.clone(), tabs.clone()));
    }
}

#[cfg(not(any(unix, windows)))]
pub async fn serve(_browser: Arc<Browser>, _tabs: mpsc::Sender<BrowserEvent>) {}

#[cfg(any(unix, windows))]
async fn connection<R, W>(
    id: u64,
    read: tokio::io::BufReader<R>,
    mut write: W,
    browser: Arc<Browser>,
    tabs: mpsc::Sender<BrowserEvent>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    browser.links.lock().unwrap().insert(id, tx);
    let writer = tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if write.write_all(format!("{line}\n").as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let mut lines = read.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        match message["type"].as_str() {
            Some("hello") => {
                let kind = if message["browser"] == "firefox" { "firefox" } else { "chrome" };
                browser.pages.lock().unwrap().entry(id).or_insert_with(|| (String::new(), None, 0)).0 = kind.to_string();
                say!("Browser extension connected ({} {})", message["browser"].as_str().unwrap_or("?"), message["version"].as_str().unwrap_or("?"));
            }
            Some("tab") => {
                // The tab counts whether or not the browser thinks its window is focused: the
                // window system decides which browser is in front.
                if let Some(page) = browser.tab_reported(id, page_from(&message)) {
                    let _ = tabs.send(BrowserEvent::Tab(page)).await;
                }
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
    browser.pages.lock().unwrap().remove(&id);
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
        favicon: message["favicon"].as_str().filter(|f| f.starts_with("https://") || f.starts_with("http://") || f.starts_with("data:image/"))
            .filter(|f| f.len() <= 100_000).unwrap_or("").to_string(),
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
#[cfg(any(unix, windows))]
pub fn run_host() -> Result<(), String> {
    let stdout = Arc::new(Mutex::new(std::io::stdout()));
    let socket: Arc<Mutex<Option<PrivateStream>>> = Arc::new(Mutex::new(None));
    // What to replay to a service that (re)starts: the extension's hello and its last tab.
    let replay: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // Service -> browser, (re)connecting as needed.
    {
        let (stdout, socket, replay) = (stdout.clone(), socket.clone(), replay.clone());
        std::thread::spawn(move || {
            let mut told_down = false;
            loop {
                match connect_private(&socket_path()) {
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

#[cfg(not(any(unix, windows)))]
pub fn run_host() -> Result<(), String> {
    Err("the browser link is not available on this OS yet".into())
}

// ---- registering the host with each browser ----

/// Native messaging host folders for the browsers on this computer (only browsers whose own
/// settings folder exists), with whether each is Firefox.
fn host_dirs() -> Vec<(&'static str, PathBuf, bool)> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    let candidates: Vec<(&str, PathBuf, PathBuf, bool)> = if cfg!(target_os = "macos") {
        let support = home.join("Library/Application Support");
        vec![
            ("Chrome", support.join("Google/Chrome"), support.join("Google/Chrome/NativeMessagingHosts"), false),
            ("Chromium", support.join("Chromium"), support.join("Chromium/NativeMessagingHosts"), false),
            ("Edge", support.join("Microsoft Edge"), support.join("Microsoft Edge/NativeMessagingHosts"), false),
            ("Brave", support.join("BraveSoftware/Brave-Browser"), support.join("BraveSoftware/Brave-Browser/NativeMessagingHosts"), false),
            ("Firefox", support.join("Mozilla"), support.join("Mozilla/NativeMessagingHosts"), true),
        ]
    } else if cfg!(target_os = "windows") {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData/Local"));
        let roaming = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData/Roaming"));
        let hosts_root = knoend_transport::pairing::config_dir().join("native-messaging-hosts");
        vec![
            ("Chrome", local.join("Google/Chrome/User Data"), hosts_root.join("chrome"), false),
            ("Chromium", local.join("Chromium/User Data"), hosts_root.join("chromium"), false),
            ("Edge", local.join("Microsoft/Edge/User Data"), hosts_root.join("edge"), false),
            ("Brave", local.join("BraveSoftware/Brave-Browser/User Data"), hosts_root.join("brave"), false),
            ("Firefox", roaming.join("Mozilla/Firefox"), hosts_root.join("firefox"), true),
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

#[cfg(target_os = "windows")]
fn windows_registry_key(browser_name: &str, host: &str) -> Option<String> {
    let base = match browser_name {
        "Chrome" => r"HKCU\Software\Google\Chrome\NativeMessagingHosts",
        "Chromium" => r"HKCU\Software\Chromium\NativeMessagingHosts",
        "Edge" => r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts",
        "Brave" => r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
        "Firefox" => r"HKCU\Software\Mozilla\NativeMessagingHosts",
        _ => return None,
    };
    Some(format!(r"{base}\{host}"))
}

/// Writes the host manifest for every browser found; returns one line per browser.
pub fn install() -> Result<Vec<String>, String> {
    let exe = clean_exe_path()?;
    let mut done = Vec::new();
    for (name, dir, firefox) in host_dirs() {
        // Under the current name and the one from before the rename, both running this program.
        for host in [HOST_NAME, OLD_HOST_NAME] {
            let mut manifest = json!({
                "name": host,
                "description": "KnoEnd: lets the board follow and act on the active website",
                "path": exe,
                "type": "stdio",
            });
            if firefox {
                manifest["allowed_extensions"] = json!([FIREFOX_EXTENSION_ID, OLD_FIREFOX_EXTENSION_ID]);
            } else {
                manifest["allowed_origins"] = json!([format!("chrome-extension://{CHROME_EXTENSION_ID}/")]);
            }
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = dir.join(format!("{host}.json"));
            std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap() + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
            #[cfg(target_os = "windows")]
            if let Some(reg_key) = windows_registry_key(name, host) {
                let _ = std::process::Command::new("reg")
                    .args(["add", &reg_key, "/ve", "/t", "REG_SZ", "/d", &path.to_string_lossy(), "/f"])
                    .output();
            }
            done.push(format!("{name}: {}", path.display()));
        }
    }
    if done.is_empty() {
        return Err("no Chrome, Chromium, Edge, Brave or Firefox settings folder found".into());
    }
    Ok(done)
}

/// Removes the host manifests `install` wrote.
pub fn uninstall() -> Vec<String> {
    host_dirs().into_iter().flat_map(|(name, dir, _)| [HOST_NAME, OLD_HOST_NAME].map(|host| (name, host, dir.join(format!("{host}.json")))))
        .filter_map(|(name, host, path)| {
            #[cfg(target_os = "windows")]
            if let Some(reg_key) = windows_registry_key(name, host) {
                let _ = std::process::Command::new("reg").args(["delete", &reg_key, "/f"]).output();
            }
            let _ = host;
            std::fs::remove_file(&path).ok().map(|_| format!("{name}: removed {}", path.display()))
        })
        .collect()
}

/// Whether a focused app is a web browser the extension can run in.
pub fn is_browser(app: &str, instance: &str) -> bool {
    let clean = |s: &str| {
        let lower = s.to_ascii_lowercase();
        lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
    };
    let names = [clean(app), clean(instance)];
    const BROWSERS: &[&str] = &["firefox", "navigator", "google-chrome", "chrome", "chromium", "microsoft-edge", "msedge",
                                "brave-browser", "brave", "org.mozilla.firefox", "com.google.chrome", "org.chromium.chromium",
                                "com.microsoft.edgemac", "com.brave.browser"];
    names.iter().any(|n| BROWSERS.contains(&n.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_in_front_decides_the_page() {
        let browser = Browser::default();
        let page = |host: &str| WebPage { host: host.into(), url: format!("https://{host}/"), title: String::new(), granted: true, fullscreen: false, favicon: String::new() };
        browser.pages.lock().unwrap().insert(1, ("chrome".into(), None, 0));
        browser.pages.lock().unwrap().insert(2, ("firefox".into(), None, 0));
        // Reports while neither is in front are kept, not shown.
        assert_eq!(browser.tab_reported(1, Some(page("www.linkedin.com"))), None);
        assert_eq!(browser.tab_reported(2, Some(page("www.youtube.com"))), None);
        // Switching to Chrome gives Chrome's page even though Chrome never said it was focused.
        assert_eq!(browser.focus_changed("Google-chrome", "google-chrome").map(|p| p.host), Some("www.linkedin.com".into()));
        assert_eq!(*browser.active.lock().unwrap(), Some(1));
        // Firefox's later reports do not take over while Chrome is in front; Chrome's do show.
        assert_eq!(browser.tab_reported(2, Some(page("github.com"))), None);
        assert_eq!(browser.tab_reported(1, Some(page("www.linkedin.com"))).flatten().map(|p| p.host), Some("www.linkedin.com".into()));
        assert_eq!(browser.focus_changed("firefox", "Navigator").map(|p| p.host), Some("github.com".into()));
        assert_eq!(browser.focus_changed("thunar", "thunar"), None);
        assert_eq!(browser_kind("Google-chrome", "google-chrome"), Some("chrome"));
        assert_eq!(browser_kind("firefox", "Navigator"), Some("firefox"));
    }

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
        assert!(started_by_browser(&args(&["knoend", "chrome-extension://echibclfkpicikodohehemgjefbpakpj/"])));
        assert!(started_by_browser(&args(&["knoend", "/home/u/.mozilla/native-messaging-hosts/x.json", FIREFOX_EXTENSION_ID])));
        assert!(!started_by_browser(&args(&["knoend", "serve"])));
        assert!(!started_by_browser(&args(&["knoend", "/tmp/x.json", "other@example.com"])));
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
        assert!(is_browser("chrome.exe", "chrome"));
        assert!(is_browser("msedge.exe", "msedge"));
        assert!(!is_browser("Code", "code"));
    }

    #[tokio::test]
    async fn private_local_socket_authenticates_and_round_trips() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let path = std::env::temp_dir().join(format!("knoend-sock-test-{}", std::process::id()));
        let listener = bind_private(&path).expect("bind_private");
        let client_path = path.clone();
        let client = std::thread::spawn(move || {
            let mut stream = connect_private(&client_path).expect("connect_private");
            stream.write_all(b"ping\n").unwrap();
            let mut reply = String::new();
            BufReader::new(&stream).read_line(&mut reply).unwrap();
            reply
        });
        let (mut read, mut write) = listener.accept().await.expect("accept");
        let mut line = String::new();
        read.read_line(&mut line).await.unwrap();
        assert_eq!(line, "ping\n");
        write.write_all(b"pong\n").await.unwrap();
        assert_eq!(client.join().unwrap(), "pong\n");
        let _ = std::fs::remove_file(&path);
    }
}
