//! Windows: keyboard events through Win32 `SendInput`. No special permissions are required for
//! standard desktop windows; UIPI prevents an unprivileged process from typing into an elevated
//! ("Run as administrator") window unless `knoend` is also elevated.

use std::ffi::c_void;

use crate::{key_for_usage, Key};

const INPUT_KEYBOARD: u32 = 1;
const KEYEVENTF_EXTENDEDKEY: u32 = 0x0001;
const KEYEVENTF_KEYUP: u32 = 0x0002;
const MAPVK_VK_TO_VSC: u32 = 0;
const GENERIC_ALL: u32 = 0x1000_0000;

#[repr(C)]
#[derive(Clone, Copy)]
struct KeybdInput {
    w_vk: u16,
    w_scan: u16,
    dw_flags: u32,
    time: u32,
    dw_extra_info: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MouseInput {
    dx: i32,
    dy: i32,
    mouse_data: u32,
    dw_flags: u32,
    time: u32,
    dw_extra_info: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
union InputUnion {
    ki: KeybdInput,
    mi: MouseInput,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Input {
    kind: u32,
    u: InputUnion,
}

#[link(name = "user32")]
extern "system" {
    fn SendInput(c_inputs: u32, p_inputs: *const Input, cb_size: i32) -> u32;
    fn MapVirtualKeyW(u_code: u32, u_map_type: u32) -> u32;
    fn OpenInputDesktop(flags: u32, inherit: i32, desired_access: u32) -> *mut c_void;
    fn SetThreadDesktop(desktop: *mut c_void) -> i32;
    fn CloseDesktop(desktop: *mut c_void) -> i32;
}

fn attach_input_desktop() {
    unsafe {
        let desk = OpenInputDesktop(0, 0, GENERIC_ALL);
        if !desk.is_null() && SetThreadDesktop(desk) == 0 {
            CloseDesktop(desk);
        }
    }
}

pub struct Typist;

/// Windows virtual-key codes (`VK_*`) and whether the key requires `KEYEVENTF_EXTENDEDKEY`.
fn vk_code(key: Key) -> Option<(u16, bool)> {
    Some(match key {
        Key::Letter(c @ 'a'..='z') => ((c as u8 - b'a' + b'A') as u16, false),
        Key::Digit(c @ '0'..='9') => ((c as u8) as u16, false),
        Key::F(n @ 1..=24) => (0x70 + u16::from(n - 1), false), // VK_F1 (0x70) .. VK_F24 (0x87)
        Key::Named(name) => match name {
            "Enter" => (0x0D, false),        // VK_RETURN
            "Escape" => (0x1B, false),       // VK_ESCAPE
            "Backspace" => (0x08, false),    // VK_BACK
            "Tab" => (0x09, false),          // VK_TAB
            "Space" => (0x20, false),        // VK_SPACE
            "Minus" => (0xBD, false),        // VK_OEM_MINUS
            "Equal" => (0xBB, false),        // VK_OEM_PLUS
            "BracketLeft" => (0xDB, false),  // VK_OEM_4
            "BracketRight" => (0xDD, false), // VK_OEM_6
            "Backslash" => (0xDC, false),    // VK_OEM_5
            "Semicolon" => (0xBA, false),    // VK_OEM_1
            "Quote" => (0xDE, false),        // VK_OEM_7
            "Backquote" => (0xC0, false),    // VK_OEM_3
            "Comma" => (0xBC, false),        // VK_OEM_COMMA
            "Period" => (0xBE, false),       // VK_OEM_PERIOD
            "Slash" => (0xBF, false),        // VK_OEM_2
            "Insert" => (0x2D, true),        // VK_INSERT
            "Home" => (0x24, true),          // VK_HOME
            "PageUp" => (0x21, true),        // VK_PRIOR
            "Delete" => (0x2E, true),        // VK_DELETE
            "End" => (0x23, true),           // VK_END
            "PageDown" => (0x22, true),      // VK_NEXT
            "Right" => (0x27, true),         // VK_RIGHT
            "Left" => (0x25, true),          // VK_LEFT
            "Down" => (0x28, true),          // VK_DOWN
            "Up" => (0x26, true),            // VK_UP
            _ => return None,
        },
        _ => return None,
    })
}

/// Modifier bit -> `(VK_*, extended)`.
/// Bit 1 = Ctrl (`VK_LCONTROL`), 2 = Shift (`VK_LSHIFT`), 4 = Alt (`VK_LMENU`), 8 = Win (`VK_LWIN`).
const MODIFIER_VKS: [(u8, u16, bool); 4] = [
    (1, 0xA2, false),
    (2, 0xA0, false),
    (4, 0xA4, false),
    (8, 0x5B, true),
];

fn key_input(vk: u16, extended: bool, up: bool) -> Input {
    let scan = unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) as u16 };
    let mut flags = 0;
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    Input {
        kind: INPUT_KEYBOARD,
        u: InputUnion {
            ki: KeybdInput {
                w_vk: vk,
                w_scan: scan,
                dw_flags: flags,
                time: 0,
                dw_extra_info: 0,
            },
        },
    }
}

impl Typist {
    pub fn connect() -> Result<Typist, String> {
        attach_input_desktop();
        Ok(Typist)
    }

    pub fn can_type(&self, usage: u8) -> bool {
        key_for_usage(usage).and_then(vk_code).is_some()
    }

    pub fn tap(&self, modifiers: u8, usage: u8) -> Result<(), String> {
        attach_input_desktop();
        let key = key_for_usage(usage).ok_or_else(|| format!("HID usage {usage:#x} has no key"))?;
        let (vk, ext) = vk_code(key).ok_or_else(|| format!("{key:?} has no Windows virtual-key code"))?;

        let mods: Vec<(u16, bool)> = MODIFIER_VKS
            .iter()
            .filter(|(bit, _, _)| modifiers & *bit != 0)
            .map(|(_, mvk, mext)| (*mvk, *mext))
            .collect();

        let mut events = Vec::with_capacity(mods.len() * 2 + 2);
        for &(mvk, mext) in &mods {
            events.push(key_input(mvk, mext, false));
        }
        events.push(key_input(vk, ext, false));
        events.push(key_input(vk, ext, true));
        for &(mvk, mext) in mods.iter().rev() {
            events.push(key_input(mvk, mext, true));
        }

        let sent = unsafe {
            SendInput(
                events.len() as u32,
                events.as_ptr(),
                std::mem::size_of::<Input>() as i32,
            )
        };
        if sent != events.len() as u32 {
            return Err(format!(
                "SendInput sent {sent} of {} events (target window may be running as Administrator)",
                events.len()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_hid_usage_maps_to_a_windows_vk() {
        let typist = Typist::connect().unwrap();
        for usage in 0u8..=0x73 {
            if key_for_usage(usage).is_some() {
                assert!(typist.can_type(usage), "usage {usage:#x} failed to map to VK");
            }
        }
        assert_eq!(vk_code(Key::Letter('p')), Some((0x50, false)));
        assert_eq!(vk_code(Key::F(5)), Some((0x74, false)));
        assert_eq!(vk_code(Key::Named("Left")), Some((0x25, true)));
    }

    #[test]
    fn input_struct_matches_win32_abi_size() {
        #[cfg(target_pointer_width = "64")]
        assert_eq!(std::mem::size_of::<Input>(), 40);
        #[cfg(target_pointer_width = "32")]
        assert_eq!(std::mem::size_of::<Input>(), 28);
    }
}
