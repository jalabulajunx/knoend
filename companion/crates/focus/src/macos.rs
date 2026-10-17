//! macOS: the app that owns the frontmost on-screen window, from the window server, polled (no
//! special permission: owner PID and layer need none, only window titles would). `app` = bundle id
//! (what libraries match on), `instance` = the app's display name.
//!
//! Not NSWorkspace.frontmostApplication: it is updated from the main run loop, which this command-line
//! service never runs, so it kept returning the first app (tested on macOS 26.6, 2026-10-17).

use std::time::Duration;

use objc2::rc::autoreleasepool;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSRunningApplication;
use objc2_core_foundation::CFArray;
use objc2_core_graphics::{kCGNullWindowID, CGWindowListCopyWindowInfo, CGWindowListOption};
use objc2_foundation::{ns_string, NSArray, NSDictionary, NSNumber, NSString};
use openpager_core::focus::{Focus, Os};

pub struct Watcher;

impl Watcher {
    pub fn connect() -> Result<Watcher, String> {
        Ok(Watcher)
    }

    pub fn current(&self) -> Option<Focus> {
        autoreleasepool(|_| {
            let app = NSRunningApplication::runningApplicationWithProcessIdentifier(frontmost_window_pid()?)?;
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

/// PID of the owner of the frontmost normal window (layer 0, visible). The list is front to back.
fn frontmost_window_pid() -> Option<i32> {
    let options = CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID)?;
    // CFArray of CFDictionary is toll-free bridged to NSArray of NSDictionary.
    let windows: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(&*list as *const CFArray).cast::<NSArray<NSDictionary<NSString, AnyObject>>>() };
    let number = |window: &NSDictionary<NSString, AnyObject>, key: &NSString| {
        window.objectForKey(key).and_then(|v| v.downcast::<NSNumber>().ok())
    };
    windows.iter().find_map(|window| {
        let layer = number(&window, ns_string!("kCGWindowLayer"))?.intValue();
        let alpha = number(&window, ns_string!("kCGWindowAlpha")).map_or(1.0, |a| a.doubleValue());
        if layer != 0 || alpha <= 0.0 {
            return None;
        }
        Some(number(&window, ns_string!("kCGWindowOwnerPID"))?.intValue())
    })
}
