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

/// Prints a line for the terminal and keeps it for the assistant.
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {{
        let line = format!($($arg)*);
        println!("{line}");
        $crate::journal::record(&line);
    }};
}
