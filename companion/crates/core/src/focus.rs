//! What the person is looking at: the focused app and, in a browser with the KnoEnd
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
    /// The page is full screen (a presentation showing, a video full screen).
    #[serde(default)]
    pub fullscreen: bool,
    /// The tab's icon as the browser reports it (http(s) or data: URL); empty when none.
    #[serde(skip)]
    pub favicon: String,
}

impl WebPage {
    /// Whether one library `match.web` pattern names this page: the host itself or a parent
    /// domain ("youtube.com" matches "www.youtube.com"); "amazon.*" matches any ending
    /// ("amazon.ca", "www.amazon.co.uk"); a route after the host narrows it to pages under it,
    /// segment by segment, where `*` stands for any one segment ("youtube.com/watch",
    /// "music.example/app/#/album/*/show": `#` marks an app's in-page route).
    ///
    /// A pattern ending in `@fullscreen` matches only while the page is full screen
    /// ("docs.google.com/presentation@fullscreen": a presentation showing).
    pub fn matches_pattern(&self, pattern: &str) -> bool {
        let mut pattern = pattern.trim().to_ascii_lowercase();
        if let Some(base) = pattern.strip_suffix("@fullscreen") {
            if !self.fullscreen {
                return false;
            }
            pattern = base.to_string();
        }
        let (site, route) = match pattern.find('/') {
            Some(i) => (&pattern[..i], Some(&pattern[i..])),
            None => (pattern.as_str(), None),
        };
        let host = self.host.as_str();
        let host_ok = match site.strip_suffix(".*") {
            Some(base) => host.starts_with(&format!("{base}.")) || host.contains(&format!(".{base}.")),
            None => host == site || host.ends_with(&format!(".{site}")),
        };
        host_ok && route.is_none_or(|r| {
            let want = segments(r);
            let have = segments(&self.route().to_ascii_lowercase()).into_iter().map(str::to_string).collect::<Vec<_>>();
            want.len() <= have.len() && want.iter().zip(&have).all(|(w, h)| *w == "*" || w == h)
        })
    }

    /// The URL's path ("/watch").
    pub fn path(&self) -> &str {
        let rest = self.url.split("://").nth(1).unwrap_or("");
        let path = rest.find('/').map(|i| &rest[i..]).unwrap_or("/");
        path.split(['?', '#']).next().unwrap_or("/")
    }

    /// The path plus an app's in-page route when the fragment holds one ("#/album/1/show"), as
    /// single-page apps keep their pages there: "/app/#/album/1/show". No query.
    pub fn route(&self) -> String {
        let fragment = self.url.split_once('#').map(|(_, f)| f).unwrap_or("");
        match fragment.strip_prefix('/') {
            Some(inner) => format!("{}#/{}", self.path().trim_end_matches('/').to_string() + "/", inner.split('?').next().unwrap_or("")),
            None => self.path().to_string(),
        }
    }

    /// The kind of page: the route with id-like segments replaced by `*`
    /// ("/app/#/album/0CqaylqAigA1E6bOjrSyRD/show" -> "/app/#/album/*/show").
    pub fn page_kind(&self) -> String {
        let route = self.route();
        let kind: Vec<String> = route.split('/').map(|s| if looks_like_id(s) { "*".to_string() } else { s.to_string() }).collect();
        let kind = kind.join("/");
        let kind = kind.trim_end_matches('/');
        if kind.is_empty() { "/".into() } else { kind.to_string() }
    }
}

/// Route segments, empty ones dropped ("/app/#/album" -> ["app", "#", "album"]).
fn segments(route: &str) -> Vec<&str> {
    route.split('/').filter(|s| !s.is_empty()).collect()
}

/// A segment that names one thing rather than a kind of page: numbers, hex or random-looking ids.
fn looks_like_id(segment: &str) -> bool {
    let digits = segment.chars().filter(char::is_ascii_digit).count();
    (digits > 0 && segment.len() >= 6) || segment.len() >= 20 || (!segment.is_empty() && digits == segment.len())
        || segment.contains('@') || segment.contains('%')
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
        WebPage { host: host.into(), url: format!("https://{host}/"), title: String::new(), granted: false, fullscreen: false, favicon: String::new() }
    }

    #[test]
    fn web_patterns() {
        assert!(page("www.youtube.com").matches_pattern("youtube.com"));
        assert!(page("youtube.com").matches_pattern("YouTube.com"));
        assert!(!page("notyoutube.com").matches_pattern("youtube.com"));
        assert!(page("www.amazon.co.uk").matches_pattern("amazon.*"));
        assert!(page("amazon.ca").matches_pattern("amazon.*"));
        assert!(!page("amazonia.org").matches_pattern("amazon.*"));
        let watch = WebPage { host: "www.youtube.com".into(), url: "https://www.youtube.com/watch?v=1".into(), title: String::new(), granted: true, fullscreen: false, favicon: String::new() };
        assert!(watch.matches_pattern("youtube.com/watch"));
        assert!(!page("www.youtube.com").matches_pattern("youtube.com/watch"));
        assert_eq!(watch.path(), "/watch");
        let album = WebPage { host: "192.168.50.3".into(), url: "http://192.168.50.3:4533/app/#/album/0CqaylqAigA1E6bOjrSyRD/show".into(), title: String::new(), granted: true, fullscreen: false, favicon: String::new() };
        assert_eq!(album.route(), "/app/#/album/0CqaylqAigA1E6bOjrSyRD/show");
        assert_eq!(album.page_kind(), "/app/#/album/*/show");
        assert!(album.matches_pattern("192.168.50.3/app/#/album/*/show"));
        assert!(album.matches_pattern("192.168.50.3"));
        assert!(!album.matches_pattern("192.168.50.3/app/#/artist/*/show"));
        let home = WebPage { url: "http://192.168.50.3:4533/app/#/".into(), ..album.clone() };
        assert_eq!(home.page_kind(), "/app/#");
        let slides = WebPage { url: "https://docs.google.com/presentation/d/abc123def456/edit".into(), host: "docs.google.com".into(), ..album.clone() };
        assert!(!slides.matches_pattern("docs.google.com/presentation@fullscreen"));
        let presenting = WebPage { fullscreen: true, ..slides.clone() };
        assert!(presenting.matches_pattern("docs.google.com/presentation@fullscreen"));
        assert!(presenting.matches_pattern("docs.google.com"));
        let plain = WebPage { url: "https://example.com/a/b?x=1#section".into(), ..album.clone() };
        assert_eq!(plain.route(), "/a/b");
    }
}
