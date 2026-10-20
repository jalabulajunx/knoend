//! Everything the service says, kept for the setup assistant: every line it prints (with `say!`)
//! and every event text it sends the web app. So whatever the service notices, including problems
//! nobody planned for, reaches the assistant without a change to the assistant.

use std::collections::VecDeque;
use std::sync::Mutex;

/// Lines kept: the snapshot sends the last ones, the "service_log" check all of them.
const KEPT: usize = 300;

static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

pub fn record(line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if let Ok(mut lines) = LINES.lock() {
        lines.push_back(line.to_string());
        while lines.len() > KEPT {
            lines.pop_front();
        }
    }
}

/// The last `n` lines, oldest first.
pub fn recent(n: usize) -> Vec<String> {
    LINES.lock().map(|l| l.iter().skip(l.len().saturating_sub(n)).cloned().collect()).unwrap_or_default()
}

/// A problem the service noticed: a kind (to group repeats: "link-lost", "font", ...) and the line.
#[derive(Debug, Clone)]
pub struct Problem {
    pub kind: String,
    pub text: String,
}

static PROBLEMS: std::sync::OnceLock<tokio::sync::broadcast::Sender<Problem>> = std::sync::OnceLock::new();

fn problems() -> &'static tokio::sync::broadcast::Sender<Problem> {
    PROBLEMS.get_or_init(|| tokio::sync::broadcast::channel(64).0)
}

/// Keeps the line, marked as a problem, and tells the watcher (watch.rs).
pub fn problem(kind: &str, text: &str) {
    record(&format!("PROBLEM ({kind}): {text}"));
    let _ = problems().send(Problem { kind: kind.to_string(), text: text.trim().to_string() });
}

/// Problems as they are noticed, for the watcher.
pub fn subscribe() -> tokio::sync::broadcast::Receiver<Problem> {
    problems().subscribe()
}

/// Prints a problem the service noticed and raises it: `problem!("link-lost", "...")`. The watcher
/// may ask the setup assistant about it by itself.
#[macro_export]
macro_rules! problem {
    ($kind:expr, $($arg:tt)*) => {{
        let line = format!($($arg)*);
        println!("{line}");
        $crate::journal::problem($kind, &line);
    }};
}

/// Raises a problem without printing it, for a line the web app is sent anyway (`Shared::emit`
/// prints and keeps that).
#[macro_export]
macro_rules! raise {
    ($kind:expr, $($arg:tt)*) => {{
        $crate::journal::problem($kind, &format!($($arg)*));
    }};
}

/// Prints a line for the terminal and keeps it for the assistant.
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {{
        let line = format!($($arg)*);
        println!("{line}");
        $crate::journal::record(&line);
    }};
}
