//! The apps installed on this computer, on every OS, as the focus watcher would name their windows
//! (so a library drafted for one before it ever runs matches it once it does). For setting up keys
//! for several apps at once, before they are used.
//!
//! Linux: the menus' desktop entries (class: StartupWMClass or the program; instance: the program).
//! macOS: the `.app` bundles in the Applications folders, Apple's included, and Finder (app: the
//! bundle id; instance: the name).
//! Windows: the programs registered under `App Paths` (app: "Code.exe"; instance: "Code").

/// One installed app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The launcher's id for it (desktop file id, bundle id, or exe name): the key of its icon and use.
    pub id: String,
    pub name: String,
    /// What its windows report as the focused app and instance (`Focus.app`, `Focus.instance`).
    pub app: String,
    pub instance: String,
}

/// Every installed app, by name.
pub fn list() -> Vec<Installed> {
    let mut apps = os_list();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.dedup_by(|a, b| a.app.eq_ignore_ascii_case(&b.app) && a.instance.eq_ignore_ascii_case(&b.instance));
    apps
}

#[cfg(target_os = "linux")]
fn os_list() -> Vec<Installed> {
    crate::apps::installed().into_iter().filter_map(linux_app).collect()
}

/// A desktop entry as its window will be named; None for one that cannot be told apart.
#[cfg(target_os = "linux")]
fn linux_app(entry: crate::apps::App) -> Option<Installed> {
    let last = entry.id.rsplit('.').next().unwrap_or(&entry.id).to_string();
    let instance = if entry.program.is_empty() { last.to_lowercase() } else { entry.program.clone() };
    let app = if !entry.wm_class.is_empty() { entry.wm_class.clone() } else if !entry.program.is_empty() { entry.program.clone() } else { last };
    (!app.is_empty()).then(|| Installed { id: entry.id, name: entry.name, app, instance })
}

#[cfg(target_os = "macos")]
fn os_list() -> Vec<Installed> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    // Apple's own apps (Notes, Terminal...) are in /System/Applications; Finder alone of CoreServices.
    let folders = ["/Applications", "/Applications/Utilities", "/System/Applications", "/System/Applications/Utilities"]
        .map(std::path::PathBuf::from).into_iter().chain([home.join("Applications")]);
    let bundles = folders.flat_map(|folder| std::fs::read_dir(folder).into_iter().flatten().flatten().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .chain([std::path::PathBuf::from("/System/Library/CoreServices/Finder.app")]);
    let mut apps = Vec::new();
    for path in bundles {
        let plist = path.join("Contents/Info.plist");
        let read = |key: &str| std::process::Command::new("plutil").args(["-extract", key, "raw", "-o", "-"]).arg(&plist).output().ok()
            .filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|v| !v.is_empty());
        let Some(bundle) = read("CFBundleIdentifier") else { continue };
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or(&bundle).to_string();
        apps.push(Installed { id: bundle.clone(), name: name.clone(), app: bundle, instance: name });
    }
    apps
}

#[cfg(target_os = "windows")]
fn os_list() -> Vec<Installed> {
    let mut apps = Vec::new();
    for root in ["HKLM", "HKCU"] {
        let key = format!(r"{root}\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths");
        let Ok(out) = std::process::Command::new("reg").args(["query", &key]).output() else { continue };
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let Some(exe) = line.trim().rsplit('\\').next().filter(|n| n.to_ascii_lowercase().ends_with(".exe")) else { continue };
            let stem = exe[..exe.len() - 4].to_string();
            apps.push(Installed { id: exe.to_string(), name: stem.clone(), app: exe.to_string(), instance: stem });
        }
    }
    apps
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn os_list() -> Vec<Installed> {
    Vec::new()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn entry(id: &str, wm_class: &str, program: &str) -> crate::apps::App {
        crate::apps::App { id: id.into(), name: id.into(), icon: String::new(), wm_class: wm_class.into(), program: program.into() }
    }

    #[test]
    fn linux_apps_are_named_as_their_windows_are() {
        let musescore = linux_app(entry("org.musescore.MuseScore", "MuseScore4", "mscore")).unwrap();
        assert_eq!((musescore.app.as_str(), musescore.instance.as_str()), ("MuseScore4", "mscore"));
        let thunar = linux_app(entry("thunar", "", "thunar")).unwrap();
        assert_eq!((thunar.app.as_str(), thunar.instance.as_str()), ("thunar", "thunar"));
        // Run through flatpak: the program is not known, the id's last part names it.
        let obs = linux_app(entry("com.obsproject.Studio", "", "")).unwrap();
        assert_eq!((obs.app.as_str(), obs.instance.as_str()), ("Studio", "studio"));
    }
}
