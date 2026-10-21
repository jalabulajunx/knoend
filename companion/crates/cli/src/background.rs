//! Running `knoend serve` in the background, started at login and restarted if it stops:
//! a systemd user service on Linux, a LaunchAgent on macOS. Both run this binary as it is now
//! (its full path), without opening the browser.

use std::path::PathBuf;
use std::process::Command;

const UNIT: &str = "knoend.service";
/// Names from before the rename: removed when installing, so only one service runs.
const OLD_UNIT: &str = "openpager.service";
const AGENT: &str = "io.github.jalabulajunx.knoend";
const OLD_AGENT: &str = "io.github.jalabulajunx.openpager";
const WIN_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const WIN_RUN_VALUE: &str = "KnoEnd";
const OLD_WIN_RUN_VALUE: &str = "OpenPager";

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn exe() -> Result<PathBuf, String> {
    crate::browser::clean_exe_path()
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
    if out.status.success() { Ok(text) } else { Err(format!("{program} {}: {text}", args.join(" "))) }
}

/// The systemd user unit. It needs the graphical session's DISPLAY/XAUTHORITY to follow focus and
/// type: systemd user managers started by a desktop session have them; if they arrive late,
/// `Restart=always` tries again until they do.
pub fn systemd_unit(exe: &str) -> String {
    format!("[Unit]
Description=KnoEnd companion (keys follow the focused app; board link)
Documentation=https://github.com/jalabulajunx/knoend
After=graphical-session.target

[Service]
ExecStart={exe} serve --no-open
Restart=always
RestartSec=3

[Install]
WantedBy=default.target
")
}

/// The macOS LaunchAgent: runs at login, kept alive. Accessibility (typing keys) must then be
/// allowed for the `knoend` binary itself, as nothing else launches it.
pub fn launch_agent(exe: &str) -> String {
    let log = home().join("Library/Logs/knoend.log");
    format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{AGENT}</string>
  <key>ProgramArguments</key><array><string>{exe}</string><string>serve</string><string>--no-open</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#, log = log.display())
}

fn unit_path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"));
    config.join("systemd/user").join(UNIT)
}

fn agent_path() -> PathBuf {
    home().join("Library/LaunchAgents").join(format!("{AGENT}.plist"))
}

/// Stops and removes a background service installed under the name from before the rename.
fn remove_old() -> Option<String> {
    if cfg!(target_os = "macos") {
        let path = home().join("Library/LaunchAgents").join(format!("{OLD_AGENT}.plist"));
        if !path.exists() {
            return None;
        }
        let _ = run("launchctl", &["unload", "-w", path.to_str().unwrap_or_default()]);
        let _ = std::fs::remove_file(&path);
        return Some(format!("removed the older LaunchAgent {}", path.display()));
    }
    if cfg!(target_os = "windows") {
        if run("reg", &["query", WIN_RUN_KEY, "/v", OLD_WIN_RUN_VALUE]).is_ok() {
            let _ = run("reg", &["delete", WIN_RUN_KEY, "/v", OLD_WIN_RUN_VALUE, "/f"]);
            return Some(format!("removed the older startup entry {OLD_WIN_RUN_VALUE}"));
        }
        return None;
    }
    let path = unit_path().with_file_name(OLD_UNIT);
    if !path.exists() {
        return None;
    }
    let _ = run("systemctl", &["--user", "disable", "--now", OLD_UNIT]);
    let _ = std::fs::remove_file(&path);
    Some(format!("removed the older service {}", path.display()))
}

/// Installs and starts the background service; returns what was done, line by line.
pub fn install() -> Result<Vec<String>, String> {
    let exe = exe()?;
    let exe = exe.to_str().ok_or("the knoend path is not valid UTF-8")?;
    let old = remove_old();
    if cfg!(target_os = "macos") {
        let path = agent_path();
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&path, launch_agent(exe)).map_err(|e| format!("{}: {e}", path.display()))?;
        let _ = run("launchctl", &["unload", path.to_str().unwrap_or_default()]);
        run("launchctl", &["load", "-w", path.to_str().unwrap_or_default()])?;
        return Ok(old.into_iter().chain([format!("LaunchAgent {} loaded; runs at login", path.display()),
                       "Allow knoend under System Settings > Privacy & Security > Accessibility so it can type keys".into()]).collect());
    }
    if cfg!(target_os = "windows") {
        let cmd = format!("\"{exe}\" serve --no-open");
        run("reg", &["add", WIN_RUN_KEY, "/v", WIN_RUN_VALUE, "/t", "REG_SZ", "/d", &cmd, "/f"])?;
        #[cfg(target_os = "windows")]
        if std::net::TcpStream::connect_timeout(
            &"127.0.0.1:7821".parse().unwrap(),
            std::time::Duration::from_millis(150),
        )
        .is_err()
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let _ = Command::new(exe)
                .args(["serve", "--no-open"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        }
        return Ok(old
            .into_iter()
            .chain([format!("{WIN_RUN_KEY}\\{WIN_RUN_VALUE} registered; starts at login and running in the background")])
            .collect());
    }
    let path = unit_path();
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, systemd_unit(exe)).map_err(|e| format!("{}: {e}", path.display()))?;
    run("systemctl", &["--user", "daemon-reload"])?;
    run("systemctl", &["--user", "enable", "--now", UNIT])?;
    Ok(old.into_iter().chain([format!("{} written; enabled and started (starts at login)", path.display()),
            "Logs: journalctl --user -u knoend -f".into()]).collect())
}

/// Stops and removes the background service.
pub fn uninstall() -> Result<Vec<String>, String> {
    if cfg!(target_os = "macos") {
        let path = agent_path();
        let _ = run("launchctl", &["unload", "-w", path.to_str().unwrap_or_default()]);
        let _ = std::fs::remove_file(&path);
        return Ok(vec![format!("LaunchAgent {} removed", path.display())]);
    }
    if cfg!(target_os = "windows") {
        let removed = run("reg", &["delete", WIN_RUN_KEY, "/v", WIN_RUN_VALUE, "/f"]).is_ok();
        return Ok(vec![if removed {
            format!("{WIN_RUN_KEY}\\{WIN_RUN_VALUE} removed")
        } else {
            "no background startup entry was installed".into()
        }]);
    }
    let _ = run("systemctl", &["--user", "disable", "--now", UNIT]);
    let path = unit_path();
    let removed = std::fs::remove_file(&path).is_ok();
    let _ = run("systemctl", &["--user", "daemon-reload"]);
    Ok(vec![if removed { format!("{} stopped and removed", path.display()) } else { "no background service was installed".into() }])
}

/// One line on whether the background service is installed and running.
pub fn status() -> String {
    if cfg!(target_os = "macos") {
        return match run("launchctl", &["list", AGENT]) {
            Ok(_) => format!("LaunchAgent {AGENT}: loaded"),
            Err(_) => "not installed (knoend service install)".into(),
        };
    }
    if cfg!(target_os = "windows") {
        return match run("reg", &["query", WIN_RUN_KEY, "/v", WIN_RUN_VALUE]) {
            Ok(_) => format!("Windows startup ({WIN_RUN_VALUE}): registered at login"),
            Err(_) => "not installed (knoend service install)".into(),
        };
    }
    if !unit_path().exists() {
        return "not installed (knoend service install)".into();
    }
    let active = run("systemctl", &["--user", "is-active", UNIT]).unwrap_or_else(|e| e);
    let enabled = run("systemctl", &["--user", "is-enabled", UNIT]).unwrap_or_else(|e| e);
    format!("{UNIT}: {} and {} at login", active.lines().last().unwrap_or(""), enabled.lines().last().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_runs_this_binary_and_restarts() {
        let unit = systemd_unit("/opt/knoend/knoend");
        assert!(unit.contains("ExecStart=/opt/knoend/knoend serve --no-open"));
        assert!(unit.contains("Restart=always"));
        assert!(unit.contains("WantedBy=default.target"));
        let plist = launch_agent("/Applications/knoend");
        assert!(plist.contains("<string>/Applications/knoend</string><string>serve</string><string>--no-open</string>"));
        assert!(plist.contains("<key>KeepAlive</key><true/>"));
    }
}
