//! Signals from desktop apps that are not about the focused window: notifications they post
//! ("Claude finished", "needs your approval"), and later their accessibility trees. Linux reads
//! them from the session D-Bus; other systems get nothing yet.

use serde::Serialize;

/// One desktop notification as an app posted it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notification {
    /// The app's own name for itself ("Claude").
    pub app: String,
    /// Its desktop entry when it gives one ("claude-desktop").
    pub desktop_entry: String,
    pub summary: String,
    pub body: String,
}

impl Notification {
    /// Whether this came from one of `apps` (case-insensitive, app name or desktop entry).
    pub fn from_any(&self, apps: &[&str]) -> bool {
        let mine = [self.app.to_ascii_lowercase(), self.desktop_entry.to_ascii_lowercase()];
        apps.iter().any(|a| {
            let a = a.to_ascii_lowercase();
            mine.iter().any(|m| !m.is_empty() && (m == &a || m.starts_with(&format!("{a}-")) || m.ends_with(&format!(".{a}"))))
        })
    }
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub mod apps;
/// Built everywhere (its tests run on any system); used on macOS.
pub mod mac_apps;
pub mod picture;

/// Calls `each` for every notification any app posts, until it returns false. Blocks; run it on
/// its own thread.
#[cfg(target_os = "linux")]
pub fn watch_notifications(each: impl FnMut(Notification) -> bool) -> Result<(), String> {
    linux::watch_notifications(each)
}

#[cfg(not(target_os = "linux"))]
pub fn watch_notifications(_each: impl FnMut(Notification) -> bool) -> Result<(), String> {
    Err("desktop notifications are read from D-Bus, which only Linux has".into())
}

#[cfg(test)]
mod tests {
    use super::Notification;

    #[test]
    fn notifications_are_matched_to_apps() {
        let n = |app: &str, entry: &str| Notification { app: app.into(), desktop_entry: entry.into(), summary: String::new(), body: String::new() };
        assert!(n("Claude", "").from_any(&["claude"]));
        assert!(n("", "claude-desktop").from_any(&["claude"]));
        assert!(n("", "com.anthropic.claude").from_any(&["claude"]));
        assert!(!n("Firefox", "firefox").from_any(&["claude"]));
        assert!(!n("Claudette", "").from_any(&["claude"]));
    }
}
