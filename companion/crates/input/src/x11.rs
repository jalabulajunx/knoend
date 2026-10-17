//! Linux/X11: the XTest extension types into whatever window has focus. No special permissions;
//! on Wayland it reaches only XWayland apps.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, Keycode, Window, KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use crate::{key_for_usage, Key};

pub struct Typist {
    conn: RustConnection,
    root: Window,
    min_keycode: u8,
    per_keycode: usize,
    keysyms: Vec<u32>,
}

fn keysym(key: Key) -> u32 {
    match key {
        Key::Letter(c) | Key::Digit(c) => c as u32, // Latin-1 keysyms equal their ASCII codes
        Key::F(n) => 0xFFBE + u32::from(n) - 1,     // XK_F1 .. XK_F24
        Key::Named(name) => match name {
            "Enter" => 0xFF0D,
            "Escape" => 0xFF1B,
            "Backspace" => 0xFF08,
            "Tab" => 0xFF09,
            "Space" => 0x20,
            "Minus" => 0x2D,
            "Equal" => 0x3D,
            "BracketLeft" => 0x5B,
            "BracketRight" => 0x5D,
            "Backslash" => 0x5C,
            "Semicolon" => 0x3B,
            "Quote" => 0x27,
            "Backquote" => 0x60,
            "Comma" => 0x2C,
            "Period" => 0x2E,
            "Slash" => 0x2F,
            "Insert" => 0xFF63,
            "Home" => 0xFF50,
            "PageUp" => 0xFF55,
            "Delete" => 0xFFFF,
            "End" => 0xFF57,
            "PageDown" => 0xFF56,
            "Right" => 0xFF53,
            "Left" => 0xFF51,
            "Down" => 0xFF54,
            "Up" => 0xFF52,
            _ => 0,
        },
    }
}

const MODIFIER_KEYSYMS: [(u8, u32); 4] = [(1, 0xFFE3), (2, 0xFFE1), (4, 0xFFE9), (8, 0xFFEB)]; // Control_L, Shift_L, Alt_L, Super_L

impl Typist {
    pub fn connect() -> Result<Typist, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| format!("cannot reach the X server: {e}"))?;
        conn.xtest_get_version(2, 2).map_err(|e| e.to_string())?.reply()
            .map_err(|_| "the X server has no XTest extension".to_string())?;
        let setup = conn.setup();
        let (root, min, max) = (setup.roots[screen].root, setup.min_keycode, setup.max_keycode);
        let mapping = conn.get_keyboard_mapping(min, max - min + 1).map_err(|e| e.to_string())?
            .reply().map_err(|e| e.to_string())?;
        Ok(Typist { root, min_keycode: min, per_keycode: mapping.keysyms_per_keycode as usize, keysyms: mapping.keysyms, conn })
    }

    /// The keycode that produces `sym` in the current layout (unshifted column preferred).
    fn keycode(&self, sym: u32) -> Option<Keycode> {
        for column in 0..self.per_keycode {
            for (index, chunk) in self.keysyms.chunks(self.per_keycode).enumerate() {
                if chunk.get(column) == Some(&sym) {
                    return Some(self.min_keycode + index as u8);
                }
            }
        }
        None
    }

    fn fake(&self, kind: u8, code: Keycode) -> Result<(), String> {
        self.conn.xtest_fake_input(kind, code, 0, self.root, 0, 0, 0).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Whether this layout has a key for `usage` (no keystroke is sent).
    pub fn can_type(&self, usage: u8) -> bool {
        key_for_usage(usage).and_then(|k| self.keycode(keysym(k))).is_some()
    }

    /// Presses the modifiers, taps the key, releases the modifiers.
    pub fn tap(&self, modifiers: u8, usage: u8) -> Result<(), String> {
        let key = key_for_usage(usage).ok_or_else(|| format!("HID usage {usage:#x} has no key"))?;
        let code = self.keycode(keysym(key)).ok_or_else(|| format!("{key:?} is not on this keyboard layout"))?;
        let mods: Vec<Keycode> = MODIFIER_KEYSYMS.iter()
            .filter(|(bit, _)| modifiers & bit != 0)
            .map(|(_, sym)| self.keycode(*sym).ok_or_else(|| format!("modifier {sym:#x} not on this keyboard")))
            .collect::<Result<_, _>>()?;
        for m in &mods {
            self.fake(KEY_PRESS_EVENT, *m)?;
        }
        self.fake(KEY_PRESS_EVENT, code)?;
        self.fake(KEY_RELEASE_EVENT, code)?;
        for m in mods.iter().rev() {
            self.fake(KEY_RELEASE_EVENT, *m)?;
        }
        // A round trip, so the fake events are processed before we return.
        self.conn.get_input_focus().map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        Ok(())
    }
}
