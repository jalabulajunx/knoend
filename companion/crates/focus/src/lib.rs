//! Which app has focus, per platform, behind one `Watcher` type:
//!   `Watcher::connect()`, `current()`, and `wait_for_change(previous)` (blocks).
//! Linux/X11 and macOS are implemented; Windows is not yet.

#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
pub use x11::Watcher;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Watcher;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use unsupported::Watcher;
