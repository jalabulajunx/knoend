//! Installed apps (freedesktop `.desktop` entries) and their icons from the icon theme, for the
//! board's launcher. Linux only (pictures: `picture`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One launchable app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Desktop file id without ".desktop" ("firefox"); what `gtk-launch` takes.
    pub id: String,
    pub name: String,
    /// Icon name or absolute path, as the entry gives it.
    pub icon: String,
    /// The window class its windows carry (StartupWMClass), when given.
    pub wm_class: String,
    /// The program it runs ("mscore" from `Exec=mscore %U`), without its folder; empty when the entry
    /// runs it through `env`, `flatpak` or a shell.
    pub program: String,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share"))];
    let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    dirs.push(home().join(".local/share/flatpak/exports/share"));
    dirs
}

/// Parses the `[Desktop Entry]` group of one file.
pub fn parse_entry(id: &str, text: &str) -> Option<App> {
    let mut in_entry = false;
    let mut fields: HashMap<&str, &str> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry {
            if let Some((k, v)) = line.split_once('=') {
                fields.entry(k.trim()).or_insert(v.trim());
            }
        }
    }
    if fields.get("Type").copied().unwrap_or("Application") != "Application"
        || fields.get("NoDisplay") == Some(&"true") || fields.get("Hidden") == Some(&"true") {
        return None;
    }
    Some(App {
        id: id.to_string(),
        name: fields.get("Name")?.to_string(),
        icon: fields.get("Icon").unwrap_or(&"").to_string(),
        wm_class: fields.get("StartupWMClass").unwrap_or(&"").to_string(),
        program: program(fields.get("Exec").copied().unwrap_or("")),
    })
}

/// The program an `Exec=` line runs: its first word without folders or quotes, or empty when that
/// is a launcher of other programs.
fn program(exec: &str) -> String {
    let first = exec.split_whitespace().next().unwrap_or("").trim_matches('"');
    let name = first.rsplit('/').next().unwrap_or(first);
    if ["env", "flatpak", "sh", "bash", "snap", "gtk-launch", "python3", "python"].contains(&name) {
        return String::new();
    }
    name.to_string()
}

/// Every app the menus would show; a user's own entry wins over the system one.
pub fn installed() -> Vec<App> {
    let mut apps: HashMap<String, App> = HashMap::new();
    for dir in data_dirs() {
        let Ok(entries) = std::fs::read_dir(dir.join("applications")) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if apps.contains_key(&id) {
                continue;
            }
            if let Some(app) = std::fs::read_to_string(&path).ok().and_then(|t| parse_entry(&id, &t)) {
                apps.insert(id, app);
            }
        }
    }
    let mut list: Vec<App> = apps.into_values().collect();
    list.sort_by(|a, b| a.id.cmp(&b.id));
    list
}

/// The app a window belongs to, from its WM_CLASS class and instance.
pub fn for_window<'a>(apps: &'a [App], class: &str, instance: &str) -> Option<&'a App> {
    let names = [class.to_ascii_lowercase(), instance.to_ascii_lowercase()];
    let hit = |s: &str| !s.is_empty() && names.iter().any(|n| !n.is_empty() && n == &s.to_ascii_lowercase());
    apps.iter().find(|a| hit(&a.wm_class))
        .or_else(|| apps.iter().find(|a| hit(&a.id) || hit(a.id.rsplit('.').next().unwrap_or(""))))
        .or_else(|| apps.iter().find(|a| hit(&a.name)))
}

/// The icon theme the desktop uses (GTK settings), then what it inherits, then hicolor.
fn themes() -> Vec<String> {
    let mut first = std::process::Command::new("gsettings").args(["get", "org.gnome.desktop.interface", "icon-theme"]).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_matches('\'').to_string()).filter(|t| !t.is_empty());
    if first.is_none() {
        first = std::fs::read_to_string(home().join(".config/gtk-3.0/settings.ini")).ok().and_then(|t| {
            t.lines().find_map(|l| l.strip_prefix("gtk-icon-theme-name=").map(|v| v.trim().to_string()))
        });
    }
    let mut out: Vec<String> = Vec::new();
    let mut queue: Vec<String> = first.into_iter().collect();
    while let Some(theme) = queue.pop() {
        if out.contains(&theme) || out.len() > 8 {
            continue;
        }
        let inherits = theme_roots(&theme).iter().find_map(|r| std::fs::read_to_string(r.join("index.theme")).ok())
            .and_then(|t| t.lines().find_map(|l| l.strip_prefix("Inherits=").map(str::to_string)));
        out.push(theme);
        if let Some(list) = inherits {
            queue.extend(list.split(',').rev().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
        }
    }
    if !out.iter().any(|t| t == "hicolor") {
        out.push("hicolor".into());
    }
    out
}

fn theme_roots(theme: &str) -> Vec<PathBuf> {
    let mut roots = vec![home().join(".icons").join(theme)];
    roots.extend(data_dirs().into_iter().map(|d| d.join("icons").join(theme)));
    roots.into_iter().filter(|r| r.is_dir()).collect()
}

/// Files named `name.png` / `name.svg` under `root`, with the size their folder suggests.
fn candidates(root: &Path, name: &str, out: &mut Vec<(PathBuf, u32)>) {
    let mut stack = vec![root.to_path_buf()];
    let mut seen = 0;
    while let Some(dir) = stack.pop() {
        seen += 1;
        if seen > 4000 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if stem == name && (ext == "png" || ext == "svg") {
                let size = path.components().filter_map(|c| c.as_os_str().to_str())
                    .find_map(|c| c.split(['x', '@']).next().and_then(|n| n.parse::<u32>().ok()))
                    .unwrap_or(if ext == "svg" { 512 } else { 48 });
                out.push((path, size));
            }
        }
    }
}

/// The best file for an icon name: an absolute path as given, else the first theme that has it,
/// preferring a size of at least 32, else /usr/share/pixmaps.
pub fn icon_file(icon: &str) -> Option<PathBuf> {
    if icon.is_empty() {
        return None;
    }
    let as_path = Path::new(icon);
    if as_path.is_absolute() {
        return as_path.exists().then(|| as_path.to_path_buf());
    }
    for theme in themes() {
        let mut found = Vec::new();
        for root in theme_roots(&theme) {
            candidates(&root, icon, &mut found);
        }
        if !found.is_empty() {
            // Smallest size >= 32 (sharp when shrunk), else the largest.
            found.sort_by_key(|(_, size)| if *size >= 32 { (0, *size) } else { (1, u32::MAX - size) });
            return Some(found.remove(0).0);
        }
    }
    ["png", "svg", "xpm"].iter().map(|e| PathBuf::from(format!("/usr/share/pixmaps/{icon}.{e}"))).find(|p| p.exists() && !p.ends_with(".xpm"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_parse_and_hidden_ones_are_skipped() {
        let text = "[Desktop Entry]\nType=Application\nName=Firefox\nName[fr]=Firefox FR\nIcon=firefox\nStartupWMClass=firefox\nExec=firefox %u\n[Desktop Action new]\nName=New Window\n";
        let app = parse_entry("firefox", text).unwrap();
        assert_eq!((app.name.as_str(), app.icon.as_str(), app.wm_class.as_str()), ("Firefox", "firefox", "firefox"));
        assert!(parse_entry("x", "[Desktop Entry]\nName=X\nNoDisplay=true\n").is_none());
        let apps = vec![app, App { id: "org.gnome.Nautilus".into(), name: "Files".into(), icon: String::new(), wm_class: String::new(), program: "nautilus".into() }];
        assert_eq!(for_window(&apps, "firefox", "Navigator").unwrap().id, "firefox");
        assert_eq!(for_window(&apps, "Org.gnome.Nautilus", "org.gnome.Nautilus").unwrap().id, "org.gnome.Nautilus");
        assert!(for_window(&apps, "Code", "code").is_none());
    }
}
