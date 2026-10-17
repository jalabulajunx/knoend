//! Typing a keystroke on this computer, for a board that cannot be a keyboard right now (no
//! Bluetooth host, or a USB port that only reaches a serial chip). Keys are USB HID usage IDs and
//! modifier bits (1 Ctrl, 2 Shift, 4 Alt, 8 GUI/Cmd), as in the protocol.

#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
pub use x11::Typist;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{accessibility_trusted, launching_app, Typist};

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use unsupported::Typist;

/// The layout-independent name of a key, from its USB HID usage ID (US positions for punctuation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Letter(char),
    Digit(char),
    F(u8),
    Named(&'static str),
}

pub fn key_for_usage(usage: u8) -> Option<Key> {
    Some(match usage {
        0x04..=0x1D => Key::Letter((b'a' + (usage - 0x04)) as char),
        0x1E..=0x26 => Key::Digit((b'1' + (usage - 0x1E)) as char),
        0x27 => Key::Digit('0'),
        0x3A..=0x45 => Key::F(usage - 0x3A + 1),
        0x68..=0x73 => Key::F(usage - 0x68 + 13),
        0x28 => Key::Named("Enter"),
        0x29 => Key::Named("Escape"),
        0x2A => Key::Named("Backspace"),
        0x2B => Key::Named("Tab"),
        0x2C => Key::Named("Space"),
        0x2D => Key::Named("Minus"),
        0x2E => Key::Named("Equal"),
        0x2F => Key::Named("BracketLeft"),
        0x30 => Key::Named("BracketRight"),
        0x31 => Key::Named("Backslash"),
        0x33 => Key::Named("Semicolon"),
        0x34 => Key::Named("Quote"),
        0x35 => Key::Named("Backquote"),
        0x36 => Key::Named("Comma"),
        0x37 => Key::Named("Period"),
        0x38 => Key::Named("Slash"),
        0x49 => Key::Named("Insert"),
        0x4A => Key::Named("Home"),
        0x4B => Key::Named("PageUp"),
        0x4C => Key::Named("Delete"),
        0x4D => Key::Named("End"),
        0x4E => Key::Named("PageDown"),
        0x4F => Key::Named("Right"),
        0x50 => Key::Named("Left"),
        0x51 => Key::Named("Down"),
        0x52 => Key::Named("Up"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usages_match_the_firmware_and_spec() {
        assert_eq!(key_for_usage(0x13), Some(Key::Letter('p'))); // Primary+Shift+P
        assert_eq!(key_for_usage(0x1E), Some(Key::Digit('1')));
        assert_eq!(key_for_usage(0x27), Some(Key::Digit('0')));
        assert_eq!(key_for_usage(0x3E), Some(Key::F(5)));
        assert_eq!(key_for_usage(0x73), Some(Key::F(24)));
        assert_eq!(key_for_usage(0x35), Some(Key::Named("Backquote")));
        assert_eq!(key_for_usage(0x01), None);
    }
}
