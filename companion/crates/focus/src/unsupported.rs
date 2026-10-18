//! Platforms without a focus backend yet: the service still runs (flashing, pairing, Wi-Fi), but
//! layouts do not follow the focused app.

use openpager_core::focus::Focus;

pub struct Watcher;

impl Watcher {
    pub fn connect() -> Result<Watcher, String> {
        Err("following the focused app is not supported on this OS yet".into())
    }
    pub fn current(&self) -> Option<Focus> {
        None
    }
    pub fn wait_for_change(&self, _previous: Option<&Focus>) -> Option<Focus> {
        None
    }
}

pub fn bring_to_front(_names: &[&str]) -> Result<(), String> {
    Err("bringing an app to the front is not supported on this OS yet".into())
}
