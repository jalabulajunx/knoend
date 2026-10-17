//! Talking to a board over USB serial: who are you (probe), and pairing + Wi-Fi provisioning
//! (spec/protocol.md section 4.1). Used by `openpager pair` and by the web app.

use std::time::{Duration, Instant};

use openpager_transport::auth;
use openpager_transport::pairing::Pairings;
use openpager_transport::serial::SerialLink;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct Provisioned {
    pub board: String,
    pub profile: String,
    pub firmware: String,
    pub ip: Option<String>,
}

fn hello(pairings: &Pairings) -> Value {
    json!({
        "deviceId": pairings.companion, "deviceName": pairings.companion_name, "deviceType": "companion",
        "protocolVersion": 0, "incoming": ["openpager.key"], "outgoing": ["openpager.layout", "openpager.status"],
    })
}

/// After opening the port: the ONX's CH340 auto-reset may have restarted the board, and a request
/// sent before its serial reader starts is lost. A restarted board announces itself; wait for that
/// (plus a moment), or for 1.5 s of quiet meaning it did not restart. Returns the announcement.
fn settle(link: &mut SerialLink) -> Option<Value> {
    let deadline = Instant::now() + Duration::from_millis(4000);
    let mut quiet_until = Instant::now() + Duration::from_millis(1500);
    while let Some(left) = deadline.min(quiet_until).checked_duration_since(Instant::now()) {
        let Some(line) = link.read_line(left) else { break };
        quiet_until = Instant::now() + Duration::from_millis(1500);
        if let Ok(packet) = serde_json::from_str::<openpager_core::protocol::Packet>(&line) {
            if packet.kind == "openpager.identity" {
                link.drain(Duration::from_millis(300));
                return Some(packet.body);
            }
        }
    }
    None
}

/// Sends a request and waits for its reply; sends it once more if the first goes unanswered.
fn ask(link: &mut SerialLink, kind: &str, body: Value, reply: &str, wait: Duration) -> Result<openpager_core::protocol::Packet, String> {
    link.request(kind, body.clone(), reply, wait).or_else(|_| link.request(kind, body, reply, wait))
}

/// Asks whatever is on `port` for its OpenPager identity. None: not OpenPager firmware (or not
/// answering), so the caller falls back to the chip's boot ROM.
pub fn probe(port: &str) -> Option<Value> {
    let mut link = SerialLink::open(port).ok()?;
    if let Some(announced) = settle(&mut link) {
        return Some(announced);
    }
    ask(&mut link, "openpager.identity", hello(&Pairings::load()), "openpager.identity", Duration::from_secs(2))
        .ok().filter(|p| p.kind == "openpager.identity").map(|p| p.body)
}

/// Pairs the board on `port` with this computer (fresh random key), and if `wifi` is given, sends
/// the network name and password and waits for the board to join. `log` gets progress lines;
/// neither the key nor the password is ever passed to it.
pub fn provision(port: &str, wifi: Option<(&str, &str)>, log: &mut dyn FnMut(String)) -> Result<Provisioned, String> {
    let mut pairings = Pairings::load();
    let mut link = SerialLink::open(port)?;
    settle(&mut link);

    let identity = ask(&mut link, "openpager.identity", hello(&pairings), "openpager.identity", Duration::from_secs(3))?;
    let board = identity.body["deviceId"].as_str().ok_or("board sent no deviceId")?.to_string();
    let profile = identity.body["board"]["profile"].as_str().unwrap_or("?").to_string();
    let firmware = identity.body["firmware"].as_str().unwrap_or("?").to_string();
    log(format!("Board {board} ({profile}, firmware {firmware}) answered"));

    let key = auth::new_key();
    let key_hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    let ack = link.request("openpager.pair", json!({
        "companion": pairings.companion, "companionName": pairings.companion_name, "key": key_hex,
    }), "openpager.pair.ack", Duration::from_secs(3))?;
    if ack.body["ok"] != true {
        return Err(format!("pairing refused: {}", ack.body));
    }
    pairings.boards.insert(board.clone(), key_hex);
    pairings.save().map_err(|e| format!("could not save the key: {e}"))?;
    log("Paired: the board and this computer now share a key".into());

    let Some((ssid, password)) = wifi else {
        return Ok(Provisioned { board, profile, firmware, ip: None });
    };
    let ack = link.request("openpager.wifi", json!({"ssid": ssid, "password": password}),
                           "openpager.wifi.ack", Duration::from_secs(3))?;
    if ack.body["ok"] != true {
        return Err(format!("Wi-Fi settings refused: {}", ack.body["error"]));
    }
    log(format!("Wi-Fi settings stored on the board; joining \"{ssid}\""));

    // A board that already had Wi-Fi only reports its address again (its services are already up),
    // so an address counts as success after a short grace for the "services up" line.
    let mut ip = None;
    let mut deadline = Instant::now() + Duration::from_secs(30);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Some(line) = link.read_line(left) else { break };
        if let Some(address) = line.strip_prefix("WIFI ip=") {
            ip = Some(address.trim().to_string());
            log(format!("Joined the network as {}", address.trim()));
            deadline = Instant::now() + Duration::from_secs(3);
        } else if line.starts_with("WIFI disconnected") {
            log(format!("Board: {line}"));
        } else if line.starts_with("WIFI services up") {
            log("Board is reachable on Wi-Fi".into());
            return Ok(Provisioned { board, profile, firmware, ip });
        }
    }
    if ip.is_some() {
        return Ok(Provisioned { board, profile, firmware, ip });
    }
    Err("the board did not join Wi-Fi within 30 s (check the network name and password; 2.4 GHz only)".into())
}

/// Networks the board on `port` can hear (it scans; 2.4 GHz only). Over serial only.
pub fn scan_wifi(port: &str) -> Result<Value, String> {
    let mut link = SerialLink::open(port)?;
    settle(&mut link);
    let reply = ask(&mut link, "openpager.wifi.scan", json!({}), "openpager.wifi.networks", Duration::from_secs(8))?;
    if reply.kind == "openpager.error" {
        return Err(reply.body["message"].as_str().unwrap_or("the board refused the scan").to_string());
    }
    Ok(reply.body)
}
