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
#[cfg(any(target_os = "linux", all(test, unix)))]
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

/// From Windows SetupAPI (`USB\*` present devices), including devices with no COM port driver.
#[cfg(target_os = "windows")]
fn raw_devices() -> Vec<(u16, u16, String, Vec<String>)> {
    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    #[repr(C)]
    struct SpDevinfoData {
        cb_size: u32,
        class_guid: Guid,
        dev_inst: u32,
        reserved: usize,
    }

    #[link(name = "setupapi")]
    extern "system" {
        fn SetupDiGetClassDevsW(
            class_guid: *const Guid,
            enumerator: *const u16,
            hwnd_parent: usize,
            flags: u32,
        ) -> *mut std::ffi::c_void;
        fn SetupDiEnumDeviceInfo(
            dev_info_set: *mut std::ffi::c_void,
            member_index: u32,
            dev_info_data: *mut SpDevinfoData,
        ) -> i32;
        fn SetupDiGetDeviceRegistryPropertyW(
            dev_info_set: *mut std::ffi::c_void,
            dev_info_data: *mut SpDevinfoData,
            property: u32,
            property_reg_data_type: *mut u32,
            property_buffer: *mut u8,
            property_buffer_size: u32,
            required_size: *mut u32,
        ) -> i32;
        fn SetupDiDestroyDeviceInfoList(dev_info_set: *mut std::ffi::c_void) -> i32;
    }

    const DIGCF_PRESENT: u32 = 0x00000002;
    const DIGCF_ALLCLASSES: u32 = 0x00000004;
    const SPDRP_DEVICEDESC: u32 = 0x00000000;
    const SPDRP_HARDWAREID: u32 = 0x00000001;
    const SPDRP_SERVICE: u32 = 0x00000004;
    const SPDRP_FRIENDLYNAME: u32 = 0x0000000C;

    unsafe fn prop_string(
        set: *mut std::ffi::c_void,
        data: &mut SpDevinfoData,
        prop: u32,
    ) -> Option<String> {
        let mut buf = [0u16; 512];
        let mut reg_type = 0u32;
        let mut req = 0u32;
        if SetupDiGetDeviceRegistryPropertyW(
            set,
            data,
            prop,
            &mut reg_type,
            buf.as_mut_ptr() as *mut u8,
            (buf.len() * 2) as u32,
            &mut req,
        ) == 0
        {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    let mut out: Vec<(u16, u16, String, Vec<String>)> = Vec::new();
    let usb_enum: [u16; 4] = [b'U' as u16, b'S' as u16, b'B' as u16, 0];
    unsafe {
        let set = SetupDiGetClassDevsW(
            std::ptr::null(),
            usb_enum.as_ptr(),
            0,
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        );
        if set.is_null() || set == (-1isize as *mut std::ffi::c_void) {
            return out;
        }
        let mut idx = 0u32;
        loop {
            let mut data = SpDevinfoData {
                cb_size: std::mem::size_of::<SpDevinfoData>() as u32,
                class_guid: Guid { data1: 0, data2: 0, data3: 0, data4: [0; 8] },
                dev_inst: 0,
                reserved: 0,
            };
            if SetupDiEnumDeviceInfo(set, idx, &mut data) == 0 {
                break;
            }
            idx += 1;
            let Some(hw_id) = prop_string(set, &mut data, SPDRP_HARDWAREID) else { continue };
            let Some((vid, pid)) = parse_usb_hardware_id(&hw_id) else { continue };
            let name = prop_string(set, &mut data, SPDRP_FRIENDLYNAME)
                .or_else(|| prop_string(set, &mut data, SPDRP_DEVICEDESC))
                .unwrap_or_default();
            let service = prop_string(set, &mut data, SPDRP_SERVICE);
            if let Some(existing) = out.iter_mut().find(|(v, p, _, _)| *v == vid && *p == pid) {
                if existing.2.is_empty() && !name.is_empty() {
                    existing.2 = name;
                }
                if let Some(svc) = service {
                    if !existing.3.contains(&svc) {
                        existing.3.push(svc);
                    }
                }
            } else {
                let drivers = service.into_iter().collect();
                out.push((vid, pid, name, drivers));
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

#[cfg(any(target_os = "windows", test))]
fn parse_usb_hardware_id(hw_id: &str) -> Option<(u16, u16)> {
    let upper = hw_id.to_ascii_uppercase();
    let vid_idx = upper.find("VID_")? + 4;
    let pid_idx = upper.find("PID_")? + 4;
    let vid = u16::from_str_radix(upper.get(vid_idx..vid_idx + 4)?, 16).ok()?;
    let pid = u16::from_str_radix(upper.get(pid_idx..pid_idx + 4)?, 16).ok()?;
    Some((vid, pid))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn raw_devices() -> Vec<(u16, u16, String, Vec<String>)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn sysfs_devices_and_their_drivers() {
        use super::from_sysfs;
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

    #[test]
    fn parses_windows_usb_hardware_ids() {
        use super::parse_usb_hardware_id;
        assert_eq!(
            parse_usb_hardware_id(r"USB\VID_1A86&PID_7522&REV_0264"),
            Some((0x1a86, 0x7522))
        );
        assert_eq!(
            parse_usb_hardware_id(r"USB\VID_303A&PID_1001&MI_00"),
            Some((0x303a, 0x1001))
        );
        assert_eq!(parse_usb_hardware_id(r"PCI\VEN_8086&DEV_1234"), None);
    }
}
