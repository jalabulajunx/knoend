//! The snapshot the setup assistant sees: facts gathered without touching the board. Never
//! passwords, API keys or window titles; the Wi-Fi network name is redacted from log lines.

use serde_json::{json, Value};

use crate::service::Shared;

/// Recent log lines with anything quoted after "joining" (the Wi-Fi network name) replaced.
fn redact(line: &str) -> String {
    match line.find("joining \"") {
        Some(start) => {
            let rest = &line[start + 9..];
            match rest.find('"') {
                Some(end) => format!("{}joining \"<network>\"{}", &line[..start], &rest[end + 1..]),
                None => line.to_string(),
            }
        }
        None => line.to_string(),
    }
}

#[cfg(target_os = "linux")]
fn linux_checks(ports: &[openpager_flash::Port]) -> Value {
    use std::os::unix::fs::MetadataExt;

    // Bluetooth radios that are soft-blocked (rfkill).
    let mut bluetooth = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/rfkill") {
        for entry in entries.flatten() {
            let read = |name: &str| std::fs::read_to_string(entry.path().join(name)).unwrap_or_default().trim().to_string();
            if read("type") == "bluetooth" {
                bluetooth.push(json!({"soft_blocked": read("soft") == "1", "hard_blocked": read("hard") == "1"}));
            }
        }
    }
    // Can this user open each serial port? (From file permissions; the port is not opened.)
    let id = |flag: &str| std::process::Command::new("id").arg(flag).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().filter_map(|g| g.parse::<u32>().ok()).collect::<Vec<_>>())
        .unwrap_or_default();
    let (uids, gids) = (id("-u"), id("-G"));
    let group_names = std::process::Command::new("id").arg("-Gn").output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let access: Vec<Value> = ports.iter().map(|p| match std::fs::metadata(&p.name) {
        Ok(m) => {
            let mode = m.mode();
            let writable = (uids.contains(&m.uid()) && mode & 0o200 != 0)
                || (gids.contains(&m.gid()) && mode & 0o020 != 0)
                || mode & 0o002 != 0;
            json!({"port": p.name, "user_can_open": writable, "owner_gid": m.gid()})
        }
        Err(e) => json!({"port": p.name, "error": e.to_string()}),
    }).collect();
    json!({"bluetooth_radios": bluetooth, "serial_access": access, "user_groups": group_names})
}

#[cfg(not(target_os = "linux"))]
fn linux_checks(_ports: &[openpager_flash::Port]) -> Value {
    Value::Null
}

pub async fn snapshot(shared: &Shared, ui_context: Value) -> Value {
    let status = serde_json::to_value(shared.status.read().await.clone()).unwrap_or_default();
    let ports = tokio::task::spawn_blocking(openpager_flash::list_ports).await.unwrap_or_default();
    let agent = crate::agent_config().ok();
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "openpager_version": env!("CARGO_PKG_VERSION"),
        "service_status": status,
        "usb_serial_ports": ports,
        "system_checks": linux_checks(&ports),
        "recent_log": shared.recent().iter().map(|l| redact(l)).collect::<Vec<_>>(),
        "firmware_source": shared.firmware_source,
        "ai_provider": agent.map(|a| a.describe()),
        "from_the_web_app": ui_context,
    })
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn network_names_are_redacted() {
        assert_eq!(redact("Wi-Fi settings stored on the board; joining \"Home 5G\" now"),
                   "Wi-Fi settings stored on the board; joining \"<network>\" now");
        assert_eq!(redact("Key 1 on builtin.vscode"), "Key 1 on builtin.vscode");
    }
}
