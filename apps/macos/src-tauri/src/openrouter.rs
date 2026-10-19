//! Minimal OpenRouter client for M0.
//!
//! The key is stored in the macOS Keychain and never logged. This module is
//! deliberately small: it proves key validation and one rewrite before the
//! full provider router lands in M1.

use std::time::Duration;

use elocutio_llm::{ModelRef, ProviderId};
use serde::{Deserialize, Serialize};

const KEYCHAIN_ACCOUNT: &str = "api_key";
const KEY_URL: &str = "https://openrouter.ai/api/v1/key";
const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const APP_URL: &str = "https://github.com/pri-te-sh/elocutio";
const APP_TITLE: &str = "Elocutio";

pub const DEFAULT_MODEL: &str = "anthropic/claude-haiku-5.5";
pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteTone {
    #[default]
    Neutral,
    Confident,
    Friendly,
    Concise,
    Formal,
}

impl RewriteTone {
    pub fn guidance(self) -> Option<&'static str> {
        match self {
            RewriteTone::Neutral => None,
            RewriteTone::Confident => Some("Use a confident, direct tone without hedging."),
            RewriteTone::Friendly => Some("Use a warm, friendly tone while staying professional."),
            RewriteTone::Concise => Some("Be concise; cut filler and unnecessary qualifiers."),
            RewriteTone::Formal => Some("Use a formal, precise register."),
        }
    }
}

/// Provider wire dialect. OpenRouter adds its own headers and key validation;
/// OpenAI-compatible endpoints use the same Chat Completions shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    OpenRouter,
    OpenAiCompatible,
}

impl From<ProviderKind> for ProviderId {
    fn from(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::OpenRouter => ProviderId::OpenRouter,
            ProviderKind::OpenAiCompatible => ProviderId::OpenAiCompatible,
        }
    }
}

impl ProviderKind {
    pub fn keychain_service(self) -> &'static str {
        match self {
            ProviderKind::OpenRouter => "app.elocutio.provider.openrouter",
            ProviderKind::OpenAiCompatible => "app.elocutio.provider.openai_compatible",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            ProviderKind::OpenRouter => "OpenRouter",
            ProviderKind::OpenAiCompatible => "OpenAI-compatible",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: String,
}

impl ProviderConfig {
    pub fn chat_endpoint(&self) -> String {
        match self.kind {
            ProviderKind::OpenRouter => CHAT_URL.to_string(),
            ProviderKind::OpenAiCompatible => {
                format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
            }
        }
    }

    pub fn models_endpoint(&self) -> String {
        match self.kind {
            ProviderKind::OpenRouter => KEY_URL.to_string(),
            ProviderKind::OpenAiCompatible => {
                format!("{}/models", self.base_url.trim_end_matches('/'))
            }
        }
    }
}

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

fn entry_for(kind: ProviderKind) -> Result<keyring::Entry, String> {
    keyring::Entry::new(kind.keychain_service(), KEYCHAIN_ACCOUNT)
        .map_err(|error| error.to_string())
}

pub fn key_status_for(kind: ProviderKind) -> Result<KeyStatus, String> {
    match entry_for(kind)?.get_password() {
        Ok(_) => Ok(KeyStatus { present: true }),
        Err(keyring::Error::NoEntry) => Ok(KeyStatus { present: false }),
        Err(error) => Err(error.to_string()),
    }
}

pub fn save_key_for(kind: ProviderKind, key: &str) -> Result<KeyStatus, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Enter an API key first.".into());
    }
    entry_for(kind)?
        .set_password(key)
        .map_err(|error| format!("Could not save the key in the Keychain: {error}"))?;
    Ok(KeyStatus { present: true })
}

pub fn clear_key_for(kind: ProviderKind) -> Result<KeyStatus, String> {
    match entry_for(kind)?.delete_credential() {
        Ok(()) => Ok(KeyStatus { present: false }),
        Err(keyring::Error::NoEntry) => Ok(KeyStatus { present: false }),
        Err(error) => Err(format!(
            "Could not remove the key from the Keychain: {error}"
        )),
    }
}

pub fn load_key_optional(kind: ProviderKind) -> Result<Option<String>, String> {
    match entry_for(kind)?.get_password() {
        Ok(key) if !key.trim().is_empty() => Ok(Some(key)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("Could not read the key from the Keychain: {error}")),
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Could not create the HTTP client: {error}"))
}

async fn send_chat(
    provider: &ProviderConfig,
    body: serde_json::Value,
) -> Result<ChatResponse, String> {
    let key = load_key_optional(provider.kind)?;
    if provider.kind == ProviderKind::OpenRouter && key.is_none() {
        return Err("No OpenRouter key is saved. Add one first.".into());
    }

    let mut request = client()?.post(provider.chat_endpoint()).json(&body);
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    if provider.kind == ProviderKind::OpenRouter {
        request = request
            .header("HTTP-Referer", APP_URL)
            .header("X-Title", APP_TITLE);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("Could not reach the provider: {error}"))?;
    let status = response.status();
    let raw = response
        .text()
        .await
        .map_err(|error| format!("Could not read the provider response: {error}"))?;
    if !status.is_success() {
        return Err(openrouter_error(status, &raw));
    }

    serde_json::from_str(&raw)
        .map_err(|error| format!("The provider returned an unexpected response: {error}"))
}

pub async fn test_key_for(provider: &ProviderConfig) -> Result<OpenRouterStatus, String> {
    let key = load_key_optional(provider.kind)?;
    if provider.kind == ProviderKind::OpenRouter && key.is_none() {
        return Err("No OpenRouter key is saved. Add one first.".into());
    }

    let mut request = client()?.get(provider.models_endpoint());
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    if provider.kind == ProviderKind::OpenRouter {
        request = request
            .header("HTTP-Referer", APP_URL)
            .header("X-Title", APP_TITLE);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("Could not reach the provider: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Could not read the provider response: {error}"))?;
    if !status.is_success() {
        return Err(openrouter_error(status, &body));
    }

    if provider.kind == ProviderKind::OpenRouter {
        let parsed: KeyEnvelope = serde_json::from_str(&body)
            .map_err(|error| format!("OpenRouter returned an unexpected response: {error}"))?;
        return Ok(OpenRouterStatus {
            label: parsed.data.label,
            usage: parsed.data.usage,
            limit: parsed.data.limit,
            is_free_tier: parsed.data.is_free_tier,
        });
    }

    Ok(OpenRouterStatus {
        label: Some(provider.base_url.clone()),
        usage: None,
        limit: None,
        is_free_tier: false,
    })
}

/// Rewrite through an explicit provider configuration.
pub async fn rewrite_with(
    provider: &ProviderConfig,
    text: &str,
    model: Option<String>,
    voice_guidance: Option<String>,
    tone: RewriteTone,
) -> Result<RewriteResult, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter some text to rewrite first.".into());
    }

    let model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let model_ref = ModelRef::new(provider.kind.into(), model);
    let mut system = SYSTEM_PROMPT.to_string();
    if let Some(guidance) = voice_guidance {
        system.push_str("\n\n");
        system.push_str(&guidance);
    }
    if let Some(guidance) = tone.guidance() {
        system.push_str("\n\n");
        system.push_str(guidance);
    }

    let body = serde_json::json!({
        "model": model_ref.model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": text }
        ],
        "temperature": 0.0,
        "max_tokens": 1024,
        "usage": { "include": true }
    });

    let parsed = send_chat(provider, body).await?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .ok_or_else(|| "The provider returned no text.".to_string())?;

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

/// Structured analysis through an explicit provider configuration.
pub async fn analyze_with(
    provider: &ProviderConfig,
    paragraph_id: &str,
    paragraph: &str,
    model: Option<String>,
    dialect: &str,
) -> Result<AnalyzeCompletion, String> {
    let model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let model_ref = ModelRef::new(provider.kind.into(), model);

    let body = serde_json::json!({
        "model": model_ref.model,
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

    let parsed = send_chat(provider, body).await?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .ok_or_else(|| "The provider returned no analysis text.".to_string())?;

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

#[derive(Debug, Clone, Serialize)]
pub struct AnalyzeCompletion {
    pub content: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
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
            "The provider rejected the API key (HTTP {status}). Check it in Settings. ({message})"
        ),
        402 => format!(
            "The provider says this account is out of credits (HTTP 402). Add credits or use another key. ({message})"
        ),
        404 => format!(
            "The selected model was not found (HTTP 404). Check the model id in Settings. ({message})"
        ),
        429 => format!(
            "The provider rate limit was reached (HTTP 429). Wait a moment and try again. ({message})"
        ),
        400 | 422 => format!("The provider rejected the request: {message}"),
        status if (500..=599).contains(&status) => format!(
            "The provider is having trouble right now (HTTP {status}). Try again shortly. ({message})"
        ),
        _ => format!("Provider error (HTTP {status}): {message}"),
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
    fn tone_guidance_is_optional_and_specific() {
        assert!(RewriteTone::Neutral.guidance().is_none());
        assert!(
            RewriteTone::Confident
                .guidance()
                .unwrap()
                .contains("confident")
        );
        assert!(RewriteTone::Concise.guidance().unwrap().contains("concise"));
    }

    #[test]
    fn provider_endpoints_are_built_from_the_base_url() {
        let openrouter = ProviderConfig {
            kind: ProviderKind::OpenRouter,
            base_url: "https://api.openai.com/v1".into(),
        };
        assert_eq!(openrouter.chat_endpoint(), CHAT_URL);
        assert_eq!(openrouter.models_endpoint(), KEY_URL);

        let compatible = ProviderConfig {
            kind: ProviderKind::OpenAiCompatible,
            base_url: "https://api.openai.com/v1/".into(),
        };
        assert_eq!(
            compatible.chat_endpoint(),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            compatible.models_endpoint(),
            "https://api.openai.com/v1/models"
        );
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
