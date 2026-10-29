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
    #[serde(default)]
    pub paragraph_id: String,
    /// Verbatim substring of the paragraph.
    pub original: String,
    /// Replacement text. An empty string means delete.
    pub replacement: String,
    /// Accepted case-insensitively, with a few common synonyms, because
    /// models do not always echo the schema's spelling.
    #[serde(deserialize_with = "deserialize_category")]
    pub category: Category,
    #[serde(deserialize_with = "deserialize_severity")]
    pub severity: Severity,
    #[serde(default)]
    pub explanation: String,
}

/// Normalise a model-supplied enum label: lowercase, `snake_case`.
fn label_key(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|character| match character {
            ' ' | '-' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// Parse a category label leniently ("Grammar", "word choice", "typo").
fn parse_category(value: &str) -> Option<Category> {
    Some(match label_key(value).as_str() {
        "grammar" | "grammatical" => Category::Grammar,
        "spelling" | "typo" | "misspelling" => Category::Spelling,
        "punctuation" => Category::Punctuation,
        "capitalization" | "capitalisation" | "case" => Category::Capitalization,
        "clarity" | "clear" => Category::Clarity,
        "conciseness" | "concision" | "concise" | "wordiness" => Category::Conciseness,
        "word_choice" | "wordchoice" | "vocabulary" | "diction" => Category::WordChoice,
        "consistency" => Category::Consistency,
        "tone" => Category::Tone,
        "style" => Category::Style,
        "formatting" | "format" => Category::Formatting,
        _ => return None,
    })
}

/// Parse a severity label leniently ("High", "critical", "minor").
fn parse_severity(value: &str) -> Option<Severity> {
    Some(match label_key(value).as_str() {
        "high" | "critical" | "major" | "severe" | "error" => Severity::High,
        "medium" | "moderate" | "med" | "warning" => Severity::Medium,
        "low" | "minor" | "trivial" | "info" | "suggestion" => Severity::Low,
        _ => return None,
    })
}

fn deserialize_category<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Category, D::Error> {
    let value = String::deserialize(deserializer)?;
    parse_category(&value)
        .ok_or_else(|| serde::de::Error::custom(format!("unknown category {value:?}")))
}

fn deserialize_severity<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Severity, D::Error> {
    let value = String::deserialize(deserializer)?;
    parse_severity(&value)
        .ok_or_else(|| serde::de::Error::custom(format!("unknown severity {value:?}")))
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
    // A stricter matcher that found several occurrences settles the question:
    // a looser matcher cannot tell them apart either, so it must not guess.
    match resolve_exact(paragraph_id, paragraph, edit, id.clone()) {
        Ok(suggestion) => return Ok(suggestion),
        Err(ResolveError::Ambiguous) => return Err(ResolveError::Ambiguous),
        Err(_) => {}
    }

    match resolve_normalized(paragraph_id, paragraph, edit, id.clone()) {
        Ok(suggestion) => return Ok(suggestion),
        Err(ResolveError::Ambiguous) => return Err(ResolveError::Ambiguous),
        Err(_) => {}
    }

    resolve_fuzzy(paragraph_id, paragraph, edit, id)
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

    let byte_start = find_unique(paragraph, &edit.original)?;

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

    let start = find_unique(&normalized, &needle)?;

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
    let needle = normalize(&edit.original).0.chars().collect::<Vec<_>>();
    if needle.is_empty() {
        return Err(ResolveError::NoMatch);
    }

    let chars = normalized.char_indices().collect::<Vec<_>>();
    let text = chars
        .iter()
        .map(|(_, character)| *character)
        .collect::<Vec<_>>();
    let threshold = (needle.len() / 4).max(1);
    let (start, end) = fuzzy_find(&needle, &text, threshold)?;

    let char_to_byte = |index: usize| chars.get(index).map_or(normalized.len(), |(byte, _)| *byte);
    let start = char_to_byte(start);
    let end = char_to_byte(end);

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

/// One cell of the approximate-matching table: the best alignment of a
/// needle prefix against text that ends at the current column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Alignment {
    /// Edit distance of the alignment.
    cost: usize,
    /// Characters that matched exactly. Among equally cheap alignments the
    /// one that matches more characters is the better explanation of the
    /// quote, so "wrld," lands on "world," rather than "orld,".
    matches: usize,
    /// Text index (in chars) where the alignment starts.
    start: usize,
}

impl Alignment {
    /// Lower is better: cost, then more matches, then an earlier start.
    fn key(&self) -> (usize, std::cmp::Reverse<usize>, usize) {
        (self.cost, std::cmp::Reverse(self.matches), self.start)
    }

    fn step(self, cost: usize, matched: bool) -> Self {
        Self {
            cost: self.cost + cost,
            matches: self.matches + usize::from(matched),
            start: self.start,
        }
    }
}

/// Find the substring of `text` closest to `needle` (semi-global alignment).
///
/// Unlike a fixed-width window, the match may be shorter or longer than the
/// needle, so a quote that drops or adds a character still lands on the
/// intended words. Runs in `O(needle × text)` time and `O(needle)` memory.
///
/// Returns the `[start, end)` char range of the match. Two equally close
/// matches are only ambiguous when they do not overlap: overlapping ties are
/// shifted copies of the same occurrence, not different occurrences.
fn fuzzy_find(
    needle: &[char],
    text: &[char],
    threshold: usize,
) -> Result<(usize, usize), ResolveError> {
    let rows = needle.len();
    // Column for "no text consumed yet": the needle prefix must be deleted.
    let mut previous: Vec<Alignment> = (0..=rows)
        .map(|row| Alignment {
            cost: row,
            matches: 0,
            start: 0,
        })
        .collect();
    let mut current = previous.clone();
    // Every acceptable match: its alignment and its end (exclusive).
    let mut candidates: Vec<(Alignment, usize)> = Vec::new();

    for (column, &text_char) in text.iter().enumerate() {
        let end = column + 1;
        // A match may start anywhere, for free.
        current[0] = Alignment {
            cost: 0,
            matches: 0,
            start: end,
        };
        for row in 1..=rows {
            let matched = needle[row - 1] == text_char;
            let diagonal = previous[row - 1].step(usize::from(!matched), matched);
            // The text has a character the needle lacks.
            let extra_text = previous[row].step(1, false);
            // The needle has a character the text lacks.
            let missing_text = current[row - 1].step(1, false);
            current[row] = [diagonal, extra_text, missing_text]
                .into_iter()
                .min_by_key(Alignment::key)
                .expect("three options");
        }
        let cell = current[rows];
        if cell.cost <= threshold && cell.start < end {
            candidates.push((cell, end));
        }
        std::mem::swap(&mut previous, &mut current);
    }

    let Some(&(best, best_end)) = candidates
        .iter()
        .min_by_key(|(alignment, end)| (alignment.key(), *end))
    else {
        return Err(ResolveError::NoMatch);
    };
    let disjoint_tie = candidates.iter().any(|(alignment, end)| {
        alignment.cost == best.cost && (*end <= best.start || alignment.start >= best_end)
    });
    if disjoint_tie {
        return Err(ResolveError::Ambiguous);
    }

    Ok(snap_to_words(needle, text, best.start, best_end, threshold))
}

/// Tidy a fuzzy match so it covers whole words.
///
/// Whitespace the quote does not have is trimmed from the edges, and a match
/// that starts or ends mid-word is widened to the word boundary when the
/// quote itself starts or ends with a word character and the wider span is
/// still within the distance threshold.
fn snap_to_words(
    needle: &[char],
    text: &[char],
    mut start: usize,
    mut end: usize,
    threshold: usize,
) -> (usize, usize) {
    let is_word = |character: char| character.is_alphanumeric() || character == '\'';
    let first = needle[0];
    let last = needle[needle.len() - 1];
    let original = (start, end);

    if !first.is_whitespace() {
        while start < end && text[start].is_whitespace() {
            start += 1;
        }
    }
    if !last.is_whitespace() {
        while end > start && text[end - 1].is_whitespace() {
            end -= 1;
        }
    }
    if start >= end {
        return original;
    }

    if is_word(first) {
        let mut widened = start;
        while widened > 0 && is_word(text[widened - 1]) && is_word(text[widened]) {
            widened -= 1;
        }
        if widened < start && levenshtein(needle, &text[widened..end]) <= threshold {
            start = widened;
        }
    }
    if is_word(last) {
        let mut widened = end;
        while widened < text.len() && is_word(text[widened]) && is_word(text[widened - 1]) {
            widened += 1;
        }
        if widened > end && levenshtein(needle, &text[start..widened]) <= threshold {
            end = widened;
        }
    }

    (start, end)
}

/// The byte offset of the only occurrence of `needle` in `haystack`.
///
/// Overlapping repeats count: "that that" occurs twice in "that that that".
fn find_unique(haystack: &str, needle: &str) -> Result<usize, ResolveError> {
    let Some(start) = haystack.find(needle) else {
        return Err(ResolveError::NoMatch);
    };
    // Search again one character after the first match, so an overlapping
    // repeat is not skipped the way `match_indices` would skip it.
    let next = start + haystack[start..].chars().next().map_or(1, char::len_utf8);
    if haystack[next..].contains(needle) {
        return Err(ResolveError::Ambiguous);
    }
    Ok(start)
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
        // The resolver's own confidence, derived from the matching strategy
        // that succeeded. Never a model-reported value.
        confidence: resolution.confidence(),
    })
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
    fn fuzzy_matches_may_be_shorter_or_longer_than_the_quote() {
        let cases = [
            ("Hello world, again.", "wrld,", "world,"),
            (
                "We accomodate all guests",
                "acomodate all",
                "accomodate all",
            ),
            (
                "Let's discuss the the plan.",
                "discuss the plan",
                "discuss the the plan",
            ),
            ("Its a nice day.", "It's a nice", "Its a nice"),
        ];
        for (paragraph, quote, expected) in cases {
            let suggestion = resolve_edit("p1", paragraph, &edit(quote, "x"), "e1")
                .unwrap_or_else(|error| panic!("{quote:?} did not resolve: {error:?}"));
            assert_eq!(suggestion.resolution, Resolution::Fuzzy, "{quote:?}");
            assert_eq!(suggestion.original, expected, "{quote:?}");
        }
    }

    #[test]
    fn overlapping_fuzzy_ties_are_not_ambiguous() {
        let paragraph = "This is definitely fine.";
        let suggestion = resolve_edit("p1", paragraph, &edit("definitly", "definitely"), "e1")
            .expect("one occurrence is not ambiguous");
        assert_eq!(suggestion.original, "definitely");
    }

    #[test]
    fn separate_fuzzy_ties_are_ambiguous() {
        let paragraph = "the cat sat on the mat";
        assert_eq!(
            resolve_edit("p1", paragraph, &edit("thw", "a"), "e1"),
            Err(ResolveError::Ambiguous)
        );
    }

    #[test]
    fn overlapping_exact_repeats_are_ambiguous() {
        let paragraph = "he said that that that was fine";
        assert_eq!(
            resolve_edit("p1", paragraph, &edit("that that", "that"), "e1"),
            Err(ResolveError::Ambiguous)
        );
        assert_eq!(
            resolve_edit(
                "p1",
                "He said THAT THAT THAT.",
                &edit("that that", "that"),
                "e1"
            ),
            Err(ResolveError::Ambiguous)
        );
    }

    #[test]
    fn raw_edits_accept_loose_labels_and_a_missing_explanation() {
        let edit: RawEdit = serde_json::from_str(
            r#"{"original":"a","replacement":"b","category":"Word Choice","severity":"critical"}"#,
        )
        .expect("lenient labels parse");
        assert_eq!(edit.category, Category::WordChoice);
        assert_eq!(edit.severity, Severity::High);
        assert!(edit.explanation.is_empty());
        assert!(
            serde_json::from_str::<RawEdit>(
                r#"{"original":"a","replacement":"b","category":"nonsense","severity":"low"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn levenshtein_is_small() {
        let a = "kitten".chars().collect::<Vec<_>>();
        let b = "sitting".chars().collect::<Vec<_>>();
        assert_eq!(levenshtein(&a, &b), 3);
    }
}
