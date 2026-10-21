//! macOS: the app that owns the frontmost on-screen window, from the window server, polled (no
//! special permission: owner PID and layer need none, only window titles would). `app` = bundle id
//! (what libraries match on), `instance` = the app's display name.
//!
//! Not NSWorkspace.frontmostApplication: it is updated from the main run loop, which this command-line
//! service never runs, so it kept returning the first app (tested on macOS 26.6, 2026-10-17).
//!
//! With the Accessibility permission (which typing needs anyway), the focused app comes from the
//! Accessibility API instead: a click on the desktop makes Finder the focused app with no focused
//! window, while the window list still shows the last app's windows in front. That case is "nothing
//! focused" (the launcher, as an empty workspace on Linux).

use std::ffi::c_void;
use std::time::Duration;

use objc2::rc::autoreleasepool;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSRunningApplication;
use objc2_core_foundation::CFArray;
use objc2_core_graphics::{kCGNullWindowID, CGWindowListCopyWindowInfo, CGWindowListOption};
use objc2_foundation::{ns_string, NSArray, NSDictionary, NSNumber, NSString};
use knoend_core::focus::{Focus, Os};

pub struct Watcher;

impl Watcher {
    pub fn connect() -> Result<Watcher, String> {
        Ok(Watcher)
    }

    pub fn current(&self) -> Option<Focus> {
        autoreleasepool(|_| {
            let mut app = match focused_app() {
                Some((pid, has_window)) => NSRunningApplication::runningApplicationWithProcessIdentifier(pid).map(|a| (a, has_window)),
                None => None,
            };
            // A click on the Dock focuses the Dock for a moment: it is not an app the person uses.
            if app.as_ref().map_or(true, |(a, _)| a.bundleIdentifier().is_some_and(|b| b.to_string() == DOCK)) {
                app = NSRunningApplication::runningApplicationWithProcessIdentifier(frontmost_window_pid()?).map(|a| (a, true));
            }
            let (app, has_window) = app?;
            let bundle = app.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default();
            let name = app.localizedName().map(|s| s.to_string()).unwrap_or_default();
            if (bundle.is_empty() && name.is_empty()) || (bundle == FINDER && !has_window) {
                return None;
            }
            let pid = u32::try_from(app.processIdentifier()).ok();
            Some(Focus { os: Os::Macos, app: bundle, instance: name, title: String::new(), web: None, pid, terminal: None })
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

const FINDER: &str = "com.apple.finder";
const DOCK: &str = "com.apple.dock";

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateSystemWide() -> *const c_void;
    fn AXUIElementSetMessagingTimeout(element: *const c_void, seconds: f32) -> i32;
    fn AXUIElementCopyAttributeValue(element: *const c_void, attribute: *const c_void, value: *mut *const c_void) -> i32;
    fn AXUIElementGetPid(element: *const c_void, pid: *mut i32) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(object: *const c_void);
}

/// The focused app's PID and whether a real window of it has focus, from the Accessibility API. None
/// without the Accessibility permission, or when nothing answers within half a second.
fn focused_app() -> Option<(i32, bool)> {
    // NSString is toll-free bridged to the CFString the attribute names are.
    let attribute = |name: &'static NSString| (name as *const NSString).cast::<c_void>();
    unsafe {
        if !AXIsProcessTrusted() {
            return None;
        }
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }
        AXUIElementSetMessagingTimeout(system, 0.5);
        let mut app = std::ptr::null();
        let found = AXUIElementCopyAttributeValue(system, attribute(ns_string!("AXFocusedApplication")), &mut app) == 0;
        CFRelease(system);
        if !found || app.is_null() {
            return None;
        }
        let mut pid = 0;
        let has_pid = AXUIElementGetPid(app, &mut pid) == 0;
        // On the desktop Finder's focused "window" is the desktop itself, an AXScrollArea: a real
        // window has the role AXWindow (seen on macOS 26.6, 2026-10-18).
        let mut has_window = false;
        let mut window = std::ptr::null();
        if AXUIElementCopyAttributeValue(app, attribute(ns_string!("AXFocusedWindow")), &mut window) == 0 && !window.is_null() {
            let mut role = std::ptr::null();
            if AXUIElementCopyAttributeValue(window, attribute(ns_string!("AXRole")), &mut role) == 0 && !role.is_null() {
                has_window = (*role.cast::<NSString>()).to_string() == "AXWindow";
                CFRelease(role);
            }
            CFRelease(window);
        }
        CFRelease(app);
        has_pid.then_some((pid, has_window))
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

/// Brings the app called any of `names` (bundle id or display name) to the front with `open`.
pub fn bring_to_front(names: &[&str]) -> Result<(), String> {
    for name in names.iter().filter(|n| !n.is_empty()) {
        let flag = if name.contains('.') { "-b" } else { "-a" };
        if std::process::Command::new("open").args([flag, name]).status().is_ok_and(|s| s.success()) {
            return Ok(());
        }
    }
    Err(format!("no app called {}", names.join(" / ")))
}
