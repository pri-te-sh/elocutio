//! Validate a model payload and turn it into safe suggestions.

use elocutio_core::Suggestion;
use serde::{Deserialize, Serialize};

use crate::chunk::Paragraph;
use crate::merge::{DEFAULT_SUGGESTION_LIMIT, cap_suggestions};
use crate::resolve::{RawEdit, ResolveError, resolve_edit};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedEdit {
    pub paragraph_id: String,
    pub original: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub suggestions: Vec<Suggestion>,
    pub rejected: Vec<RejectedEdit>,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    /// Kept as raw values so one off-schema edit is rejected on its own
    /// instead of failing the whole response.
    #[serde(default)]
    suggestions: Vec<serde_json::Value>,
}

/// Parse model JSON, resolve every quote, and drop ambiguous or overlapping
/// suggestions. A malformed payload is an error; individual bad edits are
/// rejected and reported without failing the whole document.
///
/// Suggestion spans are relative to their own paragraph (the paragraph named
/// by the `p3` prefix of the suggestion id). Overlap is therefore only ever
/// checked between suggestions of the same paragraph.
pub fn parse_response(content: &str, paragraphs: &[Paragraph]) -> Result<AnalysisResult, String> {
    let envelope = parse_envelope(content)?;

    let mut suggestions = Vec::new();
    let mut rejected = Vec::new();

    for (index, value) in envelope.suggestions.into_iter().enumerate() {
        let edit: RawEdit = match serde_json::from_value(value.clone()) {
            Ok(edit) => edit,
            Err(error) => {
                let field = |name: &str| {
                    value
                        .get(name)
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                rejected.push(RejectedEdit {
                    paragraph_id: field("paragraph_id"),
                    original: field("original"),
                    reason: format!("invalid edit: {error}"),
                });
                continue;
            }
        };

        let paragraph_id = if edit.paragraph_id.trim().is_empty() && paragraphs.len() == 1 {
            paragraphs[0].id.clone()
        } else {
            edit.paragraph_id.clone()
        };
        let Some(paragraph_index) = paragraphs
            .iter()
            .position(|paragraph| paragraph.id == paragraph_id)
        else {
            rejected.push(RejectedEdit {
                paragraph_id,
                original: edit.original.clone(),
                reason: "unknown paragraph id".to_string(),
            });
            continue;
        };
        let paragraph = &paragraphs[paragraph_index];

        match resolve_edit(
            &paragraph.id,
            &paragraph.text,
            &edit,
            format!("e{}", index + 1),
        ) {
            Ok(suggestion) => suggestions.push((paragraph_index, suggestion)),
            Err(error) => rejected.push(RejectedEdit {
                paragraph_id: edit.paragraph_id.clone(),
                original: edit.original.clone(),
                reason: reject_reason(&error),
            }),
        }
    }

    let (suggestions, dropped) = dedupe_and_sort(suggestions);
    rejected.extend(dropped);

    Ok(AnalysisResult {
        suggestions,
        rejected,
    })
}

/// How many `{` positions to try before giving up on finding the payload.
const MAX_JSON_START_ATTEMPTS: usize = 16;

/// Find the JSON envelope in a model reply.
///
/// Models wrap the payload in reasoning blocks, Markdown fences, and chatty
/// prose before or after it. Rather than strip each wrapper, read the first
/// complete JSON value that starts at a `{` and ignore whatever follows it.
fn parse_envelope(content: &str) -> Result<Envelope, String> {
    let content = strip_think_blocks(content);
    let mut first_error = None;

    for (attempt, (start, _)) in content
        .match_indices('{')
        .take(MAX_JSON_START_ATTEMPTS)
        .enumerate()
    {
        let mut values =
            serde_json::Deserializer::from_str(&content[start..]).into_iter::<serde_json::Value>();
        match values.next() {
            // A later brace may be a nested edit object of a broken payload;
            // only an object that names `suggestions` counts as the envelope
            // then, so a syntax error is never mistaken for "no suggestions".
            Some(Ok(value)) if attempt == 0 || value.get("suggestions").is_some() => {
                match serde_json::from_value::<Envelope>(value) {
                    Ok(envelope) => return Ok(envelope),
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
            Some(Ok(_)) | None => {}
            Some(Err(error)) => {
                first_error.get_or_insert(error);
            }
        }
    }

    let error = match first_error {
        Some(error) => error.to_string(),
        // No object at all: report what serde says about the whole reply.
        None => serde_json::from_str::<Envelope>(content).err().map_or_else(
            || "no JSON object found".to_string(),
            |error| error.to_string(),
        ),
    };
    Err(format!("Model returned invalid JSON: {error}"))
}

/// Drop leading `<think>…</think>` reasoning blocks.
fn strip_think_blocks(content: &str) -> &str {
    let mut trimmed = content.trim();
    while trimmed.starts_with("<think>") {
        let Some(think_end) = trimmed.find("</think>") else {
            break;
        };
        trimmed = trimmed[think_end + "</think>".len()..].trim();
    }
    trimmed
}

fn reject_reason(error: &ResolveError) -> String {
    match error {
        ResolveError::EmptyOriginal => "empty original quote".to_string(),
        ResolveError::NoMatch => "quote not found".to_string(),
        ResolveError::Ambiguous => "quote is ambiguous".to_string(),
        ResolveError::NoChange => "replacement is identical".to_string(),
    }
}

/// Remove duplicates and overlapping suggestions, order them, and cap the
/// result at [`DEFAULT_SUGGESTION_LIMIT`].
///
/// Each suggestion carries the index of its paragraph. Spans are
/// paragraph-relative, so two suggestions are only compared when they belong
/// to the same paragraph; the result is ordered by paragraph, then position.
///
/// Everything that is removed is returned as a [`RejectedEdit`] so nothing is
/// dropped silently.
fn dedupe_and_sort(
    mut suggestions: Vec<(usize, Suggestion)>,
) -> (Vec<Suggestion>, Vec<RejectedEdit>) {
    suggestions.sort_by(|(paragraph_a, a), (paragraph_b, b)| {
        paragraph_a
            .cmp(paragraph_b)
            .then_with(|| a.span.utf16_start.cmp(&b.span.utf16_start))
            .then_with(|| b.severity.cmp(&a.severity))
    });

    let mut result: Vec<(usize, Suggestion)> = Vec::new();
    let mut rejected: Vec<RejectedEdit> = Vec::new();
    for (paragraph, suggestion) in suggestions {
        let same_paragraph = || {
            result
                .iter()
                .filter(|(existing_paragraph, _)| *existing_paragraph == paragraph)
                .map(|(_, existing)| existing)
        };

        if same_paragraph().any(|existing| {
            existing.span.utf16_start == suggestion.span.utf16_start
                && existing.span.utf16_end == suggestion.span.utf16_end
                && existing.replacement == suggestion.replacement
        }) {
            rejected.push(RejectedEdit {
                paragraph_id: paragraph_id_of(&suggestion.id),
                original: suggestion.original.clone(),
                reason: "duplicate of an earlier suggestion".to_string(),
            });
            continue;
        }

        // Sorted by paragraph then start, so the last kept suggestion of this
        // paragraph is the only one that can overlap.
        if let Some(previous) = same_paragraph().next_back()
            && suggestion.span.utf16_start < previous.span.utf16_end
        {
            rejected.push(RejectedEdit {
                paragraph_id: paragraph_id_of(&suggestion.id),
                original: suggestion.original.clone(),
                reason: "overlaps an earlier suggestion".to_string(),
            });
            continue;
        }

        result.push((paragraph, suggestion));
    }

    let result = result
        .into_iter()
        .map(|(_, suggestion)| suggestion)
        .collect();
    let (capped, over_limit) = cap_suggestions(result, DEFAULT_SUGGESTION_LIMIT);
    rejected.extend(over_limit);
    (capped, rejected)
}

/// Extract the paragraph id from a suggestion id of the form `p3:e7`.
fn paragraph_id_of(id: &str) -> String {
    match id.split_once(':') {
        Some((paragraph_id, _)) if !paragraph_id.is_empty() => paragraph_id.to_string(),
        _ => id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::chunk_paragraphs;
    use elocutio_core::{Category, Severity};

    fn paragraphs() -> Vec<Paragraph> {
        chunk_paragraphs("Their going to the conference.\n\nThis is definitely fine.")
    }

    fn payload(suggestions: &str) -> String {
        format!(r#"{{"suggestions":[{suggestions}]}}"#)
    }

    #[test]
    fn parses_and_resolves_a_clean_payload() {
        let json = payload(
            r#"{"paragraph_id":"p1","original":"Their going","replacement":"They're going","category":"grammar","severity":"high","explanation":"Subject-verb agreement."}"#,
        );
        let result = parse_response(&json, &paragraphs()).unwrap();
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].replacement, "They're going");
        assert!(result.rejected.is_empty());
    }

    #[test]
    fn rejects_unknown_paragraphs_and_bad_quotes() {
        let json = payload(
            r#"{"paragraph_id":"p9","original":"x","replacement":"y","category":"grammar","severity":"low","explanation":"x"},
               {"paragraph_id":"p1","original":"not here","replacement":"y","category":"grammar","severity":"low","explanation":"x"}"#,
        );
        let result = parse_response(&json, &paragraphs()).unwrap();
        assert!(result.suggestions.is_empty());
        assert_eq!(result.rejected.len(), 2);
    }

    #[test]
    fn defaults_a_missing_id_for_a_single_paragraph() {
        let json = payload(
            r#"{"original":"Their going","replacement":"They're going","category":"grammar","severity":"high","explanation":"Agreement."}"#,
        );
        let result = parse_response(&json, &paragraphs()[..1]).unwrap();
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].id, "p1:e1");
    }

    #[test]
    fn strips_markdown_fences() {
        let json = format!(
            "```json\n{}\n```",
            payload(
                r#"{"paragraph_id":"p1","original":"Their going","replacement":"They're going","category":"grammar","severity":"high","explanation":"Agreement."}"#
            )
        );
        assert_eq!(
            parse_response(&json, &paragraphs())
                .unwrap()
                .suggestions
                .len(),
            1
        );
    }

    #[test]
    fn reads_the_first_json_value_and_ignores_trailing_prose() {
        let body = payload(
            r#"{"paragraph_id":"p1","original":"Their going","replacement":"They're going","category":"grammar","severity":"high","explanation":"Agreement."}"#,
        );
        for reply in [
            format!("```json\n{body}\n```\nHope this helps!"),
            format!("{body}\n\nNote: I kept the {{tone}} as is."),
            format!("Here you go:\n{body}"),
            format!("<think>maybe {{x}}</think>\n```\n{body}\n```"),
        ] {
            let result = parse_response(&reply, &paragraphs())
                .unwrap_or_else(|error| panic!("{reply:?} failed: {error}"));
            assert_eq!(result.suggestions.len(), 1, "{reply:?}");
        }
        assert!(parse_response("no json here", &paragraphs()).is_err());
        // A broken envelope is an error, not an empty result read from a
        // nested edit object.
        let broken = r#"{"suggestions":[{"paragraph_id":"p1","original":"x"} oops]}"#;
        assert!(parse_response(broken, &paragraphs()).is_err());
    }

    #[test]
    fn one_off_schema_edit_does_not_fail_the_response() {
        let json = payload(
            r#"{"paragraph_id":"p1","original":"Their going","replacement":"They're going","category":"Grammar","severity":"critical"},
               {"paragraph_id":"p2","original":"definitely","replacement":"surely","category":"vibes","severity":"low","explanation":"x"},
               {"paragraph_id":"p2","replacement":"surely","category":"style","severity":"low"}"#,
        );
        let result = parse_response(&json, &paragraphs()).unwrap();
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].category, Category::Grammar);
        assert_eq!(result.suggestions[0].severity, Severity::High);
        assert_eq!(result.rejected.len(), 2);
        assert!(
            result
                .rejected
                .iter()
                .all(|entry| entry.reason.starts_with("invalid edit"))
        );
        assert_eq!(result.rejected[0].original, "definitely");
        assert_eq!(result.rejected[0].paragraph_id, "p2");
    }

    #[test]
    fn spans_in_different_paragraphs_never_overlap() {
        let paragraphs = chunk_paragraphs("Teh cat sat.\n\nThier dog ran.");
        let json = payload(
            r#"{"paragraph_id":"p2","original":"Thier","replacement":"Their","category":"spelling","severity":"high","explanation":"x"},
               {"paragraph_id":"p1","original":"Teh","replacement":"The","category":"spelling","severity":"high","explanation":"x"}"#,
        );
        let result = parse_response(&json, &paragraphs).unwrap();
        assert!(result.rejected.is_empty(), "{:?}", result.rejected);
        let ids: Vec<&str> = result.suggestions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["p1:e2", "p2:e1"]);
    }

    #[test]
    fn drops_overlapping_suggestions() {
        let json = payload(
            r#"{"paragraph_id":"p1","original":"Their","replacement":"They're","category":"grammar","severity":"high","explanation":"Agreement."},
               {"paragraph_id":"p1","original":"Their going","replacement":"They are going","category":"clarity","severity":"medium","explanation":"Clarity."}"#,
        );
        let result = parse_response(&json, &paragraphs()).unwrap();
        assert_eq!(result.suggestions.len(), 1);
    }

    #[test]
    fn reports_the_overlap_it_dropped() {
        let json = payload(
            r#"{"paragraph_id":"p1","original":"Their","replacement":"They're","category":"grammar","severity":"high","explanation":"Agreement."},
               {"paragraph_id":"p1","original":"Their going","replacement":"They are going","category":"clarity","severity":"medium","explanation":"Clarity."}"#,
        );
        let result = parse_response(&json, &paragraphs()).unwrap();
        assert_eq!(result.rejected.len(), 1);
        assert!(result.rejected[0].reason.contains("overlaps"));
    }

    #[test]
    fn reports_suggestions_dropped_by_the_limit() {
        // Zero-padded so every quote is a unique substring of the paragraph.
        let paragraph = (0..30)
            .map(|index| format!("Their going{index:02} to the conference."))
            .collect::<Vec<_>>()
            .join(" ");
        let paragraphs = crate::chunk::chunk_paragraphs(&paragraph);
        let edits: Vec<String> = (0..30)
            .map(|index| {
                format!(
                    r#"{{"paragraph_id":"p1","original":"going{index:02}","replacement":"going{index:02}x","category":"grammar","severity":"low","explanation":"x"}}"#
                )
            })
            .collect();
        let result = parse_response(&payload(&edits.join(",")), &paragraphs).unwrap();
        assert_eq!(result.suggestions.len(), 20);
        assert!(
            result
                .rejected
                .iter()
                .any(|entry| entry.reason.contains("suggestion limit"))
        );
    }
}
