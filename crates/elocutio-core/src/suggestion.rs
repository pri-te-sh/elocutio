use serde::{Deserialize, Serialize};

/// The kind of writing issue a suggestion addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Grammar,
    Spelling,
    Punctuation,
    Capitalization,
    Clarity,
    Conciseness,
    WordChoice,
    Consistency,
    Tone,
    Style,
    Formatting,
}

impl Category {
    /// Every category, in a stable order suitable for settings UI.
    pub const ALL: [Category; 11] = [
        Category::Grammar,
        Category::Spelling,
        Category::Punctuation,
        Category::Capitalization,
        Category::Clarity,
        Category::Conciseness,
        Category::WordChoice,
        Category::Consistency,
        Category::Tone,
        Category::Style,
        Category::Formatting,
    ];
}

/// How strongly a suggestion should be surfaced.
///
/// `Low < Medium < High`, so sorting by severity is deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
}

/// How confidently the client located the quoted text in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// Exact substring match.
    Exact,
    /// Match after Unicode/quote/whitespace normalization.
    Normalized,
    /// Match after fuzzy comparison.
    Fuzzy,
}

/// A range in a document, expressed in both offset systems.
///
/// AppKit and the Accessibility API use UTF-16 ranges; Rust strings are
/// UTF-8/char based; Swift's `String.Index` is grapheme-cluster based. Store
/// all three at the boundary so no layer has to guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSpan {
    pub utf16_start: usize,
    pub utf16_end: usize,
    pub grapheme_start: usize,
    pub grapheme_end: usize,
}

impl TextSpan {
    pub const fn new(
        utf16_start: usize,
        utf16_end: usize,
        grapheme_start: usize,
        grapheme_end: usize,
    ) -> Self {
        Self {
            utf16_start,
            utf16_end,
            grapheme_start,
            grapheme_end,
        }
    }

    pub const fn utf16_len(&self) -> usize {
        self.utf16_end.saturating_sub(self.utf16_start)
    }

    pub const fn grapheme_len(&self) -> usize {
        self.grapheme_end.saturating_sub(self.grapheme_start)
    }

    pub const fn is_empty(&self) -> bool {
        self.utf16_start == self.utf16_end
    }
}

/// A single, minimal edit proposed to the writer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: String,
    pub span: TextSpan,
    pub original: String,
    pub replacement: String,
    pub category: Category,
    pub severity: Severity,
    pub explanation: String,
    pub resolution: Resolution,
}

impl Suggestion {
    /// A suggestion that changes nothing is never shown.
    pub fn is_noop(&self) -> bool {
        self.original == self.replacement
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Suggestion {
        Suggestion {
            id: "s1".into(),
            span: TextSpan::new(10, 15, 10, 15),
            original: "tehre".into(),
            replacement: "there".into(),
            category: Category::Spelling,
            severity: Severity::High,
            explanation: "Misspelling of 'there'.".into(),
            resolution: Resolution::Exact,
        }
    }

    #[test]
    fn suggestion_round_trips_through_json() {
        let original = sample();
        let json = serde_json::to_string(&original).expect("serialize");
        let decoded: Suggestion = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, decoded);
    }

    #[test]
    fn category_and_severity_use_snake_case_on_the_wire() {
        assert_eq!(
            serde_json::to_string(&Category::WordChoice).unwrap(),
            "\"word_choice\""
        );
        assert_eq!(serde_json::to_string(&Severity::High).unwrap(), "\"high\"");
    }

    #[test]
    fn severity_ordering_is_low_medium_high() {
        assert!(Severity::Low < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
    }

    #[test]
    fn span_helpers_count_offsets() {
        let span = TextSpan::new(4, 10, 3, 8);
        assert_eq!(span.utf16_len(), 6);
        assert_eq!(span.grapheme_len(), 5);
        assert!(!span.is_empty());
        assert!(TextSpan::new(2, 2, 1, 1).is_empty());
    }

    #[test]
    fn noop_suggestions_are_detected() {
        let mut suggestion = sample();
        suggestion.replacement = suggestion.original.clone();
        assert!(suggestion.is_noop());
    }

    #[test]
    fn every_category_is_listed_once() {
        let mut seen = std::collections::HashSet::new();
        for category in Category::ALL {
            assert!(seen.insert(category), "duplicate category: {category:?}");
        }
        assert_eq!(seen.len(), Category::ALL.len());
    }
}
