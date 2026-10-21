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
    #[serde(default)]
    suggestions: Vec<RawEdit>,
}

/// Parse model JSON, resolve every quote, and drop ambiguous or overlapping
/// suggestions. A malformed payload is an error; individual bad edits are
/// rejected and reported without failing the whole document.
pub fn parse_response(content: &str, paragraphs: &[Paragraph]) -> Result<AnalysisResult, String> {
    let content = strip_code_fence(content);
    let envelope: Envelope = serde_json::from_str(content)
        .map_err(|error| format!("Model returned invalid JSON: {error}"))?;

    let mut suggestions = Vec::new();
    let mut rejected = Vec::new();

    for (index, edit) in envelope.suggestions.into_iter().enumerate() {
        let paragraph_id = if edit.paragraph_id.trim().is_empty() && paragraphs.len() == 1 {
            paragraphs[0].id.clone()
        } else {
            edit.paragraph_id.clone()
        };
        let Some(paragraph) = paragraphs
            .iter()
            .find(|paragraph| paragraph.id == paragraph_id)
        else {
            rejected.push(RejectedEdit {
                paragraph_id,
                original: edit.original.clone(),
                reason: "unknown paragraph id".to_string(),
            });
            continue;
        };

        match resolve_edit(
            &paragraph.id,
            &paragraph.text,
            &edit,
            format!("e{}", index + 1),
        ) {
            Ok(suggestion) => suggestions.push(suggestion),
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

fn strip_code_fence(content: &str) -> &str {
    let trimmed = content.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        trimmed
    }
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
/// Everything that is removed is returned as a [`RejectedEdit`] so nothing is
/// dropped silently.
fn dedupe_and_sort(mut suggestions: Vec<Suggestion>) -> (Vec<Suggestion>, Vec<RejectedEdit>) {
    suggestions.sort_by(|a, b| {
        a.span
            .utf16_start
            .cmp(&b.span.utf16_start)
            .then_with(|| b.severity.cmp(&a.severity))
    });

    let mut result: Vec<Suggestion> = Vec::new();
    let mut rejected: Vec<RejectedEdit> = Vec::new();
    for suggestion in suggestions {
        if result.iter().any(|existing| {
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

        if let Some(previous) = result.last()
            && suggestion.span.utf16_start < previous.span.utf16_end
        {
            rejected.push(RejectedEdit {
                paragraph_id: paragraph_id_of(&suggestion.id),
                original: suggestion.original.clone(),
                reason: "overlaps an earlier suggestion".to_string(),
            });
            continue;
        }

        result.push(suggestion);
    }

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
