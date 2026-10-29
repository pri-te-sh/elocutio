//! The streaming provider client.
//!
//! The socket lives here; the decoding lives in `elocutio-llm`, where it is
//! pure and unit-tested. This module reads bytes off the wire, feeds them to
//! the crate's SSE decoder, and hands back canonical [`StreamEvent`]s.

use elocutio_llm::{ChatRequest, ErrorKind, Provider, ProviderError, SseDecoder, StreamEvent};

use super::http::Endpoint;

// The streaming client is the M1 surface for token-by-token delivery. The
// rewrite and analysis paths still use the non-streaming call today, so the
// reader is not wired into a command yet.

/// Collect a whole streamed response into the final text and usage.
///
/// This is the shape the app needs today: the rewrite and analysis paths read
/// `choices[0].message.content` and the token counts. Streaming is wired
/// through the same code path so both surfaces agree.
#[derive(Debug, Clone, Default, PartialEq)]
#[allow(dead_code)]
pub struct StreamedResponse {
    /// The concatenated assistant text.
    pub text: String,
    /// The model the provider said it used.
    pub model: String,
    /// The finish reason the provider reported.
    pub finish_reason: Option<elocutio_llm::FinishReason>,
    /// The token counts, when the provider reported them.
    pub usage: Option<elocutio_llm::Usage>,
}

impl StreamedResponse {
    /// True when the provider reported that it ran out of output tokens.
    #[allow(dead_code)]
    pub fn is_truncated(&self) -> bool {
        self.finish_reason
            .map(|reason| reason.is_truncated())
            .unwrap_or(false)
    }
}

/// Read a streamed chat completion and collect the events.
///
/// The reader stops at the first terminal event, so a `Done` or `Error` ends
/// the stream.
#[allow(dead_code)]
pub async fn stream_chat(
    adapter: &dyn Provider,
    endpoint: &Endpoint,
    request: &ChatRequest,
) -> Result<StreamedResponse, ProviderError> {
    let mut body = adapter.build_request(request);
    body["stream"] = serde_json::Value::Bool(true);

    let client = super::http::streaming_client()
        .map_err(|error| ProviderError::new(ErrorKind::Unknown, error))?;

    let mut request_builder = client.post(&endpoint.url).json(&body);
    if let Some(key) = &endpoint.key {
        request_builder = request_builder.bearer_auth(key);
    }
    if endpoint.send_attribution_headers {
        request_builder = request_builder
            .header("HTTP-Referer", super::http::APP_URL)
            .header("X-Title", super::http::APP_TITLE);
    }

    let result = read_stream(adapter, request_builder).await;

    // A streamed request leaves the Mac like any other, so it is recorded in
    // the network log too. The entry holds metadata, never the text.
    let host = crate::network_log::host_of(&endpoint.url);
    let _ = match &result {
        Ok(collected) => crate::network_log::record_chat(
            &host,
            adapter.display_name(),
            Some(&request.model.model),
            collected
                .usage
                .as_ref()
                .map_or((0, 0), |usage| (usage.input_tokens, usage.output_tokens)),
            collected.usage.as_ref().and_then(|usage| usage.cost_usd()),
            crate::network_log::Outcome::success(200),
        ),
        Err(error) => crate::network_log::record_chat(
            &host,
            adapter.display_name(),
            Some(&request.model.model),
            (0, 0),
            None,
            crate::network_log::Outcome::failure(None, error.message.clone()),
        ),
    };
    result
}

async fn read_stream(
    adapter: &dyn Provider,
    request_builder: reqwest::RequestBuilder,
) -> Result<StreamedResponse, ProviderError> {
    let mut response = tokio::time::timeout(super::REQUEST_TIMEOUT, request_builder.send())
        .await
        .map_err(|_| ProviderError::new(ErrorKind::Timeout, "no response from the provider"))?
        .map_err(|error| adapter.map_transport_error(&error.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let retry_after = super::http::retry_after_seconds(response.headers());
        let raw = response.text().await.unwrap_or_default();
        return Err(adapter
            .map_http_error(status.as_u16(), &raw)
            .with_retry_after(retry_after));
    }

    let mut decoder = SseDecoder::new();
    // The event decoder holds the finish reason back until `[DONE]`, because
    // OpenRouter and OpenAI send usage in a chunk after the finish reason.
    let mut events = elocutio_llm::StreamEventDecoder::new();
    let mut collected = StreamedResponse::default();

    loop {
        let next = tokio::time::timeout(super::STREAM_IDLE_TIMEOUT, response.chunk())
            .await
            .map_err(|_| ProviderError::new(ErrorKind::Timeout, "the stream stalled"))?;
        let Some(chunk) = next.map_err(|error| {
            adapter.map_transport_error(&format!("could not read the stream: {error}"))
        })?
        else {
            break;
        };
        // Feed raw bytes: a chunk can end in the middle of a multi-byte
        // character, and the decoder holds those bytes back until the
        // character is complete.
        for frame in decoder.push_bytes(&chunk) {
            for event in events.events(&frame) {
                match event {
                    StreamEvent::Start { model } => {
                        if collected.model.is_empty() {
                            collected.model = model;
                        }
                    }
                    StreamEvent::TextDelta { text } => collected.text.push_str(&text),
                    StreamEvent::Usage { usage } => collected.usage = Some(usage),
                    StreamEvent::Done { finish_reason } => {
                        collected.finish_reason = Some(finish_reason);
                        return Ok(collected);
                    }
                    StreamEvent::Error { error } => return Err(error),
                }
            }
        }
    }

    // The server closed the connection without a `[DONE]` sentinel.
    if let Some(frame) = decoder.finish() {
        for event in events.events(&frame) {
            match event {
                StreamEvent::TextDelta { text } => collected.text.push_str(&text),
                StreamEvent::Usage { usage } => collected.usage = Some(usage),
                StreamEvent::Done { finish_reason } => {
                    collected.finish_reason = Some(finish_reason);
                }
                StreamEvent::Error { error } => return Err(error),
                StreamEvent::Start { .. } => {}
            }
        }
    }
    if let Some(StreamEvent::Done { finish_reason }) = events.finish() {
        collected.finish_reason = Some(finish_reason);
    }

    Ok(collected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use elocutio_llm::FinishReason;

    #[test]
    fn a_truncated_response_is_flagged() {
        let mut response = StreamedResponse::default();
        assert!(!response.is_truncated());
        response.finish_reason = Some(FinishReason::Length);
        assert!(response.is_truncated());
    }

    #[test]
    fn an_empty_response_has_no_text() {
        let response = StreamedResponse::default();
        assert!(response.text.is_empty());
        assert!(response.model.is_empty());
        assert!(response.usage.is_none());
    }
}
