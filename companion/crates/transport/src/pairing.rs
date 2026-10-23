//! This companion's identity and the keys it shares with boards, in
//! `$XDG_CONFIG_HOME/knoend/companion.json` (mode 600). An OS keyring is the planned home.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::auth::Key;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pairings {
    pub companion: String,
    #[serde(rename = "companionName")]
    pub companion_name: String,
    /// deviceId -> key as hex
    pub boards: BTreeMap<String, String>,
}

fn config_root() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
        PathBuf::from(home).join(".config")
    })
}

pub fn config_dir() -> PathBuf {
    config_root().join("knoend")
}

/// Moves the settings folder from its name before the rename (`openpager`) to `knoend`, once, so
/// pairings, API keys, libraries and drafts carry over. Leaves both alone when `knoend` already
/// exists. Returns a line to print when it moved something.
pub fn move_old_settings() -> Option<String> {
    let (old, new) = (config_root().join("openpager"), config_dir());
    if !old.is_dir() || new.exists() {
        return None;
    }
    fs::rename(&old, &new).ok().map(|_| format!("Settings moved from {} to {} (the project is now KnoEnd)", old.display(), new.display()))
}

fn path() -> PathBuf {
    config_dir().join("companion.json")
}

impl Pairings {
    pub fn load() -> Pairings {
        fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| {
            let host = fs::read_to_string("/etc/hostname").ok()
                .or_else(|| std::env::var("COMPUTERNAME").ok())
                .or_else(|| std::env::var("HOSTNAME").ok())
                .map(|h| h.trim().to_lowercase())
                .unwrap_or_default();
            Pairings {
                companion: format!("companion-{}", &crate::auth::nonce()[..6]),
                companion_name: if host.is_empty() { "Computer".into() } else { host },
                boards: BTreeMap::new(),
            }
        })
    }

    pub fn save(&self) -> std::io::Result<()> {
        fs::create_dir_all(config_dir())?;
        let text = serde_json::to_string_pretty(self).expect("serialisable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path())?;
            file.write_all(text.as_bytes())?;
            fs::set_permissions(path(), fs::Permissions::from_mode(0o600))
        }
        #[cfg(not(unix))]
        {
            fs::File::create(path())?.write_all(text.as_bytes())
        }
    }

    pub fn key_for(&self, board: &str) -> Option<Key> {
        self.boards.get(board).and_then(|h| hex::decode(h).ok()).and_then(|b| b.try_into().ok())
    }

    /// Removes a board from pairings (`companion.json`).
    pub fn forget(&mut self, board: &str) -> bool {
        self.boards.remove(board).is_some()
    }
}
