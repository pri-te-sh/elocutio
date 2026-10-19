#![forbid(unsafe_code)]

//! Provider-agnostic request and response types.
//!
//! OpenAI Chat Completions is the base dialect, not the internal model. Every
//! adapter compiles these canonical types to its wire format and normalizes
//! errors and streaming events back. Network clients live in the desktop app,
//! not here, so this crate stays pure and testable.

use serde::{Deserialize, Serialize};
use thiserror::Error;

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

/// A canonical completion request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: ModelRef,
    pub messages: Vec<Message>,
    pub system: Option<String>,
    pub max_output_tokens: Option<u32>,
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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cost_usd_micros: Option<u64>,
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

/// A normalized provider failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{kind:?}: {message}")]
pub struct ProviderError {
    pub kind: ErrorKind,
    pub message: String,
    pub retry_after_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Auth,
    RateLimit,
    Quota,
    InvalidRequest,
    ContextLength,
    ContentFilter,
    Overloaded,
    Timeout,
    Network,
    Server,
    Unsupported,
    Cancelled,
    Unknown,
}

impl ProviderError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retry_after_seconds: None,
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::RateLimit
                | ErrorKind::Overloaded
                | ErrorKind::Timeout
                | ErrorKind::Network
                | ErrorKind::Server
        )
    }
}

/// The minimal contract every provider adapter implements.
///
/// The async HTTP implementation lives in the desktop app; this trait keeps the
/// shape stable and lets tests use a fake provider.
pub trait Provider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn display_name(&self) -> &str;
    fn supports_structured_output(&self) -> bool;
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
    }
}
