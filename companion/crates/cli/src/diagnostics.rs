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

/// macOS: what stops a board being found or keys being typed, none of it visible from the port
/// list alone (all confirmed on macOS 26.6, 2026-10-17).
#[cfg(target_os = "macos")]
fn macos_checks(ports: &[openpager_flash::Port]) -> Value {
    // A USB-serial chip with no serial port means no driver claimed it: Apple's CH34x driver
    // covers 1a86:7523 and 1a86:55d4 but not the CH340K's 1a86:7522 (ONX2432G028).
    let usb: Vec<Value> = usb_devices().into_iter()
        .filter(|(vid, pid, _)| openpager_flash::bridge(*vid, *pid) != "unknown")
        .map(|(vid, pid, name)| json!({
            "usb_id": format!("{vid:04x}:{pid:04x}"),
            "name": name,
            "bridge": openpager_flash::bridge(vid, pid),
            "has_serial_port": ports.iter().any(|p| p.vid == vid && p.pid == pid),
        }))
        .collect();
    // WCH's own driver (needed for 1a86:7522): installed, then approved by the person.
    let wch = std::process::Command::new("systemextensionsctl").arg("list").output().ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().find(|l| l.contains("cn.wch.")).map(str::to_string))
        .map(|line| line.rsplit('[').next().unwrap_or("").trim_end_matches(']').to_string())
        .unwrap_or_else(|| "not installed".into());
    json!({
        "usb_serial_devices": usb,
        "wch_driver_extension": wch,
        "accessibility": {
            "can_type_keys": openpager_input::accessibility_trusted(),
            "app_to_allow": openpager_input::launching_app(),
        },
    })
}

/// (vendor id, product id, name) of every USB device, from System Information.
#[cfg(target_os = "macos")]
fn usb_devices() -> Vec<(u16, u16, String)> {
    fn walk(node: &Value, out: &mut Vec<(u16, u16, String)>) {
        match node {
            Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            Value::Object(map) => {
                let hex = |key: &str| map.get(key).and_then(Value::as_str)
                    .and_then(|s| u16::from_str_radix(s.trim_start_matches("0x").split_whitespace().next()?, 16).ok());
                if let (Some(vid), Some(pid)) = (hex("USBDeviceKeyVendorID"), hex("USBDeviceKeyProductID")) {
                    out.push((vid, pid, map.get("_name").and_then(Value::as_str).unwrap_or("").to_string()));
                }
                map.values().for_each(|v| walk(v, out));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Ok(o) = std::process::Command::new("system_profiler").args(["SPUSBHostDataType", "-json"]).output() {
        if let Ok(value) = serde_json::from_slice::<Value>(&o.stdout) {
            walk(&value, &mut out);
        }
    }
    out
}

fn system_checks(ports: &[openpager_flash::Port]) -> Value {
    #[cfg(target_os = "linux")]
    return linux_checks(ports);
    #[cfg(target_os = "macos")]
    return macos_checks(ports);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = ports;
        Value::Null
    }
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
        "system_checks": system_checks(&ports),
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
