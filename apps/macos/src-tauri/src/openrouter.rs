//! The provider client for M0/M1.
//!
//! The key is stored in the macOS Keychain and never logged. This module
//! keeps the public surface the rest of the app already calls —
//! `rewrite_with`, `analyze_with`, `test_key_for` — and routes the actual
//! request building and error handling through the adapter layer in
//! `elocutio-llm`.
//!
//! Two rules matter here:
//!
//! * **`temperature` is never sent.** The Claude 5 family rejects any
//!   `temperature` other than `1` with an HTTP 400, and the default model is
//!   `anthropic/claude-haiku-5.5`. Omitting the key is valid everywhere.
//! * **Every failure is normalized** to the `elocutio-llm` error taxonomy
//!   before it becomes a user-facing sentence.

use elocutio_llm::{ChatRequest, ErrorKind, Provider, ProviderError, ProviderId, ResponseFormat};
use serde::{Deserialize, Serialize};

#[path = "llm/mod.rs"]
pub mod llm;

use llm::http::Endpoint;

const KEYCHAIN_ACCOUNT: &str = "api_key";
const KEY_URL: &str = "https://openrouter.ai/api/v1/key";
const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

pub const DEFAULT_MODEL: &str = "anthropic/claude-haiku-5.5";
pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// The output budget for a rewrite.
///
/// The default model has adaptive thinking on, and thinking tokens count
/// against `max_tokens`, so a tight cap can cut the response off before any
/// text appears. 2048 leaves headroom for a thinking pass plus a rewritten
/// passage while staying far below the model's output limit. The tradeoff is
/// that a runaway generation can burn up to 2048 tokens before the cap bites
/// instead of 1024; the cost difference is a fraction of a cent.
pub const REWRITE_MAX_TOKENS: u32 = 2048;

/// The output budget for an analysis.
///
/// An analysis returns a JSON array of suggestions, which is larger than a
/// rewrite. 4096 leaves room for the thinking pass, the JSON envelope, and a
/// long list of suggestions. The tradeoff is the same as above: the worst
/// case costs more before the cap stops it.
pub const ANALYSIS_MAX_TOKENS: u32 = 4096;

/// The estimated cost of a request, in USD.
///
/// The estimate uses the catalog price for the model when it is known, and a
/// conservative default when it is not. It is deliberately an over-estimate:
/// a request that is refused because of the cap should never turn out to
/// have been cheap.
pub fn estimate_cost_usd(model: &str, input_chars: usize, output_tokens: u32) -> f64 {
    // A rough 4 characters per token is close enough for a pre-flight check
    // and errs on the high side for English prose.
    let input_tokens = (input_chars as f64 / 4.0).ceil();
    let output_tokens = output_tokens as f64;

    let (input_per_million, output_per_million) = match elocutio_llm::catalog::find(model) {
        Some(entry) => (
            entry.price_input_per_million_usd,
            entry.price_output_per_million_usd,
        ),
        None => (3.0, 15.0),
    };

    (input_tokens * input_per_million + output_tokens * output_per_million) / 1_000_000.0
}

/// Whether a request fits under the user's cost cap.
///
/// A cap of `None` means no cap. The estimate is checked before the request
/// goes out, so a runaway live session cannot spend past the ceiling.
pub fn check_cost_cap(
    model: &str,
    input_chars: usize,
    output_tokens: u32,
    cap: Option<f64>,
) -> Result<(), String> {
    let Some(cap) = cap else {
        return Ok(());
    };
    if !cap.is_finite() || cap < 0.0 {
        return Ok(());
    }

    let estimate = estimate_cost_usd(model, input_chars, output_tokens);
    if estimate > cap {
        return Err(format!(
            "This request would cost about ${estimate:.4}, which is over your ${cap:.2} cost cap. Raise the cap in Settings or analyze less text."
        ));
    }
    Ok(())
}

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

/// The endpoint and key for a provider configuration.
async fn endpoint_for(provider: &ProviderConfig) -> Result<Endpoint, String> {
    let key = load_key_optional(provider.kind)?;
    if provider.kind == ProviderKind::OpenRouter && key.is_none() {
        return Err("No OpenRouter key is saved. Add one first.".into());
    }
    Ok(Endpoint {
        url: provider.chat_endpoint(),
        key,
        send_attribution_headers: provider.kind == ProviderKind::OpenRouter,
    })
}

pub async fn test_key_for(provider: &ProviderConfig) -> Result<OpenRouterStatus, String> {
    let key = load_key_optional(provider.kind)?;
    if provider.kind == ProviderKind::OpenRouter && key.is_none() {
        return Err("No OpenRouter key is saved. Add one first.".into());
    }

    let mut request = llm::http::client()?.get(provider.models_endpoint());
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    if provider.kind == ProviderKind::OpenRouter {
        request = request
            .header("HTTP-Referer", llm::http::APP_URL)
            .header("X-Title", llm::http::APP_TITLE);
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
    let mut system = SYSTEM_PROMPT.to_string();
    if let Some(guidance) = voice_guidance {
        system.push_str("\n\n");
        system.push_str(&guidance);
    }
    if let Some(guidance) = tone.guidance() {
        system.push_str("\n\n");
        system.push_str(guidance);
    }

    // `temperature` is deliberately left unset: the Claude 5 family rejects
    // any value other than 1, and omitting the key is valid for every model.
    let request = llm::build_chat_request(
        provider.kind,
        &model,
        Some(system),
        text.to_string(),
        Some(REWRITE_MAX_TOKENS),
        ResponseFormat::Text,
    );

    let response = send_request(provider, &request).await?;
    let content = response.text.trim().to_string();
    if content.is_empty() {
        return Err("The provider returned no text.".into());
    }

    // Voice copy guard: refuse output that repeats the writer's own sample
    // content. The guard lives here rather than at each call site so every
    // provider path fails closed, not just the system-wide rewrite action.
    // See docs/VOICE-LEARNING.md §11.
    crate::voice::check_for_copy(&content)?;

    Ok(RewriteResult {
        text: content,
        model: model_name(&response, &request),
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        cost_usd: response.usage.cost_usd(),
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

/// The model to report for a completed request.
///
/// Providers normally echo the model they used. A few OpenAI-compatible
/// endpoints omit it, so the requested id is used as the fallback and the
/// usage log never records a blank model.
fn model_name(response: &elocutio_llm::ChatResponse, request: &ChatRequest) -> String {
    let reported = response.model.trim();
    if reported.is_empty() {
        request.model.model.clone()
    } else {
        reported.to_string()
    }
}

/// Send a canonical request through the provider's adapter.
async fn send_request(
    provider: &ProviderConfig,
    request: &ChatRequest,
) -> Result<elocutio_llm::ChatResponse, String> {
    let adapter = llm::adapter_for(provider.kind).map_err(|error| llm::user_message(&error))?;
    let endpoint = endpoint_for(provider).await?;
    llm::http::send_chat(adapter.as_ref(), &endpoint, request)
        .await
        .map_err(|error| llm::user_message(&error))
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

    // The strict JSON-schema response format is what OpenRouter expects for
    // structured output, and it is what makes the suggestion list parseable.
    let request = llm::build_chat_request(
        provider.kind,
        &model,
        Some(elocutio_analysis::system_prompt().to_string()),
        elocutio_analysis::build_user_prompt(paragraph_id, paragraph, dialect),
        Some(ANALYSIS_MAX_TOKENS),
        ResponseFormat::JsonSchema {
            name: "suggestions".to_string(),
            schema: elocutio_analysis::json_schema(),
            strict: true,
        },
    );

    let response = send_request(provider, &request).await?;
    let content = response.text.clone();
    if content.trim().is_empty() {
        return Err("The provider returned no analysis text.".into());
    }

    Ok(AnalyzeCompletion {
        content,
        model: model_name(&response, &request),
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        cost_usd: response.usage.cost_usd(),
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

// The functions below are the provider layer's public surface for the rest of
// the app. They have no caller yet — the model picker and the error surface
// are wired up in a later milestone — so they are allowed to sit unused.

/// The models Elocutio offers for a provider.
///
/// The list is the curated catalog, not a live query: it is the set of models
/// the app is willing to recommend, with the context window and price needed
/// to show a cost estimate.
#[allow(dead_code)]
pub fn list_models(provider: &ProviderConfig) -> Vec<llm::ModelOption> {
    llm::list_models(provider.kind)
}

/// The models Elocutio offers for a provider kind.
#[allow(dead_code)]
pub fn list_models_for(kind: ProviderKind) -> Vec<llm::ModelOption> {
    llm::list_models(kind)
}

/// Turn a provider failure into the sentence the user sees.
///
/// The taxonomy decides the headline; the provider's own message is appended
/// as a short detail.
#[allow(dead_code)]
pub fn provider_error_message(error: &ProviderError) -> String {
    llm::user_message(error)
}

/// True when a failure is worth retrying.
#[allow(dead_code)]
pub fn is_retryable(error: &ProviderError) -> bool {
    llm::is_retryable(error)
}

/// The user-facing message for an HTTP failure.
///
/// The wording per status is preserved from the original client, and the
/// status is mapped through the shared taxonomy so the app has one place that
/// knows what a 402 means.
fn openrouter_error(status: reqwest::StatusCode, body: &str) -> String {
    let adapter = elocutio_llm::OpenRouterAdapter;
    let error = adapter.map_http_error(status.as_u16(), body);
    let kind = error.kind;
    let detail = error.message.trim();

    match kind {
        ErrorKind::Auth => format!(
            "The provider rejected the API key (HTTP {status}). Check it in Settings. ({detail})"
        ),
        ErrorKind::Quota => format!(
            "The provider says this account is out of credits (HTTP 402). Add credits or use another key. ({detail})"
        ),
        ErrorKind::RateLimit => format!(
            "The provider rate limit was reached (HTTP 429). Wait a moment and try again. ({detail})"
        ),
        ErrorKind::InvalidRequest if status == reqwest::StatusCode::NOT_FOUND => format!(
            "The selected model was not found (HTTP 404). Check the model id in Settings. ({detail})"
        ),
        ErrorKind::InvalidRequest => format!("The provider rejected the request: {detail}"),
        ErrorKind::Overloaded => format!(
            "The provider is having trouble right now (HTTP {status}). Try again shortly. ({detail})"
        ),
        ErrorKind::Server => format!(
            "The provider is having trouble right now (HTTP {status}). Try again shortly. ({detail})"
        ),
        _ => format!("Provider error (HTTP {status}): {detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elocutio_llm::{Message, ModelRef};

    #[test]
    fn default_model_is_set() {
        assert!(!DEFAULT_MODEL.is_empty());
        assert!(DEFAULT_MODEL.contains('/'));
    }

    #[test]
    fn only_transient_statuses_are_retried() {
        // The taxonomy is the single source of truth for the retry policy.
        let retryable = |status: reqwest::StatusCode| {
            elocutio_llm::ErrorKind::from_status(status.as_u16()).is_retryable()
        };
        assert!(retryable(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(retryable(reqwest::StatusCode::BAD_GATEWAY));
        assert!(!retryable(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!retryable(reqwest::StatusCode::BAD_REQUEST));
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

    #[test]
    fn the_rewrite_body_has_no_temperature_key() {
        // The Claude 5 family rejects any `temperature` other than 1, so the
        // key must be absent from the wire format for the default model.
        let request = ChatRequest::new(
            ModelRef::new(ProviderId::OpenRouter, DEFAULT_MODEL),
            vec![Message::user("Proofread this.")],
        )
        .with_system("You are a precise copy editor.")
        .with_max_output_tokens(REWRITE_MAX_TOKENS);
        assert_eq!(request.temperature, None);

        let adapter = elocutio_llm::OpenRouterAdapter;
        let body = adapter.build_request(&request);
        let object = body.as_object().expect("the body is an object");
        assert!(
            !object.contains_key("temperature"),
            "the default model rejects a temperature key"
        );
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["max_tokens"], REWRITE_MAX_TOKENS);
    }

    #[test]
    fn the_wire_format_is_unchanged_except_for_the_missing_temperature() {
        // The pre-change client sent exactly these keys. The only difference
        // is that `temperature` is gone, which is the fix.
        let request = ChatRequest::new(
            ModelRef::new(ProviderId::OpenRouter, DEFAULT_MODEL),
            vec![Message::user("Proofread this.")],
        )
        .with_system("You are a precise copy editor.")
        .with_max_output_tokens(REWRITE_MAX_TOKENS);

        let body = elocutio_llm::OpenRouterAdapter.build_request(&request);
        let keys: Vec<&str> = body
            .as_object()
            .expect("the body is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert!(keys.contains(&"model"));
        assert!(keys.contains(&"messages"));
        assert!(keys.contains(&"max_tokens"));
        assert!(keys.contains(&"usage"));
        assert!(
            !keys.contains(&"temperature"),
            "the key must be absent, not zero"
        );
    }

    #[test]
    fn the_analysis_body_keeps_the_strict_json_schema() {
        let request = ChatRequest::new(
            ModelRef::new(ProviderId::OpenRouter, DEFAULT_MODEL),
            vec![Message::user("Analyze this.")],
        )
        .with_response_format(ResponseFormat::JsonSchema {
            name: "suggestions".to_string(),
            schema: elocutio_analysis::json_schema(),
            strict: true,
        });

        let adapter = elocutio_llm::OpenRouterAdapter;
        let body = adapter.build_request(&request);
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(
            body["response_format"]["json_schema"]["name"],
            "suggestions"
        );
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert!(
            !body
                .as_object()
                .expect("the body is an object")
                .contains_key("temperature")
        );
    }

    #[test]
    fn a_missing_model_name_falls_back_to_the_requested_model() {
        let request = ChatRequest::new(
            ModelRef::new(ProviderId::OpenRouter, DEFAULT_MODEL),
            vec![Message::user("Hi")],
        );
        let response = elocutio_llm::ChatResponse {
            id: "gen-1".to_string(),
            model: String::new(),
            text: "Hello".to_string(),
            finish_reason: elocutio_llm::FinishReason::Stop,
            usage: Default::default(),
        };
        assert_eq!(model_name(&response, &request), DEFAULT_MODEL);

        let response = elocutio_llm::ChatResponse {
            model: "anthropic/claude-haiku-5.5:free".to_string(),
            ..response
        };
        assert_eq!(
            model_name(&response, &request),
            "anthropic/claude-haiku-5.5:free"
        );
    }

    #[test]
    fn the_model_list_comes_from_the_catalog() {
        let provider = ProviderConfig {
            kind: ProviderKind::OpenRouter,
            base_url: DEFAULT_OPENAI_BASE_URL.to_string(),
        };
        let models = list_models(&provider);
        assert!(!models.is_empty());
        assert!(models.iter().any(|model| model.id == DEFAULT_MODEL));
    }
}
