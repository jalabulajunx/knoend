//! Windows: the app that owns the foreground window (`GetForegroundWindow`, then the owning
//! process's image name via `QueryFullProcessImageNameW`). Window titles are never read.
//!
//! `app` is the executable file name (`"Code.exe"`, what `match.windows` in recipes names), and
//! `instance` is the stem without `.exe` (`"Code"`), so either form matches.
//!
//! Clicking the desktop (`Progman` / `WorkerW`) or the taskbar (`Shell_TrayWnd`) counts as
//! "nothing focused" (`None`), showing the launcher on the board, while a real File Explorer
//! folder window (`CabinetWClass` / `ExploreWClass`) reports `explorer.exe`. For UWP apps hosted
//! by `ApplicationFrameHost.exe` (such as Calculator or Settings), the child `CoreWindow`'s
//! process is used instead of the frame host.

use std::ffi::c_void;
use std::path::PathBuf;
use std::time::Duration;

use knoend_core::focus::{Focus, Os};

type Hwnd = isize;
type Handle = *mut c_void;
type Bool = i32;

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const SW_SHOW: i32 = 5;
const SW_RESTORE: i32 = 9;
const GENERIC_ALL: u32 = 0x1000_0000;
const GW_OWNER: u32 = 4;
const GWL_EXSTYLE: i32 = -20;
const WS_EX_TOPMOST: i32 = 0x0000_0008;
const WS_EX_TOOLWINDOW: i32 = 0x0000_0080;
const WS_EX_APPWINDOW: i32 = 0x0004_0000;
const WS_EX_NOACTIVATE: i32 = 0x0800_0000;
const DWMWA_CLOAKED: u32 = 14;
const HWND_TOPMOST: Hwnd = -1;
const HWND_NOTOPMOST: Hwnd = -2;
const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_SHOWWINDOW: u32 = 0x0040;
const ASFW_ANY: u32 = 0xFFFF_FFFF;
const VK_NONAME: u8 = 0xFC;
const KEYEVENTF_KEYUP: u32 = 0x0002;
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

#[repr(C)]
struct StartupInfoW {
    cb: u32,
    lp_reserved: *mut u16,
    lp_desktop: *mut u16,
    lp_title: *mut u16,
    dw_x: u32,
    dw_y: u32,
    dw_x_size: u32,
    dw_y_size: u32,
    dw_x_count_chars: u32,
    dw_y_count_chars: u32,
    dw_fill_attribute: u32,
    dw_flags: u32,
    w_show_window: u16,
    cb_reserved2: u16,
    lp_reserved2: *mut u8,
    h_std_input: Handle,
    h_std_output: Handle,
    h_std_error: Handle,
}

#[repr(C)]
struct ProcessInformation {
    h_process: Handle,
    h_thread: Handle,
    dw_process_id: u32,
    dw_thread_id: u32,
}

#[link(name = "user32")]
extern "system" {
    fn GetForegroundWindow() -> Hwnd;
    fn GetWindowThreadProcessId(hwnd: Hwnd, process_id: *mut u32) -> u32;
    fn GetClassNameW(hwnd: Hwnd, class_name: *mut u16, max_count: i32) -> i32;
    fn GetWindowTextLengthW(hwnd: Hwnd) -> i32;
    fn GetWindowTextW(hwnd: Hwnd, lp_string: *mut u16, n_max_count: i32) -> i32;
    fn GetWindow(hwnd: Hwnd, cmd: u32) -> Hwnd;
    fn GetWindowLongW(hwnd: Hwnd, index: i32) -> i32;
    fn IsWindowVisible(hwnd: Hwnd) -> Bool;
    fn IsIconic(hwnd: Hwnd) -> Bool;
    fn ShowWindow(hwnd: Hwnd, cmd_show: i32) -> Bool;
    fn BringWindowToTop(hwnd: Hwnd) -> Bool;
    fn SetWindowPos(hwnd: Hwnd, insert_after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> Bool;
    fn SetForegroundWindow(hwnd: Hwnd) -> Bool;
    fn SwitchToThisWindow(hwnd: Hwnd, alt_tab: Bool);
    fn AllowSetForegroundWindow(process_id: u32) -> Bool;
    fn AttachThreadInput(id_attach: u32, id_attach_to: u32, attach: Bool) -> Bool;
    fn keybd_event(vk: u8, scan: u8, flags: u32, extra_info: usize);
    fn EnumWindows(enum_func: unsafe extern "system" fn(Hwnd, isize) -> Bool, lparam: isize) -> Bool;
    fn EnumChildWindows(parent: Hwnd, enum_func: unsafe extern "system" fn(Hwnd, isize) -> Bool, lparam: isize) -> Bool;
    fn OpenInputDesktop(flags: u32, inherit: Bool, desired_access: u32) -> Handle;
    fn SetThreadDesktop(desktop: Handle) -> Bool;
    fn CloseDesktop(desktop: Handle) -> Bool;
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmGetWindowAttribute(hwnd: Hwnd, attribute: u32, pv_attribute: *mut c_void, cb_attribute: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
    fn OpenProcess(desired_access: u32, inherit_handle: Bool, process_id: u32) -> Handle;
    fn QueryFullProcessImageNameW(process: Handle, flags: u32, exe_name: *mut u16, size: *mut u32) -> Bool;
    fn CreateProcessW(
        application_name: *const u16,
        command_line: *mut u16,
        process_attributes: *const c_void,
        thread_attributes: *const c_void,
        inherit_handles: Bool,
        creation_flags: u32,
        environment: *const c_void,
        current_directory: *const u16,
        startup_info: *const StartupInfoW,
        process_information: *mut ProcessInformation,
    ) -> Bool;
    fn CloseHandle(object: Handle) -> Bool;
}

/// When `knoend` is started from a coding agent's sandboxed shell on Windows (e.g.
/// `codex-windows-sandbox-service` in Antigravity/Codex), the process begins on an isolated
/// non-interactive desktop where `GetForegroundWindow()` is 0 and `EnumWindows` is empty.
/// Attaching the calling thread to the active input desktop lets it see the user's real windows.
fn attach_input_desktop() {
    unsafe {
        let desk = OpenInputDesktop(0, 0, GENERIC_ALL);
        if !desk.is_null() && SetThreadDesktop(desk) == 0 {
            CloseDesktop(desk);
        }
    }
}

pub struct Watcher;

impl Watcher {
    pub fn connect() -> Result<Watcher, String> {
        attach_input_desktop();
        Ok(Watcher)
    }

    pub fn current(&self) -> Option<Focus> {
        attach_input_desktop();
        let hwnd = unsafe { GetForegroundWindow() };
        focus_for_window(hwnd)
    }

    pub fn wait_for_change(&self, previous: Option<&Focus>) -> Option<Focus> {
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let now = self.current();
            let changed = match (&now, previous) {
                (Some(a), Some(b)) => a.app != b.app || a.instance != b.instance || a.title != b.title,
                (None, None) => false,
                _ => true,
            };
            if changed {
                return now;
            }
        }
    }
}

/// Window classes of the Windows desktop and taskbar: focusing them means "nothing focused"
/// (shows the launcher on the board), whereas `CabinetWClass` / `ExploreWClass` are real File
/// Explorer folder windows.
pub fn is_desktop_or_shell_class(class: &str, exe_name: &str) -> bool {
    if matches!(
        class,
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "NotifyIconOverflowWindow"
            | "XamlExplorerHostIslandWindow"
            | "Windows.UI.Core.CoreWindow"
    ) {
        return true;
    }
    if matches!(
        exe_name.to_ascii_lowercase().as_str(),
        "pickerhost.exe"
            | "searchhost.exe"
            | "startmenuexperiencehost.exe"
            | "shellexperiencehost.exe"
            | "shellhost.exe"
            | "lockapp.exe"
            | "textinputhost.exe"
    ) {
        return true;
    }
    exe_name.eq_ignore_ascii_case("explorer.exe")
        && !matches!(class, "CabinetWClass" | "ExploreWClass")
}

/// Splits an executable file name (`"Code.exe"`) into `(app, instance)` = `("Code.exe", "Code")`.
pub fn app_and_stem(file_name: &str) -> Option<(String, String)> {
    let app = file_name.trim();
    if app.is_empty() {
        return None;
    }
    let stem = app
        .strip_suffix(".exe")
        .or_else(|| app.strip_suffix(".EXE"))
        .or_else(|| {
            if app.len() > 4 && app[app.len() - 4..].eq_ignore_ascii_case(".exe") {
                Some(&app[..app.len() - 4])
            } else {
                None
            }
        })
        .unwrap_or(app);
    Some((app.to_string(), stem.to_string()))
}

/// Whether a Windows executable (`app` = `"Code.exe"`, `instance` = `"Code"`) matches `name`.
pub fn app_matches(app: &str, instance: &str, name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    let name_stem = name.strip_suffix(".exe").unwrap_or(&name);
    !name.is_empty()
        && [app, instance].iter().any(|part| {
            let part = part.to_ascii_lowercase();
            let part_stem = part.strip_suffix(".exe").unwrap_or(&part);
            part == name
                || part_stem == name_stem
                || part.ends_with(&format!(".{name}"))
                || part_stem.ends_with(&format!(".{name_stem}"))
                || name_stem.ends_with(&format!(".{part_stem}"))
        })
}

fn window_class(hwnd: Hwnd) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

fn raw_window_pid(hwnd: Hwnd) -> Option<u32> {
    if hwnd == 0 {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    (pid != 0).then_some(pid)
}

/// For UWP windows hosted by `ApplicationFrameHost.exe`, finds the child window belonging to the
/// real app process.
fn resolve_uwp_pid(hwnd: Hwnd, host_pid: u32) -> u32 {
    struct Search {
        host_pid: u32,
        child_pid: u32,
    }
    unsafe extern "system" fn visit(child: Hwnd, lparam: isize) -> Bool {
        let search = &mut *(lparam as *mut Search);
        let mut pid = 0u32;
        GetWindowThreadProcessId(child, &mut pid);
        if pid != 0 && pid != search.host_pid {
            search.child_pid = pid;
            return 0; // stop enumeration
        }
        1
    }
    let mut search = Search { host_pid, child_pid: 0 };
    unsafe { EnumChildWindows(hwnd, visit, (&mut search as *mut Search) as isize) };
    if search.child_pid != 0 { search.child_pid } else { host_pid }
}

/// Full path to the executable of process `pid`, or None if the process cannot be queried.
pub fn process_exe(pid: u32) -> Option<PathBuf> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 || len == 0 {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

fn effective_pid_and_exe(hwnd: Hwnd) -> Option<(u32, PathBuf)> {
    let pid = raw_window_pid(hwnd)?;
    let exe = process_exe(pid)?;
    if exe.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case("ApplicationFrameHost.exe")) {
        let real_pid = resolve_uwp_pid(hwnd, pid);
        if real_pid != pid {
            if let Some(real_exe) = process_exe(real_pid) {
                return Some((real_pid, real_exe));
            }
        }
    }
    Some((pid, exe))
}

fn focus_for_window(hwnd: Hwnd) -> Option<Focus> {
    if hwnd == 0 {
        return None;
    }
    let class = window_class(hwnd);
    let (pid, exe) = effective_pid_and_exe(hwnd)?;
    let file_name = exe.file_name()?.to_str()?;
    if is_desktop_or_shell_class(&class, file_name) {
        return None;
    }
    let (app, instance) = app_and_stem(file_name)?;
    let title = window_title(hwnd, Some(pid));
    Some(Focus { os: Os::Windows, app, instance, title, web: None, pid: None, terminal: None })
}

fn raw_window_title(hwnd: Hwnd) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let copied = GetWindowTextW(hwnd, buf.as_mut_ptr(), len + 1);
        String::from_utf16_lossy(&buf[..copied.max(0) as usize])
    }
}

fn find_process_title(target_pid: u32) -> String {
    attach_input_desktop();
    struct Search {
        pid: u32,
        title: String,
    }
    unsafe extern "system" fn check_window(hwnd: Hwnd, lparam: isize) -> Bool {
        let search = &mut *(lparam as *mut Search);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == search.pid && IsWindowVisible(hwnd) != 0 {
            let t = raw_window_title(hwnd);
            if !t.is_empty() {
                let lower = t.to_ascii_lowercase();
                if lower.contains("slide show") || lower.contains("diaporama") {
                    search.title = t;
                    return 0; // slide show window has highest priority
                }
                if search.title.is_empty() {
                    search.title = t;
                }
            }
        }
        1
    }
    let mut search = Search { pid: target_pid, title: String::new() };
    unsafe { EnumWindows(check_window, &mut search as *mut _ as isize) };
    search.title
}

fn window_title(hwnd: Hwnd, pid: Option<u32>) -> String {
    let title = raw_window_title(hwnd);
    if !title.is_empty() {
        return title;
    }
    if let Some(pid) = pid {
        let proc_title = find_process_title(pid);
        if !proc_title.is_empty() {
            return proc_title;
        }
    }
    String::new()
}

unsafe fn is_main_window(hwnd: Hwnd) -> bool {
    if IsWindowVisible(hwnd) == 0 {
        return false;
    }
    let mut cloaked: u32 = 0;
    if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), 4) == 0 && cloaked != 0 {
        return false;
    }
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
    if (ex_style & (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)) != 0 {
        return false;
    }
    if GetWindow(hwnd, GW_OWNER) != 0 && (ex_style & WS_EX_APPWINDOW) == 0 {
        return false;
    }
    GetWindowTextLengthW(hwnd) > 0
}

fn find_window(names: &[&str]) -> Option<(Hwnd, u32)> {
    attach_input_desktop();
    // Check foreground window first if it is a real main window.
    let fg = unsafe { GetForegroundWindow() };
    if unsafe { is_main_window(fg) } {
        if let Some((pid, exe)) = effective_pid_and_exe(fg) {
            if let Some((app, instance)) = exe.file_name().and_then(|n| n.to_str()).and_then(app_and_stem) {
                let class = window_class(fg);
                if !is_desktop_or_shell_class(&class, &app) && names.iter().any(|n| app_matches(&app, &instance, n)) {
                    return Some((fg, pid));
                }
            }
        }
    }
    struct Target<'a> {
        names: &'a [&'a str],
        found: Option<(Hwnd, u32)>,
        fallback: Option<(Hwnd, u32)>,
    }
    unsafe extern "system" fn visit(hwnd: Hwnd, lparam: isize) -> Bool {
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let target = &mut *(lparam as *mut Target<'_>);
        if let Some((pid, exe)) = effective_pid_and_exe(hwnd) {
            if let Some((app, instance)) = exe.file_name().and_then(|n| n.to_str()).and_then(app_and_stem) {
                let class = window_class(hwnd);
                if !is_desktop_or_shell_class(&class, &app) && target.names.iter().any(|n| app_matches(&app, &instance, n)) {
                    if is_main_window(hwnd) {
                        target.found = Some((hwnd, pid));
                        return 0;
                    }
                    if target.fallback.is_none() {
                        target.fallback = Some((hwnd, pid));
                    }
                }
            }
        }
        1
    }
    let mut target = Target { names, found: None, fallback: None };
    unsafe { EnumWindows(visit, (&mut target as *mut Target<'_>) as isize) };
    target.found.or(target.fallback)
}

unsafe fn unlock_foreground() {
    keybd_event(VK_NONAME, 0, 0, 0);
    keybd_event(VK_NONAME, 0, KEYEVENTF_KEYUP, 0);
    AllowSetForegroundWindow(ASFW_ANY);
}

/// Brings the first visible main window of the app called any of `names` to the front.
pub fn bring_to_front(names: &[&str]) -> Result<(), String> {
    attach_input_desktop();
    let (hwnd, _) = find_window(names).ok_or_else(|| format!("no open window of {}", names.join(" / ")))?;
    unsafe {
        let fg = GetForegroundWindow();
        let cur_tid = GetCurrentThreadId();
        let fg_tid = if fg != 0 { GetWindowThreadProcessId(fg, std::ptr::null_mut()) } else { 0 };
        let target_tid = GetWindowThreadProcessId(hwnd, std::ptr::null_mut());
        let attached_fg = fg_tid != 0 && fg_tid != cur_tid && AttachThreadInput(cur_tid, fg_tid, 1) != 0;
        let attached_target =
            target_tid != 0 && target_tid != cur_tid && target_tid != fg_tid && AttachThreadInput(cur_tid, target_tid, 1) != 0;

        unlock_foreground();

        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        let was_topmost = (GetWindowLongW(hwnd, GWL_EXSTYLE) & WS_EX_TOPMOST) != 0;
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        if !was_topmost {
            SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        }
        BringWindowToTop(hwnd);
        let ok = SetForegroundWindow(hwnd);
        SwitchToThisWindow(hwnd, 1);

        if attached_target {
            AttachThreadInput(cur_tid, target_tid, 0);
        }
        if attached_fg {
            AttachThreadInput(cur_tid, fg_tid, 0);
        }

        if ok == 0 && GetForegroundWindow() != hwnd {
            return Err(format!("Windows refused to focus {}", names.join(" / ")));
        }
    }
    Ok(())
}

/// Spawns a command line on the interactive `Winsta0\Default` desktop so newly opened apps and
/// browser tabs appear on the user's screen even if `knoend` was started from a sandboxed shell.
pub fn spawn_on_input_desktop(command_line: &str) -> Result<(), String> {
    attach_input_desktop();
    unsafe {
        unlock_foreground();
        let mut desktop: Vec<u16> = "Winsta0\\Default".encode_utf16().chain(std::iter::once(0)).collect();
        let mut cmd: Vec<u16> = command_line.encode_utf16().chain(std::iter::once(0)).collect();
        let mut si: StartupInfoW = std::mem::zeroed();
        si.cb = std::mem::size_of::<StartupInfoW>() as u32;
        si.lp_desktop = desktop.as_mut_ptr();
        let mut pi: ProcessInformation = std::mem::zeroed();
        let ok = CreateProcessW(
            std::ptr::null(),
            cmd.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NEW_CONSOLE,
            std::ptr::null(),
            std::ptr::null(),
            &si,
            &mut pi,
        );
        if ok == 0 {
            return Err(format!("could not start {command_line}"));
        }
        if !pi.h_thread.is_null() {
            CloseHandle(pi.h_thread);
        }
        if !pi.h_process.is_null() {
            CloseHandle(pi.h_process);
        }
        Ok(())
    }
}

/// The process id of the first visible window of the app called any of `names`.
pub fn window_pid(names: &[&str]) -> Option<u32> {
    find_window(names).map(|(_, pid)| pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executables_split_into_app_and_stem() {
        assert_eq!(app_and_stem("Code.exe"), Some(("Code.exe".into(), "Code".into())));
        assert_eq!(app_and_stem("FIREFOX.EXE"), Some(("FIREFOX.EXE".into(), "FIREFOX".into())));
        assert_eq!(app_and_stem(""), None);
    }

    #[test]
    fn desktop_and_taskbar_classes_are_ignored_while_folder_windows_count() {
        assert!(is_desktop_or_shell_class("Progman", "explorer.exe"));
        assert!(is_desktop_or_shell_class("WorkerW", "explorer.exe"));
        assert!(is_desktop_or_shell_class("Shell_TrayWnd", "explorer.exe"));
        assert!(!is_desktop_or_shell_class("CabinetWClass", "explorer.exe"));
        assert!(!is_desktop_or_shell_class("Chrome_WidgetWin_1", "Code.exe"));
    }

    #[test]
    fn app_names_match_with_or_without_exe() {
        assert!(app_matches("Code.exe", "Code", "Code.exe"));
        assert!(app_matches("Code.exe", "Code", "code"));
        assert!(app_matches("Claude.exe", "Claude", "com.anthropic.Claude"));
        assert!(!app_matches("claude-helper.exe", "claude-helper", "Claude"));
        assert!(!app_matches("Code.exe", "Code", ""));
    }
}
