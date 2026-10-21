#![forbid(unsafe_code)]

//! Canonical streaming events and a dependency-free SSE frame decoder.
//!
//! The decoder is deliberately hand-rolled: Elocutio speaks Server-Sent
//! Events to OpenRouter and to OpenAI-compatible endpoints, and the wire
//! format is small enough that a new crate dependency would cost more than it
//! saves. Everything here is pure and synchronous, so it is unit-testable
//! without a socket.

use crate::{ErrorKind, FinishReason, ProviderError, Usage};
use serde::{Deserialize, Serialize};

/// The sentinel that terminates an OpenAI-style SSE stream.
pub const DONE_SENTINEL: &str = "[DONE]";

/// A normalized streaming event.
///
/// Every provider's stream is collapsed into this union so the UI never sees
/// a provider-specific event name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// The provider accepted the request and named the model it will use.
    Start { model: String },
    /// A chunk of assistant text.
    TextDelta { text: String },
    /// Token accounting, usually delivered in the final chunk.
    Usage { usage: Usage },
    /// The stream ended normally.
    Done { finish_reason: FinishReason },
    /// The stream ended with a failure.
    Error { error: ProviderError },
}

impl StreamEvent {
    /// The text carried by a delta, if this event carries text.
    pub fn text(&self) -> Option<&str> {
        match self {
            StreamEvent::TextDelta { text } => Some(text),
            _ => None,
        }
    }

    /// True when this event ends the stream.
    pub fn is_terminal(&self) -> bool {
        matches!(self, StreamEvent::Done { .. } | StreamEvent::Error { .. })
    }
}

/// One decoded Server-Sent Events frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseFrame {
    /// The `event:` field, when the producer set one.
    pub event: Option<String>,
    /// The joined `data:` field. Multiple `data:` lines are joined with `\n`.
    pub data: String,
    /// The `id:` field, when the producer set one.
    pub id: Option<String>,
    /// The `retry:` field in milliseconds, when the producer set one.
    pub retry: Option<u64>,
}

impl SseFrame {
    /// True when the frame carries no data and no event name.
    pub fn is_empty(&self) -> bool {
        self.event.is_none() && self.data.is_empty() && self.id.is_none() && self.retry.is_none()
    }
}

/// An incremental Server-Sent Events decoder.
///
/// Feed it bytes as they arrive; it returns the frames that became complete.
/// Bytes are buffered until a frame terminator arrives, so a frame split
/// across two reads is still decoded correctly.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buffer: String,
    /// Bytes that arrived but do not yet form a complete UTF-8 character.
    ///
    /// A network chunk can end in the middle of a multi-byte character. The
    /// bytes are held here until the rest of the character arrives, so the
    /// decoder never emits a replacement character.
    pending_bytes: Vec<u8>,
    /// Set when the last decoded frame was the `[DONE]` sentinel.
    done: bool,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// True once the `[DONE]` sentinel has been seen.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Feed a chunk of bytes and return every frame that completed.
    ///
    /// Returns an empty vector when the chunk only contains a partial frame.
    /// A chunk that ends mid-UTF-8 sequence is buffered until the rest
    /// arrives, so the decoder never produces a replacement character.
    pub fn push(&mut self, chunk: &str) -> Vec<SseFrame> {
        self.buffer.push_str(chunk);
        let mut frames = Vec::new();

        while let Some((content_end, terminator_len)) = next_frame_boundary(&self.buffer) {
            let raw = self.buffer[..content_end].to_string();
            self.buffer = self.buffer[content_end + terminator_len..].to_string();

            if let Some(frame) = parse_sse_frame(&raw) {
                if frame.data.trim() == DONE_SENTINEL {
                    self.done = true;
                }
                frames.push(frame);
            }
        }

        frames
    }

    /// Feed raw bytes and return every frame that completed.
    ///
    /// This is the entry point for a network stream. A chunk that ends in the
    /// middle of a multi-byte UTF-8 character is held back until the rest of
    /// the character arrives, so the decoded text is never corrupted.
    pub fn push_bytes(&mut self, bytes: &[u8]) -> Vec<SseFrame> {
        self.pending_bytes.extend_from_slice(bytes);

        let valid_len = match std::str::from_utf8(&self.pending_bytes) {
            Ok(_) => self.pending_bytes.len(),
            // An incomplete character at the very end: wait for more bytes.
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            // Genuinely invalid UTF-8. Drop it rather than loop forever.
            Err(_) => {
                self.pending_bytes.clear();
                return Vec::new();
            }
        };

        if valid_len == 0 {
            return Vec::new();
        }

        let text = String::from_utf8_lossy(&self.pending_bytes[..valid_len]).into_owned();
        self.pending_bytes.drain(..valid_len);
        self.push(&text)
    }

    /// Flush whatever is left in the buffer as a final frame.
    ///
    /// Some servers close the connection without a trailing blank line; the
    /// trailing partial frame still carries useful data.
    pub fn finish(&mut self) -> Option<SseFrame> {
        if self.buffer.trim().is_empty() {
            self.buffer.clear();
            return None;
        }
        let raw = std::mem::take(&mut self.buffer);
        let frame = parse_sse_frame(&raw)?;
        if frame.data.trim() == DONE_SENTINEL {
            self.done = true;
        }
        Some(frame)
    }
}

/// Find the end of the next complete SSE frame in `buffer`.
///
/// Returns `(end_of_frame, start_of_rest)` as byte offsets, or `None` when
/// the buffer does not hold a complete frame yet.
fn next_frame_boundary(buffer: &str) -> Option<(usize, usize)> {
    // A frame ends at a blank line. Three line-terminator shapes end a
    // frame: `LF LF`, `CRLF CRLF`, and the mixed `LF CRLF`.
    let candidates = [
        buffer.find("\n\n").map(|index| (index, 2)),
        buffer.find("\r\n\r\n").map(|index| (index, 4)),
        buffer.find("\n\r\n").map(|index| (index, 3)),
    ];
    candidates
        .into_iter()
        .flatten()
        .min_by_key(|(index, _)| *index)
}

/// Parse one complete SSE frame body (the text between frame separators).
///
/// Returns `None` for a frame that carries nothing at all, which is what a
/// stray blank line or a comment-only frame produces.
pub fn parse_sse_frame(raw: &str) -> Option<SseFrame> {
    let mut frame = SseFrame::default();
    let mut data_lines: Vec<&str> = Vec::new();

    for line in raw.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        // A line starting with ':' is a comment (used as a keep-alive).
        if line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "data" => data_lines.push(value),
            "event" => frame.event = Some(value.to_string()),
            "id" => frame.id = Some(value.to_string()),
            "retry" => frame.retry = value.trim().parse::<u64>().ok(),
            _ => {}
        }
    }

    if !data_lines.is_empty() {
        frame.data = data_lines.join("\n");
    }

    if frame.is_empty() {
        return None;
    }
    Some(frame)
}

/// Parse a whole SSE document into frames.
///
/// This is the batch counterpart to [`SseDecoder`], handy in tests and for
/// replaying a saved response.
pub fn parse_sse_document(document: &str) -> Vec<SseFrame> {
    let mut decoder = SseDecoder::new();
    let mut frames = decoder.push(document);
    if let Some(frame) = decoder.finish() {
        frames.push(frame);
    }
    frames
}

/// A chunk of an OpenAI-style Chat Completions stream.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatCompletionChunk {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub choices: Vec<ChunkChoice>,
    #[serde(default)]
    pub usage: Option<ChunkUsage>,
    /// Some OpenAI-compatible servers report the failure inside the body.
    #[serde(default)]
    pub error: Option<ChunkError>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChunkChoice {
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub delta: Option<ChunkDelta>,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ChunkDelta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChunkUsage {
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    #[serde(default)]
    pub completion_tokens: Option<u64>,
    #[serde(default)]
    pub total_tokens: Option<u64>,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokenDetails>,
    #[serde(default)]
    pub completion_tokens_details: Option<CompletionTokenDetails>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PromptTokenDetails {
    #[serde(default)]
    pub cached_tokens: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompletionTokenDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChunkError {
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
}

/// Normalize an OpenAI-style finish reason.
pub fn normalize_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason.map(str::trim) {
        None | Some("") => FinishReason::Unknown,
        Some("stop") | Some("end_turn") | Some("stop_sequence") => FinishReason::Stop,
        Some("length") | Some("max_tokens") => FinishReason::Length,
        Some("tool_calls") | Some("function_call") | Some("tool_use") => FinishReason::ToolCalls,
        Some("content_filter") | Some("safety") | Some("recitation") => FinishReason::ContentFilter,
        Some("error") => FinishReason::Error,
        Some(_) => FinishReason::Unknown,
    }
}

/// Turn one decoded SSE frame into canonical stream events.
///
/// The `data` payload of an OpenAI-style stream is a JSON chunk. A frame
/// whose payload is the `[DONE]` sentinel ends the stream. Anything that
/// cannot be understood produces an [`StreamEvent::Error`] rather than
/// silently dropping text.
pub fn events_from_frame(frame: &SseFrame) -> Vec<StreamEvent> {
    let data = frame.data.trim();
    if data.is_empty() {
        return Vec::new();
    }
    if data == DONE_SENTINEL {
        return vec![StreamEvent::Done {
            finish_reason: FinishReason::Stop,
        }];
    }

    let chunk: ChatCompletionChunk = match serde_json::from_str(data) {
        Ok(chunk) => chunk,
        Err(error) => {
            return vec![StreamEvent::Error {
                error: ProviderError::new(
                    ErrorKind::Unknown,
                    format!("The provider sent a stream chunk we could not read: {error}"),
                ),
            }];
        }
    };

    let mut events = Vec::new();

    if let Some(error) = chunk.error {
        let kind = ErrorKind::Unknown.refine_from_code(error.code.as_deref());
        events.push(StreamEvent::Error {
            error: ProviderError::new(
                kind,
                error
                    .message
                    .unwrap_or_else(|| "The provider reported an error mid-stream.".to_string()),
            ),
        });
        return events;
    }

    if let Some(model) = chunk.model {
        events.push(StreamEvent::Start { model });
    }

    for choice in &chunk.choices {
        if let Some(text) = choice
            .delta
            .as_ref()
            .and_then(|delta| delta.content.as_ref())
            .filter(|text| !text.is_empty())
        {
            events.push(StreamEvent::TextDelta { text: text.clone() });
        }
    }

    if let Some(usage) = chunk.usage {
        events.push(StreamEvent::Usage {
            usage: Usage {
                input_tokens: usage.prompt_tokens.unwrap_or(0),
                output_tokens: usage.completion_tokens.unwrap_or(0),
                cached_input_tokens: usage
                    .prompt_tokens_details
                    .as_ref()
                    .and_then(|details| details.cached_tokens),
                reasoning_tokens: usage
                    .completion_tokens_details
                    .as_ref()
                    .and_then(|details| details.reasoning_tokens),
                cost_usd_micros: usage.cost.map(|cost| (cost * 1_000_000.0).round() as u64),
            },
        });
    }

    // The finish reason comes last so a consumer that stops at `Done` has
    // already seen the usage block.
    for choice in &chunk.choices {
        if let Some(reason) = choice.finish_reason.as_deref() {
            events.push(StreamEvent::Done {
                finish_reason: normalize_finish_reason(Some(reason)),
            });
        }
    }

    events
}

/// Turn a whole SSE document into canonical stream events.
pub fn events_from_sse_document(document: &str) -> Vec<StreamEvent> {
    parse_sse_document(document)
        .iter()
        .flat_map(events_from_frame)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_frame_is_decoded() {
        let mut decoder = SseDecoder::new();
        let frames = decoder.push("data: hello\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "hello");
        assert!(!decoder.is_done());
    }

    #[test]
    fn several_frames_in_one_chunk_are_decoded() {
        let mut decoder = SseDecoder::new();
        let frames = decoder.push("data: one\n\ndata: two\n\n");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].data, "one");
        assert_eq!(frames[1].data, "two");
    }

    #[test]
    fn a_frame_split_across_chunks_is_buffered_until_complete() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push("data: par").is_empty());
        assert!(decoder.push("tial").is_empty());
        let frames = decoder.push("\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "partial");
    }

    #[test]
    fn the_done_sentinel_is_recognized() {
        let mut decoder = SseDecoder::new();
        let frames = decoder.push("data: [DONE]\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "[DONE]");
        assert!(decoder.is_done());
    }

    #[test]
    fn multi_line_data_is_joined_with_newlines() {
        let mut decoder = SseDecoder::new();
        let frames = decoder.push("data: line one\ndata: line two\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "line one\nline two");
    }

    #[test]
    fn comments_are_ignored() {
        let mut decoder = SseDecoder::new();
        let frames = decoder.push(": keep-alive\n\ndata: real\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "real");
    }

    #[test]
    fn a_comment_only_frame_produces_nothing() {
        let mut decoder = SseDecoder::new();
        let frames = decoder.push(": ping\n\n");
        assert!(frames.is_empty());
    }

    #[test]
    fn event_and_id_fields_are_captured() {
        let mut decoder = SseDecoder::new();
        let frames = decoder.push("event: message\nid: 42\nretry: 1500\ndata: body\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event.as_deref(), Some("message"));
        assert_eq!(frames[0].id.as_deref(), Some("42"));
        assert_eq!(frames[0].retry, Some(1500));
        assert_eq!(frames[0].data, "body");
    }

    #[test]
    fn carriage_returns_are_stripped() {
        let mut decoder = SseDecoder::new();
        let frames = decoder.push("data: windows\r\n\r\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "windows");
    }

    #[test]
    fn a_value_without_a_leading_space_keeps_its_spacing() {
        let mut decoder = SseDecoder::new();
        let frames = decoder.push("data:  two spaces\n\n");
        assert_eq!(frames[0].data, " two spaces");
    }

    #[test]
    fn a_utf8_character_split_across_byte_chunks_is_reassembled() {
        // "café" — the "é" is two bytes (0xC3 0xA9). A chunk boundary in the
        // middle must not corrupt it.
        let mut decoder = SseDecoder::new();
        let payload = "data: café\n\n".as_bytes();

        let first = &payload[..payload.len() - 3];
        let second = &payload[payload.len() - 3..];

        // Feed the first chunk, then the rest.
        assert!(decoder.push_bytes(first).is_empty());
        let frames = decoder.push_bytes(second);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "café");
    }

    #[test]
    fn a_byte_split_in_the_middle_of_a_character_yields_no_frame_yet() {
        let mut decoder = SseDecoder::new();
        // "data: " then 0xC3, which starts a two-byte character whose second
        // byte has not arrived yet.
        assert!(
            decoder
                .push_bytes(&[b'd', b'a', b't', b'a', b':', b' ', 0xC3])
                .is_empty()
        );
        // The rest of the character plus the frame terminator completes it.
        let frames = decoder.push_bytes(&[0xA9, b'\n', b'\n']);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "\u{e9}");
        assert!(
            !frames[0].data.contains('\u{fffd}'),
            "a split character must not become a replacement character"
        );
    }

    #[test]
    fn byte_chunks_produce_the_same_frames_as_string_chunks() {
        let document = "data: one\n\ndata: two\n\ndata: [DONE]\n\n";
        let from_string = parse_sse_document(document);

        let mut decoder = SseDecoder::new();
        let mut from_bytes = Vec::new();
        for chunk in document.as_bytes().chunks(3) {
            from_bytes.extend(decoder.push_bytes(chunk));
        }
        assert_eq!(from_string, from_bytes);
    }

    #[test]
    fn a_partial_utf8_sequence_is_not_split() {
        // "é" is two bytes. The first byte alone is not valid UTF-8, so a
        // byte-oriented decoder would corrupt it. Our decoder takes `&str`
        // chunks, which means the caller must hand it complete UTF-8; this
        // test proves a split *character* is reassembled correctly when the
        // caller hands over the text after it has been decoded.
        let mut decoder = SseDecoder::new();
        let frames = decoder.push("data: caf\u{e9}\n\n");
        assert_eq!(frames[0].data, "caf\u{e9}");
    }

    #[test]
    fn the_buffer_is_flushed_on_finish() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push("data: tail").is_empty());
        let frame = decoder.finish().expect("a trailing frame is flushed");
        assert_eq!(frame.data, "tail");
    }

    #[test]
    fn finish_on_an_empty_buffer_returns_nothing() {
        let mut decoder = SseDecoder::new();
        decoder.push("data: done\n\n");
        assert!(decoder.finish().is_none());
    }

    #[test]
    fn a_document_with_many_frames_parses_in_order() {
        let document = "data: {\"a\":1}\n\ndata: {\"b\":2}\n\ndata: [DONE]\n\n";
        let frames = parse_sse_document(document);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].data, "{\"a\":1}");
        assert_eq!(frames[2].data, "[DONE]");
    }

    #[test]
    fn openai_chunks_become_canonical_events() {
        let document = concat!(
            "data: {\"model\":\"anthropic/claude-haiku-5.5\",\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );
        let events = events_from_sse_document(document);
        assert_eq!(
            events[0],
            StreamEvent::Start {
                model: "anthropic/claude-haiku-5.5".to_string()
            }
        );
        assert_eq!(events[1].text(), Some("Hel"));
        assert_eq!(events[2].text(), Some("lo"));
        assert!(matches!(
            events[3],
            StreamEvent::Usage {
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 2,
                    ..
                }
            }
        ));
        assert!(matches!(
            events[4],
            StreamEvent::Done {
                finish_reason: FinishReason::Stop
            }
        ));
        assert!(events[4].is_terminal());
    }

    #[test]
    fn a_malformed_chunk_becomes_an_error_event() {
        let events = events_from_sse_document("data: {not json\n\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StreamEvent::Error { .. }));
        assert!(events[0].is_terminal());
    }

    #[test]
    fn an_inline_error_becomes_an_error_event() {
        let document =
            "data: {\"error\":{\"message\":\"overloaded\",\"code\":\"overloaded_error\"}}\n\n";
        let events = events_from_sse_document(document);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Error { error } => {
                assert_eq!(error.kind, ErrorKind::Overloaded);
                assert_eq!(error.message, "overloaded");
            }
            other => panic!("expected an error event, got {other:?}"),
        }
    }

    #[test]
    fn finish_reasons_are_normalized() {
        assert_eq!(normalize_finish_reason(Some("stop")), FinishReason::Stop);
        assert_eq!(
            normalize_finish_reason(Some("max_tokens")),
            FinishReason::Length
        );
        assert_eq!(
            normalize_finish_reason(Some("tool_calls")),
            FinishReason::ToolCalls
        );
        assert_eq!(
            normalize_finish_reason(Some("content_filter")),
            FinishReason::ContentFilter
        );
        assert_eq!(normalize_finish_reason(None), FinishReason::Unknown);
    }
}
