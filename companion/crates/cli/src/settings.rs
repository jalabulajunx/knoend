//! Everything the person can set on KnoEnd, described in one place: what each setting means, the
//! values it takes, what it is now and when a change shows. The web app, the setup assistant's
//! snapshot and the assistant's one-click fixes all read this list, so a setting added here is one
//! the assistant can see, explain and change, with no change to its prompt.
//!
//! AI provider settings are not here: they carry keys and have their own page and checks.

use serde_json::{json, Value};

use crate::display::{self, Display, KeySize, Theme};
use crate::service::Shared;

/// One setting as the assistant and the web app see it.
fn describe(id: &str, title: &str, meaning: &str, choices: Value, current: Value, shows: &str) -> Value {
    json!({"id": id, "title": title, "meaning": meaning, "choices": choices, "current": current, "shows": shows})
}

/// The look settings: each board keeps its own (display.rs `BoardLook`).
const PER_BOARD: &[&str] = &["key_size", "theme", "turn", "key_font", "start_font", "start_size"];

/// The board settings apply to: `wanted` when it is paired, else the first connected board, else
/// the first paired one; None when no board is paired.
async fn target_board(shared: &Shared, wanted: Option<&str>) -> Option<String> {
    let paired: Vec<String> = knoend_transport::pairing::Pairings::load().boards.keys().cloned().collect();
    if let Some(id) = wanted.filter(|id| paired.iter().any(|p| p == id)) {
        return Some(id.to_string());
    }
    let connected = shared.status.read().await.boards.first().map(|b| b.id.clone());
    connected.or_else(|| paired.first().cloned())
}

/// Every setting, with its current value; the look settings for the first connected board.
pub async fn all(shared: &Shared) -> Vec<Value> {
    all_for(shared, None).await
}

/// Local path settings remain editable in the local UI, but never enter AI context.
pub async fn for_assistant(shared: &Shared) -> Vec<Value> {
    let mut settings=all(shared).await;
    for setting in &mut settings { redact_local_path(setting); }
    settings
}

fn redact_local_path(setting:&mut Value) {
    if setting["kind"]=="local_vbs" || setting["kind"]=="local_pptx" {
        let path=setting["current"].as_str().unwrap_or_default();
        let configured=!path.is_empty();let exists=configured&&std::path::Path::new(path).is_file();
        setting["current"]=Value::Null;
        setting["configured"]=json!(configured);setting["exists"]=json!(exists);
        setting["private"]=json!(true);
    }
}

/// Every setting, with its current value; the look settings (`per_board`) for board `wanted`.
pub async fn all_for(shared: &Shared, wanted: Option<&str>) -> Vec<Value> {
    let target = target_board(shared, wanted).await;
    let saved = display::load();
    let d = target.as_deref().map(|id| saved.for_board(id)).unwrap_or_else(|| saved.clone());
    let connected = shared.status.read().await.boards.clone();
    let board = target.as_deref().and_then(|id| connected.iter().find(|b| b.id == id)).cloned();
    let shown = board.as_ref().and_then(|b| b.display.clone());
    let can_look = shown.is_some();
    let can_font = board.as_ref().is_some_and(|b| b.fonts.is_some());
    let can_listen = connected.iter().any(|b| b.mic);
    let paired: Vec<String> = knoend_transport::pairing::Pairings::load().boards.keys().cloned().collect();
    let board_choices: Vec<Value> = paired.iter().map(|id| {
        let label = match connected.iter().find(|b| &b.id == id) {
            Some(b) => format!("{} ({id}, connected)", b.profile),
            None => format!("{id} (not connected)"),
        };
        json!({"value": id, "label": label})
    }).collect();
    let mut settings = vec![
        describe("codex_interruptions_enabled","Codex interruptions","Allow supported local Codex hooks to show approval requests and completion alerts. Completion alerts also need pager enabled; approval policy and hook trust remain controlled by Codex.",json!([{"value":true,"label":"On"},{"value":false,"label":"Off"}]),json!(crate::codex_hooks::enabled()),"next hook event"),
        describe("pager_enabled", "Windows pager interruptions",
                 "Show visible Teams, ChatGPT and Codex notifications on the board, with Open app, Dismiss and Start Steno for Teams. Suppressed notifications are unavailable.",
                 json!([{"value": true, "label": "On"}, {"value": false, "label": "Off"}]),
                 json!(crate::pager::config().enabled), "at once"),
        describe("board", "Board",
                 "Which board the look settings (those with per_board) apply to: each board keeps its own key size, look, \
                  way round, fonts and start-up size, and a newly paired board starts from the defaults. To change another \
                  board's look, give its id as \"board\" with the changes.",
                 json!(board_choices), json!(target), "chooses which board the look settings below show and change"),
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
                 turn_choices(board.as_ref().is_some_and(|b| b.native_portrait)),
                 json!(d.turn),
                 if can_look { "at once, on the board (firmware that cannot turn refuses it and says so)" } else { "after the board's firmware is updated" }),
        font_setting("key_font", "Key font",
                     "The font of the key labels: any Google Fonts family (its name exactly as Google Fonts writes it), or \
                      empty for the built-in Montserrat. Plain, open fonts read best at the small sizes on keys; a \
                      character the font lacks is drawn in Montserrat.", &d.key_font, can_font),
        font_setting("start_font", "Start-up font",
                     "The font of the name the board shows for a moment when it starts (Kn°End): any Google Fonts family, \
                      or empty for the built-in Montserrat. Decorative fonts suit it.", &d.start_font, can_font),
        number_setting("start_size", "Start-up size",
                       "How big the name the board shows for a moment when it starts (Kn°End) is, in pixels. The board shows \
                        it at the new size at once, for 2 seconds, as a preview.",
                       d.start_size.into(), 24, 96,
                       if can_look { "at once, on the board (a 2-second preview)" } else { "after the board's firmware is updated" }),
        signed_setting("listen_quiet", "Listening: quiet below",
                       "While the board listens for notes (a Listen key), sounds quieter than this many dB below full \
                        scale count as silence. Lower (-70) hears soft singing or a distant instrument but also the room; \
                        higher (-45) ignores background noise but needs a louder note. The board's serial log prints how \
                        loud each second was while it listens.",
                       d.listen_quiet.into(), -80, -20,
                       if can_listen { "the next time the board starts listening" } else { "on a board with a microphone (this one has none, or its firmware cannot listen)" }),
        list_setting("home_apps", "Home screen apps",
                     "Apps and sites the board shows first on its home screen (when no app is in front), in this order: \
                      \"app:<id>\" with the id from the installed_apps check (\"app:org.musescore.MuseScore\"), or \
                      \"site:<site>\" (\"site:github.com\"). Tapping one opens it. Empty: only the most used.",
                     &d.home_apps),
        describe("home_fill", "Home screen: fill with most used",
                 "After the pinned apps, fill the board's home screen with the apps and sites used most. Off: only the \
                  pinned ones.",
                 json!([{"value": true, "label": "On"}, {"value": false, "label": "Off: pinned only"}]),
                 json!(d.home_fill), "at once, on the board's home screen"),
        describe("assistant_watches", "Assistant watches in the background",
                 "When on, problems the service notices by itself (the board dropping again and again, a step or a font \
                  that fails, the board refusing something) are put to the setup assistant without the person asking, \
                  and its answer opens in the web app. Uses the AI provider: at most one question per kind of problem \
                  every 5 minutes.",
                 json!([{"value": true, "label": "On"}, {"value": false, "label": "Off"}]),
                 json!(d.assistant_watches), "at once"),
    ];
    let mut recorder = describe("recorder_launcher", "Recorder launcher",
        "An existing local .vbs shortcut opened only when Start Steno is tapped. Opening the recorder does not automatically start capture.",
        Value::Null, json!(crate::pager::config().recorder_launcher), "the next Start Steno tap");
    recorder["kind"] = json!("local_vbs");
    settings.push(recorder);
    let notes=crate::powerpoint::notes_settings();
    settings.push(describe("presentation_notes_source","Presentation notes source","Use the recipe policy, live unsaved PowerPoint notes, saved PPTX extraction, or guarded live-to-PPTX fallback.",
        json!([{"value":"recipe","label":"Recipe"},{"value":"live","label":"Live PowerPoint"},{"value":"pptx","label":"Saved PPTX"},{"value":"live_or_pptx","label":"Live, then exact PPTX"}]),json!(notes.source),"next presentation refresh"));
    let mut file=describe("presentation_pptx_file","Exact saved PPTX","Optional local file binding. Empty uses the exact path reported by PowerPoint; a different active deck is rejected.",Value::Null,json!(notes.file),"next presentation refresh");file["kind"]=json!("local_pptx");settings.push(file);
    for setting in settings.iter_mut().filter(|s| s["id"].as_str().is_some_and(|id| PER_BOARD.contains(&id))) {
        setting["per_board"] = json!(true);
        setting["board"] = json!(target);
    }
    settings
}

/// The four turns, named for how the board usually sits: on a board that is portrait at turn 0
/// (an AMOLED watch), turn 0 is Portrait.
pub fn turn_choices(native_portrait: bool) -> Value {
    let (usual, across) = if native_portrait { ("Portrait", "Landscape") } else { ("Landscape", "Portrait") };
    json!([{"value": 0, "label": usual}, {"value": 1, "label": across},
           {"value": 2, "label": format!("{usual}, upside down")}, {"value": 3, "label": format!("{across}, upside down")}])
}

/// A font setting: no fixed choices, a Google Fonts family name or empty.
fn font_setting(id: &str, title: &str, meaning: &str, current: &str, can: bool) -> Value {
    let mut setting = describe(id, title, meaning, Value::Null, json!(current),
                               if can { "at once, on the board (it downloads and sends the font)" } else { "after the board's firmware is updated" });
    setting["kind"] = json!("google_font");
    setting["examples"] = json!(["Atkinson Hyperlegible", "Lexend", "Edu AU VIC WA NT Dots", ""]);
    setting
}

/// A setting that holds a list of launcher keys ("app:firefox", "site:github.com").
fn list_setting(id: &str, title: &str, meaning: &str, current: &[String]) -> Value {
    let mut setting = describe(id, title, meaning, Value::Null, json!(current), "at once, on the board's home screen");
    setting["kind"] = json!("list");
    setting["examples"] = json!([["app:firefox", "site:github.com"], []]);
    setting
}

/// A launcher key: "app:<id>" or "site:<site>".
fn home_key(key: &str) -> bool {
    key.len() <= 200 && key.split_once(':').is_some_and(|(kind, id)| (kind == "app" || kind == "site") && !id.trim().is_empty())
}

/// A setting that takes a whole number between `min` and `max`.
fn number_setting(id: &str, title: &str, meaning: &str, current: u64, min: u64, max: u64, shows: &str) -> Value {
    let mut setting = describe(id, title, meaning, Value::Null, json!(current), shows);
    setting["kind"] = json!("number");
    setting["min"] = json!(min);
    setting["max"] = json!(max);
    setting
}

/// A setting that takes a whole number between `min` and `max`, which may be below zero.
fn signed_setting(id: &str, title: &str, meaning: &str, current: i64, min: i64, max: i64, shows: &str) -> Value {
    let mut setting = describe(id, title, meaning, Value::Null, json!(current), shows);
    setting["kind"] = json!("number");
    setting["min"] = json!(min);
    setting["max"] = json!(max);
    setting
}

/// How a change reads to the person ("Key size: Big: 2x2"), or None when `value` is not one the
/// setting takes.
pub fn change_label(setting: &Value, value: &Value) -> Option<String> {
    if !allowed(setting, value) {
        return None;
    }
    if setting["kind"]=="local_vbs" || setting["kind"]=="local_pptx" {
        return Some(if value.as_str().unwrap_or_default().is_empty(){"cleared"}else{"configured"}.into());
    }
    let label = setting["choices"].as_array().into_iter().flatten().find(|c| &c["value"] == value)
        .and_then(|c| c["label"].as_str()).map(str::to_string)
        .unwrap_or_else(|| match value {
            Value::Number(n) => n.to_string(),
            Value::Array(keys) if keys.is_empty() => "none".to_string(),
            Value::Array(keys) => keys.iter().filter_map(|k| k.as_str()).map(|k| k.split_once(':').map_or(k, |(_, id)| id))
                .collect::<Vec<_>>().join(", "),
            _ => value.as_str().filter(|f| !f.is_empty()).unwrap_or("built-in (Montserrat)").to_string(),
        });
    Some(label)
}

/// Whether `value` is one of the setting's choices (for a font: a family name, or empty).
fn allowed(setting: &Value, value: &Value) -> bool {
    if setting["kind"] == "local_pptx" { return value.as_str().is_some_and(|p|p.is_empty()||(std::path::Path::new(p).is_absolute()&&std::path::Path::new(p).is_file()&&p.to_ascii_lowercase().ends_with(".pptx"))); }
    if setting["kind"] == "local_vbs" {
        return value.as_str().is_some_and(|p| p.is_empty() ||
            (std::path::Path::new(p).is_absolute() && std::path::Path::new(p).is_file() && p.to_ascii_lowercase().ends_with(".vbs") && !p.contains('"')));
    }
    if setting["kind"] == "google_font" {
        return value.as_str().is_some_and(|f| f.is_empty() || crate::fonts::valid_family(f));
    }
    if setting["kind"] == "list" {
        return value.as_array().is_some_and(|keys| keys.len() <= 36 && keys.iter().all(|k| k.as_str().is_some_and(home_key)));
    }
    if setting["kind"] == "number" {
        return value.as_i64().is_some_and(|n| Some(n) >= setting["min"].as_i64() && Some(n) <= setting["max"].as_i64());
    }
    setting["choices"].as_array().into_iter().flatten().any(|c| &c["value"] == value)
}

/// Applies changes `{id: value}`, each checked against its choices; unknown ids and values are
/// refused, and nothing is changed then. Returns what each change was, in words.
pub async fn apply(shared: &Shared, changes: &serde_json::Map<String, Value>) -> Result<Vec<String>, String> {
    // The look settings go to the board named by "board", else the first connected one.
    let wanted = changes.get("board").and_then(Value::as_str);
    if wanted.is_some() && target_board(shared, wanted).await.as_deref() != wanted {
        return Err(format!("no board {} is paired with this computer", wanted.unwrap_or_default()));
    }
    let target = target_board(shared, wanted).await;
    let settings = all_for(shared, target.as_deref()).await;
    let mut done = Vec::new();
    for (id, value) in changes.iter().filter(|(id, _)| id.as_str() != "board") {
        let setting = settings.iter().find(|s| s["id"] == id.as_str()).ok_or_else(|| format!("there is no setting called {id}"))?;
        let label = change_label(setting, value)
            .ok_or_else(|| if setting["kind"]=="local_vbs" || setting["kind"]=="local_pptx" {
                format!("{} needs an existing absolute file of the required type, or an empty value",setting["title"].as_str().unwrap_or(id))
            } else {format!("{} cannot be {value}", setting["title"].as_str().unwrap_or(id))})?;
        done.push(format!("{}: {label}", setting["title"].as_str().unwrap_or(id)));
    }
    if let Some(enabled)=changes.get("codex_interruptions_enabled").and_then(Value::as_bool){crate::codex_hooks::configure(enabled)?;}
    if changes.contains_key("presentation_notes_source")||changes.contains_key("presentation_pptx_file"){crate::powerpoint::configure_notes(changes.get("presentation_notes_source").and_then(Value::as_str),changes.get("presentation_pptx_file").and_then(Value::as_str))?;}
    let pager_enabled = changes.get("pager_enabled").and_then(Value::as_bool);
    let recorder_launcher = changes.get("recorder_launcher").and_then(Value::as_str);
    if pager_enabled.is_some() || recorder_launcher.is_some() {
        crate::pager::configure(pager_enabled, recorder_launcher)?;
    }
    let mut saved = display::load();
    let mut d = target.as_deref().map(|id| saved.for_board(id)).unwrap_or_else(|| saved.clone());
    let look = (d.theme, d.turn, d.key_font.clone(), d.start_font.clone(), d.start_size);
    let home = (d.home_apps.clone(), d.home_fill);
    let mut key_size = None;
    for (id, value) in changes {
        match id.as_str() {
            "key_size" => key_size = serde_json::from_value::<KeySize>(value.clone()).ok(),
            "theme" => d.theme = serde_json::from_value::<Theme>(value.clone()).map_err(|e| e.to_string())?,
            "turn" => d.turn = value.as_u64().and_then(|t| u8::try_from(t).ok()).ok_or("turn must be 0-3")?,
            "key_font" => d.key_font = value.as_str().unwrap_or_default().trim().to_string(),
            "start_font" => d.start_font = value.as_str().unwrap_or_default().trim().to_string(),
            "assistant_watches" => d.assistant_watches = value.as_bool().unwrap_or(true),
            "home_apps" => d.home_apps = value.as_array().into_iter().flatten().filter_map(|k| k.as_str().map(str::to_string)).collect(),
            "home_fill" => d.home_fill = value.as_bool().unwrap_or(true),
            "listen_quiet" => d.listen_quiet = value.as_i64().and_then(|n| i8::try_from(n).ok()).ok_or("listening's quiet level must be -80 to -20")?,
            "start_size" => d.start_size = value.as_u64().and_then(|n| u8::try_from(n).ok()).ok_or("start size must be 24-96")?,
            _ => {}
        }
    }
    if let Some(k) = key_size {
        d.key_size = k;
    }
    // The computer's settings at the top level, the look as the board's own.
    saved.assistant_watches = d.assistant_watches;
    saved.home_apps = d.home_apps.clone();
    saved.home_fill = d.home_fill;
    saved.listen_quiet = d.listen_quiet;
    match target.as_deref() {
        Some(id) => saved.set_board(id, &d),
        None => saved = Display { boards: saved.boards.clone(), ..d.clone() },
    }
    display::save(&saved)?;
    if let Some(k) = key_size {
        shared.key_size_changed(k);
    } else if home != (d.home_apps.clone(), d.home_fill) {
        // The board's screen is drawn again (the home screen with it), as for a new key size.
        shared.key_size_changed(d.key_size);
    }
    if let Some(id) = target.filter(|_| look != (d.theme, d.turn, d.key_font.clone(), d.start_font.clone(), d.start_size)) {
        shared.look_changed(id);
    }
    if let Some(board) = changes.get("board").and_then(Value::as_str) {
        done.insert(0, format!("Board: {board}"));
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_paths_are_omitted_from_assistant_settings() {
        for kind in ["local_vbs","local_pptx"] {
            let mut setting=json!({"kind":kind,"current":"example-private-file"});
            redact_local_path(&mut setting);
            assert!(setting["current"].is_null());assert_eq!(setting["configured"],true);
            assert_eq!(setting["private"],true);assert_eq!(setting["exists"],false);
            assert!(!setting.to_string().contains("example-private-file"));
            let mut empty=json!({"kind":kind,"current":""});redact_local_path(&mut empty);
            assert_eq!(empty["configured"],false);
            assert_eq!(change_label(&empty,&json!("")).as_deref(),Some("cleared"));
        }
        let mut ordinary=json!({"kind":"number","current":2});redact_local_path(&mut ordinary);
        assert_eq!(ordinary["current"],2);
    }

    #[test]
    fn turns_are_named_for_how_the_board_usually_sits() {
        assert_eq!(turn_choices(false)[0]["label"], "Landscape");
        assert_eq!(turn_choices(true)[0]["label"], "Portrait");
        assert_eq!(turn_choices(true)[3]["label"], "Landscape, upside down");
    }

    #[test]
    fn a_value_must_be_one_of_the_choices() {
        let setting = describe("turn", "Way round", "", json!([{"value": 0, "label": "Landscape"}, {"value": 1, "label": "Portrait"}]),
                               json!(0), "");
        assert!(allowed(&setting, &json!(1)));
        assert!(!allowed(&setting, &json!(5)));
        assert!(!allowed(&setting, &json!("1")), "the value's type counts");
        let font = font_setting("key_font", "Key font", "", "", true);
        assert!(allowed(&font, &json!("Atkinson Hyperlegible")) && allowed(&font, &json!("")));
        assert!(!allowed(&font, &json!("x&text=<script>")) && !allowed(&font, &json!(3)));
        let size = number_setting("start_size", "Start-up size", "", 64, 24, 96, "");
        assert!(allowed(&size, &json!(62)) && !allowed(&size, &json!(100)) && !allowed(&size, &json!("62")));
        assert_eq!(change_label(&size, &json!(62)).as_deref(), Some("62"));
        let home = list_setting("home_apps", "Home screen apps", "", &[]);
        assert!(allowed(&home, &json!(["app:org.musescore.MuseScore", "site:github.com"])) && allowed(&home, &json!([])));
        assert!(!allowed(&home, &json!(["firefox"])) && !allowed(&home, &json!(["app:"])) && !allowed(&home, &json!("app:x")));
        assert_eq!(change_label(&home, &json!(["app:firefox", "site:github.com"])).as_deref(), Some("firefox, github.com"));
        let quiet = signed_setting("listen_quiet", "Listening: quiet below", "", -60, -80, -20, "");
        assert!(allowed(&quiet, &json!(-70)) && !allowed(&quiet, &json!(-90)) && !allowed(&quiet, &json!(-10)));
    }
}
