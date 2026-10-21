#![forbid(unsafe_code)]

//! Provider adapters: compile canonical requests to a wire format, and
//! normalize responses and failures back.
//!
//! An adapter is a pure translator. It never opens a socket: the async HTTP
//! client lives in the desktop app, which takes the compiled body, sends it,
//! and hands the raw response back to the adapter for parsing.

use serde_json::{Map, Value, json};

use crate::{
    ChatRequest, ChatResponse, ErrorKind, FinishReason, ModelRef, ProviderError, ProviderId,
    ResponseFormat, Role, Usage, catalog,
};

/// The contract every provider adapter implements.
///
/// `build_request` and `parse_response` are the two halves of a round trip:
/// the app sends the compiled body, then feeds the raw response back.
pub trait Provider: Send + Sync {
    /// Which provider this adapter speaks for.
    fn id(&self) -> ProviderId;

    /// The name to show in the UI.
    fn display_name(&self) -> &str;

    /// True when the provider accepts `response_format` with a JSON schema.
    fn supports_structured_output(&self) -> bool;

    /// Compile a canonical request into this provider's wire format.
    fn build_request(&self, request: &ChatRequest) -> Value;

    /// Parse a raw response body into a canonical response.
    fn parse_response(&self, body: &str) -> Result<ChatResponse, ProviderError>;

    /// Normalize a transport failure into the error taxonomy.
    fn map_transport_error(&self, error: &str) -> ProviderError {
        ProviderError::new(ErrorKind::Network, error)
    }

    /// Normalize an HTTP failure into the error taxonomy.
    ///
    /// `body` is the raw response text; the provider's own error envelope is
    /// mined for a message and a code.
    fn map_http_error(&self, status: u16, body: &str) -> ProviderError {
        let envelope = error_envelope(body);
        let kind = ErrorKind::from_status(status).refine_from_code(envelope.code.as_deref());
        let message = envelope
            .message
            .unwrap_or_else(|| format!("The provider returned HTTP {status}."));
        ProviderError::new(kind, message)
    }

    /// True when the model accepts a `temperature` parameter.
    ///
    /// The Claude 5 family accepts only `temperature: 1` and rejects anything
    /// else with an HTTP 400, so the parameter must be omitted entirely.
    fn supports_temperature(&self, model: &str) -> bool {
        catalog::supports_temperature(model)
    }
}

/// The provider-neutral details mined out of a failed response body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErrorEnvelope {
    /// The provider's own explanation.
    pub message: Option<String>,
    /// The provider's machine-readable code, when it sent one.
    pub code: Option<String>,
}

/// Pull the message and code out of a provider's error envelope.
///
/// Understands the OpenAI shape (`{"error":{"message":...,"code":...}}`) and
/// the flat shape some OpenAI-compatible servers use
/// (`{"message":...,"code":...}`). A body that is not JSON at all is used
/// verbatim as the message, truncated to a readable length.
pub fn error_envelope(body: &str) -> ErrorEnvelope {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        let trimmed = body.trim();
        return ErrorEnvelope {
            message: if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.chars().take(400).collect())
            },
            code: None,
        };
    };
    let error = value.get("error").unwrap_or(&value);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let code = error.get("code").and_then(code_to_string).or_else(|| {
        error
            .get("type")
            .and_then(code_to_string)
            .or_else(|| value.get("type").and_then(code_to_string))
    });
    ErrorEnvelope { message, code }
}

fn code_to_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
}

/// An adapter for OpenRouter.
///
/// OpenRouter speaks the OpenAI Chat Completions dialect and adds a few
/// extras: usage accounting in the body, a `provider` routing block, and the
/// `HTTP-Referer` / `X-Title` headers the app already sends.
#[derive(Debug, Clone, Default)]
pub struct OpenRouterAdapter;

impl Provider for OpenRouterAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::OpenRouter
    }

    fn display_name(&self) -> &str {
        "OpenRouter"
    }

    fn supports_structured_output(&self) -> bool {
        true
    }

    fn build_request(&self, request: &ChatRequest) -> Value {
        let mut body = chat_completions_body(request);
        // OpenRouter returns token usage and cost only when asked.
        body["usage"] = json!({ "include": true });
        body
    }

    fn parse_response(&self, body: &str) -> Result<ChatResponse, ProviderError> {
        parse_chat_completions(body)
    }
}

/// An adapter for any OpenAI-compatible endpoint.
///
/// The same Chat Completions dialect as OpenRouter, without the OpenRouter
/// extensions.
#[derive(Debug, Clone, Default)]
pub struct OpenAiCompatibleAdapter;

impl Provider for OpenAiCompatibleAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAiCompatible
    }

    fn display_name(&self) -> &str {
        "OpenAI-compatible"
    }

    fn supports_structured_output(&self) -> bool {
        true
    }

    fn build_request(&self, request: &ChatRequest) -> Value {
        chat_completions_body(request)
    }

    fn parse_response(&self, body: &str) -> Result<ChatResponse, ProviderError> {
        parse_chat_completions(body)
    }
}

/// Build the adapter for a provider id.
///
/// Returns `None` for providers that have no adapter yet (Anthropic direct,
/// Google, Ollama); those land in M3.
pub fn adapter_for(provider: ProviderId) -> Option<Box<dyn Provider>> {
    match provider {
        ProviderId::OpenRouter => Some(Box::new(OpenRouterAdapter)),
        ProviderId::OpenAiCompatible => Some(Box::new(OpenAiCompatibleAdapter)),
        _ => None,
    }
}

/// The adapter for a provider id, or a clear error.
pub fn require_adapter(provider: ProviderId) -> Result<Box<dyn Provider>, ProviderError> {
    adapter_for(provider).ok_or_else(|| {
        ProviderError::new(
            ErrorKind::Unsupported,
            format!("Elocutio does not have an adapter for {provider:?} yet."),
        )
    })
}

/// Compile a canonical request into the OpenAI Chat Completions wire format.
///
/// The shape is shared by OpenRouter and every OpenAI-compatible endpoint:
/// `model`, `messages`, and the optional `max_tokens`, `temperature`,
/// `stream`, and `response_format` keys.
///
/// `temperature` is only written when the request asks for it *and* the model
/// accepts it. Omitting the key entirely is valid for every model, and it is
/// the only way to talk to the Claude 5 family, which rejects any
/// `temperature` other than `1` with an HTTP 400.
fn chat_completions_body(request: &ChatRequest) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    if let Some(system) = request
        .system
        .as_ref()
        .filter(|text| !text.trim().is_empty())
    {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in &request.messages {
        messages.push(json!({
            "role": role_name(message.role),
            "content": message.content,
        }));
    }

    let mut body = Map::new();
    body.insert(
        "model".to_string(),
        Value::String(request.model.model.clone()),
    );
    body.insert("messages".to_string(), Value::Array(messages));

    if let Some(max_tokens) = request.max_output_tokens {
        body.insert("max_tokens".to_string(), json!(max_tokens));
    }

    if let Some(temperature) = request
        .temperature
        .filter(|_| catalog::supports_temperature(&request.model.model))
    {
        body.insert("temperature".to_string(), json!(temperature));
    }

    if request.stream {
        body.insert("stream".to_string(), Value::Bool(true));
    }

    match &request.response_format {
        ResponseFormat::Text => {}
        ResponseFormat::JsonObject => {
            body.insert(
                "response_format".to_string(),
                json!({ "type": "json_object" }),
            );
        }
        ResponseFormat::JsonSchema {
            name,
            schema,
            strict,
        } => {
            body.insert(
                "response_format".to_string(),
                json!({
                    "type": "json_schema",
                    "json_schema": {
                        "name": name,
                        "strict": strict,
                        "schema": schema,
                    }
                }),
            );
        }
    }

    Value::Object(body)
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

/// Parse an OpenAI-style Chat Completions response.
pub fn parse_chat_completions(body: &str) -> Result<ChatResponse, ProviderError> {
    let value: Value = serde_json::from_str(body).map_err(|error| {
        ProviderError::new(
            ErrorKind::Unknown,
            format!("The provider returned an unexpected response: {error}"),
        )
    })?;

    if let Some(envelope) = value.get("error") {
        let kind = ErrorKind::Unknown.refine_from_code(
            envelope
                .get("code")
                .and_then(code_to_string)
                .or_else(|| envelope.get("type").and_then(code_to_string))
                .as_deref(),
        );
        let message = envelope
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("The provider reported an error.");
        return Err(ProviderError::new(kind, message));
    }

    let choices = value
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ProviderError::new(
                ErrorKind::Unknown,
                "The provider returned an unexpected response: no choices.",
            )
        })?;

    let first = choices
        .first()
        .ok_or_else(|| ProviderError::new(ErrorKind::Unknown, "The provider returned no text."))?;

    let text = first
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .or_else(|| {
            first
                .get("delta")
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
        })
        .unwrap_or_default()
        .to_string();

    let finish_reason = first
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(|reason| crate::stream::normalize_finish_reason(Some(reason)))
        .unwrap_or(FinishReason::Unknown);

    let usage = value.get("usage").and_then(parse_usage).unwrap_or_default();

    Ok(ChatResponse {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        model: value
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        text,
        finish_reason,
        usage,
    })
}

fn parse_usage(value: &Value) -> Option<Usage> {
    let object = value.as_object()?;
    Some(Usage {
        input_tokens: object
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: object
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cached_input_tokens: object
            .get("prompt_tokens_details")
            .and_then(|details| details.get("cached_tokens"))
            .and_then(Value::as_u64),
        reasoning_tokens: object
            .get("completion_tokens_details")
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_u64),
        cost_usd_micros: object
            .get("cost")
            .and_then(Value::as_f64)
            .map(|cost| (cost * 1_000_000.0).round() as u64),
    })
}

/// Resolve a model id against the catalog, keeping the original id when the
/// catalog does not know it.
///
/// Users can type any model id, so an unknown id is not an error; it simply
/// has no curated metadata.
pub fn resolve_model(model_ref: &ModelRef) -> Option<&'static catalog::CatalogEntry> {
    catalog::find_loose(&model_ref.model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    fn request(model: &str) -> ChatRequest {
        ChatRequest::new(
            ModelRef::new(ProviderId::OpenRouter, model),
            vec![Message::user("Proofread this.")],
        )
        .with_system("You are a precise copy editor.")
    }

    #[test]
    fn the_body_carries_the_model_and_the_messages() {
        let body = OpenRouterAdapter.build_request(&request("anthropic/claude-haiku-5.5"));
        assert_eq!(body["model"], "anthropic/claude-haiku-5.5");
        let messages = body["messages"].as_array().expect("messages is an array");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are a precise copy editor.");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Proofread this.");
    }

    #[test]
    fn temperature_is_omitted_for_models_that_reject_it() {
        let mut request = request("anthropic/claude-haiku-5.5");
        request.temperature = Some(0.0);
        let body = OpenRouterAdapter.build_request(&request);
        assert!(
            !body
                .as_object()
                .expect("body is an object")
                .contains_key("temperature"),
            "Haiku 5.5 rejects any temperature but 1, so the key must be absent"
        );
    }

    #[test]
    fn temperature_is_kept_for_models_that_accept_it() {
        let mut request = request("openai/gpt-5.6-sol");
        request.temperature = Some(0.2);
        let body = OpenRouterAdapter.build_request(&request);
        let temperature = body["temperature"]
            .as_f64()
            .expect("temperature is a number");
        assert!((temperature - 0.2).abs() < 1e-6, "got {temperature}");
    }

    #[test]
    fn temperature_is_absent_when_the_request_does_not_ask_for_one() {
        let body = OpenRouterAdapter.build_request(&request("openai/gpt-5.6-sol"));
        assert!(
            !body
                .as_object()
                .expect("body is an object")
                .contains_key("temperature")
        );
    }

    #[test]
    fn max_tokens_is_written_when_present() {
        let mut request = request("openai/gpt-5.6-sol");
        request.max_output_tokens = Some(1024);
        let body = OpenRouterAdapter.build_request(&request);
        assert_eq!(body["max_tokens"], 1024);
    }

    #[test]
    fn max_tokens_is_absent_when_not_requested() {
        let body = OpenRouterAdapter.build_request(&request("openai/gpt-5.6-sol"));
        assert!(
            !body
                .as_object()
                .expect("body is an object")
                .contains_key("max_tokens")
        );
    }

    #[test]
    fn text_format_writes_no_response_format() {
        let body = OpenRouterAdapter.build_request(&request("openai/gpt-5.6-sol"));
        assert!(
            !body
                .as_object()
                .expect("body is an object")
                .contains_key("response_format")
        );
    }

    #[test]
    fn json_object_format_writes_the_object_type() {
        let mut request = request("openai/gpt-5.6-sol");
        request.response_format = ResponseFormat::JsonObject;
        let body = OpenRouterAdapter.build_request(&request);
        assert_eq!(body["response_format"]["type"], "json_object");
    }

    #[test]
    fn json_schema_format_writes_the_schema_envelope() {
        let mut request = request("openai/gpt-5.6-sol");
        request.response_format = ResponseFormat::JsonSchema {
            name: "suggestions".to_string(),
            schema: json!({ "type": "object" }),
            strict: true,
        };
        let body = OpenRouterAdapter.build_request(&request);
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(
            body["response_format"]["json_schema"]["name"],
            "suggestions"
        );
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(
            body["response_format"]["json_schema"]["schema"]["type"],
            "object"
        );
    }

    #[test]
    fn openrouter_asks_for_usage_but_the_compatible_adapter_does_not() {
        let request = request("anthropic/claude-haiku-5.5");
        let openrouter = OpenRouterAdapter.build_request(&request);
        assert_eq!(openrouter["usage"]["include"], true);

        let compatible = OpenAiCompatibleAdapter.build_request(&request);
        assert!(
            !compatible
                .as_object()
                .expect("body is an object")
                .contains_key("usage")
        );
    }

    #[test]
    fn the_stream_flag_is_written_only_when_streaming() {
        let mut request = request("openai/gpt-5.6-sol");
        assert!(
            !OpenRouterAdapter
                .build_request(&request)
                .as_object()
                .expect("body is an object")
                .contains_key("stream")
        );
        request.stream = true;
        assert_eq!(OpenRouterAdapter.build_request(&request)["stream"], true);
    }

    #[test]
    fn the_body_is_valid_json() {
        let mut request = request("anthropic/claude-haiku-5.5");
        request.temperature = Some(0.0);
        request.max_output_tokens = Some(1024);
        request.response_format = ResponseFormat::JsonSchema {
            name: "suggestions".to_string(),
            schema: json!({ "type": "object" }),
            strict: true,
        };
        let body = OpenRouterAdapter.build_request(&request);
        let text = serde_json::to_string(&body).expect("the body serializes");
        let decoded: Value = serde_json::from_str(&text).expect("the body is valid json");
        assert_eq!(decoded["model"], "anthropic/claude-haiku-5.5");
    }

    #[test]
    fn a_response_is_parsed_into_the_canonical_shape() {
        let body = r#"{
            "id": "gen-1",
            "model": "anthropic/claude-haiku-5.5",
            "choices": [{"message": {"content": "Hello there."}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 12, "completion_tokens": 3, "cost": 0.000123}
        }"#;
        let response = OpenRouterAdapter
            .parse_response(body)
            .expect("the response parses");
        assert_eq!(response.id, "gen-1");
        assert_eq!(response.model, "anthropic/claude-haiku-5.5");
        assert_eq!(response.text, "Hello there.");
        assert_eq!(response.finish_reason, FinishReason::Stop);
        assert_eq!(response.usage.input_tokens, 12);
        assert_eq!(response.usage.output_tokens, 3);
        assert_eq!(response.usage.cost_usd_micros, Some(123));
    }

    #[test]
    fn a_response_without_choices_is_an_error() {
        let error = OpenRouterAdapter
            .parse_response(r#"{"model":"x"}"#)
            .expect_err("no choices is an error");
        assert_eq!(error.kind, ErrorKind::Unknown);
        assert!(error.message.contains("no choices"));
    }

    #[test]
    fn an_inline_error_envelope_is_an_error() {
        let error = OpenRouterAdapter
            .parse_response(r#"{"error":{"message":"bad key","code":"invalid_api_key"}}"#)
            .expect_err("an error envelope is an error");
        assert_eq!(error.kind, ErrorKind::Auth);
        assert_eq!(error.message, "bad key");
    }

    #[test]
    fn http_errors_map_onto_the_taxonomy() {
        let adapter = OpenRouterAdapter;
        assert_eq!(adapter.map_http_error(401, "{}").kind, ErrorKind::Auth);
        assert_eq!(adapter.map_http_error(402, "{}").kind, ErrorKind::Quota);
        assert_eq!(
            adapter.map_http_error(404, "{}").kind,
            ErrorKind::InvalidRequest
        );
        assert_eq!(adapter.map_http_error(429, "{}").kind, ErrorKind::RateLimit);
        assert_eq!(adapter.map_http_error(500, "{}").kind, ErrorKind::Server);
        assert_eq!(
            adapter.map_http_error(503, "{}").kind,
            ErrorKind::Overloaded
        );
    }

    #[test]
    fn the_error_envelope_mines_the_provider_message() {
        let body = r#"{"error":{"message":"No auth credentials found","code":401}}"#;
        let envelope = error_envelope(body);
        assert_eq!(
            envelope.message.as_deref(),
            Some("No auth credentials found")
        );
        assert_eq!(envelope.code.as_deref(), Some("401"));
    }

    #[test]
    fn the_error_envelope_understands_the_flat_shape() {
        let envelope = error_envelope(r#"{"message":"nope","code":"rate_limit"}"#);
        assert_eq!(envelope.message.as_deref(), Some("nope"));
        assert_eq!(envelope.code.as_deref(), Some("rate_limit"));
    }

    #[test]
    fn the_error_envelope_keeps_a_plain_text_body() {
        // Some providers answer an error with plain text, not JSON. The
        // detail must survive so the user still sees why it failed.
        let envelope = error_envelope("upstream failed");
        assert_eq!(envelope.message.as_deref(), Some("upstream failed"));
        assert_eq!(envelope.code, None);
    }

    #[test]
    fn the_error_envelope_truncates_a_long_plain_text_body() {
        let body = "x".repeat(1000);
        let envelope = error_envelope(&body);
        let message = envelope.message.expect("the body is kept");
        assert_eq!(message.chars().count(), 400);
    }

    #[test]
    fn the_error_envelope_is_empty_for_an_empty_body() {
        assert_eq!(error_envelope("   "), ErrorEnvelope::default());
    }

    #[test]
    fn an_error_code_can_refine_the_status() {
        let adapter = OpenRouterAdapter;
        let error = adapter.map_http_error(
            400,
            r#"{"error":{"message":"too long","code":"context_length_exceeded"}}"#,
        );
        assert_eq!(error.kind, ErrorKind::ContextLength);
        assert_eq!(error.message, "too long");
    }

    #[test]
    fn the_adapters_report_their_identity() {
        assert_eq!(OpenRouterAdapter.id(), ProviderId::OpenRouter);
        assert_eq!(OpenRouterAdapter.display_name(), "OpenRouter");
        assert!(OpenRouterAdapter.supports_structured_output());
        assert_eq!(OpenAiCompatibleAdapter.id(), ProviderId::OpenAiCompatible);
        assert_eq!(OpenAiCompatibleAdapter.display_name(), "OpenAI-compatible");
        assert!(OpenAiCompatibleAdapter.supports_structured_output());
    }

    #[test]
    fn an_adapter_can_be_looked_up_by_provider_id() {
        assert_eq!(
            adapter_for(ProviderId::OpenRouter)
                .expect("openrouter has an adapter")
                .id(),
            ProviderId::OpenRouter
        );
        assert!(adapter_for(ProviderId::Ollama).is_none());
        let error = match require_adapter(ProviderId::Ollama) {
            Ok(_) => panic!("ollama has no adapter yet"),
            Err(error) => error,
        };
        assert_eq!(error.kind, ErrorKind::Unsupported);
    }

    #[test]
    fn an_unknown_model_resolves_to_nothing() {
        assert!(resolve_model(&ModelRef::new(ProviderId::OpenRouter, "x/y")).is_none());
        assert_eq!(
            resolve_model(&ModelRef::new(ProviderId::OpenRouter, "claude-haiku-5.5"))
                .expect("the default resolves")
                .id,
            "anthropic/claude-haiku-5.5"
        );
    }
}
