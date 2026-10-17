//! Which app has focus. Linux/X11 for now: EWMH `_NET_ACTIVE_WINDOW` on the root window, so it
//! works with i3 and any standards-following window manager. macOS and Windows backends: Tom.

use openpager_core::focus::{Focus, Os};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, Window};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

pub struct X11Focus {
    conn: RustConnection,
    root: Window,
    active: u32,
    wm_name: u32,
    utf8: u32,
}

impl X11Focus {
    pub fn connect() -> Result<X11Focus, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| format!("cannot reach the X server: {e}"))?;
        let root = conn.setup().roots[screen].root;
        let atom = |name: &str| -> Result<u32, String> {
            Ok(conn.intern_atom(false, name.as_bytes()).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom)
        };
        let (active, wm_name, utf8) = (atom("_NET_ACTIVE_WINDOW")?, atom("_NET_WM_NAME")?, atom("UTF8_STRING")?);
        conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE))
            .map_err(|e| e.to_string())?;
        conn.flush().map_err(|e| e.to_string())?;
        Ok(X11Focus { conn, root, active, wm_name, utf8 })
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
        Some(Focus { os: Os::Linux, app, instance, title })
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
