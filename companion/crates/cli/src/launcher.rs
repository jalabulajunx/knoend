//! What the board shows when nothing is focused: the apps and sites the person uses most and most
//! recently, with their own icons; a tap opens one. Usage is counted here (seconds in front),
//! kept in `usage.json` in the config folder, and never leaves this computer.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use knoend_core::focus::Focus;
use serde::{Deserialize, Serialize};

/// Something that can be opened from the launcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    /// A desktop entry id (Linux, `gtk-launch`) or a bundle id (macOS, `open -b`).
    App { id: String },
    /// A website, opened in the default browser.
    Site { host: String },
}

impl Target {
    /// The board's image name for its icon ("app:firefox", "site:github.com").
    pub fn image_name(&self) -> String {
        let raw = match self {
            Target::App { id } => format!("app:{id}"),
            Target::Site { host } => format!("site:{host}"),
        };
        raw.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || ":._-".contains(*c)).take(65).collect()
    }

    /// Opens it, detached from the service.
    pub fn open(&self) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        match self {
            Target::App { id } => {
                if knoend_focus::bring_to_front(&[id.as_str()]).is_ok() {
                    return Ok(());
                }
                if let Some(exe) = knoend_desktop::win_apps::exe_path(id) {
                    if knoend_focus::spawn_on_input_desktop(&format!("\"{}\"", exe.display())).is_ok() {
                        return Ok(());
                    }
                }
                return knoend_focus::spawn_on_input_desktop(&format!("cmd.exe /C start \"\" \"{id}\""));
            }
            Target::Site { host } => {
                let _ = knoend_focus::bring_to_front(&["msedge.exe", "chrome.exe", "firefox.exe", "brave.exe"]);
                let url = format!("https://{host}/");
                return knoend_focus::spawn_on_input_desktop(&format!("rundll32.exe url.dll,FileProtocolHandler {url}"));
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let (program, args): (&str, Vec<String>) = match self {
                Target::App { id } if cfg!(target_os = "macos") => ("open", vec!["-b".into(), id.clone()]),
                Target::App { id } => ("gtk-launch", vec![id.clone()]),
                Target::Site { host } if cfg!(target_os = "macos") => ("open", vec![format!("https://{host}/")]),
                Target::Site { host } => ("xdg-open", vec![format!("https://{host}/")]),
            };
            std::process::Command::new(program).args(&args)
                .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
                .spawn().map(|_| ()).map_err(|e| format!("{program}: {e}"))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub target: Target,
    /// Seconds in front, all time.
    pub seconds: f64,
    /// Last time in front (Unix seconds).
    pub last: u64,
}

impl Entry {
    /// Most used and most recent: time in front fading with a one-week half-life, plus a bonus for
    /// today.
    fn score(&self, now: u64) -> f64 {
        let age_days = now.saturating_sub(self.last) as f64 / 86_400.0;
        self.seconds * 0.5f64.powf(age_days / 7.0) + if age_days < 1.0 { 600.0 } else { 0.0 }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Usage {
    items: HashMap<String, Entry>,
    #[serde(skip)]
    current: Option<(String, Entry, Option<Instant>)>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Usage {
    fn path() -> PathBuf {
        knoend_transport::pairing::config_dir().join("usage.json")
    }

    pub fn load() -> Usage {
        std::fs::read_to_string(Self::path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn save(&self) {
        let _ = std::fs::write(Self::path(), serde_json::to_string(self).unwrap_or_default());
    }

    /// The focused app or site changed (None: nothing in front): credit the time to the last one.
    pub fn focused(&mut self, now_in_front: Option<(String, Entry)>) {
        if let Some((key, entry, Some(since))) = self.current.take() {
            let item = self.items.entry(key).or_insert(Entry { seconds: 0.0, ..entry.clone() });
            item.seconds += since.elapsed().as_secs_f64().min(8.0 * 3600.0);
            item.last = now();
            item.name = entry.name;
        }
        self.current = now_in_front.map(|(k, e)| (k, e, Some(Instant::now())));
        self.save();
    }

    /// Forgets the apps `gone` names (by id) and saves: on macOS, background programs such as
    /// loginwindow were counted before #74.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn forget_apps(&mut self, gone: impl Fn(&str) -> bool) {
        if self.drop_apps(gone) {
            self.save();
        }
    }

    /// Drops the apps `gone` names; sites stay. True if any went.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn drop_apps(&mut self, gone: impl Fn(&str) -> bool) -> bool {
        let before = self.items.len();
        self.items.retain(|key, _| key.strip_prefix("app:").is_none_or(|id| !gone(id)));
        self.items.len() != before
    }

    /// The top `n`, best first.
    /// Seconds `key` ("app:firefox") has been in front, all time.
    pub fn seconds(&self, key: &str) -> f64 {
        self.items.get(key).map_or(0.0, |e| e.seconds)
    }

    /// The board's home screen, at most `n`: the pinned apps and sites (`pinned`, keys like
    /// "app:firefox"), in their order, then, when `fill`, the most used of the rest. `name` names a
    /// pinned app never used yet (its installed name).
    pub fn home(&self, pinned: &[String], fill: bool, n: usize, name: impl Fn(&str) -> Option<String>) -> Vec<Entry> {
        let mut out: Vec<Entry> = pinned.iter().filter_map(|key| {
            if let Some(used) = self.items.get(key) {
                return Some(used.clone());
            }
            let (kind, id) = key.split_once(':')?;
            let target = match kind {
                "app" => Target::App { id: id.to_string() },
                "site" => Target::Site { host: id.to_string() },
                _ => return None,
            };
            let shown = if kind == "site" { id.to_string() } else { name(id).unwrap_or_else(|| id.rsplit('.').next().unwrap_or(id).trim_end_matches(".exe").to_string()) };
            Some(Entry { name: shown, target, seconds: 0.0, last: 0 })
        }).collect();
        if fill {
            let now = now();
            let mut rest: Vec<(&String, &Entry)> = self.items.iter().filter(|(k, _)| !pinned.contains(k)).collect();
            rest.sort_by(|a, b| b.1.score(now).total_cmp(&a.1.score(now)));
            out.extend(rest.into_iter().map(|(_, e)| e.clone()));
        }
        out.truncate(n);
        out
    }

}

/// What to count for a focus: the site in the browser, else the app (by its desktop entry on
/// Linux, its bundle id on macOS). None for things that cannot be opened again.
pub fn usage_key(focus: &Focus, apps: &[AppInfo]) -> Option<(String, Entry)> {
    let entry = |name: String, target: Target| Entry { name, target, seconds: 0.0, last: now() };
    if let Some(web) = &focus.web {
        let host = web.host.strip_prefix("www.").unwrap_or(&web.host).to_string();
        return Some((format!("site:{host}"), entry(host.clone(), Target::Site { host: web.host.clone() })));
    }
    if cfg!(target_os = "macos") {
        #[cfg(target_os = "macos")]
        if knoend_desktop::mac_apps::background(&focus.app) {
            return None;
        }
        return (!focus.app.is_empty()).then(|| (format!("app:{}", focus.app),
                                                 entry(focus.display_name().to_string(), Target::App { id: focus.app.clone() })));
    }
    if cfg!(target_os = "windows") {
        if focus.app.is_empty() {
            return None;
        }
        #[cfg(target_os = "windows")]
        if let Some(pid) = knoend_focus::window_pid(&[focus.app.as_str(), focus.instance.as_str()]) {
            if let Some(exe) = knoend_focus::process_exe(pid) {
                knoend_desktop::win_apps::remember_exe(&focus.app, exe);
            }
        }
        return Some((
            format!("app:{}", focus.app.to_ascii_lowercase()),
            entry(focus.display_name().to_string(), Target::App { id: focus.app.clone() }),
        ));
    }
    let app = apps.iter().find(|a| a.matches(&focus.app, &focus.instance))?;
    Some((format!("app:{}", app.id), entry(app.name.clone(), Target::App { id: app.id.clone() })))
}

/// The part of an installed app the launcher needs (Linux: from its desktop entry).
#[derive(Debug, Clone)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub icon: String,
    wm_class: String,
}

impl AppInfo {
    fn matches(&self, class: &str, instance: &str) -> bool {
        let hit = |s: &str| !s.is_empty() && (s.eq_ignore_ascii_case(class) || s.eq_ignore_ascii_case(instance));
        hit(&self.wm_class) || hit(&self.id) || hit(self.id.rsplit('.').next().unwrap_or("")) || hit(&self.name)
    }
}

/// Installed apps (Linux); empty elsewhere.
pub fn installed_apps() -> Vec<AppInfo> {
    #[cfg(target_os = "linux")]
    {
        // Same order of preference as knoend_desktop::apps::for_window: window class first.
        let mut apps: Vec<AppInfo> = knoend_desktop::apps::installed().into_iter()
            .map(|a| AppInfo { id: a.id, name: a.name, icon: a.icon, wm_class: a.wm_class }).collect();
        apps.sort_by_key(|a| a.wm_class.is_empty());
        apps
    }
    #[cfg(not(target_os = "linux"))]
    Vec::new()
}

/// 32x32 RGBA icon for a launcher target: the app's icon (Linux: icon theme; macOS: the app
/// bundle's .icns), or a site's saved favicon.
pub fn icon_pixels(target: &Target, name: &str, apps: &[AppInfo], favicons: &HashMap<String, Vec<u8>>) -> Option<Vec<u8>> {
    match target {
        Target::Site { host } => favicons.get(host).cloned(),
        #[cfg(target_os = "linux")]
        Target::App { id } => {
            let _ = name;
            let app = apps.iter().find(|a| &a.id == id)?;
            knoend_desktop::picture::render(&knoend_desktop::apps::icon_file(&app.icon)?, 32)
        }
        #[cfg(target_os = "macos")]
        Target::App { id } => {
            let _ = apps;
            knoend_desktop::mac_apps::icon(id, name, 32)
        }
        #[cfg(target_os = "windows")]
        Target::App { id } => {
            let _ = (name, apps);
            knoend_desktop::win_apps::icon(id, 32)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        Target::App { .. } => {
            let _ = (name, apps);
            None
        }
    }
}

/// RGBA -> the board's colour images: RGB565 with its two bytes swapped (as LVGL draws with
/// LV_COLOR_16_SWAP), then alpha: 3 bytes a pixel.
pub fn board_pixels(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4).flat_map(|p| {
        let c = ((p[0] as u16 >> 3) << 11) | ((p[1] as u16 >> 2) << 5) | (p[2] as u16 >> 3);
        [(c >> 8) as u8, (c & 0xff) as u8, p[3]]
    }).collect()
}

/// Saved favicons (32x32 RGBA PNG per site) in the cache folder.
pub fn favicon_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
            PathBuf::from(home).join(".cache")
        })
        .join("knoend/favicons")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_pack_as_the_board_expects() {
        // Pure red, opaque: RGB565 0xF800, swapped bytes F8 00, then alpha.
        assert_eq!(board_pixels(&[255, 0, 0, 255]), vec![0xF8, 0x00, 0xFF]);
        assert_eq!(board_pixels(&[0, 0, 255, 128]), vec![0x00, 0x1F, 128]);
    }

    #[test]
    fn pinned_apps_come_first_then_the_most_used() {
        let mut usage = Usage::default();
        let entry = |name: &str, target: Target, seconds: f64| Entry { name: name.into(), target, seconds, last: now() };
        usage.items.insert("app:firefox".into(), entry("Firefox", Target::App { id: "firefox".into() }, 900.0));
        usage.items.insert("site:github.com".into(), entry("github.com", Target::Site { host: "github.com".into() }, 300.0));
        let names = |home: &[Entry]| home.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        let pinned = vec!["app:org.musescore.MuseScore".to_string(), "site:github.com".to_string()];
        let name = |id: &str| (id == "org.musescore.MuseScore").then(|| "MuseScore".to_string());
        assert_eq!(names(&usage.home(&pinned, true, 9, name)), ["MuseScore", "github.com", "Firefox"], "pinned, then the rest by use");
        assert_eq!(names(&usage.home(&pinned, false, 9, name)), ["MuseScore", "github.com"], "pinned only");
        assert_eq!(names(&usage.home(&[], true, 1, name)), ["Firefox"]);
    }

    #[test]
    fn background_programs_are_forgotten() {
        let mut usage = Usage::default();
        let entry = |name: &str, target: Target| Entry { name: name.into(), target, seconds: 60.0, last: now() };
        for id in ["com.apple.Terminal", "com.apple.loginwindow"] {
            usage.items.insert(format!("app:{id}"), entry(id, Target::App { id: id.into() }));
        }
        usage.items.insert("site:github.com".into(), entry("github.com", Target::Site { host: "github.com".into() }));
        assert!(!usage.drop_apps(|_| false));
        assert!(usage.drop_apps(|id| id == "com.apple.loginwindow"));
        let mut kept: Vec<_> = usage.items.keys().cloned().collect();
        kept.sort();
        assert_eq!(kept, ["app:com.apple.Terminal", "site:github.com"], "sites stay");
    }

    #[test]
    fn image_names_fit_the_board() {
        assert_eq!(Target::App { id: "com.anthropic.Claude".into() }.image_name(), "app:com.anthropic.claude");
        assert_eq!(Target::Site { host: "www.GitHub.com".into() }.image_name(), "site:www.github.com");
    }

    #[test]
    fn most_used_and_recent_come_first() {
        let mut usage = Usage::default();
        let e = |name: &str, seconds: f64, last: u64| Entry { name: name.into(), target: Target::App { id: name.into() }, seconds, last };
        let t = now();
        usage.items.insert("a".into(), e("old-heavy", 36000.0, t - 30 * 86400));
        usage.items.insert("b".into(), e("today", 300.0, t - 60));
        usage.items.insert("c".into(), e("week", 4000.0, t - 3 * 86400));
        let names: Vec<_> = usage.home(&[], true, 3, |_| None).iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["week", "old-heavy", "today"]); // 2971, 1846, 900
    }
}
