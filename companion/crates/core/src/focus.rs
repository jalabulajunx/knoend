//! What the person is looking at: the focused app and, in a browser with the OpenPager
//! extension, the site in its active tab.

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
    /// The active tab, when the focused app is a browser whose extension reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<WebPage>,
}

/// A browser tab as the extension reports it (`spec/browser.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WebPage {
    /// Lower-case host, e.g. "www.youtube.com".
    pub host: String,
    pub url: String,
    pub title: String,
    /// Board buttons are turned on for this site (the extension holds its host permission).
    pub granted: bool,
}

impl WebPage {
    /// Whether one library `match.web` pattern names this site: the host itself or a parent
    /// domain ("youtube.com" matches "www.youtube.com"); "amazon.*" matches any ending
    /// ("amazon.ca", "www.amazon.co.uk").
    pub fn matches_pattern(&self, pattern: &str) -> bool {
        let pattern = pattern.trim().to_ascii_lowercase();
        let host = self.host.as_str();
        if let Some(base) = pattern.strip_suffix(".*") {
            return host.starts_with(&format!("{base}.")) || host.contains(&format!(".{base}."));
        }
        host == pattern || host.ends_with(&format!(".{pattern}"))
    }
}

impl Focus {
    /// A short, human name for messages ("Unsupported app: Claude").
    pub fn display_name(&self) -> &str {
        if let Some(web) = &self.web {
            return web.host.strip_prefix("www.").unwrap_or(&web.host);
        }
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

#[cfg(test)]
mod tests {
    use super::WebPage;

    fn page(host: &str) -> WebPage {
        WebPage { host: host.into(), url: format!("https://{host}/"), title: String::new(), granted: false }
    }

    #[test]
    fn web_patterns() {
        assert!(page("www.youtube.com").matches_pattern("youtube.com"));
        assert!(page("youtube.com").matches_pattern("YouTube.com"));
        assert!(!page("notyoutube.com").matches_pattern("youtube.com"));
        assert!(page("www.amazon.co.uk").matches_pattern("amazon.*"));
        assert!(page("amazon.ca").matches_pattern("amazon.*"));
        assert!(!page("amazonia.org").matches_pattern("amazon.*"));
    }
}
