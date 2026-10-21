//! Setting up keys for several installed apps at once, before they are used: the web app lists
//! what is installed (most used first), says which already have keys, and the person picks some
//! for the configure agent to draft. Drafts run a few at a time; each one is saved like a draft made
//! from the board (unreviewed, in the AI folder) and the web app hears how each went.
//!
//! Only the names of the apps the person picks reach the AI provider.

use std::sync::Arc;

use knoend_core::focus::{Focus, Os};
use knoend_core::library::Source;
use serde_json::{json, Value};

use crate::service::Shared;

/// Drafts running at once: enough to finish a handful in a minute or two without tripping the
/// provider's rate limits (or a coding-agent CLI's).
const AT_ONCE: usize = 2;
/// The most apps one request may ask for.
const MOST: usize = 12;

fn focus_of(app: &knoend_desktop::installed::Installed) -> Focus {
    Focus { os: Os::current(), app: app.app.clone(), instance: app.instance.clone(), title: String::new(), web: None, pid: None, terminal: None }
}

/// What the keys for an app are now: "ready" (a built-in or community recipe), "yours" (the
/// person's own library), "drafted" (an AI draft) or "none".
fn state(source: Option<&Source>) -> &'static str {
    match source {
        Some(Source::Builtin | Source::Community) => "ready",
        Some(Source::User) => "yours",
        Some(Source::Ai) => "drafted",
        None => "none",
    }
}

/// The installed apps, most used first, then those with keys, then by name:
/// `[{id, name, state, keys, library, minutes}]`.
pub async fn list() -> Value {
    let (apps, catalog, usage) = tokio::task::spawn_blocking(|| {
        (knoend_desktop::installed::list(), crate::service::load_catalog(), crate::launcher::Usage::load())
    }).await.unwrap_or_default();
    let mut out: Vec<(f64, u8, Value)> = apps.iter().map(|app| {
        let library = catalog.find(&focus_of(app));
        let state = state(library.map(|l| &l.pack.source));
        let seconds = usage.seconds(&format!("app:{}", app.id));
        let rank = match state { "ready" | "yours" => 1, "drafted" => 2, _ => 3 };
        (seconds, rank, json!({"id": app.id, "name": app.name, "state": state, "minutes": (seconds / 60.0).round(),
                               "keys": library.map(|l| l.keys.len()), "library": library.map(|l| l.pack.name.clone())}))
    }).collect();
    out.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1))
        .then_with(|| a.2["name"].as_str().unwrap_or("").to_lowercase().cmp(&b.2["name"].as_str().unwrap_or("").to_lowercase())));
    json!({"os": format!("{:?}", Os::current()), "apps": out.into_iter().map(|(_, _, v)| v).collect::<Vec<_>>()})
}

/// Starts drafting keys for the apps `ids` (launcher ids from `list`); progress reaches the web app
/// as "app-setup" events `{id, name, state: "waiting" | "drafting" | "done" | "failed", keys, error}`.
pub async fn draft(shared: &Arc<Shared>, ids: Vec<String>) -> Result<Value, String> {
    crate::agent_config().map_err(|e| format!("set up the AI first (AI page): {e}"))?;
    if ids.is_empty() || ids.len() > MOST {
        return Err(format!("choose between 1 and {MOST} apps"));
    }
    let installed = tokio::task::spawn_blocking(knoend_desktop::installed::list).await.map_err(|e| e.to_string())?;
    let chosen: Vec<_> = ids.iter().filter_map(|id| installed.iter().find(|a| &a.id == id).cloned()).collect();
    if chosen.len() != ids.len() {
        return Err("some of those apps are not installed here".into());
    }
    say!("App setup: drafting keys for {}", chosen.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "));
    for app in &chosen {
        shared.emit("app-setup", json!({"id": app.id, "name": app.name, "state": "waiting"}));
    }
    let gate = Arc::new(tokio::sync::Semaphore::new(AT_ONCE));
    for app in chosen.clone() {
        let (shared, gate) = (shared.clone(), gate.clone());
        tokio::spawn(async move {
            let Ok(_turn) = gate.acquire().await else { return };
            shared.emit("app-setup", json!({"id": app.id, "name": app.name, "state": "drafting"}));
            let target = focus_of(&app);
            let result = crate::configure(&target, true).await;
            match &result {
                Ok((_, library)) => {
                    say!("App setup: {} keys for {}", library.keys.len(), app.name);
                    shared.emit("app-setup", json!({"id": app.id, "name": app.name, "state": "done", "keys": library.keys.len()}));
                }
                Err(e) => {
                    say!("App setup: drafting {} failed: {e}", app.name);
                    shared.emit("app-setup", json!({"id": app.id, "name": app.name, "state": "failed", "error": e}));
                }
            }
            shared.configured(target, result).await;
        });
    }
    Ok(json!({"started": chosen.len()}))
}

/// The app's icon as a 32x32 PNG, from its own files.
pub async fn icon(id: String) -> Option<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        let apps = crate::launcher::installed_apps();
        let name = knoend_desktop::installed::list().into_iter().find(|a| a.id == id).map(|a| a.name).unwrap_or_default();
        let rgba = crate::launcher::icon_pixels(&crate::launcher::Target::App { id }, &name, &apps, &Default::default())?;
        knoend_desktop::picture::png(&rgba, 32, 32)
    }).await.ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipes_count_as_ready_and_drafts_as_drafted() {
        assert_eq!(state(Some(&Source::Builtin)), "ready");
        assert_eq!(state(Some(&Source::Community)), "ready");
        assert_eq!(state(Some(&Source::User)), "yours");
        assert_eq!(state(Some(&Source::Ai)), "drafted");
        assert_eq!(state(None), "none");
    }

    #[test]
    fn an_installed_app_is_found_by_its_recipe() {
        // As the installed list names Firefox on Linux: its window class and program.
        let firefox = knoend_desktop::installed::Installed { id: "firefox".into(), name: "Firefox".into(),
                                                             app: "firefox".into(), instance: "firefox".into() };
        if Os::current() == Os::Linux {
            assert!(crate::service::load_catalog().find(&focus_of(&firefox)).is_some(), "the Firefox recipe matches");
        }
    }

    /// Prints what this computer has installed, as the web app would list it:
    /// `cargo test -p knoend print_installed -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn print_installed() {
        for app in list().await["apps"].as_array().unwrap() {
            println!("{:8} {:>5} min  {}  ({})", app["state"].as_str().unwrap(), app["minutes"], app["name"].as_str().unwrap(), app["id"].as_str().unwrap());
        }
    }
}
