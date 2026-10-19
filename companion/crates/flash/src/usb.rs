//! USB-serial chips on the bus, whether or not a driver gave them a serial port. A board can be
//! plugged in and powered with no port at all (macOS without WCH's driver for the CH340K; Linux
//! with `brltty` holding a CH340, or no `ch341` module), and the port list alone then says only
//! "no board". This tells the wizard and the setup assistant what the computer itself sees.

use serde::Serialize;

use crate::{bridge, Port};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsbDevice {
    /// "vvvv:pppp"
    pub usb_id: String,
    pub vid: u16,
    pub pid: u16,
    pub name: String,
    /// "ch340", "cp210x", "espressif-native", ...
    pub bridge: String,
    pub has_serial_port: bool,
    /// Linux: the kernel drivers bound to its interfaces ("ch341", "cdc_acm", "usbfs", ...);
    /// empty when none is. Not reported on macOS.
    pub drivers: Vec<String>,
}

/// Every USB device whose IDs belong to a USB-serial bridge a board may use.
pub fn usb_serial_devices(ports: &[Port]) -> Vec<UsbDevice> {
    raw_devices().into_iter()
        .filter(|(vid, pid, _, _)| bridge(*vid, *pid) != "unknown")
        .map(|(vid, pid, name, drivers)| UsbDevice {
            usb_id: format!("{vid:04x}:{pid:04x}"),
            vid,
            pid,
            name,
            bridge: bridge(vid, pid).to_string(),
            has_serial_port: ports.iter().any(|p| p.vid == vid && p.pid == pid),
            drivers,
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn raw_devices() -> Vec<(u16, u16, String, Vec<String>)> {
    from_sysfs(std::path::Path::new("/sys/bus/usb/devices"))
}

/// Reads devices from a sysfs `usb/devices` folder (a parameter so it can be tested).
#[cfg(any(target_os = "linux", test))]
fn from_sysfs(root: &std::path::Path) -> Vec<(u16, u16, String, Vec<String>)> {
    let read = |path: std::path::PathBuf| std::fs::read_to_string(path).ok().map(|s| s.trim().to_string());
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return out };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in &entries {
        let dir = entry.path();
        let (Some(vid), Some(pid)) = (read(dir.join("idVendor")), read(dir.join("idProduct"))) else { continue };
        let (Ok(vid), Ok(pid)) = (u16::from_str_radix(&vid, 16), u16::from_str_radix(&pid, 16)) else { continue };
        let name = read(dir.join("product")).unwrap_or_default();
        // Interfaces are "<device>:<config>.<n>" entries next to the device.
        let device = entry.file_name().to_string_lossy().to_string();
        let mut drivers: Vec<String> = entries.iter()
            .filter(|i| i.file_name().to_string_lossy().starts_with(&format!("{device}:")))
            .filter_map(|i| std::fs::read_link(i.path().join("driver")).ok())
            .filter_map(|link| link.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        drivers.sort();
        drivers.dedup();
        out.push((vid, pid, name, drivers));
    }
    out
}

/// From System Information (`system_profiler SPUSBHostDataType -json`).
#[cfg(target_os = "macos")]
fn raw_devices() -> Vec<(u16, u16, String, Vec<String>)> {
    use serde_json::Value;
    fn walk(node: &Value, out: &mut Vec<(u16, u16, String, Vec<String>)>) {
        match node {
            Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            Value::Object(map) => {
                let hex = |key: &str| map.get(key).and_then(Value::as_str)
                    .and_then(|s| u16::from_str_radix(s.trim_start_matches("0x").split_whitespace().next()?, 16).ok());
                if let (Some(vid), Some(pid)) = (hex("USBDeviceKeyVendorID"), hex("USBDeviceKeyProductID")) {
                    out.push((vid, pid, map.get("_name").and_then(Value::as_str).unwrap_or("").to_string(), Vec::new()));
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

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn raw_devices() -> Vec<(u16, u16, String, Vec<String>)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::from_sysfs;

    #[test]
    fn sysfs_devices_and_their_drivers() {
        let root = std::env::temp_dir().join(format!("knoend-sysfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dev = root.join("1-2");
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::write(dev.join("idVendor"), "1a86\n").unwrap();
        std::fs::write(dev.join("idProduct"), "7522\n").unwrap();
        std::fs::write(dev.join("product"), "USB Serial\n").unwrap();
        // Interface 1-2:1.0 bound to "usbfs" (what brltty's claim looks like).
        let iface = root.join("1-2:1.0");
        std::fs::create_dir_all(&iface).unwrap();
        let driver = root.join("drivers/usbfs");
        std::fs::create_dir_all(&driver).unwrap();
        std::os::unix::fs::symlink(&driver, iface.join("driver")).unwrap();
        // A hub without IDs we care about is still listed here; filtering happens above.
        let got = from_sysfs(&root);
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(got, vec![(0x1a86, 0x7522, "USB Serial".to_string(), vec!["usbfs".to_string()])]);
    }
}
