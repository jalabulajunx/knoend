//! What runs in a terminal app's active tab, from the process table (`ps`, macOS and Linux): the
//! app's tabs are the terminals its child processes run on, and its program is that terminal's
//! foreground process group. No permission needed, and the same for every terminal app.
//!
//! Which tab is active: first the window's title, which most terminal apps take from the active
//! tab and shells such as fish and zsh fill with the running command and the folder ("man ls
//! ~/Projects"); then the tab typed in last (a terminal device's access time moves on input).
//! Linux keeps that access time to 8 s, so where the title says nothing (bash at a prompt sets the
//! same title in every tab of one folder), switching tabs and typing within 8 s can keep the
//! previous tab's keys until the next input after that.
//!
//! `sudo` runs its command on a terminal of its own (`use_pty`), opened from the tab: that terminal
//! is part of the tab, and its program follows `sudo` in the tab's programs.

use std::path::PathBuf;
use std::time::SystemTime;

use knoend_core::focus::TerminalTab;

/// The active tab of the terminal app with process id `app`, whose window is titled `title`, or
/// None when it runs no terminal.
pub fn active_tab(app: u32, title: &str) -> Option<TerminalTab> {
    let out = std::process::Command::new("ps").args(["-A", "-o", "pid=,ppid=,pgid=,tpgid=,tty=,args="]).output().ok()?;
    let table = parse(&String::from_utf8_lossy(&out.stdout));
    let home = std::env::var_os("HOME").map(PathBuf::from);
    pick(&table, app, title, home.as_deref(),
         |tty| std::fs::metadata(format!("/dev/{tty}")).and_then(|m| m.accessed()).ok(),
         |pid| std::fs::read_link(format!("/proc/{pid}/cwd")).ok())
}

#[derive(Debug)]
struct Process {
    pid: u32,
    ppid: u32,
    pgid: u32,
    tpgid: i64,
    tty: String,
    program: String,
}

fn parse(ps: &str) -> Vec<Process> {
    ps.lines().filter_map(|line| {
        let mut f = line.split_whitespace();
        let (pid, ppid, pgid, tpgid, tty) = (f.next()?.parse().ok()?, f.next()?.parse().ok()?, f.next()?.parse().ok()?,
                                             f.next()?.parse().ok()?, f.next()?.to_string());
        let args: Vec<&str> = f.collect();
        Some(Process { pid, ppid, pgid, tpgid, tty, program: program(&args) })
    }).collect()
}

/// The program's name from its command line: "/usr/bin/less" -> "less", a login shell's "-zsh" ->
/// "zsh", and for a script run by an interpreter its script ("node /opt/bin/claude" -> "claude").
fn program(args: &[&str]) -> String {
    let name = |arg: &str| arg.rsplit('/').next().unwrap_or(arg).trim_start_matches('-').to_string();
    let first = args.first().map(|a| name(a)).unwrap_or_default();
    const INTERPRETERS: &[&str] = &["node", "bun", "deno", "python", "python3"];
    match args.get(1).filter(|a| !a.starts_with('-')) {
        Some(script) if INTERPRETERS.contains(&first.as_str()) => name(script),
        _ => first,
    }
}

/// A terminal's foreground process group, the group's leader first. A process that ended but is
/// not yet collected shows as "<defunct>": not a program.
fn foreground<'a>(table: &'a [Process], tty: &str) -> Vec<&'a Process> {
    let mut found: Vec<&Process> = table.iter()
        .filter(|p| p.tty == tty && i64::from(p.pgid) == p.tpgid && !p.program.starts_with('<')).collect();
    found.sort_by_key(|p| (p.pid != p.pgid, p.pid));
    found
}

fn pick(table: &[Process], app: u32, title: &str, home: Option<&std::path::Path>,
        last_input: impl Fn(&str) -> Option<SystemTime>, cwd: impl Fn(u32) -> Option<PathBuf>) -> Option<TerminalTab> {
    // The app's descendants, and the terminals they run on.
    let mut family = vec![app];
    let mut i = 0;
    while i < family.len() {
        let parent = family[i];
        family.extend(table.iter().filter(|p| p.ppid == parent && p.pid != parent).map(|p| p.pid));
        i += 1;
    }
    let by_pid = |pid: u32| table.iter().find(|p| p.pid == pid);
    // Not the terminal the app itself was started from (an xterm launched from another terminal
    // keeps it): only its children's.
    let own = by_pid(app).map(|p| p.tty.as_str());
    let on_terminal = |p: &&Process| p.pid != app && family.contains(&p.pid) && has_tty(&p.tty) && Some(p.tty.as_str()) != own;
    // A terminal opened by a program in a tab (sudo's): the program running on it, by terminal.
    let nested: Vec<(&str, u32)> = table.iter().filter(on_terminal).filter_map(|p| {
        let parent = by_pid(p.ppid).filter(|q| q.pid != app && has_tty(&q.tty) && q.tty != p.tty && Some(q.tty.as_str()) != own)?;
        Some((p.tty.as_str(), parent.pid))
    }).collect();
    let mut ttys: Vec<&str> = table.iter().filter(on_terminal).map(|p| p.tty.as_str())
        .filter(|t| !nested.iter().any(|(n, _)| n == t)).collect();
    ttys.sort_unstable();
    ttys.dedup();

    // Each tab's programs: its foreground, then what runs on terminals it opened.
    let programs = |tty: &str| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut queue: Vec<&Process> = foreground(table, tty);
        let mut seen_ttys = vec![tty.to_string()];
        let mut i = 0;
        while i < queue.len() {
            let p = queue[i];
            if !out.contains(&p.program) {
                out.push(p.program.clone());
            }
            // A foreground program may have opened a terminal of its own (not a background job:
            // an xterm started from this tab is a window of its own).
            for (n, opener) in &nested {
                if *opener == p.pid && !seen_ttys.iter().any(|s| s == n) {
                    seen_ttys.push(n.to_string());
                    queue.extend(foreground(table, n));
                }
            }
            i += 1;
        }
        out
    };

    // The title first; among tabs it does not tell apart, the one typed in last.
    let words: Vec<&str> = title.split_whitespace().collect();
    let score = |tty: &str| -> u8 {
        let shown = programs(tty);
        let named = shown.iter().any(|p| !TerminalTab::is_wrapper(p) && words.iter().any(|w| w.rsplit('/').next() == Some(p.as_str())));
        let folder = foreground(table, tty).first().and_then(|p| cwd(p.pid)).is_some_and(|dir| {
            if home.is_some_and(|h| h == dir) {
                return words.iter().any(|w| *w == "~" || w.trim_end_matches(':').ends_with(":~"));
            }
            let Some(base) = dir.file_name().and_then(|b| b.to_str()) else { return false };
            words.iter().any(|w| w.split('/').any(|segment| segment.trim_end_matches(':') == base))
        });
        2 * u8::from(named) + u8::from(folder)
    };
    let scored: Vec<(&str, u8)> = ttys.iter().map(|t| (*t, score(t))).collect();
    let best = scored.iter().map(|(_, s)| *s).max()?;
    let tty = scored.iter().filter(|(_, s)| *s == best).max_by_key(|(t, _)| last_input(t))?.0.to_string();
    let programs = programs(&tty);
    Some(TerminalTab { tty, programs })
}

/// `ps` shows "??" (macOS) or "?" (Linux) for a process without a terminal.
fn has_tty(tty: &str) -> bool {
    !tty.is_empty() && !tty.starts_with('?')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Terminal.app (2218) with one tab, VS Code's terminal host (76388) with two; the second VS Code
    // tab runs `man less`, whose pager joins man's process group.
    const PS: &str = "\
 2218     1  2218     0 ??       /System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal
17099  2218 17099 17102 ttys000  login -pf tom
17102 17099 17102 17102 ttys000  -zsh
76388 76338 76338     0 ??       /Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper.app/Contents/MacOS/Code Helper --type=utility
84262 76388 84262 90001 ttys002  /bin/zsh -il
90001 84262 90001 90001 ttys002  man less
90007 90001 90001 90001 ttys002  less -is
90009 90001 90001 90001 ttys002  <defunct>
76574 76388 76574 76574 ttys003  /bin/zsh -il
 3000 17102  3000 17102 ttys000  xterm
 3001  3000  3001  3001 ttys009  bash
";

    #[test]
    fn the_tab_typed_in_last_and_its_foreground_programs() {
        let table = parse(PS);
        let at = |secs: u64| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
        let tab = pick(&table, 76388, "", None, |tty| if tty == "ttys002" { at(200) } else { at(100) }, |_| None).unwrap();
        assert_eq!(tab, TerminalTab { tty: "ttys002".into(), programs: vec!["man".into(), "less".into()] });
        let tab = pick(&table, 76388, "", None, |tty| if tty == "ttys003" { at(300) } else { at(100) }, |_| None).unwrap();
        assert_eq!(tab.programs, vec!["zsh"], "a shell at its prompt");
        let tab = pick(&table, 2218, "", None, |tty| if tty == "ttys000" { at(2) } else { at(1) }, |_| None).unwrap();
        assert_eq!(tab, TerminalTab { tty: "ttys000".into(), programs: vec!["zsh".into()] }, "login is not in the foreground");
        assert!(pick(&table, 1234, "", None, |_| at(1), |_| None).is_none(), "an app without terminals");
        let tab = pick(&table, 3000, "", None, |tty| if tty == "ttys000" { at(500) } else { at(1) }, |_| None).unwrap();
        assert_eq!(tab.tty, "ttys009", "an xterm started from Terminal: not Terminal's tab");
    }

    // Xfce Terminal (4276) with three tabs: `man ls` in ~/Projects, fish at its prompt in ~/src, and
    // `sudo pacman -Syu`, which sudo runs on a terminal of its own (pts/5).
    const XFCE: &str = "\
 4276     1  4276    -1 ?        xfce4-terminal
 4400  4276  4400  4500 pts/2    fish
 4500  4400  4500  4500 pts/2    man ls
 4501  4500  4500  4500 pts/2    less
 4600  4276  4600  4600 pts/3    fish
 4700  4276  4700  4800 pts/4    fish
 4800  4700  4800  4800 pts/4    sudo pacman -Syu
 4801  4800  4801  4802 pts/5    sudo pacman -Syu
 4802  4801  4802  4802 pts/5    pacman -Syu
";

    #[test]
    fn the_title_names_the_tab_before_typing_does() {
        let table = parse(XFCE);
        let home = std::path::Path::new("/home/r");
        let at = |secs: u64| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
        // The ~/src tab was typed in last.
        let typed = |tty: &str| if tty == "pts/3" { at(300) } else { at(100) };
        let cwd = |pid: u32| Some(PathBuf::from(match pid { 4400..=4599 => "/home/r/Projects", 4600..=4699 => "/home/r/src", _ => "/home/r" }));
        let tab = pick(&table, 4276, "man ls ~/Projects", Some(home), typed, cwd).unwrap();
        assert_eq!(tab, TerminalTab { tty: "pts/2".into(), programs: vec!["man".into(), "less".into()] }, "switched to without typing");
        let tab = pick(&table, 4276, "Terminal - ~/Projects", Some(home), |t| if t == "pts/2" { at(1) } else { at(100) }, cwd).unwrap();
        assert_eq!(tab.tty, "pts/2", "the folder alone tells it");
        assert_eq!(pick(&table, 4276, "Terminal", Some(home), typed, cwd).unwrap().tty, "pts/3", "a title that says nothing: typing decides");
        let tab = pick(&table, 4276, "sudo pacman -Syu ~", Some(home), typed, cwd).unwrap();
        assert_eq!(tab, TerminalTab { tty: "pts/4".into(), programs: vec!["sudo".into(), "pacman".into()] }, "sudo's own terminal is part of the tab");
        assert_eq!(tab.program(), Some("pacman"));
        assert!(!["pts/5"].contains(&pick(&table, 4276, "", Some(home), |t| if t == "pts/5" { at(900) } else { at(1) }, cwd).unwrap().tty.as_str()),
                "sudo's terminal is never a tab of its own");
    }

    #[test]
    fn program_names() {
        assert_eq!(program(&["/opt/homebrew/bin/node", "/opt/homebrew/bin/claude", "--resume"]), "claude");
        assert_eq!(program(&["node", "--inspect", "app.js"]), "node");
        assert_eq!(program(&["-zsh"]), "zsh");
        assert_eq!(program(&["claude"]), "claude");
    }
}
