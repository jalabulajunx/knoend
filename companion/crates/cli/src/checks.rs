//! Checks the setup assistant may ask the service to run before it answers, when the snapshot is
//! not enough. Each is read-only: it never resets, writes to or erases a board, and never opens
//! its serial port (opening it restarts some boards). The list is in the assistant's snapshot; a
//! check added here is one the assistant can use, with no change to its prompt.

use std::time::Duration;

use serde_json::{json, Value};

use crate::service::Shared;

/// Every check: id and what it finds out.
pub const ALL: &[(&str, &str)] = &[
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
     "Every setting the person can change, its meaning, choices and current value (also in the snapshot)."),
    ("board_serial_log",
     "Reads what the board itself prints on its USB serial line for 12 s: start-up, Wi-Fi, fonts, and any crash \
      (\"Guru Meditation\", \"panic\", a backtrace) with its cause. Only runs while the board is not connected over \
      Wi-Fi (crashing, restarting or offline) and is plugged in by USB, because opening the port restarts some boards. \
      The way to find out why a board keeps restarting."),
];

pub fn describe() -> Value {
    Value::Array(ALL.iter().map(|(id, what)| json!({"id": id, "finds": what})).collect())
}

/// Runs the check `id`; an unknown id says so.
pub async fn run(id: &str, shared: &Shared) -> Value {
    match id {
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
        "service_log" => json!({"lines": crate::journal::recent(300)}),
        "usb_now" => json!({"ports": tokio::task::spawn_blocking(knoend_flash::list_ports).await.unwrap_or_default()}),
        "settings" => json!({"settings": crate::settings::all(shared).await}),
        "board_serial_log" => board_serial_log(shared).await,
        _ => json!({"error": format!("no check called {id}")}),
    }
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
