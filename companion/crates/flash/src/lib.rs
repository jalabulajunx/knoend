//! Finding, identifying and flashing boards over USB, with `espflash` (MIT/Apache-2.0).
//!
//! Firmware comes from a release manifest (`manifest.json`): every part carries its flash offset
//! and SHA-256, and nothing is written unless every part verifies first.

pub mod manifest;

use std::time::Duration;

use espflash::connection::{Connection, ResetAfterOperation, ResetBeforeOperation};
use espflash::flasher::Flasher;
use espflash::image_format::Segment;
use espflash::target::ProgressCallbacks;
use serde::Serialize;
use serialport::{SerialPortType, UsbPortInfo};

/// A USB serial port a board might be on.
#[derive(Debug, Clone, Serialize)]
pub struct Port {
    pub name: String,
    pub vid: u16,
    pub pid: u16,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    /// What the USB IDs suggest: "espressif-native" (USB-Serial-JTAG), "ch340", "cp210x", ...
    pub bridge: String,
}

/// What the chip itself reports from its boot ROM.
#[derive(Debug, Clone, Serialize)]
pub struct Chip {
    pub chip: String,
    pub revision: Option<String>,
    pub flash_size: String,
    pub features: Vec<String>,
    pub mac: Option<String>,
}

/// Progress of one flashing job, for the web app.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "stage", rename_all = "lowercase")]
pub enum Progress {
    Connecting,
    Erasing,
    Writing { part: usize, parts: usize, offset: u32, done: usize, total: usize },
    Verifying { part: usize },
    Done,
}

/// The kind of USB-serial bridge a USB vendor ID belongs to (what board definitions list).
pub fn bridge(vid: u16, pid: u16) -> &'static str {
    match (vid, pid) {
        (0x303a, _) => "espressif-native",
        (0x1a86, _) => "ch340",
        (0x10c4, _) => "cp210x",
        (0x0403, _) => "ftdi",
        _ => "unknown",
    }
}

/// USB serial ports only (Bluetooth and PCI ports are not boards).
pub fn list_ports() -> Vec<Port> {
    let mut ports: Vec<Port> = serialport::available_ports().unwrap_or_default().into_iter()
        .filter_map(|p| match p.port_type {
            SerialPortType::UsbPort(usb) => Some(Port {
                bridge: bridge(usb.vid, usb.pid).to_string(),
                name: p.port_name,
                vid: usb.vid,
                pid: usb.pid,
                manufacturer: usb.manufacturer,
                product: usb.product,
            }),
            _ => None,
        })
        .collect();
    // macOS lists every port twice (cu.* and tty.*); the cu.* one is the one to open.
    ports.retain(|p| !p.name.starts_with("/dev/tty.") || !ports_has_cu(&p.name));
    ports
}

fn ports_has_cu(tty: &str) -> bool {
    std::path::Path::new(&tty.replacen("/dev/tty.", "/dev/cu.", 1)).exists()
}

fn usb_info(port: &str) -> UsbPortInfo {
    serialport::available_ports().unwrap_or_default().into_iter()
        .find(|p| p.port_name == port)
        .and_then(|p| match p.port_type {
            SerialPortType::UsbPort(usb) => Some(usb),
            _ => None,
        })
        .unwrap_or(UsbPortInfo { vid: 0, pid: 0, serial_number: None, manufacturer: None, product: None })
}

fn connect(port: &str, after: ResetAfterOperation) -> Result<Flasher, String> {
    let serial = serialport::new(port, 115_200)
        .flow_control(serialport::FlowControl::None)
        .timeout(Duration::from_secs(3))
        .open_native()
        .map_err(|e| format!("cannot open {port}: {e}"))?;
    let connection = Connection::new(serial, usb_info(port), after, ResetBeforeOperation::DefaultReset, 115_200);
    // Stub on, verify on, skip unchanged regions off, chip auto-detected, 460800 baud after sync.
    Flasher::connect(connection, true, true, false, None, Some(460_800))
        .map_err(|e| format!("could not reach the chip's bootloader on {port}: {e}"))
}

/// Puts the chip in its boot ROM, reads what it is, then resets it back into its firmware.
pub fn identify(port: &str) -> Result<Chip, String> {
    let mut flasher = connect(port, ResetAfterOperation::HardReset)?;
    let info = flasher.device_info().map_err(|e| e.to_string())?;
    let chip = Chip {
        chip: info.chip.to_string(),
        revision: info.revision.map(|(major, minor)| format!("v{major}.{minor}")),
        flash_size: info.flash_size.to_string(),
        features: info.features.clone(),
        mac: info.mac_address.clone(),
    };
    flasher.connection().reset_after(false, info.chip).map_err(|e| e.to_string())?;
    Ok(chip)
}

struct Reporter<'a> {
    part: usize,
    parts: usize,
    offset: u32,
    total: usize,
    sink: &'a mut dyn FnMut(Progress),
}

impl ProgressCallbacks for Reporter<'_> {
    fn init(&mut self, addr: u32, total: usize) {
        self.part += 1; // one init per part, in order
        self.offset = addr;
        self.total = total;
        (self.sink)(Progress::Writing { part: self.part, parts: self.parts, offset: addr, done: 0, total });
    }
    fn update(&mut self, current: usize) {
        (self.sink)(Progress::Writing { part: self.part, parts: self.parts, offset: self.offset, done: current, total: self.total });
    }
    fn verifying(&mut self) {
        (self.sink)(Progress::Verifying { part: self.part });
    }
    fn finish(&mut self, _skipped: bool) {}
}

/// Writes `parts` (offset, bytes) and resets the board into the new firmware. With `erase_all`,
/// the whole flash is erased first, including stored keys and Wi-Fi settings.
pub fn flash(port: &str, parts: &[(u32, Vec<u8>)], erase_all: bool, progress: &mut dyn FnMut(Progress))
    -> Result<(), String> {
    progress(Progress::Connecting);
    let mut flasher = connect(port, ResetAfterOperation::HardReset)?;
    if erase_all {
        progress(Progress::Erasing);
        flasher.erase_flash().map_err(|e| format!("erase failed: {e}"))?;
    }
    // All parts in ONE write session: espflash restarts the chip at the end of every session, so
    // writing parts one by one would reboot the board after the first (seen on the ONX).
    let segments: Vec<Segment> = parts.iter().map(|(offset, data)| Segment::new(*offset, data)).collect();
    let mut reporter = Reporter { part: 0, parts: parts.len(), offset: 0, total: 0, sink: progress };
    flasher.write_bins_to_flash(&segments, &mut reporter).map_err(|e| {
        format!("writing part {} at {:#x} failed: {e}", reporter.part.max(1), reporter.offset)
    })?;
    let chip = flasher.chip();
    flasher.connection().reset_after(false, chip).map_err(|e| e.to_string())?;
    progress(Progress::Done);
    Ok(())
}
