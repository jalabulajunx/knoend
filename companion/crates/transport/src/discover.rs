//! Finding boards on the local network: mDNS service `_openpager._tcp` with TXT `id` and `pv`.

use std::net::IpAddr;
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};

pub const SERVICE: &str = "_openpager._tcp.local.";

#[derive(Debug, Clone)]
pub struct FoundBoard {
    pub device_id: String,
    pub protocol_version: String,
    pub address: IpAddr,
    pub port: u16,
    pub host: String,
}

/// Browses for up to `timeout`, returning early once `wanted` (if given) has been seen.
pub fn discover(timeout: Duration, wanted: Option<&str>) -> Result<Vec<FoundBoard>, String> {
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let events = daemon.browse(SERVICE).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + timeout;
    let mut found: Vec<FoundBoard> = Vec::new();

    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else { break };
        if let ServiceEvent::ServiceResolved(info) = event {
            let Some(device_id) = info.get_property_val_str("id").map(str::to_string) else { continue };
            let Some(address) = info.get_addresses_v4().into_iter().next().map(IpAddr::V4) else { continue };
            found.retain(|b| b.device_id != device_id);
            found.push(FoundBoard {
                protocol_version: info.get_property_val_str("pv").unwrap_or("").to_string(),
                device_id: device_id.clone(),
                address,
                port: info.get_port(),
                host: info.get_hostname().to_string(),
            });
            if wanted == Some(device_id.as_str()) {
                break;
            }
        }
    }
    let _ = daemon.shutdown();
    Ok(found)
}
