//! Finding, identifying and flashing boards over USB, with `espflash` (MIT/Apache-2.0).
//!
//! Firmware comes from a release manifest (`manifest.json`): every part carries its flash offset
//! and SHA-256, and nothing is written unless every part verifies first.

pub mod manifest;
pub mod on_board;
mod usb;

pub use usb::{usb_serial_devices, UsbDevice};

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
    /// The USB serial number: on a native-USB ESP32 board its MAC address ("28:84:85:90:BE:B8").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    /// What the USB IDs suggest: "espressif-native" (USB-Serial-JTAG), "ch340", "cp210x", ...
    pub bridge: String,
}

impl Port {
    /// The board's MAC as KnoEnd writes board ids ("2884..."), when the USB serial number is one
    /// (native-USB ESP32 boards); None for a USB-serial bridge (CH340, CP210x).
    pub fn mac_id(&self) -> Option<String> {
        let serial = self.serial.as_deref()?;
        let hex: String = serial.chars().filter(|c| *c != ':').collect::<String>().to_lowercase();
        (self.bridge == "espressif-native" && hex.len() == 12 && hex.chars().all(|c| c.is_ascii_hexdigit())).then_some(hex)
    }
}

/// What the chip itself reports from its boot ROM.
#[derive(Debug, Clone, Serialize)]
pub struct Chip {
    pub chip: String,
    pub revision: Option<String>,
    pub flash_size: String,
    pub features: Vec<String>,
    pub mac: Option<String>,
    /// The firmware on it now (partition table, apps, their components), when it could be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware: Option<on_board::OnBoard>,
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
                serial: usb.serial_number,
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
    let mut last_err = String::new();
    let mut serial = None;
    for attempt in 0..6 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(200));
        }
        match serialport::new(port, 115_200)
            .flow_control(serialport::FlowControl::None)
            .timeout(Duration::from_secs(3))
            .open_native() {
            Ok(s) => {
                serial = Some(s);
                break;
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    let serial = serial.ok_or_else(|| format!("cannot open {port}: {last_err}"))?;
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
        firmware: read_firmware(&mut flasher),
    };
    flasher.connection().reset_after(false, info.chip).map_err(|e| e.to_string())?;
    Ok(chip)
}

/// Reads `size` bytes at `offset` (nothing is written).
fn read(flasher: &mut Flasher, offset: u32, size: u32) -> Option<Vec<u8>> {
    let path = std::env::temp_dir().join(format!("knoend-read-{}-{offset:x}.bin", std::process::id()));
    let result = flasher.read_flash(offset, size, 0x1000, 64, path.clone()).ok().and_then(|_| std::fs::read(&path).ok());
    let _ = std::fs::remove_file(&path);
    result
}

/// The firmware the board carries now: its partition table, and each app's descriptor and
/// component names from the first part of the app (on_board.rs). None when it cannot be read.
fn read_firmware(flasher: &mut Flasher) -> Option<on_board::OnBoard> {
    let table = read(flasher, on_board::TABLE_OFFSET, on_board::TABLE_SIZE)?;
    let partitions = on_board::parse_table(&table);
    let mut apps = Vec::new();
    for p in partitions.iter().filter(|p| p.kind == "app") {
        // An empty slot reads 0xFF: one block tells, before reading more.
        let first = read(flasher, p.offset, 0x1000)?;
        if !on_board::is_app_image(&first) {
            continue;
        }
        let head = read(flasher, p.offset, on_board::APP_HEAD.min(p.size))?;
        apps.extend(on_board::parse_app(&p.name, &head));
    }
    Some(on_board::OnBoard { partitions, apps })
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
