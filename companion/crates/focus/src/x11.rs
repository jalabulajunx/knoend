//! Linux/X11: EWMH `_NET_ACTIVE_WINDOW` on the root window, so it works with i3 and any
//! standards-following window manager.

use knoend_core::focus::{Focus, Os};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, Window};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

pub struct Watcher {
    conn: RustConnection,
    root: Window,
    active: u32,
    wm_name: u32,
    utf8: u32,
    wm_pid: u32,
}

impl Watcher {
    pub fn connect() -> Result<Watcher, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| format!("cannot reach the X server: {e}"))?;
        let root = conn.setup().roots[screen].root;
        let atom = |name: &str| -> Result<u32, String> {
            Ok(conn.intern_atom(false, name.as_bytes()).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom)
        };
        let (active, wm_name, utf8, wm_pid) = (atom("_NET_ACTIVE_WINDOW")?, atom("_NET_WM_NAME")?, atom("UTF8_STRING")?, atom("_NET_WM_PID")?);
        conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE))
            .map_err(|e| e.to_string())?;
        conn.flush().map_err(|e| e.to_string())?;
        Ok(Watcher { conn, root, active, wm_name, utf8, wm_pid })
    }

    /// The focused app now, or None (empty desktop, or a window without WM_CLASS).
    pub fn current(&self) -> Option<Focus> {
        let reply = self.conn.get_property(false, self.root, self.active, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
        let window = reply.value32()?.next().filter(|&w| w != 0)?;
        let class = self.conn.get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256).ok()?.reply().ok()?;
        // WM_CLASS is "instance\0class\0".
        let mut parts = class.value.split(|&b| b == 0).map(|p| String::from_utf8_lossy(p).to_string());
        let instance = parts.next().unwrap_or_default();
        let app = parts.next().unwrap_or_default();
        if app.is_empty() && instance.is_empty() {
            return None;
        }
        let title = self.conn.get_property(false, window, self.wm_name, self.utf8, 0, 256).ok()
            .and_then(|c| c.reply().ok()).map(|r| String::from_utf8_lossy(&r.value).to_string()).unwrap_or_default();
        let pid = self.conn.get_property(false, window, self.wm_pid, AtomEnum::CARDINAL, 0, 1).ok()
            .and_then(|c| c.reply().ok()).and_then(|r| r.value32().and_then(|mut v| v.next()));
        Some(Focus { os: Os::Linux, app, instance, title, web: None, pid, terminal: None })
    }

    /// Blocks until the focused app changes (title changes within the same app are ignored),
    /// then returns the new focus.
    pub fn wait_for_change(&self, previous: Option<&Focus>) -> Option<Focus> {
        loop {
            match self.conn.wait_for_event() {
                Ok(Event::PropertyNotify(event)) if event.atom == self.active => {
                    let now = self.current();
                    let changed = match (&now, previous) {
                        (Some(a), Some(b)) => a.app != b.app || a.instance != b.instance,
                        (None, None) => false,
                        _ => true,
                    };
                    if changed {
                        return now;
                    }
                }
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    }
}

/// Whether a window's WM_CLASS parts belong to the app called `name` ("Claude" matches
/// "com.anthropic.Claude"; case-insensitive).
pub fn class_matches(class: &str, instance: &str, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    !name.is_empty() && [class, instance].iter().any(|part| {
        let part = part.to_ascii_lowercase();
        part == name || part.ends_with(&format!(".{name}"))
    })
}

/// Asks the window manager to show and focus the first window of the app called any of `names`
/// (EWMH `_NET_ACTIVE_WINDOW` sent as a pager, which window managers honour even across
/// workspaces, i3 included).
pub fn bring_to_front(names: &[&str]) -> Result<(), String> {
    use x11rb::protocol::xproto::ClientMessageEvent;
    let (conn, screen) = x11rb::connect(None).map_err(|e| format!("cannot reach the X server: {e}"))?;
    let root = conn.setup().roots[screen].root;
    let atom = |name: &str| -> Result<u32, String> {
        Ok(conn.intern_atom(false, name.as_bytes()).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom)
    };
    let (clients, active) = (atom("_NET_CLIENT_LIST")?, atom("_NET_ACTIVE_WINDOW")?);
    let list = conn.get_property(false, root, clients, AtomEnum::WINDOW, 0, 4096).map_err(|e| e.to_string())?
        .reply().map_err(|e| e.to_string())?;
    let windows: Vec<Window> = list.value32().map(|v| v.collect()).unwrap_or_default();
    let window = windows.into_iter().find(|&w| {
        let Some(class) = conn.get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256).ok().and_then(|c| c.reply().ok()) else { return false };
        let mut parts = class.value.split(|&b| b == 0).map(|p| String::from_utf8_lossy(p).to_string());
        let (instance, class) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
        names.iter().any(|n| class_matches(&class, &instance, n))
    }).ok_or_else(|| format!("no open window of {}", names.join(" / ")))?;
    // Source 2 = a pager acting for the person; timestamp 0 = now.
    let event = ClientMessageEvent::new(32, window, active, [2u32, 0, 0, 0, 0]);
    conn.send_event(false, root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event)
        .map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())
}

/// The process id of the first window of the app called any of `names` (EWMH `_NET_WM_PID`), so
/// its program can be looked at. None when no window has one.
pub fn window_pid(names: &[&str]) -> Option<u32> {
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[screen].root;
    let atom = |name: &str| conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok().map(|r| r.atom);
    let (clients, pid) = (atom("_NET_CLIENT_LIST")?, atom("_NET_WM_PID")?);
    let list = conn.get_property(false, root, clients, AtomEnum::WINDOW, 0, 4096).ok()?.reply().ok()?;
    let windows: Vec<Window> = list.value32()?.collect();
    windows.into_iter().find_map(|w| {
        let class = conn.get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256).ok()?.reply().ok()?;
        let mut parts = class.value.split(|&b| b == 0).map(|p| String::from_utf8_lossy(p).to_string());
        let (instance, class) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
        if !names.iter().any(|n| class.eq_ignore_ascii_case(n) || instance.eq_ignore_ascii_case(n)) {
            return None;
        }
        conn.get_property(false, w, pid, AtomEnum::CARDINAL, 0, 1).ok()?.reply().ok()?.value32()?.next()
    })
}

#[cfg(test)]
mod tests {
    use super::class_matches;

    #[test]
    fn app_names_match_window_classes() {
        assert!(class_matches("com.anthropic.Claude", "com.anthropic.claude", "Claude"));
        assert!(class_matches("firefox", "Navigator", "Firefox"));
        assert!(!class_matches("claude-helper", "x", "Claude"));
        assert!(!class_matches("x", "y", ""));
    }
}
