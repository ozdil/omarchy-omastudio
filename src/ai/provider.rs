//! AI providers for the edit assistant: Anthropic's Messages API, OpenAI-compatible
//! chat completions (OpenAI, OpenRouter, Google Gemini) and local servers (Ollama,
//! LM Studio).
//!
//! API keys live in the desktop's Secret Service (GNOME Keyring, KeePassXC, ...)
//! through `secret-tool`, under `service=omastudio provider=<id>`; an environment
//! variable such as `ANTHROPIC_API_KEY` is used when no key is stored. Keys never
//! appear on a command line: curl reads its headers from stdin. The chosen provider
//! and model are kept in `~/.config/omastudio/ai-provider.json`.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::security::{self, run_bounded_command_with_stdin, secure_command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// `POST /v1/messages`
    Anthropic,
    /// `POST /v1/chat/completions`
    OpenAi,
}

#[derive(Debug, Clone, Copy)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub wire: Wire,
    pub endpoint: &'static str,
    /// Empty: the user chooses the model (or the local server's first model is used)
    pub default_model: &'static str,
    pub key_env: Option<&'static str>,
    pub local: bool,
}

pub const PROVIDERS: &[Provider] = &[
    Provider { id: "anthropic", name: "Anthropic Claude", wire: Wire::Anthropic, endpoint: "https://api.anthropic.com/v1/messages", default_model: "claude-opus-5-5", key_env: Some("ANTHROPIC_API_KEY"), local: false },
    Provider { id: "openai", name: "OpenAI", wire: Wire::OpenAi, endpoint: "https://api.openai.com/v1/chat/completions", default_model: "", key_env: Some("OPENAI_API_KEY"), local: false },
    Provider { id: "openrouter", name: "OpenRouter", wire: Wire::OpenAi, endpoint: "https://openrouter.ai/api/v1/chat/completions", default_model: "", key_env: Some("OPENROUTER_API_KEY"), local: false },
    Provider { id: "gemini", name: "Google Gemini", wire: Wire::OpenAi, endpoint: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions", default_model: "", key_env: Some("GEMINI_API_KEY"), local: false },
    Provider { id: "ollama", name: "Ollama (local)", wire: Wire::OpenAi, endpoint: "http://127.0.0.1:11434/v1/chat/completions", default_model: "", key_env: None, local: true },
    Provider { id: "lmstudio", name: "LM Studio (local)", wire: Wire::OpenAi, endpoint: "http://127.0.0.1:1234/v1/chat/completions", default_model: "", key_env: None, local: true },
];

pub fn find(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// The provider and model the user chose.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Choice {
    pub provider: String,
    pub model: String,
    /// Overrides the provider's endpoint (another OpenAI-compatible server)
    pub endpoint: Option<String>,
}

fn choice_path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config"));
    config.join("omastudio/ai-provider.json")
}

pub fn load_choice() -> Choice {
    security::read_secure_file(&choice_path(), 64 * 1024)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_choice(choice: &Choice) -> Result<(), String> {
    let path = choice_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_vec_pretty(choice).map_err(|e| e.to_string())?;
    security::atomic_write_user(&path, &text).map_err(|e| format!("Cannot write {}: {e}", path.display()))
}

fn secret_tool() -> std::process::Command {
    let mut cmd = secure_command("secret-tool");
    // libsecret reaches the keyring over the session bus
    if let Ok(bus) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        cmd.env("DBUS_SESSION_BUS_ADDRESS", bus);
    }
    cmd
}

/// Stored key from the Secret Service, else the provider's environment variable.
pub fn api_key(p: &Provider) -> Option<String> {
    let mut cmd = secret_tool();
    cmd.args(["lookup", "service", "omastudio", "provider", p.id]);
    if let Ok((0, out, _)) = run_bounded_command_with_stdin(cmd, Duration::from_secs(10), None) {
        let key = String::from_utf8_lossy(&out).trim().to_string();
        if valid_key(&key) {
            return Some(key);
        }
    }
    p.key_env.and_then(|v| std::env::var(v).ok()).map(|k| k.trim().to_string()).filter(|k| valid_key(k))
}

fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 512 && key.chars().all(|c| c.is_ascii_graphic())
}

pub fn store_key(p: &Provider, key: &str) -> Result<(), String> {
    let key = key.trim();
    if !valid_key(key) {
        return Err("The key is empty or contains spaces or control characters".to_string());
    }
    let mut cmd = secret_tool();
    let label = format!("OmaStudio {} API key", p.name);
    cmd.args(["store", "--label", &label, "service", "omastudio", "provider", p.id]);
    match run_bounded_command_with_stdin(cmd, Duration::from_secs(60), Some(key.as_bytes())) {
        Ok((0, _, _)) => Ok(()),
        Ok((_, _, err)) => Err(format!("secret-tool could not store the key: {}", String::from_utf8_lossy(&err).trim())),
        Err(e) => Err(format!("secret-tool is not available ({e}); install libsecret and a keyring")),
    }
}

pub fn delete_key(p: &Provider) -> Result<(), String> {
    let mut cmd = secret_tool();
    cmd.args(["clear", "service", "omastudio", "provider", p.id]);
    run_bounded_command_with_stdin(cmd, Duration::from_secs(30), None)
        .map_err(|e| e.to_string())
        .and_then(|(code, _, err)| if code == 0 { Ok(()) } else { Err(String::from_utf8_lossy(&err).trim().to_string()) })
}

/// An endpoint curl may use: HTTPS, or plain HTTP to this machine only.
fn checked_endpoint(url: &str) -> Result<&str, String> {
    if !url.chars().all(|c| c.is_ascii_graphic()) || url.contains('@') {
        return Err("Endpoint contains spaces, control characters or user info".to_string());
    }
    let loopback = ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"];
    if url.starts_with("https://") || loopback.iter().any(|l| url.starts_with(l)) {
        Ok(url)
    } else {
        Err("Endpoint must use HTTPS (plain HTTP only to localhost)".to_string())
    }
}

/// Runs curl with headers on stdin and an optional body file; returns the response body.
fn curl(url: &str, headers: &[String], body: Option<&std::path::Path>, timeout: Duration) -> Result<Value, String> {
    let url = checked_endpoint(url)?;
    let mut cmd = secure_command("curl");
    cmd.args(["-sS", "--max-time", &timeout.as_secs().to_string(), "-K", "-"]);
    if let Some(body) = body {
        cmd.args(["-X", "POST", "--data-binary"]).arg(format!("@{}", body.display()));
    }
    cmd.args(["--", url]);
    let config: String = headers.iter().map(|h| format!("header = \"{}\"\n", h.replace('"', "\\\""))).collect();
    let (code, out, err) = run_bounded_command_with_stdin(cmd, timeout + Duration::from_secs(5), Some(config.as_bytes()))
        .map_err(|e| format!("Cannot run curl: {e}"))?;
    if code != 0 {
        return Err(format!("Request to {url} failed: {}", String::from_utf8_lossy(&err).trim()));
    }
    let value: Value = serde_json::from_slice(&out).map_err(|_| format!("Unexpected answer from {url}"))?;
    if let Some(e) = value.get("error") {
        let msg = e.get("message").and_then(Value::as_str).map(String::from).unwrap_or_else(|| e.to_string());
        return Err(format!("Provider error: {msg}"));
    }
    Ok(value)
}

/// Models a local server offers (`GET /v1/models`); None when it is not running.
pub fn local_models(p: &Provider) -> Option<Vec<String>> {
    let url = p.endpoint.replace("/chat/completions", "/models");
    let v = curl(&url, &[], None, Duration::from_secs(3)).ok()?;
    Some(
        v.get("data")?
            .as_array()?
            .iter()
            .filter_map(|m| m.get("id").and_then(Value::as_str).map(String::from))
            .collect(),
    )
}

#[derive(Debug, Serialize)]
pub struct ProviderStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub local: bool,
    /// A key is stored (cloud) or the server answers (local)
    pub ready: bool,
    pub models: Vec<String>,
    pub default_model: &'static str,
}

pub fn status() -> Vec<ProviderStatus> {
    PROVIDERS
        .iter()
        .map(|p| {
            let (ready, models) = if p.local {
                match local_models(p) {
                    Some(m) => (true, m),
                    None => (false, Vec::new()),
                }
            } else {
                (api_key(p).is_some(), Vec::new())
            };
            ProviderStatus { id: p.id, name: p.name, local: p.local, ready, models, default_model: p.default_model }
        })
        .collect()
}

/// A request resolved to a provider, model, endpoint and key.
pub struct Target {
    pub provider: &'static Provider,
    pub model: String,
    pub endpoint: String,
    key: Option<String>,
}

/// Resolves the provider: explicit arguments, then the saved choice, then the first
/// local server that runs, then the first provider with a key.
pub fn resolve(provider: Option<&str>, model: Option<&str>) -> Result<Target, String> {
    let saved = load_choice();
    let id = provider.map(String::from).or_else(|| (!saved.provider.is_empty()).then(|| saved.provider.clone()));
    let p = match id {
        Some(id) => find(&id).ok_or_else(|| format!("Unknown provider {id}"))?,
        None => PROVIDERS
            .iter()
            .find(|p| if p.local { local_models(p).is_some_and(|m| !m.is_empty()) } else { api_key(p).is_some() })
            .ok_or("No AI provider is set up. Store a key with `omastudio ai key <provider>` or start Ollama or LM Studio.")?,
    };
    let same = saved.provider == p.id;
    let mut model = model
        .map(String::from)
        .or_else(|| (same && !saved.model.is_empty()).then(|| saved.model.clone()))
        .unwrap_or_else(|| p.default_model.to_string());
    if model.is_empty() && p.local {
        model = local_models(p).and_then(|m| m.into_iter().next()).unwrap_or_default();
    }
    if model.is_empty() {
        return Err(format!("Choose a model for {}: `omastudio ai use {} <model>`", p.name, p.id));
    }
    let endpoint = saved.endpoint.filter(|_| same).unwrap_or_else(|| p.endpoint.to_string());
    let key = if p.local { None } else { Some(api_key(p).ok_or_else(|| format!("No API key for {}: `omastudio ai key {}`", p.name, p.id))?) };
    Ok(Target { provider: p, model, endpoint, key })
}

/// Sends a system prompt, a JPEG and a text, and asks for JSON matching `schema`.
pub fn ask_json(target: &Target, system: &str, jpeg: &[u8], text: &str, schema: &Value) -> Result<Value, String> {
    let image = base64(jpeg);
    let (body, mut headers) = match target.provider.wire {
        Wire::Anthropic => {
            let mut body = json!({
                "model": target.model,
                "max_tokens": 16000,
                "system": system,
                "output_config": { "effort": "medium", "format": { "type": "json_schema", "schema": schema } },
                "messages": [{ "role": "user", "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": image } },
                    { "type": "text", "text": text }
                ]}],
            });
            let mut headers = vec!["Content-Type: application/json".to_string(), "anthropic-version: 2026-10-21".to_string()];
            // On a classifier decline the API re-runs the request on a fallback model
            if matches!(target.model.as_str(), "claude-opus-5-5" | "claude-sonnet-5-5" | "claude-fable-5-1")
                && target.endpoint.starts_with("https://api.anthropic.com/")
            {
                body["fallbacks"] = json!("default");
                headers.push("anthropic-beta: server-side-fallback-2026-07-01".to_string());
            }
            if let Some(key) = &target.key {
                headers.push(format!("x-api-key: {key}"));
            }
            (body, headers)
        }
        Wire::OpenAi => {
            let body = json!({
                "model": target.model,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": [
                        { "type": "image_url", "image_url": { "url": format!("data:image/jpeg;base64,{image}") } },
                        { "type": "text", "text": text }
                    ]}
                ],
                "response_format": { "type": "json_schema", "json_schema": { "name": "edit", "strict": true, "schema": schema } },
            });
            let mut headers = vec!["Content-Type: application/json".to_string()];
            if let Some(key) = &target.key {
                headers.push(format!("Authorization: Bearer {key}"));
            }
            (body, headers)
        }
    };
    if target.provider.id == "openrouter" {
        headers.push("X-Title: OmaStudio".to_string());
    }

    // The body holds the image: too large for an argument, so it goes through a file
    let file = security::get_secure_preview_dir().join(format!("omastudio_ai_{}.json", std::process::id()));
    security::atomic_write_user(&file, body.to_string().as_bytes()).map_err(|e| e.to_string())?;
    let timeout = Duration::from_secs(if target.provider.local { 600 } else { 180 });
    let answer = curl(&target.endpoint, &headers, Some(&file), timeout);
    let _ = std::fs::remove_file(&file);
    let answer = answer?;

    let text = match target.provider.wire {
        Wire::Anthropic => {
            if answer.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
                return Err("The model declined this request".to_string());
            }
            answer.get("content").and_then(Value::as_array).into_iter().flatten()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<String>()
        }
        Wire::OpenAi => answer.pointer("/choices/0/message/content").and_then(Value::as_str).unwrap_or("").to_string(),
    };
    parse_json_object(&text).ok_or_else(|| "The model's answer was not valid JSON".to_string())
}

/// The JSON object in a model's answer, tolerating code fences or surrounding prose.
fn parse_json_object(text: &str) -> Option<Value> {
    if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(text.trim()) {
        return Some(v);
    }
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str::<Value>(text.get(start..=end)?).ok().filter(Value::is_object)
}

pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn endpoints_need_https_or_loopback() {
        assert!(checked_endpoint("https://api.anthropic.com/v1/messages").is_ok());
        assert!(checked_endpoint("http://127.0.0.1:11434/v1/chat/completions").is_ok());
        assert!(checked_endpoint("http://example.com/v1").is_err());
        assert!(checked_endpoint("http://localhost.evil.com/v1").is_err());
        assert!(checked_endpoint("https://a b").is_err());
        assert!(checked_endpoint("http://127.0.0.1:1@evil.com/v1").is_err());
    }

    #[test]
    fn finds_json_in_prose() {
        assert_eq!(parse_json_object("```json\n{\"a\": 1}\n```").unwrap()["a"], 1);
        assert!(parse_json_object("no json").is_none());
    }
}
