//! Shortcut strings ("Primary+Shift+P") to USB HID modifier bits and usage IDs.

use crate::focus::Os;

pub const CTRL: u8 = 1;
pub const SHIFT: u8 = 2;
pub const ALT: u8 = 4;
pub const META: u8 = 8;

/// One key press the board can type: modifier bits plus a USB HID keyboard usage ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub modifiers: u8,
    pub usage: u8,
}

/// Parses the grammar in spec/protocol.md section 5. `Primary` becomes Cmd on macOS, Ctrl elsewhere.
pub fn parse_shortcut(text: &str, os: Os) -> Result<Chord, String> {
    let parts: Vec<&str> = text.split('+').collect();
    let (key, modifiers) = parts.split_last().ok_or_else(|| "empty shortcut".to_string())?;
    let mut bits = 0u8;
    for modifier in modifiers {
        bits |= match *modifier {
            "Primary" => if os == Os::Macos { META } else { CTRL },
            "Ctrl" => CTRL,
            "Shift" => SHIFT,
            "Alt" => ALT,
            "Meta" => META,
            other => return Err(format!("unknown modifier {other:?} in {text:?}")),
        };
    }
    let usage = usage_for(key).ok_or_else(|| format!("unknown key {key:?} in {text:?}"))?;
    Ok(Chord { modifiers: bits, usage })
}

/// USB HID usage IDs (HID Usage Tables, keyboard page 0x07), US layout positions.
fn usage_for(key: &str) -> Option<u8> {
    let bytes = key.as_bytes();
    if bytes.len() == 1 {
        let c = bytes[0];
        return match c {
            b'A'..=b'Z' => Some(0x04 + (c - b'A')),
            b'1'..=b'9' => Some(0x1E + (c - b'1')),
            b'0' => Some(0x27),
            _ => None,
        };
    }
    if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
        return match n {
            1..=12 => Some(0x3A + n - 1),
            13..=24 => Some(0x68 + n - 13),
            _ => None,
        };
    }
    Some(match key {
        "Enter" => 0x28,
        "Escape" => 0x29,
        "Backspace" => 0x2A,
        "Tab" => 0x2B,
        "Space" => 0x2C,
        "Minus" => 0x2D,
        "Equal" => 0x2E,
        "BracketLeft" => 0x2F,
        "BracketRight" => 0x30,
        "Backslash" => 0x31,
        "Semicolon" => 0x33,
        "Quote" => 0x34,
        "Backquote" => 0x35,
        "Comma" => 0x36,
        "Period" => 0x37,
        "Slash" => 0x38,
        "Insert" => 0x49,
        "Home" => 0x4A,
        "PageUp" => 0x4B,
        "Delete" => 0x4C,
        "End" => 0x4D,
        "PageDown" => 0x4E,
        "Right" => 0x4F,
        "Left" => 0x50,
        "Down" => 0x51,
        "Up" => 0x52,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_is_ctrl_or_cmd() {
        assert_eq!(parse_shortcut("Primary+Shift+P", Os::Linux), Ok(Chord { modifiers: CTRL | SHIFT, usage: 0x13 }));
        assert_eq!(parse_shortcut("Primary+Shift+P", Os::Macos), Ok(Chord { modifiers: META | SHIFT, usage: 0x13 }));
    }

    #[test]
    fn function_and_named_keys() {
        assert_eq!(parse_shortcut("F5", Os::Linux).unwrap().usage, 0x3E);
        assert_eq!(parse_shortcut("F13", Os::Linux).unwrap().usage, 0x68);
        assert_eq!(parse_shortcut("F24", Os::Linux).unwrap().usage, 0x73);
        assert_eq!(parse_shortcut("Ctrl+Backquote", Os::Linux), Ok(Chord { modifiers: CTRL, usage: 0x35 }));
        assert_eq!(parse_shortcut("0", Os::Linux).unwrap().usage, 0x27);
    }

    #[test]
    fn rejects_unknown() {
        assert!(parse_shortcut("Hyper+A", Os::Linux).is_err());
        assert!(parse_shortcut("Ctrl+F25", Os::Linux).is_err());
        assert!(parse_shortcut("Ctrl+é", Os::Linux).is_err());
    }
}
