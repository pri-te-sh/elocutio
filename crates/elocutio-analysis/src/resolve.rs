//! Quote-to-offset resolution.
//!
//! The model returns verbatim quotes. We try an exact match first, then a
//! character-preserving normalization (case, smart quotes, dashes, whitespace),
//! then a bounded fuzzy match. Ambiguous matches are rejected, never guessed.

use elocutio_core::{Category, Resolution, Severity, Suggestion, TextSpan};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

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

/// Resolve an edit using exact, then normalized, then fuzzy matching.
pub fn resolve_edit(
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

    let id = id.into();
    let mut last_error = match resolve_exact(paragraph_id, paragraph, edit, id.clone()) {
        Ok(suggestion) => return Ok(suggestion),
        Err(error) => error,
    };

    match resolve_normalized(paragraph_id, paragraph, edit, id.clone()) {
        Ok(suggestion) => return Ok(suggestion),
        Err(error) => last_error = prefer_error(last_error, error),
    }

    match resolve_fuzzy(paragraph_id, paragraph, edit, id) {
        Ok(suggestion) => Ok(suggestion),
        Err(error) => Err(prefer_error(last_error, error)),
    }
}

/// Exact substring matching.
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

    build_suggestion(
        paragraph_id,
        paragraph,
        edit,
        id.into(),
        byte_start,
        byte_start + edit.original.len(),
        Resolution::Exact,
    )
}

fn resolve_normalized(
    paragraph_id: &str,
    paragraph: &str,
    edit: &RawEdit,
    id: String,
) -> Result<Suggestion, ResolveError> {
    let (normalized, map) = normalize(paragraph);
    let needle = normalize(&edit.original).0;
    if needle.is_empty() {
        return Err(ResolveError::NoMatch);
    }

    let mut matches = normalized.match_indices(&needle);
    let Some((start, _)) = matches.next() else {
        return Err(ResolveError::NoMatch);
    };
    if matches.next().is_some() {
        return Err(ResolveError::Ambiguous);
    }

    let end = start + needle.len();
    let byte_start = *map.get(start).ok_or(ResolveError::NoMatch)?;
    let byte_end = map.get(end).copied().unwrap_or(paragraph.len());
    if byte_start >= byte_end || byte_end > paragraph.len() {
        return Err(ResolveError::NoMatch);
    }

    build_suggestion(
        paragraph_id,
        paragraph,
        edit,
        id,
        byte_start,
        byte_end,
        Resolution::Normalized,
    )
}

fn resolve_fuzzy(
    paragraph_id: &str,
    paragraph: &str,
    edit: &RawEdit,
    id: String,
) -> Result<Suggestion, ResolveError> {
    let (normalized, map) = normalize(paragraph);
    let needle = normalize(&edit.original).0;
    let needle_chars = needle.chars().collect::<Vec<_>>();
    if needle_chars.is_empty() {
        return Err(ResolveError::NoMatch);
    }

    let chars = normalized.char_indices().collect::<Vec<_>>();
    if chars.len() < needle_chars.len() {
        return Err(ResolveError::NoMatch);
    }

    let mut best: Option<(usize, usize, usize)> = None; // (distance, start_byte, end_byte)
    let mut ties = 0;

    for start_index in 0..=(chars.len() - needle_chars.len()) {
        let start_byte = chars[start_index].0;
        let end_index = start_index + needle_chars.len();
        let end_byte = if end_index < chars.len() {
            chars[end_index].0
        } else {
            normalized.len()
        };
        let candidate = normalized[start_byte..end_byte].chars().collect::<Vec<_>>();
        let distance = levenshtein(&needle_chars, &candidate);

        match best {
            Some((best_distance, _, _)) if distance > best_distance => {}
            Some((best_distance, _, _)) if distance == best_distance => ties += 1,
            _ => {
                best = Some((distance, start_byte, end_byte));
                ties = 1;
            }
        }
    }

    let Some((distance, start, end)) = best else {
        return Err(ResolveError::NoMatch);
    };
    let threshold = (needle_chars.len() / 4).max(1);
    if distance > threshold {
        return Err(ResolveError::NoMatch);
    }
    if ties > 1 {
        return Err(ResolveError::Ambiguous);
    }

    let byte_start = *map.get(start).ok_or(ResolveError::NoMatch)?;
    let byte_end = map.get(end).copied().unwrap_or(paragraph.len());
    if byte_start >= byte_end || byte_end > paragraph.len() {
        return Err(ResolveError::NoMatch);
    }

    build_suggestion(
        paragraph_id,
        paragraph,
        edit,
        id,
        byte_start,
        byte_end,
        Resolution::Fuzzy,
    )
}

fn build_suggestion(
    paragraph_id: &str,
    paragraph: &str,
    edit: &RawEdit,
    id: String,
    byte_start: usize,
    byte_end: usize,
    resolution: Resolution,
) -> Result<Suggestion, ResolveError> {
    if byte_end > paragraph.len()
        || byte_start >= byte_end
        || !paragraph.is_char_boundary(byte_start)
        || !paragraph.is_char_boundary(byte_end)
    {
        return Err(ResolveError::NoMatch);
    }

    Ok(Suggestion {
        id: format!("{paragraph_id}:{id}"),
        span: text_span(paragraph, byte_start, byte_end),
        original: paragraph[byte_start..byte_end].to_string(),
        replacement: edit.replacement.clone(),
        category: edit.category,
        severity: edit.severity,
        explanation: edit.explanation.clone(),
        resolution,
    })
}

fn prefer_error(a: ResolveError, b: ResolveError) -> ResolveError {
    // Ambiguity is more useful to surface than a simple miss.
    if a == ResolveError::Ambiguous || b == ResolveError::Ambiguous {
        ResolveError::Ambiguous
    } else {
        b
    }
}

/// Lowercase, map smart quotes/dashes to ASCII, and collapse whitespace while
/// keeping a mapping from each normalized byte back to the original byte.
fn normalize(text: &str) -> (String, Vec<usize>) {
    let mut normalized = String::new();
    let mut map = Vec::new();
    let mut last_space = false;

    for (byte_index, character) in text.char_indices() {
        let character = match character {
            '‘' | '’' => '\'',
            '“' | '”' => '"',
            '–' | '—' => '-',
            '\u{00A0}' => ' ',
            other => other,
        };

        for lower in character.to_lowercase() {
            if lower.is_whitespace() {
                if !last_space {
                    normalized.push(' ');
                    map.push(byte_index);
                    last_space = true;
                }
            } else {
                normalized.push(lower);
                for _ in 0..lower.len_utf8() {
                    map.push(byte_index);
                }
                last_space = false;
            }
        }
    }

    while normalized.ends_with(' ') {
        normalized.pop();
        map.pop();
    }

    (normalized, map)
}

fn text_span(text: &str, byte_start: usize, byte_end: usize) -> TextSpan {
    TextSpan {
        utf16_start: text[..byte_start].encode_utf16().count(),
        utf16_end: text[..byte_end].encode_utf16().count(),
        grapheme_start: text[..byte_start].graphemes(true).count(),
        grapheme_end: text[..byte_end].graphemes(true).count(),
    }
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut previous = (0..=b.len()).collect::<Vec<_>>();
    let mut current = vec![0; b.len() + 1];

    for (i, a_char) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, b_char) in b.iter().enumerate() {
            let cost = usize::from(a_char != b_char);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[b.len()]
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
        let suggestion = resolve_edit("p1", paragraph, &edit("Their going", "They're going"), "e1")
            .expect("resolves");
        assert_eq!(suggestion.id, "p1:e1");
        assert_eq!(suggestion.resolution, Resolution::Exact);
        assert_eq!(suggestion.span.utf16_start, 0);
        assert_eq!(suggestion.span.utf16_end, 11);
    }

    #[test]
    fn rejects_an_ambiguous_exact_repeat() {
        let paragraph = "the cat saw the dog";
        assert_eq!(
            resolve_edit("p1", paragraph, &edit("the", "a"), "e1"),
            Err(ResolveError::Ambiguous)
        );
    }

    #[test]
    fn normalizes_case_quotes_and_whitespace() {
        let paragraph = "She said “Hello   world”.";
        let suggestion = resolve_edit("p1", paragraph, &edit("\"hello world\"", "Hello."), "e1")
            .expect("resolves normalized");
        assert_eq!(suggestion.resolution, Resolution::Normalized);
        assert_eq!(suggestion.original, "“Hello   world”");
    }

    #[test]
    fn resolves_a_small_typo_fuzzily() {
        let paragraph = "This is definitely fine.";
        let suggestion = resolve_edit("p1", paragraph, &edit("definately", "definitely"), "e1")
            .expect("fuzzy resolves");
        assert_eq!(suggestion.resolution, Resolution::Fuzzy);
        assert_eq!(suggestion.original, "definitely");
        assert_eq!(suggestion.replacement, "definitely");
    }

    #[test]
    fn fuzzy_rejects_a_distant_quote() {
        let paragraph = "A completely different sentence.";
        assert_eq!(
            resolve_edit("p1", paragraph, &edit("something else", "x"), "e1"),
            Err(ResolveError::NoMatch)
        );
    }

    #[test]
    fn computes_utf16_offsets_for_emoji() {
        let paragraph = "Hello 👋 world";
        let suggestion = resolve_edit("p1", paragraph, &edit("👋", "🙂"), "e1").expect("resolves");
        assert_eq!(suggestion.span.utf16_start, 6);
        assert_eq!(suggestion.span.utf16_end, 8);
        assert_eq!(suggestion.span.grapheme_start, 6);
        assert_eq!(suggestion.span.grapheme_end, 7);
    }

    #[test]
    fn supports_deletion_with_empty_replacement() {
        let paragraph = "Remove this word, please.";
        let suggestion = resolve_edit("p1", paragraph, &edit(" this", ""), "e1").expect("resolves");
        assert_eq!(suggestion.replacement, "");
        assert_eq!(suggestion.span.utf16_len(), 5);
    }

    #[test]
    fn levenshtein_is_small() {
        let a = "kitten".chars().collect::<Vec<_>>();
        let b = "sitting".chars().collect::<Vec<_>>();
        assert_eq!(levenshtein(&a, &b), 3);
    }
}
