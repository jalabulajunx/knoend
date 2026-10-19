//! Finding boards on the local network: mDNS service `_knoend._tcp` with TXT `id` and `pv`. Boards
//! from before the rename advertise only `_openpager._tcp`; current ones advertise both.

use std::net::IpAddr;
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};

pub const SERVICE: &str = "_knoend._tcp.local.";
/// The service name from before the rename.
pub const OLD_SERVICE: &str = "_openpager._tcp.local.";

#[derive(Debug, Clone)]
pub struct FoundBoard {
    pub device_id: String,
    pub protocol_version: String,
    pub address: IpAddr,
    pub port: u16,
    pub host: String,
    /// Seen only under the old service name: firmware from before the rename, which understands
    /// only `openpager.*` messages.
    pub old_names: bool,
}

/// Browses for up to `timeout`, returning early once `wanted` (if given) has been seen.
pub fn discover(timeout: Duration, wanted: Option<&str>) -> Result<Vec<FoundBoard>, String> {
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let current = daemon.browse(SERVICE).map_err(|e| e.to_string())?;
    let old = daemon.browse(OLD_SERVICE).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + timeout;
    let mut found: Vec<FoundBoard> = Vec::new();

    'browse: while Instant::now() < deadline {
        for (events, old_names) in [(&current, false), (&old, true)] {
            let Ok(event) = events.recv_timeout(Duration::from_millis(50)) else { continue };
            let ServiceEvent::ServiceResolved(info) = event else { continue };
            let Some(device_id) = info.get_property_val_str("id").map(str::to_string) else { continue };
            let Some(address) = info.get_addresses_v4().into_iter().next().map(IpAddr::V4) else { continue };
            // A board seen under the current name speaks it, whatever else it advertises.
            let old_names = old_names && !found.iter().any(|b| b.device_id == device_id && !b.old_names);
            found.retain(|b| b.device_id != device_id);
            found.push(FoundBoard {
                protocol_version: info.get_property_val_str("pv").unwrap_or("").to_string(),
                device_id: device_id.clone(),
                address,
                port: info.get_port(),
                host: info.get_hostname().to_string(),
                old_names,
            });
            // Stop early only once the board is known by its current name (or time is nearly up).
            if wanted == Some(device_id.as_str()) && !old_names {
                break 'browse;
            }
        }
    }
    let _ = daemon.shutdown();
    Ok(found)
}
