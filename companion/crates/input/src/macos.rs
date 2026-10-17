//! macOS: keyboard events through Core Graphics. Posting them needs the Accessibility permission
//! (System Settings > Privacy & Security > Accessibility, add the program running `openpager`);
//! without it macOS drops them silently, so we check first and say so.
//! Written without a Mac to test on: compile-checked in CI, behaviour untested until tried.

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

use crate::{key_for_usage, Key};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

pub struct Typist;

/// macOS virtual key codes (Events.h kVK_*), US layout positions.
fn keycode(key: Key) -> Option<CGKeyCode> {
    Some(match key {
        Key::Letter(c) => match c {
            'a' => 0x00, 's' => 0x01, 'd' => 0x02, 'f' => 0x03, 'h' => 0x04, 'g' => 0x05, 'z' => 0x06,
            'x' => 0x07, 'c' => 0x08, 'v' => 0x09, 'b' => 0x0B, 'q' => 0x0C, 'w' => 0x0D, 'e' => 0x0E,
            'r' => 0x0F, 'y' => 0x10, 't' => 0x11, 'o' => 0x1F, 'u' => 0x20, 'i' => 0x22, 'p' => 0x23,
            'l' => 0x25, 'j' => 0x26, 'k' => 0x28, 'n' => 0x2D, 'm' => 0x2E,
            _ => return None,
        },
        Key::Digit(c) => match c {
            '1' => 0x12, '2' => 0x13, '3' => 0x14, '4' => 0x15, '6' => 0x16, '5' => 0x17,
            '9' => 0x19, '7' => 0x1A, '8' => 0x1C, '0' => 0x1D,
            _ => return None,
        },
        Key::F(n) => match n {
            1 => 0x7A, 2 => 0x78, 3 => 0x63, 4 => 0x76, 5 => 0x60, 6 => 0x61, 7 => 0x62, 8 => 0x64,
            9 => 0x65, 10 => 0x6D, 11 => 0x67, 12 => 0x6F, 13 => 0x69, 14 => 0x6B, 15 => 0x71,
            16 => 0x6A, 17 => 0x40, 18 => 0x4F, 19 => 0x50, 20 => 0x5A,
            _ => return None,
        },
        Key::Named(name) => match name {
            "Enter" => 0x24, "Escape" => 0x35, "Backspace" => 0x33, "Tab" => 0x30, "Space" => 0x31,
            "Minus" => 0x1B, "Equal" => 0x18, "BracketLeft" => 0x21, "BracketRight" => 0x1E,
            "Backslash" => 0x2A, "Semicolon" => 0x29, "Quote" => 0x27, "Backquote" => 0x32,
            "Comma" => 0x2B, "Period" => 0x2F, "Slash" => 0x2C, "Insert" => 0x72, "Home" => 0x73,
            "PageUp" => 0x74, "Delete" => 0x75, "End" => 0x77, "PageDown" => 0x79, "Right" => 0x7C,
            "Left" => 0x7B, "Down" => 0x7D, "Up" => 0x7E,
            _ => return None,
        },
    })
}

fn flags(modifiers: u8) -> CGEventFlags {
    let mut flags = CGEventFlags::empty();
    if modifiers & 1 != 0 { flags |= CGEventFlags::CGEventFlagControl; }
    if modifiers & 2 != 0 { flags |= CGEventFlags::CGEventFlagShift; }
    if modifiers & 4 != 0 { flags |= CGEventFlags::CGEventFlagAlternate; }
    if modifiers & 8 != 0 { flags |= CGEventFlags::CGEventFlagCommand; }
    flags
}

impl Typist {
    pub fn connect() -> Result<Typist, String> {
        if unsafe { AXIsProcessTrusted() } {
            Ok(Typist)
        } else {
            Err("macOS needs Accessibility permission to type for the board: System Settings > Privacy & Security > Accessibility, then add the app that runs openpager (e.g. Terminal) and restart it".into())
        }
    }

    pub fn can_type(&self, usage: u8) -> bool {
        key_for_usage(usage).and_then(keycode).is_some()
    }

    pub fn tap(&self, modifiers: u8, usage: u8) -> Result<(), String> {
        let key = key_for_usage(usage).ok_or_else(|| format!("HID usage {usage:#x} has no key"))?;
        let code = keycode(key).ok_or_else(|| format!("{key:?} has no macOS key code"))?;
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| "no event source".to_string())?;
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(source.clone(), code, down).map_err(|_| "could not create key event".to_string())?;
            event.set_flags(flags(modifiers));
            event.post(CGEventTapLocation::HID);
        }
        Ok(())
    }
}
