//! The local web app and its API, on 127.0.0.1 only.
//!
//! Security: every request must name this server as its Host (blocks DNS rebinding); a request
//! carrying an Origin must come from this server's own page; every /api call needs the session
//! token, which is only ever handed to the page this server serves. The Wi-Fi password passes
//! through to the board over USB and is never logged or stored here.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::service::Shared;

const INDEX: &str = include_str!("../../../../web/index.html");
const APP_JS: &str = include_str!("../../../../web/app.js");
const STYLE: &str = include_str!("../../../../web/style.css");

#[derive(Clone)]
struct App {
    shared: Arc<Shared>,
    token: String,
    port: u16,
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn fail(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"error": message.into()})))
}

pub async fn serve(shared: Arc<Shared>, port: u16, open: bool) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await
        .map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    let token = openpager_transport::auth::nonce();
    let app = App { shared, token, port };

    let api = Router::new()
        .route("/state", get(state))
        .route("/events", get(events))
        .route("/firmware", get(firmware))
        .route("/ports", get(ports))
        .route("/identify", post(identify))
        .route("/flash", post(flash))
        .route("/provision", post(provision))
        .route("/drafts", get(drafts))
        .route("/ai", get(ai_settings).post(save_ai_settings))
        .route("/assist", post(assist))
        .layer(middleware::from_fn_with_state(app.clone(), require_token));
    let router = Router::new()
        .route("/", get(index))
        .route("/app.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], APP_JS) }))
        .route("/style.css", get(|| async { ([(header::CONTENT_TYPE, "text/css")], STYLE) }))
        .nest("/api", api)
        .layer(middleware::from_fn_with_state(app.clone(), require_local))
        .with_state(app);

    let url = format!("http://127.0.0.1:{port}/");
    println!("OpenPager is at {url}");
    if open {
        let opener = if cfg!(target_os = "macos") { "open" } else if cfg!(windows) { "explorer" } else { "xdg-open" };
        let _ = std::process::Command::new(opener).arg(&url).spawn();
    }
    axum::serve(listener, router).await.map_err(|e| e.to_string())
}

async fn require_local(State(app): State<App>, request: Request, next: Next) -> Response {
    let host = request.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let ours = [format!("127.0.0.1:{}", app.port), format!("localhost:{}", app.port)];
    if !ours.iter().any(|h| h == host) {
        return fail(StatusCode::FORBIDDEN, "wrong host").into_response();
    }
    if let Some(origin) = request.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        if !ours.iter().any(|h| origin == format!("http://{h}")) {
            return fail(StatusCode::FORBIDDEN, "requests from other sites are not allowed").into_response();
        }
    }
    next.run(request).await
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

async fn require_token(State(app): State<App>, Query(query): Query<TokenQuery>, headers: HeaderMap,
                       request: Request, next: Next) -> Response {
    // EventSource cannot send headers, so the event stream alone takes the token in the URL.
    let given = headers.get("x-openpager-token").and_then(|t| t.to_str().ok()).map(str::to_string)
        .or_else(|| (request.uri().path() == "/events").then_some(query.token).flatten());
    if !given.is_some_and(|t| openpager_transport::auth::equal(&t, &app.token)) {
        return fail(StatusCode::UNAUTHORIZED, "missing or wrong token").into_response();
    }
    next.run(request).await
}

async fn index(State(app): State<App>) -> Html<String> {
    Html(INDEX.replace("{{TOKEN}}", &app.token))
}

async fn state(State(app): State<App>) -> ApiResult {
    let status = app.shared.status.read().await.clone();
    Ok(Json(json!({"status": status, "version": env!("CARGO_PKG_VERSION"), "firmwareSource": app.shared.firmware_source,
                   "os": std::env::consts::OS})))
}

async fn events(State(app): State<App>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(app.shared.events.subscribe())
        .filter_map(|item| item.ok())
        .map(|value| Ok(Event::default().data(value.to_string())));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn firmware(State(app): State<App>) -> ApiResult {
    let manifest = crate::firmware::manifest(&app.shared.firmware_source).await
        .map_err(|e| fail(StatusCode::BAD_GATEWAY, format!("firmware list unavailable: {e}")))?;
    Ok(Json(json!({"source": app.shared.firmware_source, "manifest": manifest})))
}

async fn ports() -> ApiResult {
    let ports = tokio::task::spawn_blocking(openpager_flash::list_ports).await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({"ports": ports})))
}

#[derive(Deserialize)]
struct PortRequest {
    port: String,
}

/// Who is on this port: OpenPager's own identity if it answers (gentle), else the chip's boot
/// ROM (resets the board). Returns ranked board suggestions from the firmware manifest.
async fn identify(State(app): State<App>, Json(request): Json<PortRequest>) -> ApiResult {
    let _serial = app.shared.serial.clone().try_lock_owned()
        .map_err(|_| fail(StatusCode::CONFLICT, "the USB port is busy with another job"))?;
    let port = openpager_flash::list_ports().into_iter().find(|p| p.name == request.port)
        .ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("{} is not connected", request.port)))?;

    let name = port.name.clone();
    let running = tokio::task::spawn_blocking(move || crate::provision::probe(&name)).await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let profile = running.as_ref().and_then(|r| r["board"]["profile"].as_str()).map(str::to_string);

    let name = port.name.clone();
    let chip = tokio::task::spawn_blocking(move || openpager_flash::identify(&name)).await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let (chip, chip_error) = match chip {
        Ok(c) => (Some(c), None),
        Err(e) => (None, Some(e)),
    };
    let candidates = match crate::firmware::manifest(&app.shared.firmware_source).await {
        Ok(m) => serde_json::to_value(m.candidates(&port, chip.as_ref(), profile.as_deref())).unwrap_or_default(),
        Err(_) => json!([]),
    };
    Ok(Json(json!({"port": port, "chip": chip, "chipError": chip_error, "running": running, "candidates": candidates})))
}

#[derive(Deserialize)]
struct FlashRequest {
    port: String,
    board: String,
    #[serde(default)]
    erase: bool,
}

/// Starts a flashing job; progress and the result arrive as "flash" events.
async fn flash(State(app): State<App>, Json(request): Json<FlashRequest>) -> ApiResult {
    let guard = app.shared.serial.clone().try_lock_owned()
        .map_err(|_| fail(StatusCode::CONFLICT, "the USB port is busy with another job"))?;
    let source = app.shared.firmware_source.clone();
    let manifest = crate::firmware::manifest(&source).await
        .map_err(|e| fail(StatusCode::BAD_GATEWAY, format!("firmware list unavailable: {e}")))?;
    let parts = crate::firmware::parts_for(&source, &manifest, &request.board).await
        .map_err(|e| fail(StatusCode::BAD_REQUEST, e))?;
    let shared = app.shared.clone();
    let version = manifest.version.clone();
    shared.emit("flash", json!({"stage": "verified", "text": format!("Firmware {version} for {}: {} parts verified", request.board, parts.len())}));

    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        let mut last_percent = None;
        let result = openpager_flash::flash(&request.port, &parts, request.erase, &mut |p| {
            if let openpager_flash::Progress::Writing { done, total, .. } = &p {
                let percent = if *total == 0 { 100 } else { done * 100 / total };
                if last_percent == Some(percent) {
                    return;
                }
                last_percent = Some(percent);
            }
            shared.emit("flash", serde_json::to_value(&p).unwrap_or_default());
        });
        match result {
            Ok(()) => shared.emit("flash", json!({"stage": "finished", "version": version, "text": "Flashed and restarted"})),
            Err(e) => shared.emit("flash", json!({"stage": "failed", "error": e, "text": format!("Flashing failed: {e}")})),
        }
    });
    Ok(Json(json!({"started": true})))
}

#[derive(Deserialize)]
struct ProvisionRequest {
    port: String,
    ssid: Option<String>,
    password: Option<String>,
}

/// Pairs the board over USB and, if a network is given, hands it the Wi-Fi settings.
async fn provision(State(app): State<App>, Json(request): Json<ProvisionRequest>) -> ApiResult {
    let _serial = app.shared.serial.clone().try_lock_owned()
        .map_err(|_| fail(StatusCode::CONFLICT, "the USB port is busy with another job"))?;
    let shared = app.shared.clone();
    let result = tokio::task::spawn_blocking(move || {
        let wifi = match (&request.ssid, &request.password) {
            (Some(ssid), password) if !ssid.trim().is_empty() => Some((ssid.trim().to_string(), password.clone().unwrap_or_default())),
            _ => None,
        };
        crate::provision::provision(&request.port, wifi.as_ref().map(|(s, p)| (s.as_str(), p.as_str())),
                                    &mut |line| shared.emit("provision", json!({"text": line})))
    }).await.map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let done = result.map_err(|e| fail(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(serde_json::to_value(done).unwrap_or_default()))
}

/// AI-drafted libraries waiting for review.
async fn drafts() -> ApiResult {
    let dir = crate::service::ai_libraries();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for path in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")) {
            if let Ok(library) = std::fs::read_to_string(&path).map_err(|e| e.to_string())
                .and_then(|t| openpager_core::library::Library::parse(&t).map_err(|e| e.to_string())) {
                out.push(json!({"id": library.pack.id, "name": library.pack.name, "keys": library.keys.len(),
                                "labels": library.keys.iter().map(|k| k.label.clone()).collect::<Vec<_>>(),
                                "file": path.file_name().map(|n| n.to_string_lossy().to_string())}));
            }
        }
    }
    Ok(Json(json!({"drafts": out})))
}

// ---- AI settings and the setup assistant ----

fn agent_toml() -> std::path::PathBuf {
    openpager_transport::pairing::config_dir().join("agent.toml")
}

/// Provider, model and where the key comes from. The key itself is never returned.
async fn ai_settings() -> ApiResult {
    match crate::agent_config() {
        Ok(config) => Ok(Json(json!({
            "configured": agent_toml().exists(),
            "provider": format!("{:?}", config.provider).to_lowercase(),
            "model": config.model().ok(),
            "key": config.key_source(),
            "describe": config.describe(),
        }))),
        Err(e) => Ok(Json(json!({"configured": false, "error": e}))),
    }
}

#[derive(Deserialize)]
struct AiRequest {
    provider: String,
    model: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
}

/// Saves provider + model to agent.toml and the key to the local key store, then checks them with
/// a tiny request.
async fn save_ai_settings(Json(request): Json<AiRequest>) -> ApiResult {
    let allowed = ["gemini", "openai", "openrouter", "anthropic", "ollama", "openai-compatible"];
    if !allowed.contains(&request.provider.as_str()) {
        return Err(fail(StatusCode::BAD_REQUEST, "unknown provider"));
    }
    let mut table = toml::Table::new();
    table.insert("provider".into(), request.provider.clone().into());
    for (name, value) in [("model", &request.model), ("base_url", &request.base_url)] {
        if let Some(v) = value.as_ref().map(|v| v.trim()).filter(|v| !v.is_empty()) {
            table.insert(name.into(), v.to_string().into());
        }
    }
    let text = format!("# Written by the OpenPager web app. API keys are kept in secrets.json, not here.\n{}",
                       toml::to_string(&table).map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?);
    std::fs::create_dir_all(agent_toml().parent().expect("parent")).ok();
    std::fs::write(agent_toml(), text).map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let config = crate::agent_config().map_err(|e| fail(StatusCode::BAD_REQUEST, e))?;
    if let Some(key) = request.api_key.as_ref().map(|k| k.trim()).filter(|k| !k.is_empty()) {
        let name = config.api_key_name().ok_or_else(|| fail(StatusCode::BAD_REQUEST, "this provider takes no API key"))?;
        openpager_agents::secrets::save(&name, key).map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    match openpager_agents::assistant::check(&config).await {
        Ok(reply) => Ok(Json(json!({"ok": true, "describe": config.describe(), "key": config.key_source(), "reply": reply}))),
        Err(e) => Ok(Json(json!({"ok": false, "describe": config.describe(), "key": config.key_source(), "error": e}))),
    }
}

#[derive(Deserialize)]
struct AssistRequest {
    #[serde(default)]
    question: String,
    #[serde(default)]
    context: Value,
}

/// Asks the setup assistant. Returns its advice, the buttons it suggests, and exactly what was sent.
async fn assist(State(app): State<App>, Json(request): Json<AssistRequest>) -> ApiResult {
    let config = crate::agent_config().map_err(|e| fail(StatusCode::BAD_REQUEST, e))?;
    let snapshot = crate::diagnostics::snapshot(&app.shared, request.context).await;
    let advice = openpager_agents::assistant::advise(&config, &request.question, &snapshot).await
        .map_err(|e| fail(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(json!({"answer": advice.answer, "actions": advice.actions, "sent": snapshot, "by": config.describe()})))
}
