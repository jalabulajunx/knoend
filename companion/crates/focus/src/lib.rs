//! Which app has focus, per platform, behind one `Watcher` type:
//!   `Watcher::connect()`, `current()`, and `wait_for_change(previous)` (blocks); and
//!   `bring_to_front(names)` to show an app's window.
//! Linux/X11, macOS and Windows are implemented.

#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
pub use x11::{bring_to_front, window_pid, Watcher};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{bring_to_front, Watcher};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{bring_to_front, process_exe, spawn_on_input_desktop, window_pid, Watcher};

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub use unsupported::{bring_to_front, Watcher};

/// The process id of an app's window, where the platform tells (Linux/X11 and Windows).
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn window_pid(_names: &[&str]) -> Option<u32> {
    None
}
