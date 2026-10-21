//! Things on a Mac only the person can approve, brought to the screen for them: the settings page
//! to switch on, or a USB driver's installer. A fixed list; nothing is approved or installed here.
//! The setup assistant offers these as buttons (see `knoend_agents::assistant::ACTIONS`).

/// WCH's macOS driver for its USB-serial chips (needed for the CH340K, 1a86:7522), from WCH's
/// own GitHub account.
const WCH_DRIVER_URL: &str = "https://raw.githubusercontent.com/WCHSoftGroup/ch34xser_macos/main/CH34xVCPDriver.pkg";
/// Developer ID team of Nanjing Qinheng Microelectronics (WCH), which signs that installer.
const WCH_TEAM: &str = "5JZGQTGU4W";

/// Opens `target` on this computer: "bluetooth_settings" (all platforms), or on macOS
/// "accessibility_settings", "driver_settings" or "usb_driver".
pub async fn open(target: &str) -> Result<String, String> {
    if target == "bluetooth_settings" {
        return open_bluetooth_settings();
    }
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
            // A new folder only this user can write, so nothing can swap the file between the
            // check and the opening.
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            let dir = std::env::temp_dir().join(format!("knoend-driver-{}-{nanos}", std::process::id()));
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700).create(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?;
            #[cfg(not(unix))]
            std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = dir.join("CH34xVCPDriver.pkg");
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

fn open_bluetooth_settings() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        knoend_focus::spawn_on_input_desktop("explorer.exe ms-settings:bluetooth")?;
        return Ok("Opened Windows Settings > Bluetooth & devices".into());
    }
    #[cfg(target_os = "macos")]
    {
        open_url("x-apple.systempreferences:com.apple.BluetoothSettings")?;
        return Ok("Opened System Settings > Bluetooth".into());
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        for (cmd, args) in [
            ("blueman-manager", &[][..]),
            ("gnome-control-center", &["bluetooth"][..]),
            ("systemsettings", &["kcm_bluetooth"][..]),
            ("overskride", &[][..]),
        ] {
            if std::process::Command::new(cmd).args(args)
                .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
                .spawn().is_ok()
            {
                return Ok(format!("Opened Bluetooth settings ({cmd})"));
            }
        }
        Err("could not find a Bluetooth settings app; open your desktop's Bluetooth settings to pair \"KnoEnd\"".into())
    }
}
