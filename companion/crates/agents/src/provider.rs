//! Model providers. Two wire formats cover them all:
//!   - OpenAI-compatible chat completions: OpenAI, OpenRouter, Gemini (its OpenAI-compatible
//!     endpoint), Ollama, and any other compatible server;
//!   - Anthropic Messages API.
//! and one that is not a web API: Claude Code on this computer (`cli_tool.rs`), for a person with a
//! Claude plan but no API key.
//!
//! Configuration: `~/.config/knoend/agent.toml` (provider, model, and optionally base_url,
//! `[options]` added to every request and `omit` for parameters never to send). API keys come from
//! the environment, or from keys saved in the web app (`secrets.rs`), never from agent.toml.
//!
//! Providers change what they accept. A person must never need a rebuild for that: a parameter
//! the API rejects is dropped (or renamed, when the error names the replacement) and the request
//! retried, and every failure says what to change in the web app's AI settings.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Ollama,
    Openai,
    Openrouter,
    Gemini,
    Anthropic,
    OpenaiCompatible,
    /// The `claude` command on this computer, signed in with the person's own account.
    ClaudeCode,
}

impl Provider {
    /// The name agent.toml and the web app use ("openai-compatible", "claude-code").
    pub fn name(self) -> &'static str {
        match self {
            Provider::Ollama => "ollama",
            Provider::Openai => "openai",
            Provider::Openrouter => "openrouter",
            Provider::Gemini => "gemini",
            Provider::Anthropic => "anthropic",
            Provider::OpenaiCompatible => "openai-compatible",
            Provider::ClaudeCode => "claude-code",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub provider: Provider,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Seconds to wait for one reply. Default 180; 900 for Ollama, which may run on the CPU.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Extra fields for every request body (e.g. `reasoning_effort = "low"`); they override ours.
    #[serde(default)]
    pub options: serde_json::Map<String, Value>,
    /// Request fields never to send (e.g. `["temperature"]`).
    #[serde(default)]
    pub omit: Vec<String>,
}

impl Default for AgentConfig {
    /// Local and private by default: Ollama on this machine.
    fn default() -> Self {
        AgentConfig { provider: Provider::Ollama, model: None, base_url: None, api_key_env: None, timeout_secs: None,
                      options: Default::default(), omit: Vec::new() }
    }
}

impl AgentConfig {
    pub fn load(path: &Path) -> Result<AgentConfig, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(_) => Ok(AgentConfig::default()),
        }
    }

    pub fn base_url(&self) -> Result<String, String> {
        if let Some(url) = &self.base_url {
            return Ok(url.trim_end_matches('/').to_string());
        }
        Ok(match self.provider {
            Provider::Ollama => "http://localhost:11434/v1",
            Provider::Openai => "https://api.openai.com/v1",
            Provider::Openrouter => "https://openrouter.ai/api/v1",
            Provider::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
            Provider::Anthropic => "https://api.anthropic.com",
            Provider::OpenaiCompatible => return Err("openai-compatible needs base_url in agent.toml".into()),
            Provider::ClaudeCode => "",
        }.to_string())
    }

    /// `model` from agent.toml, else `<PROVIDER>_MODEL` from the environment, else a default where
    /// one is sensible (hosted OpenAI-style providers must name a model).
    pub fn model(&self) -> Result<String, String> {
        if let Some(model) = &self.model {
            return Ok(model.clone());
        }
        let env = match self.provider {
            Provider::Ollama => "OLLAMA_MODEL",
            Provider::Openai | Provider::OpenaiCompatible => "OPENAI_MODEL",
            Provider::Openrouter => "OPENROUTER_MODEL",
            Provider::Gemini => "GEMINI_MODEL",
            Provider::Anthropic => "ANTHROPIC_MODEL",
            // No model: Claude Code uses the one the person chose there.
            Provider::ClaudeCode => return Ok(String::new()),
        };
        if let Ok(model) = std::env::var(env) {
            if !model.is_empty() {
                return Ok(model);
            }
        }
        match self.provider {
            Provider::Ollama => Ok("qwen2.5-coder:7b".into()),
            Provider::Anthropic => Ok("claude-opus-5".into()),
            _ => Err("set `model` in ~/.config/knoend/agent.toml for this provider".into()),
        }
    }

    /// The environment variable (or saved-secret name) this provider's key goes by; None for
    /// providers that need no key.
    pub fn api_key_name(&self) -> Option<String> {
        Some(match (&self.api_key_env, self.provider) {
            (Some(var), _) => var.clone(),
            (None, Provider::Ollama | Provider::OpenaiCompatible | Provider::ClaudeCode) => return None,
            (None, Provider::Openai) => "OPENAI_API_KEY".into(),
            (None, Provider::Openrouter) => "OPENROUTER_API_KEY".into(),
            (None, Provider::Gemini) => "GEMINI_API_KEY".into(),
            (None, Provider::Anthropic) => "ANTHROPIC_API_KEY".into(),
        })
    }

    /// Where the key comes from: "env", "saved", "missing", or "none-needed".
    pub fn key_source(&self) -> &'static str {
        match self.api_key_name() {
            None => "none-needed",
            Some(name) if std::env::var(&name).is_ok_and(|v| !v.is_empty()) => "env",
            Some(name) if crate::secrets::get(&name).is_some() => "saved",
            Some(_) => "missing",
        }
    }

    fn api_key(&self) -> Result<Option<String>, String> {
        let Some(var) = self.api_key_name() else { return Ok(None) };
        if let Some(saved) = std::env::var(&var).ok().filter(|v| !v.is_empty()).or_else(|| crate::secrets::get(&var)) {
            return Ok(Some(saved));
        }
        Err(format!("no API key: set {var}, or enter the key in the web app's AI settings"))
    }

    /// A short description for logs and the saved file: provider and model, never the key.
    pub fn describe(&self) -> String {
        let model = self.model().unwrap_or_else(|_| "?".into());
        format!("{:?}/{}", self.provider, if model.is_empty() { "default" } else { &model }).to_lowercase()
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub(crate) role: &'static str,
    pub(crate) content: String,
}

impl Message {
    pub fn user(content: String) -> Message {
        Message { role: "user", content }
    }
    pub fn assistant(content: String) -> Message {
        Message { role: "assistant", content }
    }
}

/// Fields that make the request what it is; never dropped or renamed.
const ESSENTIAL: &[&str] = &["model", "messages", "system"];

/// Parameters a provider rejected for a model in this run, so later requests skip them at once.
static REJECTED: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);

/// One completion: returns the model's text.
pub async fn complete(config: &AgentConfig, system: &str, messages: &[Message]) -> Result<String, String> {
    let timeout = config.timeout_secs.unwrap_or(match config.provider {
        Provider::Ollama => 900,
        Provider::ClaudeCode => 300,
        _ => 180,
    });
    if config.provider == Provider::ClaudeCode {
        return crate::cli_tool::complete_claude(config.model.as_deref(), timeout, system, messages).await;
    }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(timeout)).build().map_err(|e| e.to_string())?;
    let base = config.base_url()?;
    let model = config.model()?;
    let key = config.api_key()?;
    let history: Vec<Value> = messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect();
    let anthropic = config.provider == Provider::Anthropic;

    let mut body = if anthropic {
        json!({
            "model": model,
            "max_tokens": 16000,
            "system": system,
            "messages": history,
            // If a safety classifier declines, let the API retry on a fallback model.
            "fallbacks": "default",
        })
    } else {
        let mut all = vec![json!({"role": "system", "content": system})];
        all.extend(history);
        json!({"model": model, "messages": all, "temperature": 0.2})
    };
    let fields = body.as_object_mut().expect("object");
    for (name, value) in &config.options {
        fields.insert(name.clone(), value.clone());
    }
    let memo = format!("{base} {model}");
    let learned = REJECTED.lock().unwrap().as_ref().and_then(|m| m.get(&memo).cloned()).unwrap_or_default();
    for name in config.omit.iter().chain(&learned) {
        fields.remove(name);
    }

    let mut dropped = Vec::new();
    let reply = loop {
        let (status, reply) = send(&client, &base, key.as_deref(), &body, config.provider).await?;
        if status.is_success() {
            break reply;
        }
        // A parameter this model does not take: drop it (or rename it as the error says), retry.
        if status == reqwest::StatusCode::BAD_REQUEST && dropped.len() < 3 {
            let fields = body.as_object_mut().expect("object");
            if let Some((param, instead)) = rejected_param(&reply, fields) {
                let value = fields.remove(&param);
                if let (Some(new), Some(value)) = (instead, value) {
                    fields.insert(new, value);
                }
                dropped.push(param);
                continue;
            }
        }
        return Err(explain(config, &base, status, &reply));
    };
    if !dropped.is_empty() {
        let mut map = REJECTED.lock().unwrap();
        map.get_or_insert_with(HashMap::new).entry(memo).or_default().extend(dropped);
    }

    if anthropic {
        if reply["stop_reason"] == "refusal" {
            return Err("the model declined this request".into());
        }
        let text: String = reply["content"].as_array().into_iter().flatten()
            .filter(|block| block["type"] == "text").filter_map(|block| block["text"].as_str()).collect();
        return if text.is_empty() { Err("empty reply from the model".into()) } else { Ok(text) };
    }
    reply["choices"][0]["message"]["content"].as_str().map(str::to_string)
        .filter(|t| !t.is_empty()).ok_or_else(|| "empty reply from the model".into())
}

/// One request in the provider's wire format: the status and the parsed reply.
async fn send(client: &reqwest::Client, base: &str, key: Option<&str>, body: &Value, provider: Provider)
              -> Result<(reqwest::StatusCode, Value), String> {
    let request = if provider == Provider::Anthropic {
        client.post(format!("{base}/v1/messages"))
            .header("x-api-key", key.unwrap_or_default())
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "server-side-fallback-2026-07-01")
    } else {
        let request = client.post(format!("{base}/chat/completions"));
        match key {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    };
    let response = request.json(body).send().await.map_err(|e| {
        format!("{provider:?} request to {base} failed: {e}. What to do: check the internet connection and the \
                 API address in AI settings (leave it empty for the provider's default){}",
                if provider == Provider::Ollama { "; for Ollama, check it is running (ollama serve)" } else { "" })
    })?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    // Some errors come back as plain text or HTML; keep them readable.
    let reply: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({"error": {"message": text.chars().take(300).collect::<String>()}}));
    // Gemini wraps its errors in a one-element list.
    let reply = match reply {
        Value::Array(mut items) if items.len() == 1 && items[0].get("error").is_some() => items.remove(0),
        other => other,
    };
    Ok((status, reply))
}

/// The optional request field a 400 error blames, and its replacement if the error names one.
/// Covers `error.param` (OpenAI style), "Unsupported parameter: 'x'", "'x' is not supported",
/// "x: Extra inputs are not permitted" (Anthropic style), `fieldViolations[].field` and
/// `Unknown name "x"` (Gemini) and "Use 'y' instead".
fn rejected_param(reply: &Value, fields: &serde_json::Map<String, Value>) -> Option<(String, Option<String>)> {
    let error = if reply["error"].is_object() { &reply["error"] } else { reply };
    let message = error["message"].as_str().or_else(|| reply["error"].as_str()).unwrap_or("");
    let droppable = |name: &str| fields.contains_key(name) && !ESSENTIAL.contains(&name);
    let quoted: Vec<&str> = message.split('\'').skip(1).step_by(2)
        .chain(message.split('"').skip(1).step_by(2)).collect();
    let violation = error["details"].as_array().into_iter().flatten()
        .filter_map(|d| d["fieldViolations"].as_array()).flatten()
        .filter_map(|v| v["field"].as_str()).find(|f| droppable(f)).map(str::to_string);
    let param = error["param"].as_str().filter(|p| droppable(p)).map(str::to_string)
        .or(violation)
        .or_else(|| quoted.iter().find(|q| droppable(q)).map(|q| q.to_string()))
        .or_else(|| message.split(':').next().map(str::trim).filter(|p| droppable(p)).map(str::to_string))?;
    let instead = message.find("instead").and_then(|end| {
        message[..end].rsplit('\'').nth(1).filter(|y| *y != param && !y.is_empty() && !y.contains(' ')).map(str::to_string)
    });
    Some((param, instead))
}

/// The provider's error, plus what the person can change (all in the web app's AI settings).
fn explain(config: &AgentConfig, base: &str, status: reqwest::StatusCode, reply: &Value) -> String {
    let detail = reply["error"]["message"].as_str().map(str::to_string)
        .unwrap_or_else(|| reply["error"].to_string());
    let todo = match status.as_u16() {
        401 | 403 => "the key was refused. Check it was copied whole, is for this provider, and the account has \
                      access and credit; enter it again in AI settings.".to_string(),
        404 => format!("the model or the address was not found. Pick a model from the list in AI settings (it \
                        comes from the provider), and leave the API address empty unless you need another \
                        one (now {base})."),
        429 => "too many requests or no credit left. Wait a minute, or check the account's usage and billing.".into(),
        400 | 422 => "the provider refused the request. If it names a setting this model does not take, add it to \
                      \"Leave out\" in AI settings > Advanced (or set the value it wants under request options), \
                      or choose another model.".into(),
        500..=599 => "the provider had a problem. Try again in a minute, or choose another model.".into(),
        _ => "check the provider, model and key in AI settings.".into(),
    };
    format!("{:?} returned {status}: {}. What to do: {todo}", config.provider, detail.trim().trim_end_matches('.'))
}

/// Model names the provider offers, sorted. `key` overrides the saved/env key for this one request
/// (used by the web app before the key is saved).
pub async fn list_models(config: &AgentConfig, key: Option<&str>) -> Result<Vec<String>, String> {
    if config.provider == Provider::ClaudeCode {
        // Claude Code's own short names; empty means its default.
        return Ok(["haiku", "opus", "sonnet"].map(String::from).to_vec());
    }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let base = config.base_url()?;
    let key = match key.filter(|k| !k.is_empty()) {
        Some(k) => Some(k.to_string()),
        None => config.api_key().ok().flatten(),
    };
    let request = if config.provider == Provider::Anthropic {
        client.get(format!("{base}/v1/models?limit=100"))
            .header("x-api-key", key.unwrap_or_default())
            .header("anthropic-version", "2023-06-01")
    } else {
        let mut r = client.get(format!("{base}/models"));
        if let Some(key) = key {
            r = r.bearer_auth(key);
        }
        r
    };
    let response = request.send().await.map_err(|e| format!("could not reach {:?}: {e}", config.provider))?;
    let status = response.status();
    let reply: Value = response.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{:?} returned {status}: {}", config.provider,
                           reply["error"]["message"].as_str().unwrap_or("check the API key")));
    }
    let mut models: Vec<String> = reply["data"].as_array().into_iter().flatten()
        .filter_map(|m| m["id"].as_str())
        .map(|id| id.trim_start_matches("models/").to_string())
        .collect();
    // Keep chat models: drop embedding, image, speech, moderation and similar endpoints.
    const NOT_CHAT: &[&str] = &["embed", "imagen", "image", "tts", "speech", "whisper", "transcribe", "audio",
                                "realtime", "moderation", "dall-e", "veo", "aqa", "learnlm"];
    models.retain(|m| { let l = m.to_lowercase(); !NOT_CHAT.iter().any(|w| l.contains(w)) });
    models.sort();
    models.dedup();
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> serde_json::Map<String, Value> {
        json!({"model": "m", "messages": [], "temperature": 0.2, "max_tokens": 100, "fallbacks": "default"})
            .as_object().unwrap().clone()
    }

    #[test]
    fn rejected_parameters_are_found_in_each_error_style() {
        let openai = json!({"error": {"message": "Unsupported value: 'temperature' does not support 0.2 with this model.", "param": "temperature"}});
        assert_eq!(rejected_param(&openai, &fields()), Some(("temperature".into(), None)));
        let rename = json!({"error": {"message": "Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead.", "param": "max_tokens"}});
        assert_eq!(rejected_param(&rename, &fields()), Some(("max_tokens".into(), Some("max_completion_tokens".into()))));
        let anthropic = json!({"type": "error", "error": {"type": "invalid_request_error", "message": "fallbacks: Extra inputs are not permitted"}});
        assert_eq!(rejected_param(&anthropic, &fields()), Some(("fallbacks".into(), None)));
        // Gemini (as received 2026-10-18, after unwrapping its one-element list).
        let gemini_unknown = json!({"error": {"code": 400, "message": "Invalid JSON payload received. Unknown name \"max_tokens\": Cannot find field.", "status": "INVALID_ARGUMENT"}});
        assert_eq!(rejected_param(&gemini_unknown, &fields()), Some(("max_tokens".into(), None)));
        let gemini_value = json!({"error": {"message": "Invalid value at 'temperature' (TYPE_FLOAT), \"hot\"",
            "details": [{"fieldViolations": [{"field": "temperature"}]}]}});
        assert_eq!(rejected_param(&gemini_value, &fields()), Some(("temperature".into(), None)));
    }

    #[test]
    fn essential_fields_and_unrelated_errors_are_left_alone() {
        let model = json!({"error": {"message": "The model 'model' does not exist", "param": "model"}});
        assert_eq!(rejected_param(&model, &fields()), None);
        let other = json!({"error": {"message": "Invalid API key"}});
        assert_eq!(rejected_param(&other, &fields()), None);
    }

    #[test]
    fn options_and_omit_come_from_agent_toml() {
        let config: AgentConfig = toml::from_str("provider = \"openai\"\nmodel = \"m\"\nomit = [\"temperature\"]\n[options]\nreasoning_effort = \"low\"\n").unwrap();
        assert_eq!(config.omit, vec!["temperature"]);
        assert_eq!(config.options["reasoning_effort"], "low");
    }
}
