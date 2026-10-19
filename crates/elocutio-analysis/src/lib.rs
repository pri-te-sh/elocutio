#![forbid(unsafe_code)]

//! Span resolution: turn model-returned quotes into verified document offsets.
//!
//! Models cannot count UTF-16 or grapheme offsets reliably. The model returns
//! a verbatim quote plus a paragraph id; this crate locates the quote and
//! computes both offset systems locally. Anything ambiguous is dropped rather
//! than applied to the wrong span.
//!
//! v0 implements exact matching. Normalized and fuzzy matching are the next
//! step and must preserve an index map back to the original text.

use elocutio_core::{Category, Resolution, Severity, Suggestion, TextSpan};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

/// The raw shape returned by the model, before local span resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawEdit {
    pub paragraph_id: String,
    /// Verbatim substring of the paragraph.
    pub original: String,
    /// Replacement text. An empty string means delete.
    pub replacement: String,
    pub category: Category,
    pub severity: Severity,
    pub explanation: String,
}

/// Why a raw edit could not be turned into a safe suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResolveError {
    #[error("original text is empty")]
    EmptyOriginal,
    #[error("original text does not occur in the paragraph")]
    NoMatch,
    #[error("original text occurs more than once; ambiguous edit")]
    Ambiguous,
    #[error("replacement is identical to the original")]
    NoChange,
}

/// Resolve an exact quote to a verified [`Suggestion`].
///
/// `id` should be unique within the request; the paragraph id is prepended to
/// keep suggestions unique across a chunked document.
pub fn resolve_exact(
    paragraph_id: &str,
    paragraph: &str,
    edit: &RawEdit,
    id: impl Into<String>,
) -> Result<Suggestion, ResolveError> {
    if edit.original.is_empty() {
        return Err(ResolveError::EmptyOriginal);
    }
    if edit.replacement == edit.original {
        return Err(ResolveError::NoChange);
    }

    let mut matches = paragraph.match_indices(&edit.original);
    let Some((byte_start, _)) = matches.next() else {
        return Err(ResolveError::NoMatch);
    };
    if matches.next().is_some() {
        return Err(ResolveError::Ambiguous);
    }

    let byte_end = byte_start + edit.original.len();
    if byte_end > paragraph.len() {
        return Err(ResolveError::NoMatch);
    }

    let id: String = id.into();

    Ok(Suggestion {
        id: format!("{paragraph_id}:{id}"),
        span: text_span(paragraph, byte_start, byte_end),
        original: edit.original.clone(),
        replacement: edit.replacement.clone(),
        category: edit.category,
        severity: edit.severity,
        explanation: edit.explanation.clone(),
        resolution: Resolution::Exact,
    })
}

/// Convert UTF-8 byte offsets into UTF-16 and grapheme offsets.
///
/// The caller must pass valid UTF-8 boundaries; `match_indices` guarantees
/// this for the exact resolver.
fn text_span(text: &str, byte_start: usize, byte_end: usize) -> TextSpan {
    TextSpan {
        utf16_start: text[..byte_start].encode_utf16().count(),
        utf16_end: text[..byte_end].encode_utf16().count(),
        grapheme_start: text[..byte_start].graphemes(true).count(),
        grapheme_end: text[..byte_end].graphemes(true).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(original: &str, replacement: &str) -> RawEdit {
        RawEdit {
            paragraph_id: "p1".into(),
            original: original.into(),
            replacement: replacement.into(),
            category: Category::Grammar,
            severity: Severity::High,
            explanation: "Test edit.".into(),
        }
    }

    #[test]
    fn resolves_a_unique_exact_match() {
        let paragraph = "Their going to the conference.";
        let suggestion =
            resolve_exact("p1", paragraph, &edit("Their going", "They're going"), "e1")
                .expect("resolves");
        assert_eq!(suggestion.id, "p1:e1");
        assert_eq!(suggestion.original, "Their going");
        assert_eq!(suggestion.replacement, "They're going");
        assert_eq!(suggestion.resolution, Resolution::Exact);
        assert_eq!(suggestion.span.utf16_start, 0);
        assert_eq!(suggestion.span.utf16_end, 11);
        assert_eq!(suggestion.span.grapheme_start, 0);
        assert_eq!(suggestion.span.grapheme_end, 11);
    }

    #[test]
    fn rejects_an_ambiguous_repeat() {
        let paragraph = "the cat saw the dog";
        assert_eq!(
            resolve_exact("p1", paragraph, &edit("the", "a"), "e1"),
            Err(ResolveError::Ambiguous)
        );
    }

    #[test]
    fn rejects_a_missing_quote() {
        let paragraph = "A clean sentence.";
        assert_eq!(
            resolve_exact("p1", paragraph, &edit("tehre", "there"), "e1"),
            Err(ResolveError::NoMatch)
        );
    }

    #[test]
    fn rejects_empty_and_noop_edits() {
        assert_eq!(
            resolve_exact("p1", "text", &edit("", "x"), "e1"),
            Err(ResolveError::EmptyOriginal)
        );
        assert_eq!(
            resolve_exact("p1", "text", &edit("text", "text"), "e1"),
            Err(ResolveError::NoChange)
        );
    }

    #[test]
    fn computes_utf16_offsets_for_emoji() {
        let paragraph = "Hello 👋 world";
        let suggestion = resolve_exact("p1", paragraph, &edit("👋", "🙂"), "e1").expect("resolves");
        // 👋 is one grapheme, two UTF-16 code units, four UTF-8 bytes.
        assert_eq!(suggestion.span.utf16_start, 6);
        assert_eq!(suggestion.span.utf16_end, 8);
        assert_eq!(suggestion.span.grapheme_start, 6);
        assert_eq!(suggestion.span.grapheme_end, 7);
    }

    #[test]
    fn computes_offsets_for_cjk() {
        let paragraph = "你好世界";
        let suggestion =
            resolve_exact("p1", paragraph, &edit("世界", "世界!"), "e1").expect("resolves");
        assert_eq!(suggestion.span.utf16_start, 2);
        assert_eq!(suggestion.span.utf16_end, 4);
        assert_eq!(suggestion.span.grapheme_start, 2);
        assert_eq!(suggestion.span.grapheme_end, 4);
    }

    #[test]
    fn supports_deletion_with_empty_replacement() {
        let paragraph = "Remove this word, please.";
        let suggestion =
            resolve_exact("p1", paragraph, &edit(" this", ""), "e1").expect("resolves");
        assert_eq!(suggestion.replacement, "");
        assert_eq!(suggestion.span.utf16_len(), 5);
    }
}
