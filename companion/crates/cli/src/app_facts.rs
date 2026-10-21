//! What an app's own files say about it, for the agent that drafts its keys: two products can share
//! a name (Antigravity the IDE and Antigravity the desktop app), and the window class alone does not
//! tell them apart. Linux: the focused window's program, and for Electron apps the name, version and
//! description in its package. Only those facts leave the computer, never paths.

use knoend_core::focus::Focus;
use serde_json::Value;

/// Facts about the app in `focus`, one per line; empty when nothing could be read.
pub fn about(focus: &Focus) -> Vec<String> {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        let Some(pid) = knoend_focus::window_pid(&[focus.app.as_str(), focus.instance.as_str()]) else { return Vec::new() };
        #[cfg(target_os = "linux")]
        let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else { return Vec::new() };
        #[cfg(target_os = "windows")]
        let Some(exe) = knoend_focus::process_exe(pid) else { return Vec::new() };
        let mut facts = Vec::new();
        if let Some(name) = exe.file_name().and_then(|n| n.to_str()) {
            facts.push(format!("Program file: {name}"));
        }
        if let Some(described) = exe.parent().and_then(|dir| electron_package(&dir.join("resources"))) {
            facts.push(described);
        }
        facts
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = focus;
        Vec::new()
    }
}

/// "Antigravity 2.23.1: Antigravity - Agentic Desktop Application" from an Electron app's package
/// (`resources/app.asar`, or `resources/app/` for VS Code-style apps, whose product.json names them).
#[cfg_attr(not(any(target_os = "linux", target_os = "windows")), allow(dead_code))]
fn electron_package(resources: &std::path::Path) -> Option<String> {
    let app = resources.join("app");
    let product = std::fs::read_to_string(app.join("product.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let package = std::fs::read_to_string(app.join("package.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .or_else(|| asar_file(&resources.join("app.asar"), "package.json").and_then(|b| serde_json::from_slice(&b).ok()));
    let pick = |keys: &[&str]| keys.iter().find_map(|k| product.as_ref().and_then(|p| p[*k].as_str())
        .or_else(|| package.as_ref().and_then(|p| p[*k].as_str())).map(str::to_string));
    let name = pick(&["nameLong", "productName", "name"])?;
    let version = pick(&["version"]).unwrap_or_default();
    let description = pick(&["description"]).filter(|d| !d.is_empty());
    let mut line = format!("Package: {name} {version}").trim_end().to_string();
    if let Some(d) = description {
        line += &format!(": {}", d.chars().take(160).collect::<String>());
    }
    Some(line)
}

/// One file at the top of an asar archive (Electron's: a size-prefixed JSON index, then the files).
#[cfg_attr(not(any(target_os = "linux", target_os = "windows")), allow(dead_code))]
fn asar_file(path: &std::path::Path, name: &str) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 16];
    file.read_exact(&mut head).ok()?;
    let word = |i: usize| u32::from_le_bytes(head[i..i + 4].try_into().unwrap()) as u64;
    let (index_size, json_len) = (word(4), word(12));
    if json_len > 16 << 20 {
        return None;
    }
    let mut json = vec![0; json_len as usize];
    file.read_exact(&mut json).ok()?;
    let index: Value = serde_json::from_slice(&json).ok()?;
    let entry = &index["files"][name];
    let offset: u64 = entry["offset"].as_str()?.parse().ok()?;
    let size = entry["size"].as_u64().filter(|s| *s <= 1 << 20)?;
    file.seek(SeekFrom::Start(8 + index_size + offset)).ok()?;
    let mut data = vec![0; size as usize];
    file.read_exact(&mut data).ok()?;
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vscode_style_and_asar_packages_are_described() {
        let dir = std::env::temp_dir().join(format!("knoend-facts-{}", std::process::id()));
        let app = dir.join("ide/resources/app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("product.json"), r#"{"nameLong": "Antigravity IDE", "version": "1.107.0"}"#).unwrap();
        std::fs::write(app.join("package.json"), r#"{"name": "antigravity-ide", "version": "1.107.0", "description": ""}"#).unwrap();
        assert_eq!(electron_package(&dir.join("ide/resources")).as_deref(), Some("Package: Antigravity IDE 1.107.0"));

        // A minimal asar: index JSON at 16, files after 8 + index size.
        let package = br#"{"productName": "Antigravity", "version": "2.23.1", "description": "Antigravity - Agentic Desktop Application"}"#;
        let index = format!(r#"{{"files":{{"package.json":{{"size":{},"offset":"0"}}}}}}"#, package.len());
        let mut asar = Vec::new();
        let index_size = (8 + index.len()) as u32;
        for word in [4u32, index_size, index_size - 4, index.len() as u32] {
            asar.extend(word.to_le_bytes());
        }
        asar.extend(index.as_bytes());
        asar.extend(package);
        let resources = dir.join("app2/resources");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(resources.join("app.asar"), asar).unwrap();
        assert_eq!(electron_package(&resources).as_deref(), Some("Package: Antigravity 2.23.1: Antigravity - Agentic Desktop Application"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
