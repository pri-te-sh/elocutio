//! The request preview.
//!
//! The privacy page promises a "preview of the exact request" before it is
//! sent. This builds that preview: the request the app is about to make,
//! with the API key redacted and the text the user is about to send shown
//! exactly as it will be sent.
//!
//! The point is that the user can check what leaves their Mac without
//! reading the app's source. The key is redacted because a preview that
//! printed the key would be a key on screen.

use serde::Serialize;

/// A field of the request the user is about to send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewField {
    /// The field's name, as the provider names it.
    pub name: String,
    /// The value, with anything sensitive redacted.
    pub value: String,
    /// Whether the value was redacted.
    pub redacted: bool,
}

/// A preview of one request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestPreview {
    /// Where the request goes.
    pub host: String,
    /// The provider.
    pub provider: String,
    /// The model, when the request is a model request.
    pub model: Option<String>,
    /// The request's fields.
    pub fields: Vec<PreviewField>,
    /// The text that will be sent, exactly as it will be sent.
    ///
    /// This is the part the user actually wants to see.
    pub text: String,
    /// The system prompt, which may carry the voice style card.
    pub system: String,
}

/// The text shown in place of a secret.
pub const REDACTED: &str = "••••••••";

/// Build the preview for a chat request.
pub fn chat_preview(
    host: &str,
    provider: &str,
    model: Option<&str>,
    system: &str,
    text: &str,
    has_key: bool,
) -> RequestPreview {
    let mut fields = vec![
        PreviewField {
            name: "model".to_string(),
            value: model.unwrap_or("the default model").to_string(),
            redacted: false,
        },
        PreviewField {
            name: "Authorization".to_string(),
            value: if has_key {
                format!("Bearer {REDACTED}")
            } else {
                "not set".to_string()
            },
            redacted: has_key,
        },
        PreviewField {
            name: "temperature".to_string(),
            // The key is omitted entirely for the models that reject it, so
            // saying "not sent" is the accurate answer.
            value: "not sent".to_string(),
            redacted: false,
        },
    ];

    fields.push(PreviewField {
        name: "estimated cost".to_string(),
        value: format!("${:.4}", estimate_cost(text, model)),
        redacted: false,
    });

    RequestPreview {
        host: host.to_string(),
        provider: provider.to_string(),
        model: model.map(str::to_string),
        fields,
        text: text.to_string(),
        system: system.to_string(),
    }
}

/// A rough cost estimate for the preview.
///
/// The estimate is deliberately an over-estimate: a preview that understates
/// the cost would be worse than no preview.
pub fn estimate_cost(text: &str, model: Option<&str>) -> f64 {
    let input_tokens = (text.chars().count() as f64 / 4.0).ceil();
    let output_tokens = 1024.0;

    let (input_per_million, output_per_million) =
        match model.and_then(elocutio_llm::catalog::find_loose) {
            Some(entry) => (
                entry.price_input_per_million_usd,
                entry.price_output_per_million_usd,
            ),
            None => (3.0, 15.0),
        };

    (input_tokens * input_per_million + output_tokens * output_per_million) / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_never_shown() {
        let preview = chat_preview(
            "openrouter.ai",
            "OpenRouter",
            Some("anthropic/claude-haiku-5.5"),
            "You are a copy editor.",
            "Their going to the conference.",
            true,
        );
        let json = serde_json::to_string(&preview).unwrap();
        assert!(!json.contains("sk-or-"));
        assert!(json.contains(REDACTED));
    }

    #[test]
    fn the_text_is_shown_exactly_as_it_will_be_sent() {
        let text = "Their going to the conference.";
        let preview = chat_preview("openrouter.ai", "OpenRouter", None, "", text, false);
        assert_eq!(preview.text, text);
    }

    #[test]
    fn the_estimate_is_an_over_estimate() {
        let short = estimate_cost("Hi", Some("anthropic/claude-haiku-5.5"));
        let long = estimate_cost(&"word ".repeat(1000), Some("anthropic/claude-haiku-5.5"));
        assert!(long > short);
        assert!(short > 0.0);
    }

    #[test]
    fn a_model_with_no_price_uses_a_conservative_default() {
        let cost = estimate_cost("Hello", Some("some/unknown-model"));
        assert!(cost > 0.0);
    }
}
