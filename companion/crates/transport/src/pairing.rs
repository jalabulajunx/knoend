//! This companion's identity and the keys it shares with boards, in
//! `$XDG_CONFIG_HOME/openpager/companion.json` (mode 600). An OS keyring is the planned home.

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

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("openpager")
}

fn path() -> PathBuf {
    config_dir().join("companion.json")
}

impl Pairings {
    pub fn load() -> Pairings {
        fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| {
            let host = fs::read_to_string("/etc/hostname").map(|h| h.trim().to_lowercase()).unwrap_or_default();
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
}
