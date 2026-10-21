//! The desktop app's provider layer.
//!
//! The pure, testable parts live in `elocutio-llm`: canonical request and
//! response types, the adapter contract, the model catalog, the error
//! taxonomy, and the SSE decoder. This module owns the parts that need a
//! socket: the async HTTP client, the retry loop, and the streaming reader.
//!
//! Nothing here knows about a provider's URL or JSON shape directly; every
//! provider-specific decision is delegated to an adapter from the crate.

pub mod http;
pub mod streaming;

use std::time::Duration;

use elocutio_llm::{ChatRequest, ModelRef, ProviderError, ProviderId, ResponseFormat, catalog};
use serde::{Deserialize, Serialize};

use super::ProviderKind;

/// How many times a retryable failure is retried before giving up.
pub const MAX_ATTEMPTS: u32 = 3;

/// The base delay for the exponential backoff between retries.
pub const RETRY_BASE_DELAY: Duration = Duration::from_millis(250);

/// The ceiling on a single provider request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A model the app can offer in a picker.
///
/// This is the app-facing shape of a catalog entry: everything is owned
/// (`String`) so it can be serialized straight to the frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelOption {
    /// The provider-native model id, e.g. `anthropic/claude-haiku-5.5`.
    pub id: String,
    /// The name to show, e.g. `Claude Haiku 5.5`.
    pub display_name: String,
    /// Which provider serves it.
    pub provider: ProviderId,
    /// The model id to send to that provider.
    ///
    /// OpenRouter takes the canonical id (`anthropic/claude-haiku-5.5`); an
    /// OpenAI-compatible endpoint takes the short id (`claude-haiku-5.5`),
    /// because those endpoints do not accept a vendor prefix.
    pub model_id: String,
    /// The context window in tokens.
    pub context_window: u32,
    /// Approximate USD per 1M input tokens.
    pub price_input_per_million_usd: f64,
    /// Approximate USD per 1M output tokens.
    pub price_output_per_million_usd: f64,
    /// One line on what this model is good for.
    pub best_for: String,
    /// True when the model accepts a `temperature` parameter.
    ///
    /// The Claude 5 family rejects any `temperature` other than `1`, so the
    /// parameter must be left out of the request for those models.
    pub supports_temperature: bool,
    /// True when this is the app's default model.
    pub is_default: bool,
}

impl ModelOption {
    /// Build the option for a specific provider.
    fn from_entry(entry: &catalog::CatalogEntry, provider: ProviderId) -> Self {
        let model_id = match provider {
            ProviderId::OpenRouter => entry.id.to_string(),
            _ => entry.short_id().to_string(),
        };
        Self {
            id: entry.id.to_string(),
            display_name: entry.display_name.to_string(),
            provider,
            model_id,
            context_window: entry.context_window,
            price_input_per_million_usd: entry.price_input_per_million_usd,
            price_output_per_million_usd: entry.price_output_per_million_usd,
            best_for: entry.best_for.to_string(),
            supports_temperature: entry.supports_temperature(),
            is_default: entry.id == catalog::DEFAULT_MODEL_ID,
        }
    }
}

/// The catalog for a provider, ready for the frontend.
pub fn list_models(provider: ProviderKind) -> Vec<ModelOption> {
    let provider_id: ProviderId = provider.into();
    catalog::for_provider(provider_id)
        .into_iter()
        .map(|entry| ModelOption::from_entry(entry, provider_id))
        .collect()
}

/// The model to use when the user has not chosen one.
#[allow(dead_code)]
pub fn default_model_id(provider: ProviderKind) -> String {
    let provider_id: ProviderId = provider.into();
    catalog::default_model_ref(provider_id).model
}

/// The catalog entry for a model id, if the catalog knows it.
#[allow(dead_code)]
pub fn describe_model(model_id: &str) -> Option<ModelOption> {
    catalog::find_loose(model_id).map(|entry| {
        ModelOption::from_entry(
            entry,
            entry
                .providers
                .first()
                .copied()
                .unwrap_or(ProviderId::OpenRouter),
        )
    })
}

/// Turn a normalized failure into the sentence the user sees.
///
/// The provider's own message is included as a short detail so the person can
/// tell a revoked key from a typo, but the headline is always the taxonomy's
/// sentence, never raw provider JSON.
pub fn user_message(error: &ProviderError) -> String {
    let detail = error.message.trim();
    let detail = if detail.is_empty() {
        None
    } else {
        Some(detail)
    };
    error.user_message(detail)
}

/// True when a failure is worth retrying.
pub fn is_retryable(error: &ProviderError) -> bool {
    error.is_retryable()
}

/// How long to wait before the next attempt.
///
/// A `Retry-After` hint from the provider wins; otherwise the delay doubles
/// with each attempt.
pub fn backoff_delay(error: &ProviderError, attempt: u32) -> Duration {
    if let Some(seconds) = error.retry_after_seconds {
        return Duration::from_secs(seconds.max(1));
    }
    RETRY_BASE_DELAY * (1u32 << attempt.min(4))
}

/// The adapter to use for a provider kind.
pub fn adapter_for(kind: ProviderKind) -> Result<Box<dyn elocutio_llm::Provider>, ProviderError> {
    let provider_id: ProviderId = kind.into();
    elocutio_llm::require_adapter(provider_id)
}

/// Build the canonical request for a chat call.
///
/// `temperature` is left unset unless the caller asks for one *and* the model
/// accepts it, so the Claude 5 family never sees a `temperature` key.
pub fn build_chat_request(
    provider: ProviderKind,
    model: &str,
    system: Option<String>,
    user: String,
    max_output_tokens: Option<u32>,
    response_format: ResponseFormat,
) -> ChatRequest {
    let provider_id: ProviderId = provider.into();
    let mut request = ChatRequest::new(
        ModelRef::new(provider_id, model),
        vec![elocutio_llm::Message::user(user)],
    )
    .with_response_format(response_format);
    if let Some(system) = system {
        request = request.with_system(system);
    }
    if let Some(max_output_tokens) = max_output_tokens {
        request = request.with_max_output_tokens(max_output_tokens);
    }
    request
}

#[cfg(test)]
mod tests {
    use super::*;
    use elocutio_llm::ErrorKind;

    #[test]
    fn the_openrouter_catalog_is_listed() {
        let models = list_models(ProviderKind::OpenRouter);
        assert!(!models.is_empty());
        let default = models
            .iter()
            .find(|model| model.is_default)
            .expect("the default model is listed");
        assert_eq!(default.id, "anthropic/claude-haiku-5.5");
        assert_eq!(default.display_name, "Claude Haiku 5.5");
        assert!(!default.supports_temperature);
    }

    #[test]
    fn the_compatible_provider_lists_the_same_models() {
        let models = list_models(ProviderKind::OpenAiCompatible);
        assert!(!models.is_empty());
        assert!(
            models
                .iter()
                .all(|model| model.provider == ProviderId::OpenAiCompatible)
        );
    }

    #[test]
    fn a_model_option_serializes_for_the_frontend() {
        let models = list_models(ProviderKind::OpenRouter);
        let json = serde_json::to_string(&models[0]).expect("the option serializes");
        let decoded: serde_json::Value = serde_json::from_str(&json).expect("the json parses");
        assert_eq!(decoded["id"], "anthropic/claude-haiku-5.5");
        assert_eq!(decoded["model_id"], "anthropic/claude-haiku-5.5");
        assert_eq!(decoded["provider"], "open_router");
        assert_eq!(decoded["is_default"], true);
        assert_eq!(decoded["supports_temperature"], false);
        assert!(
            decoded["best_for"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
    }

    #[test]
    fn a_compatible_provider_gets_the_short_model_id() {
        let models = list_models(ProviderKind::OpenAiCompatible);
        let default = models
            .iter()
            .find(|model| model.is_default)
            .expect("the default model is listed");
        assert_eq!(default.model_id, "claude-haiku-5.5");
        assert_eq!(default.id, "anthropic/claude-haiku-5.5");
    }

    #[test]
    fn the_default_model_id_depends_on_the_provider() {
        assert_eq!(
            default_model_id(ProviderKind::OpenRouter),
            "anthropic/claude-haiku-5.5"
        );
        assert_eq!(
            default_model_id(ProviderKind::OpenAiCompatible),
            "claude-haiku-5.5"
        );
    }

    #[test]
    fn a_model_can_be_described_by_its_short_id() {
        let described = describe_model("claude-haiku-5.5").expect("the default is described");
        assert_eq!(described.id, "anthropic/claude-haiku-5.5");
        assert!(describe_model("nope/nope").is_none());
    }

    #[test]
    fn the_user_message_leads_with_the_taxonomy_sentence() {
        let error = ProviderError::new(ErrorKind::Auth, "No auth credentials found");
        let message = user_message(&error);
        assert!(message.contains("API key"));
        assert!(message.contains("No auth credentials found"));
    }

    #[test]
    fn the_user_message_handles_a_missing_detail() {
        let error = ProviderError::new(ErrorKind::Network, "");
        let message = user_message(&error);
        assert!(message.contains("Could not reach the provider"));
        assert!(!message.contains('('));
    }

    #[test]
    fn the_backoff_grows_with_the_attempt() {
        let error = ProviderError::new(ErrorKind::Server, "boom");
        let first = backoff_delay(&error, 0);
        let second = backoff_delay(&error, 1);
        assert!(second > first);
    }

    #[test]
    fn a_retry_after_hint_wins_over_the_backoff() {
        let error = ProviderError::new(ErrorKind::RateLimit, "slow down").with_retry_after(Some(3));
        assert_eq!(backoff_delay(&error, 0), Duration::from_secs(3));
    }

    #[test]
    fn a_chat_request_has_no_temperature_by_default() {
        let request = build_chat_request(
            ProviderKind::OpenRouter,
            "anthropic/claude-haiku-5.5",
            Some("system".to_string()),
            "user".to_string(),
            Some(1024),
            ResponseFormat::Text,
        );
        assert_eq!(request.temperature, None);
        assert_eq!(request.max_output_tokens, Some(1024));
        assert_eq!(request.system.as_deref(), Some("system"));
    }

    #[test]
    fn an_adapter_is_available_for_both_supported_kinds() {
        assert_eq!(
            adapter_for(ProviderKind::OpenRouter)
                .expect("openrouter has an adapter")
                .id(),
            ProviderId::OpenRouter
        );
        assert_eq!(
            adapter_for(ProviderKind::OpenAiCompatible)
                .expect("openai-compatible has an adapter")
                .id(),
            ProviderId::OpenAiCompatible
        );
    }
}
