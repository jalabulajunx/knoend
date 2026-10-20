//! Asking the setup assistant, the same way from the web app and from the watcher: the snapshot,
//! the checks it asks for (in rounds), and fixes kept only with values their settings take.

use serde_json::{json, Value};

use crate::service::Shared;

/// The assistant's answer, as the web app shows it: answer, buttons, fix (with its settings in
/// words), the checks it ran, what was sent, and which model answered.
pub async fn ask(shared: &Shared, question: &str, context: Value) -> Result<Value, String> {
    let config = crate::agent_config()?;
    let snapshot = crate::diagnostics::snapshot(shared, context).await;
    let mut earlier: Vec<(String, Value)> = Vec::new();
    let mut checked: Vec<Value> = Vec::new();
    let mut advice = knoend_agents::assistant::advise(&config, question, &snapshot, &earlier).await?;
    while !advice.checks.is_empty() && earlier.len() < knoend_agents::assistant::CHECK_ROUNDS {
        let mut results = serde_json::Map::new();
        for id in advice.checks.iter().filter(|c| crate::checks::ALL.iter().any(|(k, _)| k == &c.as_str())) {
            say!("Assistant checks: {id}");
            let result = crate::checks::run(id, shared).await;
            checked.push(json!({"check": id, "result": result}));
            results.insert(id.clone(), result);
        }
        if results.is_empty() {
            break;
        }
        earlier.push((advice.raw.clone(), Value::Object(results)));
        advice = knoend_agents::assistant::advise(&config, question, &snapshot, &earlier).await?;
    }
    if advice.answer.trim().is_empty() {
        // Rounds used up without a real answer (a model that kept writing made-up calls).
        advice.answer = format!("The assistant did not give a usable answer this time{}. Ask again, or choose another model in AI \
                                 settings.", if checked.is_empty() { String::new() } else {
                                     format!(" (it checked: {})", checked.iter().filter_map(|c| c["check"].as_str()).collect::<Vec<_>>().join(", "))
                                 });
    }
    // A settings change is offered only with values its setting takes, labelled for the person.
    let mut fix = advice.fix.clone();
    let mut settings_described = Vec::new();
    if let Some(f) = fix.as_mut() {
        if let Some(changes) = f.settings.take() {
            let known = crate::settings::all(shared).await;
            let kept: serde_json::Map<String, Value> = changes.into_iter().filter(|(id, value)| {
                known.iter().find(|s| s["id"] == id.as_str()).and_then(|s| crate::settings::change_label(s, value).map(|label| {
                    settings_described.push(json!({"id": id, "value": value, "title": s["title"], "label": label}));
                })).is_some()
            }).collect();
            f.settings = (!kept.is_empty()).then_some(kept);
        }
    }
    let fix = fix.filter(|f| *f != knoend_agents::assistant::Fix::default());
    Ok(json!({"answer": advice.answer, "actions": advice.actions, "fix": fix, "settings_described": settings_described,
              "checked": checked, "sent": snapshot, "by": config.describe()}))
}
