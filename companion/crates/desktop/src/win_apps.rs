//! Windows app paths and 32x32 RGBA icons for the board's launcher.
//! Remembers executable paths seen while apps are in focus, checks the Windows `App Paths`
//! registry when needed, and extracts 32x32 icons from `.exe` files via `SHGetFileInfoW`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static PATHS: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

fn cache_file() -> PathBuf {
    let root = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
        PathBuf::from(home).join(".config")
    });
    root.join("knoend").join("win-apps.txt")
}

fn load_map() -> HashMap<String, PathBuf> {
    std::fs::read_to_string(cache_file())
        .ok()
        .map(|t| {
            t.lines()
                .filter_map(|l| l.split_once('='))
                .map(|(k, v)| (k.trim().to_string(), PathBuf::from(v.trim())))
                .collect()
        })
        .unwrap_or_default()
}

fn save_map(map: &HashMap<String, PathBuf>) {
    let file = cache_file();
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut lines: Vec<String> = map
        .iter()
        .filter_map(|(k, v)| v.to_str().map(|s| format!("{k}={s}")))
        .collect();
    lines.sort();
    let _ = std::fs::write(file, lines.join("\n"));
}

/// Normalises an executable id (`"Code.exe"` -> `"code.exe"`, `"code"` -> `"code.exe"`).
pub fn normalise_exe_id(id: &str) -> String {
    let trimmed = id.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.ends_with(".exe") {
        trimmed
    } else {
        format!("{trimmed}.exe")
    }
}

/// Records the full `.exe` path for a running app so the launcher can re-open it and read its icon.
pub fn remember_exe(id: &str, path: PathBuf) {
    let key = normalise_exe_id(id);
    if key.is_empty() || !path.exists() {
        return;
    }
    let Ok(mut guard) = PATHS.lock() else { return };
    let map = guard.get_or_insert_with(load_map);
    if map.get(&key) != Some(&path) {
        map.insert(key, path);
        save_map(map);
    }
}

/// Finds the full `.exe` path for `id` from our remembered map, `App Paths` in the Registry, or `PATH`.
pub fn exe_path(id: &str) -> Option<PathBuf> {
    let key = normalise_exe_id(id);
    if key.is_empty() {
        return None;
    }
    if let Ok(mut guard) = PATHS.lock() {
        let map = guard.get_or_insert_with(load_map);
        if let Some(p) = map.get(&key).filter(|p| p.exists()) {
            return Some(p.clone());
        }
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(&key);
            if candidate.is_file() {
                remember_exe(&key, candidate.clone());
                return Some(candidate);
            }
        }
    }
    let windir = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    for dir in [&windir, &windir.join("System32")] {
        let candidate = dir.join(&key);
        if candidate.is_file() {
            remember_exe(&key, candidate.clone());
            return Some(candidate);
        }
    }
    None
}

/// RGBA pixels (`size` x `size`) of a Windows executable's icon, cached in memory.
pub fn icon(id: &str, size: u32) -> Option<Vec<u8>> {
    static CACHE: Mutex<Option<HashMap<String, Option<Vec<u8>>>>> = Mutex::new(None);
    let key = format!("{}|{size}", normalise_exe_id(id));
    if let Some(hit) = CACHE.lock().ok()?.get_or_insert_with(HashMap::new).get(&key) {
        return hit.clone();
    }
    let path = exe_path(id)?;
    let pixels = extract_exe_icon_rgba(&path, size);
    if let Ok(mut cache) = CACHE.lock() {
        cache.get_or_insert_with(HashMap::new).insert(key, pixels.clone());
    }
    pixels
}

#[cfg(target_os = "windows")]
fn extract_exe_icon_rgba(path: &Path, size: u32) -> Option<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct ShFileInfoW {
        h_icon: usize,
        i_icon: i32,
        dw_attributes: u32,
        sz_display_name: [u16; 260],
        sz_type_name: [u16; 80],
    }

    #[repr(C)]
    struct IconInfo {
        f_icon: i32,
        x_hotspot: u32,
        y_hotspot: u32,
        hbm_mask: usize,
        hbm_color: usize,
    }

    #[repr(C)]
    struct BitmapInfoHeader {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels_per_meter: i32,
        bi_y_pels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    #[link(name = "shell32")]
    extern "system" {
        fn SHGetFileInfoW(
            psz_path: *const u16,
            dw_file_attributes: u32,
            psfi: *mut ShFileInfoW,
            cb_file_info: u32,
            u_flags: u32,
        ) -> usize;
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetIconInfo(h_icon: usize, piconinfo: *mut IconInfo) -> i32;
        fn DestroyIcon(h_icon: usize) -> i32;
        fn GetDC(hwnd: usize) -> usize;
        fn ReleaseDC(hwnd: usize, hdc: usize) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn GetDIBits(
            hdc: usize,
            hbmp: usize,
            u_start_scan: u32,
            c_scan_lines: u32,
            lpv_bits: *mut u8,
            lpbi: *mut BitmapInfoHeader,
            u_usage: u32,
        ) -> i32;
        fn DeleteObject(ho: usize) -> i32;
    }

    const SHGFI_ICON: u32 = 0x0000_0100;
    const SHGFI_LARGEICON: u32 = 0x0000_0000;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let mut shfi: ShFileInfoW = std::mem::zeroed();
        let res = SHGetFileInfoW(
            wide.as_ptr(),
            0,
            &mut shfi,
            std::mem::size_of::<ShFileInfoW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if res == 0 || shfi.h_icon == 0 {
            return None;
        }
        let mut ii: IconInfo = std::mem::zeroed();
        if GetIconInfo(shfi.h_icon, &mut ii) == 0 {
            DestroyIcon(shfi.h_icon);
            return None;
        }
        let w = 32i32;
        let h = 32i32;
        let mut bih = BitmapInfoHeader {
            bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            bi_width: w,
            bi_height: -h, // top-down DIB
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: 0, // BI_RGB
            bi_size_image: 0,
            bi_x_pels_per_meter: 0,
            bi_y_pels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        };
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let hdc = GetDC(0);
        let lines = if ii.hbm_color != 0 {
            GetDIBits(hdc, ii.hbm_color, 0, h as u32, bgra.as_mut_ptr(), &mut bih, 0)
        } else {
            0
        };
        ReleaseDC(0, hdc);
        if ii.hbm_color != 0 {
            DeleteObject(ii.hbm_color);
        }
        if ii.hbm_mask != 0 {
            DeleteObject(ii.hbm_mask);
        }
        DestroyIcon(shfi.h_icon);
        if lines == 0 {
            return None;
        }
        let any_alpha = bgra.chunks_exact(4).any(|p| p[3] != 0);
        let mut rgba = Vec::with_capacity(bgra.len());
        for p in bgra.chunks_exact(4) {
            let a = if any_alpha { p[3] } else if p[0] != 0 || p[1] != 0 || p[2] != 0 { 255 } else { 0 };
            rgba.extend_from_slice(&[p[2], p[1], p[0], a]);
        }
        if size == 32 {
            Some(rgba)
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn extract_exe_icon_rgba(_path: &Path, _size: u32) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_exe_ids() {
        assert_eq!(normalise_exe_id("Code.EXE"), "code.exe");
        assert_eq!(normalise_exe_id("firefox"), "firefox.exe");
        assert_eq!(normalise_exe_id(""), "");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn extracts_explorer_icon_on_windows() {
        let pixels = icon("explorer.exe", 32).expect("explorer.exe should have a 32x32 icon");
        assert_eq!(pixels.len(), 32 * 32 * 4);
        assert!(pixels.iter().any(|&b| b != 0));
    }
}
