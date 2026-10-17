//! Things on a Mac only the person can approve, brought to the screen for them: the settings page
//! to switch on, or a USB driver's installer. A fixed list; nothing is approved or installed here.
//! The setup assistant offers these as buttons (see `openpager_agents::assistant::ACTIONS`).

/// WCH's macOS driver for its USB-serial chips (needed for the CH340K, 1a86:7522), from WCH's
/// own GitHub account.
const WCH_DRIVER_URL: &str = "https://raw.githubusercontent.com/WCHSoftGroup/ch34xser_macos/main/CH34xVCPDriver.pkg";
/// Developer ID team of Nanjing Qinheng Microelectronics (WCH), which signs that installer.
const WCH_TEAM: &str = "5JZGQTGU4W";

/// Opens `target` on this computer: "accessibility_settings", "driver_settings" or "usb_driver".
pub async fn open(target: &str) -> Result<String, String> {
    if !cfg!(target_os = "macos") {
        return Err("only needed on macOS".into());
    }
    match target {
        "accessibility_settings" => {
            open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")?;
            Ok("Opened Privacy & Security > Accessibility".into())
        }
        "driver_settings" => {
            open_url("x-apple.systempreferences:com.apple.LoginItems-Settings.extension")?;
            Ok("Opened Login Items & Extensions (Driver Extensions)".into())
        }
        "usb_driver" => {
            let bytes = reqwest::get(WCH_DRIVER_URL).await.and_then(|r| r.error_for_status())
                .map_err(|e| format!("could not download WCH's driver: {e}"))?
                .bytes().await.map_err(|e| e.to_string())?;
            let path = std::env::temp_dir().join("openpager-CH34xVCPDriver.pkg");
            std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            // Gatekeeper must accept it as notarized and signed by WCH before it is opened.
            let check = std::process::Command::new("spctl").args(["-a", "-vv", "-t", "install"]).arg(&path)
                .output().map_err(|e| e.to_string())?;
            let report = format!("{}{}", String::from_utf8_lossy(&check.stdout), String::from_utf8_lossy(&check.stderr));
            if !check.status.success() || !report.contains("Notarized Developer ID") || !report.contains(WCH_TEAM) {
                let _ = std::fs::remove_file(&path);
                return Err(format!("the downloaded driver did not pass Apple's check, so it was not opened: {}", report.trim()));
            }
            open_url(path.to_str().unwrap_or_default())?;
            Ok("Opened WCH's driver installer (notarized, signed by WCH)".into())
        }
        other => Err(format!("unknown target {other:?}")),
    }
}

fn open_url(url: &str) -> Result<(), String> {
    let status = std::process::Command::new("open").arg(url).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("could not open {url}")) }
}
