//! Checks the setup assistant may ask the service to run before it answers, when the snapshot is
//! not enough. Each is read-only: it never writes to or erases a board. The two that open the
//! board's USB port (`board_serial_log`, `identify_board`), which restarts some boards, say so and
//! run only while no board is connected over Wi-Fi. The list is in the assistant's snapshot; a
//! check added here is one the assistant can use, with no change to its prompt.

use std::time::Duration;

use serde_json::{json, Value};

use crate::service::Shared;

/// Every check: id and what it finds out.
pub const ALL: &[(&str, &str)] = &[
    ("codex_agent","Checks installed native hooks and content-free event counts; file presence does not prove Codex trust or actual delivery."),
    ("powerpoint_context", "Checks the presentation panel and pointer capability without reading slide text or moving the pointer."),
    ("windows_pager", "Checks pager enablement, visible-notification watcher and recorder launcher availability without opening Steno or exposing notification text."),
    ("find_boards_on_network",
     "Looks for KnoEnd boards on the local network now (mDNS, 6 s): each board's id, address and whether it is \
      paired with this computer. Tells a board that is on Wi-Fi but not reachable from one that is not on the network."),
    ("service_log",
     "The last 300 lines the KnoEnd service printed (the snapshot has the last 60): connections, board answers and \
      refusals, Wi-Fi joins, flashing, drafts."),
    ("usb_now",
     "Lists the USB serial ports again now, with each chip, without opening any of them: after the person plugged a \
      board in or changed the cable."),
    ("settings",
     "Every setting the person can change, its meaning, choices and current value (also in the snapshot), and every paired board's own look (board_looks)."),
    ("installed_apps",
     "The apps installed on this computer, most used first, each with whether it has keys (ready: a recipe; yours; \
      drafted: by the AI; none) and minutes used: to suggest which apps to set up (the app_setup action drafts them)."),
    ("board_serial_log",
     "Reads what the board itself prints on its USB serial line for 12 s: start-up, Wi-Fi, fonts, and any crash \
      (\"Guru Meditation\", \"panic\", a backtrace) with its cause. Only runs while the board is not connected over \
      Wi-Fi (crashing, restarting or offline) and is plugged in by USB, because opening the port restarts some boards. \
      The way to find out why a board keeps restarting."),
    ("identify_board",
     "Identifies the board plugged in by USB (about 10 s; reads, never writes): its chip and flash size, the firmware \
      on it now (partition table, each app's project, version and components: a vendor's board support package and \
      display and touch drivers name the board), the boards KnoEnd lists that could match, and whether KnoEnd knows it. \
      Restarts the board, so it only runs while no board is connected over Wi-Fi. The way to tell which board a new one is."),
    ("last_terminal",
     "The last terminal app the person was in and what the board made of it: its active tab (from the window title, \
      else the one typed in last), \
      the programs in that tab's foreground, the library that matched (a program's own, such as Claude Code or the \
      pager, else the terminal's) and what the board showed (an agent's waiting request or edit mode win over it). Terminal apps only: in an editor's terminal pane (VS Code) the board keeps the editor's keys. \
      When the board does not follow a terminal: no app pid or no tab means the terminal's processes were not found; \
      a tab that is not the one in front means the person switched tabs without typing (the active tab is the one \
      typed in last)."),
];

pub fn describe() -> Value {
    Value::Array(ALL.iter().map(|(id, what)| json!({"id": id, "finds": what})).collect())
}

/// Runs the check `id`; an unknown id says so.
pub async fn run(id: &str, shared: &Shared) -> Value {
    match id {
        "codex_agent"=>crate::codex_hooks::diagnostics(),
        "powerpoint_context" => {
            let mut result = crate::powerpoint::diagnostics();
            let state = shared.status.read().await;
            result["board_touchpad_supported"] = json!(!state.boards.is_empty() && state.boards.iter().all(|b|b.touchpad));
            result
        },
        "windows_pager" => crate::pager::diagnostics(),
        "find_boards_on_network" => {
            let paired: Vec<String> = knoend_transport::pairing::Pairings::load().boards.keys().cloned().collect();
            match tokio::task::spawn_blocking(|| knoend_transport::discover::discover(Duration::from_secs(6), None)).await {
                Ok(Ok(found)) => json!({"boards": found.iter().map(|b| json!({
                    "id": b.device_id, "address": b.address.to_string(), "port": b.port, "host": b.host,
                    "paired_with_this_computer": paired.contains(&b.device_id),
                    "old_firmware_names": b.old_names })).collect::<Vec<_>>(),
                    "paired_ids": paired}),
                Ok(Err(e)) => json!({"error": e, "paired_ids": paired}),
                Err(e) => json!({"error": e.to_string()}),
            }
        }
        "service_log" => json!({"lines": crate::journal::recent(300).iter().map(|l|crate::diagnostics::redact(l)).collect::<Vec<_>>()}),
        "usb_now" => json!({"ports": tokio::task::spawn_blocking(knoend_flash::list_ports).await.unwrap_or_default()}),
        "settings" => json!({"settings": crate::settings::for_assistant(shared).await, "board_looks": crate::display::load().boards}),
        "board_serial_log" => board_serial_log(shared).await,
        "identify_board" => identify_board(shared).await,
        "installed_apps" => {
            let mut list = crate::app_setup::list().await;
            // The used ones and those with keys, then a count of the rest: enough to suggest from.
            let apps = list["apps"].as_array().cloned().unwrap_or_default();
            let (shown, rest): (Vec<Value>, Vec<Value>) = apps.into_iter().partition(|a| a["minutes"].as_f64() > Some(0.0) || a["state"] != "none");
            list["apps"] = Value::Array(shown.into_iter().take(60).collect());
            list["other_apps_without_keys"] = json!(rest.iter().take(80).map(|a| a["name"].clone()).collect::<Vec<_>>());
            list
        }
        "last_terminal" => crate::service::last_terminal(),
        _ => json!({"error": format!("no check called {id}")}),
    }
}

/// Identifies the board on USB (see ALL), as the web app's Identify step does.
async fn identify_board(shared: &Shared) -> Value {
    let Ok(_port_free) = shared.serial.try_lock() else {
        return json!({"skipped": "the USB port is busy (installing or pairing)"});
    };
    let ports = tokio::task::spawn_blocking(knoend_flash::list_ports).await.unwrap_or_default();
    let Some(port) = ports.iter().find(|p| p.bridge != "unknown").or(ports.first()).cloned() else {
        return json!({"skipped": "no board plugged in by USB"});
    };
    // Identifying restarts the board on USB: not the one connected over Wi-Fi. A native-USB board
    // gives its MAC (the board id) over USB; behind a CH340 or CP210x it cannot be told.
    let status = shared.status.read().await.clone();
    if status.link == "connected" {
        let connected = status.board.as_ref().map(|b| b.id.to_lowercase());
        if port.mac_id().is_none() || port.mac_id() == connected {
            return json!({"skipped": "the board connected over Wi-Fi may be the one on USB, and identifying would restart it. \
                                      Use the web app's Identify step, or the last identification in the snapshot"});
        }
    }
    let name = port.name.clone();
    let chip = match tokio::task::spawn_blocking(move || knoend_flash::identify(&name)).await {
        Ok(Ok(chip)) => chip,
        Ok(Err(e)) => return json!({"port": port.name, "error": e}),
        Err(e) => return json!({"error": e.to_string()}),
    };
    let candidates = crate::firmware::manifest(&shared.firmware_source).await
        .map(|m| m.candidates(&port, Some(&chip), None)).unwrap_or_default();
    say!("Assistant identified the board on {}", port.name);
    crate::boards::record(&port, Some(&chip), &candidates, false, None)
}

/// The board's own serial output for a few seconds (see ALL).
async fn board_serial_log(shared: &Shared) -> Value {
    if shared.status.read().await.link == "connected" {
        return json!({"skipped": "the board is connected over Wi-Fi and working; reading its serial line would restart it"});
    }
    let Ok(_port_free) = shared.serial.try_lock() else {
        return json!({"skipped": "the USB port is busy (installing or pairing)"});
    };
    let ports = tokio::task::spawn_blocking(knoend_flash::list_ports).await.unwrap_or_default();
    let Some(port) = ports.iter().find(|p| p.bridge != "unknown").or(ports.first()).map(|p| p.name.clone()) else {
        return json!({"skipped": "no board plugged in by USB"});
    };
    let read = tokio::task::spawn_blocking(move || {
        let mut link = knoend_transport::serial::SerialLink::open(&port)?;
        let until = std::time::Instant::now() + Duration::from_secs(12);
        let mut lines = Vec::new();
        while std::time::Instant::now() < until && lines.len() < 400 {
            if let Some(line) = link.read_line(Duration::from_millis(500)) {
                // The Wi-Fi network's name stays on the computer.
                let line = match line.find("joining \"") {
                    Some(at) => format!("{}joining \"<network>\"", &line[..at]),
                    None => line,
                };
                lines.push(line);
            }
        }
        Ok::<_, String>((port, lines))
    }).await;
    match read {
        Ok(Ok((port, lines))) => {
            say!("Assistant read the board's serial log on {port} ({} lines)", lines.len());
            // A crash and its cause, first, then the rest (the last 150 lines).
            let crash: Vec<&String> = lines.iter().filter(|l| ["Guru Meditation", "panic", "Backtrace", "abort()", "rst:", "assert"]
                .iter().any(|k| l.contains(k))).collect();
            json!({"port": port, "crash_lines": crash, "lines": lines.iter().skip(lines.len().saturating_sub(150)).collect::<Vec<_>>()})
        }
        Ok(Err(e)) => json!({"error": e}),
        Err(e) => json!({"error": e.to_string()}),
    }
}
