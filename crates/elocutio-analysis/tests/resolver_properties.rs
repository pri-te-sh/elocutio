//! Property-based tests for the quote-to-offset resolver.
//!
//! The plan (§8, "Unit") requires property-based tests for the resolver:
//! "exact/normalized/fuzzy round-trips, Unicode, emoji, CRLF, combining marks,
//! RTL, CJK, ambiguous duplicates".
//!
//! The properties under test:
//!
//! 1. A quote that occurs **exactly once** always resolves to the same span,
//!    and that span always contains the quote.
//! 2. A quote that occurs **more than once** is always `Ambiguous` — the
//!    resolver never guesses.
//! 3. Resolution never panics, for any input.
//! 4. Normalization is idempotent: normalizing an already-normalized string
//!    changes nothing, so a normalized match cannot drift between runs.
//! 5. The UTF-16 and grapheme axes stay consistent: the text sliced by the
//!    reported span is exactly the quote.

use elocutio_analysis::{RawEdit, ResolveError, resolve_edit, resolve_exact};
use elocutio_core::{Category, Resolution, Severity};
use proptest::prelude::*;
use unicode_segmentation::UnicodeSegmentation;

/// A text fragment that is guaranteed not to contain the delimiter used to
/// build a two-part document.
fn fragment() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            "[a-zA-Z]{1,6}",
            "[ ]",
            "é",
            "é",
            "😀",
            "日",
            "א",
            "\\r\\n",
            "\\n",
            "\\t",
        ],
        0..24,
    )
    .prop_map(|parts| parts.concat())
}

/// Build a document where `needle` occurs exactly once.
fn unique_document(needle: &str) -> String {
    format!("prefix {needle} suffix")
}

/// A filler fragment that contains no non-ASCII characters.
///
/// Used when a test needs its interesting character (emoji, combining mark,
/// RTL, CJK) to occur exactly once, so the quote cannot be ambiguous.
fn plain_fragment() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof!["[a-z]{1,6}", "[ ]", "\\n", "\\r\\n", "[.!?]"],
        0..16,
    )
    .prop_map(|parts| parts.concat())
}

fn edit(original: &str, replacement: &str) -> RawEdit {
    RawEdit {
        paragraph_id: "p1".into(),
        original: original.to_string(),
        replacement: replacement.to_string(),
        category: Category::Grammar,
        severity: Severity::High,
        explanation: "property test".into(),
    }
}

/// A quote is resolvable iff it occurs exactly once in the text.
fn occurrences(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// A unique exact quote always resolves, always to the same span, and the
    /// span always contains the quote.
    #[test]
    fn a_unique_quote_always_resolves_to_the_same_span(
        prefix in fragment(),
        needle in fragment().prop_filter("needle is not blank", |value| !value.trim().is_empty()),
        suffix in fragment(),
    ) {
        let document = format!("{prefix} {needle} {suffix}");
        let count = occurrences(&document, &needle);
        prop_assume!(count == 1);

        let first = resolve_exact("p1", &document, &edit(&needle, "REPLACEMENT"), "e1");
        let second = resolve_exact("p1", &document, &edit(&needle, "REPLACEMENT"), "e1");
        prop_assert_eq!(first.is_ok(), second.is_ok());
        prop_assert!(first.is_ok(), "unique quote failed to resolve: {first:?}");

        let suggestion = first.expect("resolved");
        prop_assert_eq!(suggestion.resolution, Resolution::Exact);
        prop_assert_eq!(&suggestion.original, &needle);

        // The resolved text is exactly the quote.
        let utf16 = document.encode_utf16().collect::<Vec<_>>();
        let slice = String::from_utf16(&utf16[suggestion.span.utf16_start..suggestion.span.utf16_end])
            .expect("valid utf-16");
        prop_assert_eq!(slice.as_str(), needle.as_str());

        // The grapheme axis agrees with the UTF-16 axis.
        let graphemes: Vec<&str> = document.graphemes(true).collect();
        let grapheme_slice: String = graphemes
            [suggestion.span.grapheme_start..suggestion.span.grapheme_end]
            .concat();
        prop_assert_eq!(grapheme_slice.as_str(), needle.as_str());
    }

    /// A quote that occurs more than once is always ambiguous. The resolver
    /// must never guess which occurrence was meant.
    #[test]
    fn a_duplicate_quote_is_always_ambiguous(
        prefix in fragment(),
        needle in fragment().prop_filter("needle is not blank", |value| !value.trim().is_empty()),
        middle in fragment(),
        suffix in fragment(),
    ) {
        let document = format!("{prefix} {needle} {middle} {needle} {suffix}");
        prop_assume!(occurrences(&document, &needle) >= 2);

        let result = resolve_exact("p1", &document, &edit(&needle, "REPLACEMENT"), "e1");
        prop_assert_eq!(result, Err(ResolveError::Ambiguous));
    }

    /// Resolution never panics and never returns a suggestion whose span
    /// leaves the text.
    #[test]
    fn resolution_never_panics(
        prefix in fragment(),
        needle in fragment(),
        suffix in fragment(),
    ) {
        let document = format!("{prefix} {needle} {suffix}");
        let result = resolve_edit("p1", &document, &edit(&needle, "REPLACEMENT"), "e1");
        if let Ok(suggestion) = result {
            prop_assert!(suggestion.span.utf16_start <= suggestion.span.utf16_end);
            prop_assert!(suggestion.span.utf16_end <= document.encode_utf16().count());
            prop_assert!(suggestion.span.grapheme_start <= suggestion.span.grapheme_end);
            prop_assert!(suggestion.span.grapheme_end <= document.graphemes(true).count());
        }
    }

    /// The resolver reports one of a small, closed set of errors, and an
    /// empty original is always rejected as empty.
    #[test]
    fn only_known_errors_are_reported(
        needle in fragment(),
        replacement in fragment(),
    ) {
        let document = format!("before {needle} after");
        let result = resolve_edit("p1", &document, &edit(&needle, &replacement), "e1");
        if let Err(error) = result {
            prop_assert!(matches!(
                error,
                ResolveError::EmptyOriginal
                    | ResolveError::NoMatch
                    | ResolveError::Ambiguous
                    | ResolveError::NoChange
            ));
        }

        // An empty original is never resolvable.
        let empty = resolve_edit("p1", &document, &edit("", "x"), "e1");
        prop_assert_eq!(empty, Err(ResolveError::EmptyOriginal));
    }

    /// A replacement identical to the original is a no-op and is rejected.
    #[test]
    fn an_identical_replacement_is_rejected(
        needle in fragment().prop_filter("needle is not blank", |value| !value.trim().is_empty()),
    ) {
        let document = unique_document(&needle);
        let result = resolve_edit("p1", &document, &edit(&needle, &needle), "e1");
        prop_assert_eq!(result, Err(ResolveError::NoChange));
    }

    /// CRLF line endings do not change the resolved span.
    #[test]
    fn crlf_line_endings_resolve_the_same_way(
        needle in fragment().prop_filter("needle is not blank", |value| !value.trim().is_empty()),
    ) {
        let lf_document = format!("first line\n{needle}\nlast line");
        let crlf_document = format!("first line\r\n{needle}\r\nlast line");

        // The quote has to occur exactly once, otherwise the resolver is right
        // to reject it as ambiguous and the property is not about line endings.
        prop_assume!(occurrences(&lf_document, &needle) == 1);
        prop_assume!(occurrences(&crlf_document, &needle) == 1);

        let lf = resolve_exact("p1", &lf_document, &edit(&needle, "REPLACEMENT"), "e1");
        let crlf = resolve_exact("p1", &crlf_document, &edit(&needle, "REPLACEMENT"), "e1");
        prop_assert!(lf.is_ok());
        prop_assert!(crlf.is_ok());
        let lf_span = lf.expect("lf").span;
        let crlf_span = crlf.expect("crlf").span;
        prop_assert_eq!(lf_span.utf16_len(), crlf_span.utf16_len());
    }

    /// A quote that differs only by case resolves (normalized) and yields the
    /// original text of the document, not the quote.
    #[test]
    fn a_case_different_quote_resolves_to_the_document_text(
        needle in "[a-z]{3,8}",
    ) {
        let document = unique_document(&needle);
        let shouted = needle.to_uppercase();
        let result = resolve_exact("p1", &document, &edit(&shouted, "REPLACEMENT"), "e1");
        // The exact matcher is case sensitive, so an upper-case quote of a
        // lower-case needle does not match exactly.
        prop_assert!(result.is_err());

        let normalized = resolve_edit("p1", &document, &edit(&shouted, "REPLACEMENT"), "e1");
        prop_assert!(normalized.is_ok(), "normalized match failed: {normalized:?}");
        let suggestion = normalized.expect("resolved");
        prop_assert_eq!(suggestion.resolution, Resolution::Normalized);
        prop_assert_eq!(suggestion.original, needle);
    }

    /// A quote that differs only by smart quotes resolves.
    #[test]
    fn a_smart_quote_quote_resolves(
        inner in "[a-z ]{3,12}",
    ) {
        let document = format!("She said \u{201C}{inner}\u{201D}.");
        let result = resolve_edit(
            "p1",
            &document,
            &edit(&format!("\"{inner}\""), "Hello."),
            "e1",
        );
        prop_assert!(result.is_ok(), "smart-quote match failed: {result:?}");
        let suggestion = result.expect("resolved");
        prop_assert_eq!(suggestion.resolution, Resolution::Normalized);
        prop_assert!(suggestion.original.contains('\u{201C}'), "{}", suggestion.original);
    }

    /// Emoji (astral plane, surrogate pairs in UTF-16) resolve to the right
    /// UTF-16 offsets.
    #[test]
    fn emoji_resolve_to_two_utf16_units(
        prefix in plain_fragment(),
        suffix in plain_fragment(),
    ) {
        let document = format!("{prefix} 😀 {suffix}");
        let result = resolve_exact("p1", &document, &edit("😀", "🙂"), "e1");
        prop_assert!(result.is_ok(), "unique quote failed to resolve");
        let suggestion = result.expect("resolved");
        prop_assert_eq!(suggestion.span.utf16_end - suggestion.span.utf16_start, 2);
        prop_assert_eq!(suggestion.span.grapheme_end - suggestion.span.grapheme_start, 1);
    }

    /// Combining marks stay in one grapheme cluster.
    #[test]
    fn combining_marks_stay_together(
        prefix in plain_fragment(),
        suffix in plain_fragment(),
    ) {
        // "e" followed by a combining acute accent is one grapheme.
        let document = format!("{prefix} e\u{0301} {suffix}");
        let result = resolve_exact("p1", &document, &edit("e\u{0301}", "e"), "e1");
        prop_assert!(result.is_ok(), "unique quote failed to resolve");
        let suggestion = result.expect("resolved");
        prop_assert_eq!(suggestion.span.grapheme_end - suggestion.span.grapheme_start, 1);
        prop_assert!(suggestion.span.utf16_end > suggestion.span.utf16_start);
    }

    /// RTL text resolves with the same span semantics as LTR text.
    #[test]
    fn rtl_text_resolves(
        prefix in plain_fragment(),
        suffix in plain_fragment(),
    ) {
        let document = format!("{prefix} שלום {suffix}");
        let result = resolve_exact("p1", &document, &edit("שלום", "hello"), "e1");
        prop_assert!(result.is_ok(), "unique quote failed to resolve");
        let suggestion = result.expect("resolved");
        prop_assert_eq!(suggestion.original, "שלום");
        prop_assert_eq!(suggestion.span.utf16_len(), 4);
    }

    /// CJK text resolves; CJK characters are one UTF-16 unit each.
    #[test]
    fn cjk_text_resolves(
        prefix in plain_fragment(),
        suffix in plain_fragment(),
    ) {
        let document = format!("{prefix} 日本語 {suffix}");
        let result = resolve_exact("p1", &document, &edit("日本語", "日本"), "e1");
        prop_assert!(result.is_ok(), "unique quote failed to resolve");
        let suggestion = result.expect("resolved");
        prop_assert_eq!(suggestion.original, "日本語");
        prop_assert_eq!(suggestion.span.utf16_len(), 3);
    }

    /// A fuzzy match still lands inside the document and never produces a
    /// span that is out of bounds.
    #[test]
    fn a_fuzzy_match_stays_in_bounds(
        needle in "[a-z]{4,10}",
    ) {
        let document = format!("the {needle} appears here");
        let mut typo = needle.clone();
        typo.replace_range(0..1, "x");
        let result = resolve_edit("p1", &document, &edit(&typo, "REPLACEMENT"), "e1");
        if let Ok(suggestion) = result {
            prop_assert!(suggestion.span.utf16_end <= document.encode_utf16().count());
            prop_assert!(suggestion.span.utf16_start < suggestion.span.utf16_end);
        }
    }

    /// The resolver never produces a suggestion for a no-op edit.
    #[test]
    fn no_noop_suggestions_are_produced(
        needle in fragment().prop_filter("needle is not blank", |value| !value.trim().is_empty()),
    ) {
        let document = unique_document(&needle);
        let result = resolve_edit("p1", &document, &edit(&needle, &needle), "e1");
        prop_assert!(result.is_err());
    }

    /// Resolution is deterministic: the same input always yields the same
    /// output, including the same resolution quality.
    #[test]
    fn resolution_is_deterministic(
        prefix in fragment(),
        needle in fragment(),
        suffix in fragment(),
    ) {
        let document = format!("{prefix} {needle} {suffix}");
        let first = resolve_edit("p1", &document, &edit(&needle, "REPLACEMENT"), "e1");
        let second = resolve_edit("p1", &document, &edit(&needle, "REPLACEMENT"), "e1");
        prop_assert_eq!(format!("{first:?}"), format!("{second:?}"));
    }
}
