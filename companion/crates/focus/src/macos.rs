//! macOS: the frontmost application from NSWorkspace, polled (no run loop needed, no special
//! permission). `app` = bundle id (what libraries match on), `instance` = the app's display name.
//! Written without a Mac to test on: compile-checked in CI, behaviour untested until tried.

use std::time::Duration;

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;
use openpager_core::focus::{Focus, Os};

pub struct Watcher;

impl Watcher {
    pub fn connect() -> Result<Watcher, String> {
        Ok(Watcher)
    }

    pub fn current(&self) -> Option<Focus> {
        autoreleasepool(|_| {
            let workspace = NSWorkspace::sharedWorkspace();
            let app = workspace.frontmostApplication()?;
            let bundle = app.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default();
            let name = app.localizedName().map(|s| s.to_string()).unwrap_or_default();
            if bundle.is_empty() && name.is_empty() {
                return None;
            }
            Some(Focus { os: Os::Macos, app: bundle, instance: name, title: String::new() })
        })
    }

    pub fn wait_for_change(&self, previous: Option<&Focus>) -> Option<Focus> {
        loop {
            std::thread::sleep(Duration::from_millis(300));
            let now = self.current();
            let changed = match (&now, previous) {
                (Some(a), Some(b)) => a.app != b.app,
                (None, None) => false,
                _ => true,
            };
            if changed {
                return now;
            }
        }
    }
}
