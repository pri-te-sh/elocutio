#![forbid(unsafe_code)]

//! The normalized provider error taxonomy and its user-facing sentences.
//!
//! Every provider failure is reduced to an [`ErrorKind`] before it reaches the
//! UI. The UI switches on the kind, never on provider-specific strings, and
//! [`ErrorKind::user_message`] renders the sentence a person can act on.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A normalized provider failure.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[error("{kind:?}: {message}")]
pub struct ProviderError {
    pub kind: ErrorKind,
    pub message: String,
    pub retry_after_seconds: Option<u64>,
}

/// The closed set of failures Elocutio knows how to talk about.
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

    /// Attach a `Retry-After` hint so the caller can back off precisely.
    pub fn with_retry_after(mut self, seconds: Option<u64>) -> Self {
        self.retry_after_seconds = seconds;
        self
    }

    pub fn is_retryable(&self) -> bool {
        self.kind.is_retryable()
    }

    /// The sentence to show the user.
    ///
    /// `detail` is the provider's own explanation, already trimmed. It is
    /// appended in parentheses so the person can see what the provider said
    /// without the raw JSON ever reaching the screen.
    pub fn user_message(&self, detail: Option<&str>) -> String {
        let base = self.kind.user_message();
        match detail.map(str::trim).filter(|detail| !detail.is_empty()) {
            Some(detail) => format!("{base} ({detail})"),
            None => base.to_string(),
        }
    }
}

impl ErrorKind {
    /// The human sentence for this failure class.
    ///
    /// These are the messages the app shows. They are written to name the
    /// action a person can take next, not to leak provider internals.
    pub fn user_message(self) -> &'static str {
        match self {
            ErrorKind::Auth => {
                "The provider rejected the API key. Check it in Settings and save it again."
            }
            ErrorKind::Quota => {
                "The provider says this account is out of credits. Add credits or use another key."
            }
            ErrorKind::RateLimit => {
                "The provider rate limit was reached. Wait a moment and try again."
            }
            ErrorKind::InvalidRequest => "The provider rejected the request.",
            ErrorKind::ContextLength => {
                "The text is longer than the model can handle. Shorten it or pick a model with a larger context window."
            }
            ErrorKind::ContentFilter => {
                "The provider blocked this text with its content filter. Rephrase the passage and try again."
            }
            ErrorKind::Overloaded => {
                "The provider is overloaded right now. Try again shortly, or switch to another model."
            }
            ErrorKind::Timeout => "The provider took too long to answer. Try again.",
            ErrorKind::Network => {
                "Could not reach the provider. Check your connection and try again."
            }
            ErrorKind::Server => "The provider is having trouble right now. Try again shortly.",
            ErrorKind::Unsupported => {
                "This provider does not support that request. Try a different model or provider."
            }
            ErrorKind::Cancelled => "The request was cancelled.",
            ErrorKind::Unknown => "The provider returned an unexpected response.",
        }
    }

    /// True when a failure of this class is worth retrying.
    ///
    /// The policy is deliberate: a bad key, an exhausted quota, or a
    /// malformed request will fail the same way every time, so retrying only
    /// wastes time. Capacity and transport problems are usually transient.
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            ErrorKind::RateLimit
                | ErrorKind::Overloaded
                | ErrorKind::Timeout
                | ErrorKind::Network
                | ErrorKind::Server
        )
    }

    /// Map an HTTP status code onto the taxonomy.
    ///
    /// `503` and `529` are *overloaded* rather than *server* because providers
    /// use them for capacity problems, which are worth retrying on a
    /// different model.
    pub fn from_status(status: u16) -> Self {
        match status {
            401 | 403 => ErrorKind::Auth,
            402 => ErrorKind::Quota,
            404 => ErrorKind::InvalidRequest,
            408 => ErrorKind::Timeout,
            413 => ErrorKind::ContextLength,
            422 => ErrorKind::InvalidRequest,
            429 => ErrorKind::RateLimit,
            503 | 529 => ErrorKind::Overloaded,
            500..=599 => ErrorKind::Server,
            _ => ErrorKind::Unknown,
        }
    }

    /// Refine a status-derived kind using the provider's error code.
    ///
    /// A `400` is the catch-all "invalid request", but providers use it for
    /// context overflow and content-filter refusals too, and those need
    /// different sentences.
    pub fn refine_from_code(self, code: Option<&str>) -> Self {
        let Some(code) = code.map(str::to_ascii_lowercase) else {
            return self;
        };
        if code.contains("context_length") || code.contains("context_window") {
            return ErrorKind::ContextLength;
        }
        if code.contains("content_filter")
            || code.contains("content_policy")
            || code.contains("safety")
            || code.contains("refusal")
        {
            return ErrorKind::ContentFilter;
        }
        if code.contains("rate_limit") {
            return ErrorKind::RateLimit;
        }
        if code.contains("insufficient_quota") || code.contains("billing") {
            return ErrorKind::Quota;
        }
        if code.contains("invalid_api_key") || code.contains("authentication") {
            return ErrorKind::Auth;
        }
        if code.contains("overloaded") || code.contains("capacity") {
            return ErrorKind::Overloaded;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_map_to_the_taxonomy() {
        assert_eq!(ErrorKind::from_status(401), ErrorKind::Auth);
        assert_eq!(ErrorKind::from_status(402), ErrorKind::Quota);
        assert_eq!(ErrorKind::from_status(404), ErrorKind::InvalidRequest);
        assert_eq!(ErrorKind::from_status(429), ErrorKind::RateLimit);
        assert_eq!(ErrorKind::from_status(500), ErrorKind::Server);
        assert_eq!(ErrorKind::from_status(503), ErrorKind::Overloaded);
        assert_eq!(ErrorKind::from_status(529), ErrorKind::Overloaded);
        assert_eq!(ErrorKind::from_status(418), ErrorKind::Unknown);
    }

    #[test]
    fn error_codes_refine_ambiguous_statuses() {
        assert_eq!(
            ErrorKind::InvalidRequest.refine_from_code(Some("context_length_exceeded")),
            ErrorKind::ContextLength
        );
        assert_eq!(
            ErrorKind::InvalidRequest.refine_from_code(Some("content_filter")),
            ErrorKind::ContentFilter
        );
        assert_eq!(
            ErrorKind::Unknown.refine_from_code(Some("insufficient_quota")),
            ErrorKind::Quota
        );
        assert_eq!(
            ErrorKind::Auth.refine_from_code(Some("invalid_api_key")),
            ErrorKind::Auth
        );
        assert_eq!(
            ErrorKind::InvalidRequest.refine_from_code(Some("something_else")),
            ErrorKind::InvalidRequest
        );
        assert_eq!(
            ErrorKind::InvalidRequest.refine_from_code(None),
            ErrorKind::InvalidRequest
        );
    }

    #[test]
    fn the_retry_policy_lives_on_the_kind() {
        assert!(ErrorKind::RateLimit.is_retryable());
        assert!(ErrorKind::Overloaded.is_retryable());
        assert!(ErrorKind::Server.is_retryable());
        assert!(!ErrorKind::Auth.is_retryable());
        assert!(!ErrorKind::Quota.is_retryable());
        assert!(!ErrorKind::InvalidRequest.is_retryable());
        assert!(!ErrorKind::Cancelled.is_retryable());
    }

    #[test]
    fn user_message_names_the_next_action() {
        let auth = ProviderError::new(ErrorKind::Auth, "denied").user_message(None);
        assert!(auth.contains("API key"));
        assert!(auth.contains("Settings"));

        let quota = ProviderError::new(ErrorKind::Quota, "no credits").user_message(None);
        assert!(quota.contains("out of credits"));

        let rate = ProviderError::new(ErrorKind::RateLimit, "slow down").user_message(None);
        assert!(rate.contains("rate limit"));
    }

    #[test]
    fn user_message_appends_the_provider_detail() {
        let message = ProviderError::new(ErrorKind::Auth, "denied")
            .user_message(Some("No auth credentials found"));
        assert!(message.contains("No auth credentials found"));
        assert!(message.ends_with("(No auth credentials found)"));
    }

    #[test]
    fn user_message_skips_a_blank_detail() {
        let message = ProviderError::new(ErrorKind::Auth, "denied").user_message(Some("   "));
        assert!(!message.contains('('));
    }
}
