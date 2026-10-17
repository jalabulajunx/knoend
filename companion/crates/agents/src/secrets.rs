//! API keys entered in the web app, kept in `$XDG_CONFIG_HOME/openpager/secrets.json` (mode 600).
//! An environment variable with the same name always wins. Keys are never logged or sent anywhere
//! but to their own provider.

use std::collections::BTreeMap;
use std::path::PathBuf;

fn path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("openpager").join("secrets.json")
}

fn load() -> BTreeMap<String, String> {
    std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// The saved value for `name` (e.g. "GEMINI_API_KEY"), if any.
pub fn get(name: &str) -> Option<String> {
    load().remove(name).filter(|v| !v.is_empty())
}

pub fn save(name: &str, value: &str) -> std::io::Result<()> {
    let mut all = load();
    all.insert(name.to_string(), value.to_string());
    let text = serde_json::to_string_pretty(&all).expect("serialisable");
    let file = path();
    std::fs::create_dir_all(file.parent().expect("has a parent"))?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&file)?;
        f.write_all(text.as_bytes())?;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&file, text)
    }
}
