//! Everything the person can set on KnoEnd, described in one place: what each setting means, the
//! values it takes, what it is now and when a change shows. The web app, the setup assistant's
//! snapshot and the assistant's one-click fixes all read this list, so a setting added here is one
//! the assistant can see, explain and change, with no change to its prompt.
//!
//! AI provider settings are not here: they carry keys and have their own page and checks.

use serde_json::{json, Value};

use crate::display::{self, KeySize, Theme};
use crate::service::Shared;

/// One setting as the assistant and the web app see it.
fn describe(id: &str, title: &str, meaning: &str, choices: Value, current: Value, shows: &str) -> Value {
    json!({"id": id, "title": title, "meaning": meaning, "choices": choices, "current": current, "shows": shows})
}

/// Every setting, with its current value.
pub async fn all(shared: &Shared) -> Vec<Value> {
    let d = display::load();
    let board = shared.status.read().await.board.clone();
    let shown = board.as_ref().and_then(|b| b.display.clone());
    let can_look = shown.is_some();
    vec![
        describe("key_size", "Key size",
                 "The grid every page of keys on the board uses. A layout's keys fill it in order; the rest go on further \
                  pages the person swipes through. Bigger keys are easier to read and tap.",
                 json!([{"value": "big", "label": "Big: 2x2"}, {"value": "medium", "label": "Medium: 3x3"},
                        {"value": "small", "label": "Small: 4x4"}]),
                 json!(d.key_size), "at once, on the board"),
        describe("theme", "Look",
                 "The board's colours. White and Sepia are high-contrast, e-ink-like looks, easier in daylight or for \
                  low vision; app pictures turn grey or sepia.",
                 json!([{"value": "dark", "label": "Dark, in colour"}, {"value": "white", "label": "White: black on white"},
                        {"value": "sepia", "label": "Sepia: brown ink on warm paper"}]),
                 json!(d.theme),
                 if can_look { "at once, on the board" } else { "after the board's firmware is updated (this firmware has one look)" }),
        describe("turn", "Way round",
                 "Which way the board sits. Portrait is tall, with at most 3 keys across; upside down suits a cable that \
                  must come out of the other side. Touch follows the turn.",
                 json!([{"value": 0, "label": "Landscape"}, {"value": 1, "label": "Portrait"},
                        {"value": 2, "label": "Landscape, upside down"}, {"value": 3, "label": "Portrait, upside down"}]),
                 json!(d.turn),
                 if can_look { "at once, on the board (firmware that cannot turn refuses it and says so)" } else { "after the board's firmware is updated" }),
    ]
}

/// Whether `value` is one of the setting's choices.
fn allowed(setting: &Value, value: &Value) -> bool {
    setting["choices"].as_array().into_iter().flatten().any(|c| &c["value"] == value)
}

/// Applies changes `{id: value}`, each checked against its choices; unknown ids and values are
/// refused, and nothing is changed then. Returns what each change was, in words.
pub async fn apply(shared: &Shared, changes: &serde_json::Map<String, Value>) -> Result<Vec<String>, String> {
    let settings = all(shared).await;
    let mut done = Vec::new();
    for (id, value) in changes {
        let setting = settings.iter().find(|s| s["id"] == id.as_str()).ok_or_else(|| format!("there is no setting called {id}"))?;
        if !allowed(setting, value) {
            return Err(format!("{} cannot be {value}", setting["title"].as_str().unwrap_or(id)));
        }
        let label = setting["choices"].as_array().into_iter().flatten().find(|c| &c["value"] == value)
            .and_then(|c| c["label"].as_str()).unwrap_or_default();
        done.push(format!("{}: {label}", setting["title"].as_str().unwrap_or(id)));
    }
    let mut d = display::load();
    let look = (d.theme, d.turn);
    let mut key_size = None;
    for (id, value) in changes {
        match id.as_str() {
            "key_size" => key_size = serde_json::from_value::<KeySize>(value.clone()).ok(),
            "theme" => d.theme = serde_json::from_value::<Theme>(value.clone()).map_err(|e| e.to_string())?,
            "turn" => d.turn = value.as_u64().and_then(|t| u8::try_from(t).ok()).ok_or("turn must be 0-3")?,
            _ => {}
        }
    }
    if let Some(k) = key_size {
        d.key_size = k;
    }
    display::save(&d)?;
    if let Some(k) = key_size {
        shared.key_size_changed(k);
    }
    if look != (d.theme, d.turn) {
        shared.look_changed(d.clone());
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_must_be_one_of_the_choices() {
        let setting = describe("turn", "Way round", "", json!([{"value": 0, "label": "Landscape"}, {"value": 1, "label": "Portrait"}]),
                               json!(0), "");
        assert!(allowed(&setting, &json!(1)));
        assert!(!allowed(&setting, &json!(5)));
        assert!(!allowed(&setting, &json!("1")), "the value's type counts");
    }
}
