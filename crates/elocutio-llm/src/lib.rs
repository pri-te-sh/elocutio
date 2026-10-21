#![forbid(unsafe_code)]

//! Provider-agnostic request and response types.
//!
//! OpenAI Chat Completions is the base dialect, not the internal model. Every
//! adapter compiles these canonical types to its wire format and normalizes
//! errors and streaming events back. Network clients live in the desktop app,
//! not here, so this crate stays pure and testable.
//!
//! The crate is layered:
//!
//! * [`ChatRequest`] / [`ChatResponse`] — the canonical shapes.
//! * [`Provider`] — the adapter contract, with [`OpenRouterAdapter`] and
//!   [`OpenAiCompatibleAdapter`] as the shipped implementations.
//! * [`ProviderError`] / [`ErrorKind`] — the normalized failure taxonomy and
//!   the sentences the UI shows for each class.
//! * [`catalog`] — the curated model list a picker offers.
//! * [`stream`] — canonical streaming events plus a dependency-free SSE
//!   decoder.

pub mod catalog;
pub mod error;
pub mod provider;
pub mod stream;

use serde::{Deserialize, Serialize};

pub use catalog::{CatalogEntry, DEFAULT_MODEL_ID};
pub use error::{ErrorKind, ProviderError};
pub use provider::{
    OpenAiCompatibleAdapter, OpenRouterAdapter, Provider, adapter_for, require_adapter,
};
pub use stream::{SseDecoder, SseFrame, StreamEvent};

/// A provider Elocutio can talk to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    OpenRouter,
    OpenAi,
    Anthropic,
    Google,
    Ollama,
    OpenAiCompatible,
}

impl ProviderId {
    /// The name to show in the UI.
    pub fn display_name(self) -> &'static str {
        match self {
            ProviderId::OpenRouter => "OpenRouter",
            ProviderId::OpenAi => "OpenAI",
            ProviderId::Anthropic => "Anthropic",
            ProviderId::Google => "Google",
            ProviderId::Ollama => "Ollama",
            ProviderId::OpenAiCompatible => "OpenAI-compatible",
        }
    }

    /// True when Elocutio has an adapter for this provider today.
    pub fn is_supported(self) -> bool {
        adapter_for(self).is_some()
    }
}

/// A model on a specific provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: ProviderId,
    pub model: String,
}

impl ModelRef {
    pub fn new(provider: ProviderId, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }

    /// Stable key used in logs, cache keys, and settings.
    pub fn key(&self) -> String {
        format!("{:?}/{}", self.provider, self.model).to_lowercase()
    }

    /// Resolve this model against the curated catalog.
    ///
    /// Returns `None` when the catalog does not know the id, which is normal:
    /// users may type any model id their provider offers.
    pub fn resolve(&self) -> Option<&'static CatalogEntry> {
        catalog::find_loose(&self.model)
    }

    /// True when the model accepts a `temperature` parameter.
    ///
    /// The Claude 5 family rejects any `temperature` other than `1`, so the
    /// parameter must be omitted from the request for those models.
    pub fn supports_temperature(&self) -> bool {
        catalog::supports_temperature(&self.model)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }
}

/// How the model should format its response.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseFormat {
    #[default]
    Text,
    JsonObject,
    JsonSchema {
        name: String,
        schema: serde_json::Value,
        strict: bool,
    },
}

impl ResponseFormat {
    /// True when this format needs the provider to support structured output.
    pub fn is_structured(&self) -> bool {
        !matches!(self, ResponseFormat::Text)
    }
}

/// A canonical completion request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: ModelRef,
    pub messages: Vec<Message>,
    pub system: Option<String>,
    pub max_output_tokens: Option<u32>,
    /// The requested temperature. `None` means "do not send it".
    ///
    /// Leaving it unset is the safe default: the Claude 5 family rejects any
    /// `temperature` other than `1`, and the provider's own default is fine
    /// everywhere else.
    pub temperature: Option<f32>,
    pub stream: bool,
    pub response_format: ResponseFormat,
}

impl ChatRequest {
    pub fn new(model: ModelRef, messages: Vec<Message>) -> Self {
        Self {
            model,
            messages,
            system: None,
            max_output_tokens: None,
            temperature: None,
            stream: false,
            response_format: ResponseFormat::Text,
        }
    }

    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    pub fn with_response_format(mut self, format: ResponseFormat) -> Self {
        self.response_format = format;
        self
    }

    pub fn with_max_output_tokens(mut self, max_tokens: u32) -> Self {
        self.max_output_tokens = Some(max_tokens);
        self
    }

    /// Ask for a specific temperature.
    ///
    /// The adapter still drops the parameter for models that reject it, so
    /// this is safe to call unconditionally.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_streaming(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cost_usd_micros: Option<u64>,
}

impl Usage {
    /// The recorded cost in USD, when the provider reported one.
    pub fn cost_usd(&self) -> Option<f64> {
        self.cost_usd_micros
            .map(|micros| micros as f64 / 1_000_000.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub text: String,
    pub finish_reason: FinishReason,
    pub usage: Usage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Error,
    Unknown,
}

impl FinishReason {
    /// True when the provider stopped because it hit the output cap.
    pub fn is_truncated(&self) -> bool {
        matches!(self, FinishReason::Length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_key_is_stable_and_lowercase() {
        let model = ModelRef::new(ProviderId::OpenRouter, "anthropic/claude-haiku-5.5");
        assert_eq!(model.key(), "openrouter/anthropic/claude-haiku-5.5");
    }

    #[test]
    fn request_round_trips_through_json() {
        let request = ChatRequest::new(
            ModelRef::new(ProviderId::OpenRouter, "anthropic/claude-haiku-5.5"),
            vec![Message::user("Proofread this.")],
        )
        .with_system("You are a precise copy editor.")
        .with_response_format(ResponseFormat::JsonObject);

        let json = serde_json::to_string(&request).unwrap();
        let decoded: ChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, decoded);
    }

    #[test]
    fn default_response_format_is_text() {
        assert_eq!(ResponseFormat::default(), ResponseFormat::Text);
        assert!(!ResponseFormat::default().is_structured());
        assert!(ResponseFormat::JsonObject.is_structured());
    }

    #[test]
    fn retry_classification_matches_the_policy() {
        assert!(ProviderError::new(ErrorKind::RateLimit, "slow down").is_retryable());
        assert!(ProviderError::new(ErrorKind::Network, "offline").is_retryable());
        assert!(!ProviderError::new(ErrorKind::Auth, "bad key").is_retryable());
        assert!(!ProviderError::new(ErrorKind::InvalidRequest, "bad params").is_retryable());
    }

    #[test]
    fn usage_defaults_to_zero_without_optional_counts() {
        let usage = Usage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.cached_input_tokens, None);
        assert_eq!(usage.cost_usd(), None);
    }

    #[test]
    fn a_request_does_not_ask_for_a_temperature_by_default() {
        let request = ChatRequest::new(
            ModelRef::new(ProviderId::OpenRouter, "anthropic/claude-haiku-5.5"),
            vec![Message::user("Proofread this.")],
        );
        assert_eq!(request.temperature, None);
    }

    #[test]
    fn a_model_ref_resolves_against_the_catalog() {
        let model = ModelRef::new(ProviderId::OpenRouter, "anthropic/claude-haiku-5.5");
        let entry = model
            .resolve()
            .expect("the default model is in the catalog");
        assert_eq!(entry.display_name, "Claude Haiku 5.5");
        assert!(!model.supports_temperature());
    }

    #[test]
    fn provider_ids_know_whether_they_have_an_adapter() {
        assert!(ProviderId::OpenRouter.is_supported());
        assert!(ProviderId::OpenAiCompatible.is_supported());
        assert!(!ProviderId::Ollama.is_supported());
        assert_eq!(ProviderId::OpenRouter.display_name(), "OpenRouter");
    }
}
