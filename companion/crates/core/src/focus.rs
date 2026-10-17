//! What the person is looking at: the focused app (and, later, the browser tab).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// The focused application, as the OS reports it.
///
/// Linux (X11): `app` = WM_CLASS class (e.g. "Code"), `instance` = WM_CLASS instance ("code").
/// macOS: `app` = bundle id. Windows: `app` = executable name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Focus {
    pub os: Os,
    pub app: String,
    pub instance: String,
    pub title: String,
}

impl Focus {
    /// A short, human name for messages ("Unsupported app: Claude").
    pub fn display_name(&self) -> &str {
        if !self.app.is_empty() {
            self.app.rsplit('.').next().unwrap_or(&self.app)
        } else {
            &self.title
        }
    }

    /// Whether one library `match` pattern names this app. Case-insensitive, exact; on Linux it
    /// may name either the WM_CLASS class or instance. (Wildcards: open question in the spec.)
    pub fn matches_pattern(&self, pattern: &str) -> bool {
        pattern.eq_ignore_ascii_case(&self.app) || (!self.instance.is_empty() && pattern.eq_ignore_ascii_case(&self.instance))
    }
}
