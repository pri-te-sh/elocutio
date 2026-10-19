//! Minimal OpenRouter client for M0.
//!
//! The key is stored in the macOS Keychain and never logged. This module is
//! deliberately small: it proves key validation and one rewrite before the
//! full provider router lands in M1.

use std::time::Duration;

use elocutio_llm::{ModelRef, ProviderId};
use serde::{Deserialize, Serialize};

const KEYCHAIN_SERVICE: &str = "app.elocutio.provider.openrouter";
const KEYCHAIN_ACCOUNT: &str = "api_key";
const KEY_URL: &str = "https://openrouter.ai/api/v1/key";
const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const APP_URL: &str = "https://github.com/pri-te-sh/elocutio";
const APP_TITLE: &str = "Elocutio";

pub const DEFAULT_MODEL: &str = "anthropic/claude-haiku-5.5";

const SYSTEM_PROMPT: &str = "You are Elocutio, a precise copy editor. Rewrite the user's text to be clear, correct, and natural while preserving their voice. Return only the rewritten text, with no preamble, quotation marks, or explanation.";

#[derive(Debug, Clone, Serialize)]
pub struct KeyStatus {
    pub present: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenRouterStatus {
    pub label: Option<String>,
    pub usage: Option<f64>,
    pub limit: Option<f64>,
    pub is_free_tier: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RewriteResult {
    pub text: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).map_err(|error| error.to_string())
}

pub fn key_status() -> Result<KeyStatus, String> {
    match entry()?.get_password() {
        Ok(_) => Ok(KeyStatus { present: true }),
        Err(keyring::Error::NoEntry) => Ok(KeyStatus { present: false }),
        Err(error) => Err(error.to_string()),
    }
}

pub fn save_key(key: &str) -> Result<KeyStatus, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Enter an OpenRouter API key first.".into());
    }
    entry()?
        .set_password(key)
        .map_err(|error| format!("Could not save the key in the Keychain: {error}"))?;
    Ok(KeyStatus { present: true })
}

pub fn clear_key() -> Result<KeyStatus, String> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(KeyStatus { present: false }),
        Err(keyring::Error::NoEntry) => Ok(KeyStatus { present: false }),
        Err(error) => Err(format!(
            "Could not remove the key from the Keychain: {error}"
        )),
    }
}

fn load_key() -> Result<String, String> {
    match entry()?.get_password() {
        Ok(key) if !key.trim().is_empty() => Ok(key),
        Ok(_) => Err("The saved OpenRouter key is empty. Add it again.".into()),
        Err(keyring::Error::NoEntry) => Err("No OpenRouter key is saved. Add one first.".into()),
        Err(error) => Err(format!("Could not read the key from the Keychain: {error}")),
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Could not create the HTTP client: {error}"))
}

#[derive(Debug, Deserialize)]
struct KeyEnvelope {
    data: KeyData,
}

#[derive(Debug, Deserialize)]
struct KeyData {
    label: Option<String>,
    usage: Option<f64>,
    limit: Option<f64>,
    #[serde(default)]
    is_free_tier: bool,
}

pub async fn test_key() -> Result<OpenRouterStatus, String> {
    let key = load_key()?;
    let response = client()?
        .get(KEY_URL)
        .bearer_auth(&key)
        .header("HTTP-Referer", APP_URL)
        .header("X-Title", APP_TITLE)
        .send()
        .await
        .map_err(|error| format!("Could not reach OpenRouter: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Could not read the OpenRouter response: {error}"))?;

    if !status.is_success() {
        return Err(openrouter_error(status, &body));
    }

    let parsed: KeyEnvelope = serde_json::from_str(&body)
        .map_err(|error| format!("OpenRouter returned an unexpected response: {error}"))?;

    Ok(OpenRouterStatus {
        label: parsed.data.label,
        usage: parsed.data.usage,
        limit: parsed.data.limit,
        is_free_tier: parsed.data.is_free_tier,
    })
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    model: String,
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    cost: Option<f64>,
}

pub async fn rewrite(
    text: &str,
    model: Option<String>,
    voice_guidance: Option<String>,
) -> Result<RewriteResult, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter some text to rewrite first.".into());
    }

    let key = load_key()?;
    let model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let model = ModelRef::new(ProviderId::OpenRouter, model);

    let system = match voice_guidance {
        Some(guidance) => format!("{SYSTEM_PROMPT}\n\n{guidance}"),
        None => SYSTEM_PROMPT.to_string(),
    };

    let body = serde_json::json!({
        "model": model.model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": text }
        ],
        "temperature": 0.0,
        "max_tokens": 1024,
        "usage": { "include": true }
    });

    let response = client()?
        .post(CHAT_URL)
        .bearer_auth(&key)
        .header("HTTP-Referer", APP_URL)
        .header("X-Title", APP_TITLE)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("Could not reach OpenRouter: {error}"))?;

    let status = response.status();
    let raw = response
        .text()
        .await
        .map_err(|error| format!("Could not read the OpenRouter response: {error}"))?;

    if !status.is_success() {
        return Err(openrouter_error(status, &raw));
    }

    let parsed: ChatResponse = serde_json::from_str(&raw)
        .map_err(|error| format!("OpenRouter returned an unexpected response: {error}"))?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .ok_or_else(|| "OpenRouter returned no text.".to_string())?;

    Ok(RewriteResult {
        text: content.trim().to_string(),
        model: parsed.model,
        input_tokens: parsed
            .usage
            .as_ref()
            .and_then(|usage| usage.prompt_tokens)
            .unwrap_or(0),
        output_tokens: parsed
            .usage
            .as_ref()
            .and_then(|usage| usage.completion_tokens)
            .unwrap_or(0),
        cost_usd: parsed.usage.as_ref().and_then(|usage| usage.cost),
    })
}

fn openrouter_error(status: reqwest::StatusCode, body: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(|message| message.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(400).collect());

    match status.as_u16() {
        401 | 403 => format!(
            "OpenRouter rejected the API key (HTTP {status}). Check it in Settings → OpenRouter. ({message})"
        ),
        402 => format!(
            "OpenRouter says this account is out of credits (HTTP 402). Add credits or use another key. ({message})"
        ),
        404 => format!(
            "The selected model was not found (HTTP 404). Check the model id in Settings. ({message})"
        ),
        429 => format!(
            "OpenRouter rate limit reached (HTTP 429). Wait a moment and try again. ({message})"
        ),
        400 | 422 => format!("OpenRouter rejected the request: {message}"),
        status if (500..=599).contains(&status) => format!(
            "OpenRouter is having trouble right now (HTTP {status}). Try again shortly. ({message})"
        ),
        _ => format!("OpenRouter error (HTTP {status}): {message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_is_set() {
        assert!(!DEFAULT_MODEL.is_empty());
        assert!(DEFAULT_MODEL.contains('/'));
    }

    #[test]
    fn openrouter_error_extracts_message() {
        let message = openrouter_error(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"No auth credentials found","code":401}}"#,
        );
        assert!(message.contains("No auth credentials found"));
        assert!(message.contains("401"));
    }

    #[test]
    fn openrouter_error_explains_auth_and_quota() {
        let auth = openrouter_error(reqwest::StatusCode::UNAUTHORIZED, "denied");
        assert!(auth.contains("API key"));
        let quota = openrouter_error(reqwest::StatusCode::PAYMENT_REQUIRED, "no credits");
        assert!(quota.contains("out of credits"));
        let rate = openrouter_error(reqwest::StatusCode::TOO_MANY_REQUESTS, "slow down");
        assert!(rate.contains("rate limit"));
        let model = openrouter_error(reqwest::StatusCode::NOT_FOUND, "missing");
        assert!(model.contains("model was not found"));
    }

    #[test]
    fn openrouter_error_falls_back_to_body() {
        let message = openrouter_error(reqwest::StatusCode::BAD_GATEWAY, "upstream failed");
        assert!(message.contains("upstream failed"));
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalyzeCompletion {
    pub content: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
}

/// Ask the model for structured proofreading edits for one paragraph.
pub async fn analyze(
    paragraph_id: &str,
    paragraph: &str,
    model: Option<String>,
    dialect: &str,
) -> Result<AnalyzeCompletion, String> {
    let key = load_key()?;
    let model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let model = ModelRef::new(ProviderId::OpenRouter, model);

    let body = serde_json::json!({
        "model": model.model,
        "messages": [
            { "role": "system", "content": elocutio_analysis::system_prompt() },
            { "role": "user", "content": elocutio_analysis::build_user_prompt(paragraph_id, paragraph, dialect) }
        ],
        "temperature": 0.0,
        "max_tokens": 1500,
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "suggestions",
                "strict": true,
                "schema": elocutio_analysis::json_schema()
            }
        },
        "usage": { "include": true }
    });

    let response = client()?
        .post(CHAT_URL)
        .bearer_auth(&key)
        .header("HTTP-Referer", APP_URL)
        .header("X-Title", APP_TITLE)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("Could not reach OpenRouter: {error}"))?;

    let status = response.status();
    let raw = response
        .text()
        .await
        .map_err(|error| format!("Could not read the OpenRouter response: {error}"))?;
    if !status.is_success() {
        return Err(openrouter_error(status, &raw));
    }

    let parsed: ChatResponse = serde_json::from_str(&raw)
        .map_err(|error| format!("OpenRouter returned an unexpected response: {error}"))?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .ok_or_else(|| "OpenRouter returned no analysis text.".to_string())?;

    Ok(AnalyzeCompletion {
        content,
        model: parsed.model,
        input_tokens: parsed
            .usage
            .as_ref()
            .and_then(|usage| usage.prompt_tokens)
            .unwrap_or(0),
        output_tokens: parsed
            .usage
            .as_ref()
            .and_then(|usage| usage.completion_tokens)
            .unwrap_or(0),
        cost_usd: parsed.usage.as_ref().and_then(|usage| usage.cost),
    })
}
