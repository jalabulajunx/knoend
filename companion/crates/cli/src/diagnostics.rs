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
fn linux_checks(ports: &[knoend_flash::Port]) -> Value {
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
    // USB-serial chips with no port: no driver (ch341 missing) or one that is not a serial driver
    // (brltty takes CH340s through "usbfs" on some distributions).
    let brltty = ["/usr/bin/brltty", "/bin/brltty", "/usr/sbin/brltty"].iter().any(|p| std::path::Path::new(p).exists());
    json!({"bluetooth_radios": bluetooth, "serial_access": access, "user_groups": group_names,
           "usb_serial_devices": knoend_flash::usb_serial_devices(ports),
           "brltty_installed": brltty, "desktop_session": desktop_session()})
}

/// The graphical session and what KnoEnd can do in it. Following the focused app and typing
/// keys use X11 (EWMH, XTest); a Wayland session hides native Wayland windows from both, so there
/// the layout does not follow apps and the service cannot type (the board's Bluetooth keyboard
/// still can). Wayland support is planned, not built.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn desktop_session() -> Value {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    session_support(&var("XDG_SESSION_TYPE"), &var("XDG_CURRENT_DESKTOP"), !var("WAYLAND_DISPLAY").is_empty(), !var("DISPLAY").is_empty())
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn session_support(session_type: &str, desktop: &str, wayland_display: bool, x_display: bool) -> Value {
    let wayland = session_type.eq_ignore_ascii_case("wayland") || (session_type.is_empty() && wayland_display);
    let follows = !wayland && x_display;
    let note = if wayland {
        "Wayland session: keys do not follow the focused app and the service cannot type keys here yet (X11 and macOS only). \
         Bluetooth typing from the board works. An X11 (Xorg) session at login makes everything work."
    } else if !x_display {
        "No X display reachable from the service (DISPLAY is empty): keys cannot follow apps until it is set."
    } else {
        ""
    };
    json!({"type": if session_type.is_empty() { "unknown" } else { session_type }, "desktop": desktop,
           "keys_follow_apps": follows, "service_can_type": follows, "note": note})
}

/// macOS: what stops a board being found or keys being typed, none of it visible from the port
/// list alone (all confirmed on macOS 26.6, 2026-10-17).
#[cfg(target_os = "macos")]
fn macos_checks(ports: &[knoend_flash::Port]) -> Value {
    json!({
        // A USB-serial chip with no serial port means no driver claimed it: Apple's CH34x driver
        // covers 1a86:7523 and 1a86:55d4 but not the CH340K's 1a86:7522 (ONX2432G028).
        "usb_serial_devices": knoend_flash::usb_serial_devices(ports),
        "wch_driver_extension": wch_driver_state(),
        "accessibility": {
            "can_type_keys": knoend_input::accessibility_trusted(),
            "app_to_allow": knoend_input::launching_app(),
        },
    })
}

/// WCH's own driver (needed for 1a86:7522): "not installed", or its state as macOS reports it
/// ("activated waiting for user", "activated enabled").
#[cfg(target_os = "macos")]
pub fn wch_driver_state() -> String {
    std::process::Command::new("systemextensionsctl").arg("list").output().ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().find(|l| l.contains("cn.wch.")).map(str::to_string))
        .map(|line| line.rsplit('[').next().unwrap_or("").trim_end_matches(']').to_string())
        .unwrap_or_else(|| "not installed".into())
}

fn system_checks(ports: &[knoend_flash::Port]) -> Value {
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
    let ports = tokio::task::spawn_blocking(knoend_flash::list_ports).await.unwrap_or_default();
    let agent = crate::agent_config().ok();
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "knoend_version": env!("CARGO_PKG_VERSION"),
        "service_status": status,
        "usb_serial_ports": ports,
        "system_checks": system_checks(&ports),
        "recent_log": shared.recent().iter().map(|l| redact(l)).collect::<Vec<_>>(),
        "firmware_source": shared.firmware_source,
        "browser_extension_connected": shared.browser.connected(),
        "ai_provider": agent.map(|a| a.describe()),
        "ai_tools_on_this_computer": knoend_agents::cli_tool::tools_on_this_computer().await,
        // What the person can change (a fix may set any of them) and what the assistant may check.
        "settings": crate::settings::all(shared).await,
        "checks_available": crate::checks::describe(),
        "from_the_web_app": ui_context,
    })
}

#[cfg(test)]
mod tests {
    use super::{redact, session_support};

    #[test]
    fn wayland_sessions_are_reported_plainly() {
        let gnome = session_support("wayland", "ubuntu:GNOME", true, true);
        assert_eq!(gnome["keys_follow_apps"], false, "XWayland's DISPLAY does not make focus work");
        assert!(gnome["note"].as_str().unwrap().contains("do not follow"));
        let i3 = session_support("x11", "i3", false, true);
        assert_eq!((i3["keys_follow_apps"].as_bool(), i3["note"].as_str()), (Some(true), Some("")));
        assert_eq!(session_support("", "", true, false)["keys_follow_apps"], false);
    }

    #[test]
    fn network_names_are_redacted() {
        assert_eq!(redact("Wi-Fi settings stored on the board; joining \"Home 5G\" now"),
                   "Wi-Fi settings stored on the board; joining \"<network>\" now");
        assert_eq!(redact("Key 1 on builtin.vscode"), "Key 1 on builtin.vscode");
    }
}
