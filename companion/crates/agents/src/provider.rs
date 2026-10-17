//! Model providers. Two wire formats cover them all:
//!   - OpenAI-compatible chat completions: OpenAI, OpenRouter, Gemini (its OpenAI-compatible
//!     endpoint), Ollama, and any other compatible server;
//!   - Anthropic Messages API.
//!
//! Configuration: `~/.config/openpager/agent.toml` (provider, model). API keys come from the
//! environment, or from keys saved in the web app (`secrets.rs`), never from agent.toml.

use std::path::Path;
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
}

impl Default for AgentConfig {
    /// Local and private by default: Ollama on this machine.
    fn default() -> Self {
        AgentConfig { provider: Provider::Ollama, model: None, base_url: None, api_key_env: None, timeout_secs: None }
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
        };
        if let Ok(model) = std::env::var(env) {
            if !model.is_empty() {
                return Ok(model);
            }
        }
        match self.provider {
            Provider::Ollama => Ok("qwen2.5-coder:7b".into()),
            Provider::Anthropic => Ok("claude-opus-5".into()),
            _ => Err("set `model` in ~/.config/openpager/agent.toml for this provider".into()),
        }
    }

    /// The environment variable (or saved-secret name) this provider's key goes by; None for
    /// providers that need no key.
    pub fn api_key_name(&self) -> Option<String> {
        Some(match (&self.api_key_env, self.provider) {
            (Some(var), _) => var.clone(),
            (None, Provider::Ollama | Provider::OpenaiCompatible) => return None,
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
        format!("{:?}/{}", self.provider, self.model().unwrap_or_else(|_| "?".into())).to_lowercase()
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    role: &'static str,
    content: String,
}

impl Message {
    pub fn user(content: String) -> Message {
        Message { role: "user", content }
    }
    pub fn assistant(content: String) -> Message {
        Message { role: "assistant", content }
    }
}

/// One completion: returns the model's text.
pub async fn complete(config: &AgentConfig, system: &str, messages: &[Message]) -> Result<String, String> {
    let timeout = config.timeout_secs.unwrap_or(if config.provider == Provider::Ollama { 900 } else { 180 });
    let client = reqwest::Client::builder().timeout(Duration::from_secs(timeout)).build().map_err(|e| e.to_string())?;
    let base = config.base_url()?;
    let model = config.model()?;
    let key = config.api_key()?;
    let history: Vec<Value> = messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect();

    if config.provider == Provider::Anthropic {
        let body = json!({
            "model": model,
            "max_tokens": 16000,
            "system": system,
            "messages": history,
            // If a safety classifier declines, let the API retry on a fallback model.
            "fallbacks": "default",
        });
        let response = client.post(format!("{base}/v1/messages"))
            .header("x-api-key", key.unwrap_or_default())
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "server-side-fallback-2026-07-01")
            .json(&body).send().await.map_err(|e| format!("Anthropic request failed: {e}"))?;
        let status = response.status();
        let reply: Value = response.json().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("Anthropic returned {status}: {}", reply["error"]["message"]));
        }
        if reply["stop_reason"] == "refusal" {
            return Err("the model declined this request".into());
        }
        let text: String = reply["content"].as_array().into_iter().flatten()
            .filter(|block| block["type"] == "text").filter_map(|block| block["text"].as_str()).collect();
        return if text.is_empty() { Err("empty reply from the model".into()) } else { Ok(text) };
    }

    let mut all = vec![json!({"role": "system", "content": system})];
    all.extend(history);
    let mut request = client.post(format!("{base}/chat/completions"))
        .json(&json!({"model": model, "messages": all, "temperature": 0.2}));
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    let response = request.send().await.map_err(|e| format!("{:?} request failed: {e}", config.provider))?;
    let status = response.status();
    let reply: Value = response.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{:?} returned {status}: {}", config.provider, reply["error"]));
    }
    reply["choices"][0]["message"]["content"].as_str().map(str::to_string)
        .filter(|t| !t.is_empty()).ok_or_else(|| "empty reply from the model".into())
}
