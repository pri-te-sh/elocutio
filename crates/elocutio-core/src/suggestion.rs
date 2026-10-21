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

    /// Rank of the category for merge ordering.
    ///
    /// Lower rank sorts first. Objective categories (grammar, spelling,
    /// punctuation, capitalization) outrank subjective ones (style, tone)
    /// because a wrong objective fix is always wrong, while a subjective
    /// rewrite is a matter of taste.
    pub const fn rank(self) -> u8 {
        match self {
            Category::Grammar => 0,
            Category::Spelling => 1,
            Category::Punctuation => 2,
            Category::Capitalization => 3,
            Category::Clarity => 4,
            Category::Conciseness => 5,
            Category::WordChoice => 6,
            Category::Consistency => 7,
            Category::Formatting => 8,
            Category::Tone => 9,
            Category::Style => 10,
        }
    }

    /// A stable, deterministic ordering across categories.
    ///
    /// Used by the merge ranking so equal-severity suggestions always come out
    /// in the same order.
    pub const fn ordering(self) -> u8 {
        self.rank()
    }
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

impl Severity {
    /// Rank of the severity for merge ordering. Lower sorts first.
    pub const fn rank(self) -> u8 {
        match self {
            Severity::Low => 0,
            Severity::Medium => 1,
            Severity::High => 2,
        }
    }

    /// The reverse rank, so `b.cmp(&a)` sorts high severity first.
    pub const fn reverse_rank(self) -> u8 {
        2 - self.rank()
    }
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

impl Resolution {
    /// Rank of the resolution quality. Lower sorts first, so `Exact` wins.
    pub const fn rank(self) -> u8 {
        match self {
            Resolution::Exact => 0,
            Resolution::Normalized => 1,
            Resolution::Fuzzy => 2,
        }
    }

    /// The confidence the resolver assigns to a span of this quality.
    ///
    /// These are *resolver* confidence values, never model-reported ones. They
    /// describe how sure the client is that the quote points at the intended
    /// text, and are derived solely from the matching strategy that succeeded.
    pub const fn confidence(self) -> f64 {
        match self {
            Resolution::Exact => 1.0,
            Resolution::Normalized => 0.9,
            Resolution::Fuzzy => 0.75,
        }
    }
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

    /// Do two spans overlap on the UTF-16 axis?
    ///
    /// Touching spans (one ends exactly where the other starts) do **not**
    /// overlap: they are adjacent, not competing.
    pub const fn overlaps_utf16(&self, other: &TextSpan) -> bool {
        self.utf16_start < other.utf16_end && other.utf16_start < self.utf16_end
    }

    /// Do two spans overlap on the grapheme axis?
    pub const fn overlaps_grapheme(&self, other: &TextSpan) -> bool {
        self.grapheme_start < other.grapheme_end && other.grapheme_start < self.grapheme_end
    }

    /// Do two spans overlap on *either* axis?
    ///
    /// Both axes are checked because a span can be empty on one of them while
    /// still covering text on the other. Treating "overlaps on either axis" as
    /// an overlap is the conservative choice: it never lets two edits touch
    /// the same visible characters.
    pub const fn overlaps(&self, other: &TextSpan) -> bool {
        self.overlaps_utf16(other) || self.overlaps_grapheme(other)
    }

    /// Do the two spans cover exactly the same range on both axes?
    pub const fn same_range(&self, other: &TextSpan) -> bool {
        self.utf16_start == other.utf16_start
            && self.utf16_end == other.utf16_end
            && self.grapheme_start == other.grapheme_start
            && self.grapheme_end == other.grapheme_end
    }
}

/// A single, minimal edit proposed to the writer.
///
/// Note: this type derives `PartialEq` but **not** `Eq`, because `confidence`
/// is an `f64` and therefore not a total-equality type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: String,
    pub span: TextSpan,
    pub original: String,
    pub replacement: String,
    pub category: Category,
    pub severity: Severity,
    pub explanation: String,
    pub resolution: Resolution,
    /// How sure the **resolver** is that this span points at the intended text.
    ///
    /// This is derived from the resolution quality alone (see
    /// [`Resolution::confidence`]). It is *never* a model-reported confidence:
    /// the model is never asked for one and any value it produced would be
    /// discarded. A value of `0.0` means "unknown"; the field exists so the
    /// merge ranking and the UI can prefer spans the client located
    /// unambiguously over spans it merely guessed.
    ///
    /// Always within `0.0..=1.0`.
    #[serde(default = "default_confidence")]
    pub confidence: f64,
}

impl Suggestion {
    /// A suggestion that changes nothing is never shown.
    pub fn is_noop(&self) -> bool {
        self.original == self.replacement
    }

    /// Clamp a confidence value into the `0.0..=1.0` range.
    ///
    /// NaN maps to `0.0` so a bad value can never poison the ranking.
    pub fn clamp_confidence(value: f64) -> f64 {
        if value.is_nan() {
            0.0
        } else {
            value.clamp(0.0, 1.0)
        }
    }
}

fn default_confidence() -> f64 {
    0.0
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
            confidence: 1.0,
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

    #[test]
    fn category_ranks_are_unique_and_cover_every_category() {
        let mut ranks = std::collections::HashSet::new();
        for category in Category::ALL {
            assert!(
                ranks.insert(category.rank()),
                "duplicate rank for {category:?}"
            );
        }
        assert_eq!(ranks.len(), Category::ALL.len());
        assert!(Category::Grammar.rank() < Category::Style.rank());
    }

    #[test]
    fn severity_ranks_agree_with_derived_ordering() {
        assert!(Severity::High.rank() > Severity::Medium.rank());
        assert!(Severity::Medium.rank() > Severity::Low.rank());
        assert!(Severity::High.reverse_rank() < Severity::Low.reverse_rank());
    }

    #[test]
    fn resolution_ranks_agree_with_derived_ordering() {
        assert!(Resolution::Exact.rank() < Resolution::Normalized.rank());
        assert!(Resolution::Normalized.rank() < Resolution::Fuzzy.rank());
    }

    #[test]
    fn resolution_confidence_is_ordered_and_bounded() {
        assert!(Resolution::Exact.confidence() > Resolution::Normalized.confidence());
        assert!(Resolution::Normalized.confidence() > Resolution::Fuzzy.confidence());
        for resolution in [Resolution::Exact, Resolution::Normalized, Resolution::Fuzzy] {
            let confidence = resolution.confidence();
            assert!(
                (0.0..=1.0).contains(&confidence),
                "{confidence} out of range"
            );
        }
    }

    #[test]
    fn overlapping_spans_detect_overlap_on_both_axes() {
        let a = TextSpan::new(0, 5, 0, 5);
        let b = TextSpan::new(3, 8, 3, 8);
        let c = TextSpan::new(5, 9, 5, 9);
        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
        assert!(!a.overlaps(&c), "adjacent spans do not overlap");
        assert!(!c.overlaps(&a));
    }

    #[test]
    fn same_range_detects_identical_spans() {
        let a = TextSpan::new(2, 6, 1, 4);
        let b = TextSpan::new(2, 6, 1, 4);
        let c = TextSpan::new(2, 6, 1, 5);
        assert!(a.same_range(&b));
        assert!(!a.same_range(&c));
    }

    #[test]
    fn confidence_is_clamped_into_range() {
        assert_eq!(Suggestion::clamp_confidence(1.5), 1.0);
        assert_eq!(Suggestion::clamp_confidence(-0.2), 0.0);
        assert_eq!(Suggestion::clamp_confidence(0.42), 0.42);
        assert_eq!(Suggestion::clamp_confidence(f64::NAN), 0.0);
    }

    #[test]
    fn missing_confidence_deserializes_to_zero() {
        let json = r#"{"id":"s1","span":{"utf16_start":0,"utf16_end":1,"grapheme_start":0,"grapheme_end":1},"original":"a","replacement":"b","category":"grammar","severity":"low","explanation":"x","resolution":"exact"}"#;
        let suggestion: Suggestion = serde_json::from_str(json).expect("deserialize");
        assert_eq!(suggestion.confidence, 0.0);
    }
}
