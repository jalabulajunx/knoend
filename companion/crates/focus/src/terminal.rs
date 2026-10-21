//! What runs in a terminal app's active tab, from the process table (`ps`, macOS and Linux): the
//! app's tabs are the terminals its child processes run on, the active one is the one typed in last
//! (the terminal device's access time moves on input), and its program is that terminal's
//! foreground process group. No permission needed, and the same for every terminal app.
//!
//! Linux keeps a terminal's access time to 8 s, so switching tabs and typing within 8 s can keep
//! the previous tab's keys until the next input after that.

use std::time::SystemTime;

use knoend_core::focus::TerminalTab;

/// The active tab of the terminal app with process id `app`, or None when it runs no terminal.
pub fn active_tab(app: u32) -> Option<TerminalTab> {
    let out = std::process::Command::new("ps").args(["-A", "-o", "pid=,ppid=,pgid=,tpgid=,tty=,args="]).output().ok()?;
    let table = parse(&String::from_utf8_lossy(&out.stdout));
    pick(&table, app, |tty| std::fs::metadata(format!("/dev/{tty}")).and_then(|m| m.accessed()).ok())
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

fn pick(table: &[Process], app: u32, last_input: impl Fn(&str) -> Option<SystemTime>) -> Option<TerminalTab> {
    // The app's descendants, and the terminals they run on.
    let mut family = vec![app];
    let mut i = 0;
    while i < family.len() {
        let parent = family[i];
        family.extend(table.iter().filter(|p| p.ppid == parent && p.pid != parent).map(|p| p.pid));
        i += 1;
    }
    // Not the terminal the app itself was started from (an xterm launched from another terminal
    // keeps it): only its children's.
    let own = table.iter().find(|p| p.pid == app).map(|p| p.tty.as_str());
    let mut ttys: Vec<&str> = table.iter()
        .filter(|p| p.pid != app && family.contains(&p.pid) && has_tty(&p.tty) && Some(p.tty.as_str()) != own)
        .map(|p| p.tty.as_str()).collect();
    ttys.sort_unstable();
    ttys.dedup();
    let tty = ttys.iter().max_by_key(|t| last_input(t))?.to_string();
    // Its foreground process group, the group's leader first.
    // A process that ended but is not yet collected shows as "<defunct>": not a program.
    let mut foreground: Vec<&Process> = table.iter()
        .filter(|p| p.tty == tty && i64::from(p.pgid) == p.tpgid && !p.program.starts_with('<')).collect();
    foreground.sort_by_key(|p| (p.pid != p.pgid, p.pid));
    let mut programs: Vec<String> = Vec::new();
    for p in foreground {
        if !programs.contains(&p.program) {
            programs.push(p.program.clone());
        }
    }
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
        let tab = pick(&table, 76388, |tty| if tty == "ttys002" { at(200) } else { at(100) }).unwrap();
        assert_eq!(tab, TerminalTab { tty: "ttys002".into(), programs: vec!["man".into(), "less".into()] });
        let tab = pick(&table, 76388, |tty| if tty == "ttys003" { at(300) } else { at(100) }).unwrap();
        assert_eq!(tab.programs, vec!["zsh"], "a shell at its prompt");
        let tab = pick(&table, 2218, |tty| if tty == "ttys000" { at(2) } else { at(1) }).unwrap();
        assert_eq!(tab, TerminalTab { tty: "ttys000".into(), programs: vec!["zsh".into()] }, "login is not in the foreground");
        assert!(pick(&table, 1234, |_| at(1)).is_none(), "an app without terminals");
        let tab = pick(&table, 3000, |tty| if tty == "ttys000" { at(500) } else { at(1) }).unwrap();
        assert_eq!(tab.tty, "ttys009", "an xterm started from Terminal: not Terminal's tab");
    }

    #[test]
    fn program_names() {
        assert_eq!(program(&["/opt/homebrew/bin/node", "/opt/homebrew/bin/claude", "--resume"]), "claude");
        assert_eq!(program(&["node", "--inspect", "app.js"]), "node");
        assert_eq!(program(&["-zsh"]), "zsh");
        assert_eq!(program(&["claude"]), "claude");
    }
}
