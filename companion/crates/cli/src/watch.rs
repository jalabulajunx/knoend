//! The setup assistant, always listening: problems the service notices (journal.rs, `problem!`)
//! are gathered here and, without the person asking, put to the assistant, whose answer goes to
//! the web app (the floating assistant opens with it, or shows it when the web app is next opened)
//! and a pointer to it to the board's status bar. Runs whether or not the web app is open.
//!
//! Not every line is worth an AI call: a board that drops once (unplugged, a restart) is normal,
//! twice within a few minutes is not. Each kind of problem is asked about at most once every few
//! minutes, and only when the person has the AI set up and has not switched the watching off.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::time::Instant;

use crate::journal::Problem;
use crate::service::Shared;

/// Problems arriving this soon after the first join the same question.
const GATHER: Duration = Duration::from_secs(15);
/// A kind of problem is asked about at most this often.
const QUIET: Duration = Duration::from_secs(5 * 60);
/// How far back repeats count.
const WINDOW: Duration = Duration::from_secs(5 * 60);

static LATEST: Mutex<Option<Value>> = Mutex::new(None);

/// What was last raised (with the assistant's answer, when it gave one).
pub fn latest() -> Option<Value> {
    LATEST.lock().ok().and_then(|l| l.clone())
}

pub fn start(shared: Arc<Shared>) {
    tokio::spawn(run(shared));
}

/// Whether a problem of this kind, with these repeats, is worth raising.
fn worth_raising(kind: &str, repeats: usize) -> bool {
    match kind {
        // Once is a restart or an unplugged cable; again within minutes is a fault.
        "link-lost" => repeats >= 2,
        _ => true,
    }
}

fn ai_ready() -> bool {
    knoend_transport::pairing::config_dir().join("agent.toml").exists()
        && crate::agent_config().is_ok_and(|c| c.key_source() != "missing")
}

async fn run(shared: Arc<Shared>) {
    let mut problems = crate::journal::subscribe();
    let mut recent: VecDeque<(Instant, Problem)> = VecDeque::new();
    let mut asked: HashMap<String, Instant> = HashMap::new();
    loop {
        let problem = match problems.recv().await {
            Ok(p) => p,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return,
        };
        let now = Instant::now();
        recent.push_back((now, problem.clone()));
        while recent.front().is_some_and(|(at, _)| now.duration_since(*at) > WINDOW) {
            recent.pop_front();
        }
        let repeats = recent.iter().filter(|(_, p)| p.kind == problem.kind).count();
        if !worth_raising(&problem.kind, repeats) || asked.get(&problem.kind).is_some_and(|at| now.duration_since(*at) < QUIET) {
            continue;
        }
        // Gather what follows (a board that drops after a change says more in the next seconds).
        let until = Instant::now() + GATHER;
        while let Ok(Ok(more)) = tokio::time::timeout_at(until, problems.recv()).await {
            recent.push_back((Instant::now(), more));
        }
        asked.insert(problem.kind.clone(), Instant::now());
        let noticed: Vec<String> = recent.iter().map(|(_, p)| p.text.clone()).collect();
        say!("Watcher: raising {} ({} line(s))", problem.kind, noticed.len());
        shared.board_status("KnoEnd noticed a problem: see the computer".into());

        let raised = if crate::display::load().assistant_watches && ai_ready() {
            let question = format!(
                "Nobody asked: KnoEnd's service noticed this by itself, in the last minutes:\n- {}\n\nTell the person, \
                 plainly and first, what is happening and why, then what to do. Use the checks and the service log to find \
                 the cause rather than guessing, and offer the fix or button that resolves it. If it needs nothing from them, \
                 say so.", noticed.join("\n- "));
            match crate::consult::ask(&shared, &question, json!({"noticed_by_the_service": noticed})).await {
                Ok(mut answer) => {
                    answer["noticed"] = json!(noticed);
                    answer
                }
                Err(e) => json!({"noticed": noticed, "answer": format!("KnoEnd noticed a problem, but the assistant could not look at it: {e}")}),
            }
        } else {
            json!({"noticed": noticed})
        };
        if let Ok(mut latest) = LATEST.lock() {
            *latest = Some(raised.clone());
        }
        shared.emit("noticed", raised);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_drop_is_normal_two_are_not() {
        assert!(!worth_raising("link-lost", 1));
        assert!(worth_raising("link-lost", 2));
        assert!(worth_raising("font", 1));
    }
}
