//! App icons on macOS for the board's launcher: the app bundle found by its bundle id (Spotlight,
//! then the usual Applications folders by name), its `.icns` named in Info.plist, converted to PNG
//! by the system's own `sips`. Nothing to install.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// A bundle id safe to put in a Spotlight query ("com.anthropic.claudefordesktop").
pub fn valid_bundle_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 255 && id.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c))
}

/// The icon file an Info.plist value names: "AppIcon" and "AppIcon.icns" are both
/// "AppIcon.icns".
pub fn icns_name(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.contains('/') {
        return None;
    }
    Some(if value.ends_with(".icns") { value.to_string() } else { format!("{value}.icns") })
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The app bundle: Spotlight by bundle id, else `<name>.app` in the Applications folders.
fn bundle_path(bundle_id: &str, name: &str) -> Option<PathBuf> {
    if valid_bundle_id(bundle_id) {
        let query = format!("kMDItemCFBundleIdentifier == \"{bundle_id}\"");
        if let Some(found) = output("mdfind", &[&query]).and_then(|o| o.lines().find(|l| l.ends_with(".app")).map(PathBuf::from)) {
            return Some(found);
        }
    }
    if name.is_empty() || name.contains('/') {
        return None;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    [PathBuf::from("/Applications"), PathBuf::from("/System/Applications"),
     PathBuf::from("/System/Applications/Utilities"), home.join("Applications")]
        .into_iter().map(|dir| dir.join(format!("{name}.app"))).find(|p| p.exists())
}

/// A background program rather than an app the person opens: one of macOS's own under
/// /System/Library (loginwindow, Spotlight, Control Center; Finder is not), or one marked as having
/// no Dock icon (LSUIElement, LSBackgroundOnly). One not found is an app. Remembered per bundle id.
pub fn background(bundle_id: &str) -> bool {
    static CACHE: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);
    if let Some(hit) = CACHE.lock().ok().and_then(|mut c| c.get_or_insert_with(HashMap::new).get(bundle_id).copied()) {
        return hit;
    }
    let found = bundle_path(bundle_id, "").is_some_and(|bundle| {
        let plist = bundle.join("Contents/Info.plist");
        let flag = |key: &str| plist.to_str().and_then(|p| output("plutil", &["-extract", key, "raw", "-o", "-", p]))
            .is_some_and(|v| v == "true" || v == "1");
        (bundle.starts_with("/System/Library") && bundle_id != "com.apple.finder") || flag("LSUIElement") || flag("LSBackgroundOnly")
    });
    if let Ok(mut cache) = CACHE.lock() {
        cache.get_or_insert_with(HashMap::new).insert(bundle_id.to_string(), found);
    }
    found
}

/// The bundle's `.icns`: CFBundleIconFile, else CFBundleIconName, else AppIcon.icns, else any.
fn icns_file(bundle: &Path) -> Option<PathBuf> {
    let plist = bundle.join("Contents/Info.plist");
    let resources = bundle.join("Contents/Resources");
    let plist = plist.to_str()?;
    for key in ["CFBundleIconFile", "CFBundleIconName"] {
        if let Some(file) = output("plutil", &["-extract", key, "raw", "-o", "-", plist]).and_then(|v| icns_name(&v)) {
            let path = resources.join(file);
            if path.exists() {
                return Some(path);
            }
        }
    }
    let app_icon = resources.join("AppIcon.icns");
    if app_icon.exists() {
        return Some(app_icon);
    }
    std::fs::read_dir(&resources).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "icns"))
}

/// RGBA pixels (`size` x `size`) of an app's icon, remembered for the life of the service
/// (including "none", so a missing icon is not searched for again).
pub fn icon(bundle_id: &str, name: &str, size: u32) -> Option<Vec<u8>> {
    static CACHE: Mutex<Option<HashMap<String, Option<Vec<u8>>>>> = Mutex::new(None);
    let key = format!("{bundle_id}|{name}|{size}");
    if let Some(hit) = CACHE.lock().ok()?.get_or_insert_with(HashMap::new).get(&key) {
        return hit.clone();
    }
    let pixels = (|| {
        let icns = icns_file(&bundle_path(bundle_id, name)?)?;
        let out = std::env::temp_dir().join(format!("knoend-icon-{}-{}.png", std::process::id(),
                                                    bundle_id.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>()));
        // Twice the size: sips picks the nearest representation, the resize to `size` is ours.
        let ok = output("sips", &["-s", "format", "png", "-Z", &(size * 2).to_string(), icns.to_str()?, "--out", out.to_str()?]).is_some();
        let pixels = if ok { crate::picture::render(&out, size) } else { None };
        let _ = std::fs::remove_file(&out);
        pixels
    })();
    if let Ok(mut cache) = CACHE.lock() {
        cache.get_or_insert_with(HashMap::new).insert(key, pixels.clone());
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_ids_and_icon_names() {
        assert!(valid_bundle_id("com.anthropic.claudefordesktop"));
        assert!(!valid_bundle_id("x\" || kMDItemFSName == \"*"));
        assert!(!valid_bundle_id(""));
        assert_eq!(icns_name("AppIcon").as_deref(), Some("AppIcon.icns"));
        assert_eq!(icns_name("electron.icns\n").as_deref(), Some("electron.icns"));
        assert_eq!(icns_name("../x"), None);
    }
}
