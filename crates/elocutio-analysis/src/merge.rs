//! Merge local lints with LLM suggestions and rank the result.
//!
//! The app has two sources of edits: the local lint engine and the model.
//! They frequently propose the same fix, or two different fixes for the same
//! words. Silently dropping one loses information, and applying both corrupts
//! the text.
//!
//! This module implements plan §5.5 step 5 — "Dedupes overlaps, preferring
//! severity → resolution quality → interior spans" — with two additions the
//! plan's principles require:
//!
//! * **No silent drops.** Every suggestion that loses a merge is reported in
//!   a [`MergeReport`] with the reason, so the UI can explain it (principle:
//!   "Explain every edit").
//! * **Local precedence.** A local lint is a deterministic, verified rule; an
//!   LLM suggestion is a guess. When they collide, the local lint wins.
//!
//! Overlap is detected on **both** the UTF-16 and the grapheme axis, because
//! a span can be empty on one axis while still covering visible text on the
//! other.
//!
//! Model suggestions straight out of [`crate::parse_response`] carry
//! paragraph-relative spans and ids of the form `p3:e7`. Two such suggestions
//! from different paragraphs are never compared, since equal offsets in two
//! paragraphs are different text.

use elocutio_core::{Resolution, Severity, Suggestion, TextSpan};
use serde::{Deserialize, Serialize};

use crate::parse::RejectedEdit;

/// The maximum number of suggestions the app shows at once.
pub const DEFAULT_SUGGESTION_LIMIT: usize = 20;

/// Where a suggestion came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionSource {
    /// A deterministic rule from the local lint engine.
    Local,
    /// A suggestion produced by the model.
    Llm,
}

impl SuggestionSource {
    /// Human-readable label used in drop reports.
    pub const fn label(self) -> &'static str {
        match self {
            SuggestionSource::Local => "local",
            SuggestionSource::Llm => "llm",
        }
    }
}

/// Why a suggestion was dropped during a merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    /// The suggestion overlapped a higher-ranked suggestion that was kept.
    Overlap,
    /// The suggestion was an exact duplicate of a kept suggestion.
    Duplicate,
    /// The suggestion fell outside the display cap.
    OverLimit,
}

impl DropReason {
    /// A stable, human-readable explanation.
    pub const fn explanation(self) -> &'static str {
        match self {
            DropReason::Overlap => "overlaps a higher-ranked suggestion",
            DropReason::Duplicate => "duplicate of a higher-ranked suggestion",
            DropReason::OverLimit => "exceeded the suggestion limit",
        }
    }
}

/// A suggestion that did not make it into the merged list.
///
/// Kept (rather than silently discarded) so the UI can say *why* an edit is
/// missing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroppedSuggestion {
    /// The suggestion that lost.
    pub suggestion: Suggestion,
    /// Where it came from.
    pub source: SuggestionSource,
    /// Why it lost.
    pub reason: DropReason,
    /// The id of the suggestion that beat it, when there was one.
    pub kept_id: Option<String>,
    /// A human-readable explanation.
    pub detail: String,
}

/// The outcome of a merge: what was kept, and what was dropped and why.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MergeReport {
    /// Suggestions that survived the merge, in final ranked order.
    pub kept: Vec<Suggestion>,
    /// Suggestions that were dropped, with the reason for each.
    pub dropped: Vec<DroppedSuggestion>,
}

impl MergeReport {
    /// How many suggestions were dropped.
    pub fn dropped_len(&self) -> usize {
        self.dropped.len()
    }

    /// The ids of the dropped suggestions, in drop order.
    pub fn dropped_ids(&self) -> Vec<&str> {
        self.dropped
            .iter()
            .map(|entry| entry.suggestion.id.as_str())
            .collect()
    }
}

/// The result of merging local and LLM suggestions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MergeResult {
    /// The merged, ranked suggestions.
    pub suggestions: Vec<Suggestion>,
    /// What was dropped and why.
    pub report: MergeReport,
}

impl MergeResult {
    /// The kept suggestions, without the report.
    pub fn into_suggestions(self) -> Vec<Suggestion> {
        self.suggestions
    }

    /// The dropped suggestions, without the kept list.
    pub fn into_dropped(self) -> Vec<DroppedSuggestion> {
        self.report.dropped
    }
}

/// The ranking key of a suggestion.
///
/// Ordering is: severity (High first) → resolution quality (Exact first) →
/// interior spans before edge spans → span start. Everything is a total
/// order, so the merge is deterministic for any input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    /// `Higher severity first` — encoded so a plain `Ord` sorts correctly.
    severity: u8,
    /// `Better resolution first`.
    resolution: u8,
    /// `Interior span first` (0 = interior, 1 = edge), so a plain `Ord` puts
    /// interior spans first.
    edge: u8,
    /// `Earlier span first`.
    start: usize,
    /// `Earlier end first` — only a tiebreaker, but it makes the order total.
    end: usize,
    /// `Shorter span first` — final tiebreaker.
    len: usize,
}

/// Is the span an "interior" span, i.e. one that is not anchored at the very
/// start or end of the document?
///
/// A span that starts at offset 0 or ends at the document end sits on a
/// document edge, where edits are more likely to be boundary artefacts, so an
/// equally severe and equally well-resolved interior span is preferred.
///
/// `document_end` is the length of the document in UTF-16 units. When it is
/// `None` the document length is unknown and only the start-anchor rule
/// applies.
fn is_interior(span: &TextSpan, document_end: Option<usize>) -> bool {
    let starts_at_edge = span.utf16_start == 0 || span.grapheme_start == 0;
    // `document_end` is a UTF-16 length, so only the UTF-16 end is compared;
    // a grapheme count is smaller for any text with surrogate pairs.
    let ends_at_edge = document_end.is_some_and(|end| span.utf16_end >= end);
    !(starts_at_edge || ends_at_edge)
}

/// Compute the ranking key of a suggestion.
fn rank_of(suggestion: &Suggestion, document_end: Option<usize>) -> Rank {
    Rank {
        severity: suggestion.severity.reverse_rank(),
        resolution: suggestion.resolution.rank(),
        edge: u8::from(!is_interior(&suggestion.span, document_end)),
        start: suggestion.span.utf16_start,
        end: suggestion.span.utf16_end,
        len: suggestion.span.utf16_len(),
    }
}

/// Rank a list of suggestions best-first.
///
/// The result is stable and deterministic: two runs over the same input
/// always produce the same order.
///
/// The document length is not known here, so only a span at the very start
/// counts as an edge span. (The furthest suggestion end is not the document
/// end; using it would demote the last suggestion for no reason.) Use
/// [`rank_suggestions_within`] when the length is known.
pub fn rank_suggestions(suggestions: &[Suggestion]) -> Vec<Suggestion> {
    rank_suggestions_within(suggestions, None)
}

/// Rank a list of suggestions, treating `document_end` as the end of the
/// document when deciding whether a span is an edge span.
///
/// `document_end` is expressed in UTF-16 units.
pub fn rank_suggestions_within(
    suggestions: &[Suggestion],
    document_end: Option<usize>,
) -> Vec<Suggestion> {
    let mut ranked: Vec<&Suggestion> = suggestions.iter().collect();
    ranked.sort_by(|a, b| {
        rank_of(a, document_end)
            .cmp(&rank_of(b, document_end))
            .then_with(|| a.id.cmp(&b.id))
    });
    ranked.into_iter().cloned().collect()
}

/// Do two suggestions cover the same text with the same replacement?
///
/// Duplicate detection is stricter than overlap detection: it also compares
/// the proposed replacement, so two edits that change the same words in the
/// same way are recognised as the same suggestion.
fn is_duplicate(a: &Suggestion, b: &Suggestion) -> bool {
    a.span.same_range(&b.span) && a.replacement == b.replacement
}

/// The paragraph a resolver-made suggestion id (`p3:e7`) belongs to.
///
/// Ids of any other shape (local lints, hand-made ids) have no paragraph
/// scope and are compared with every suggestion.
fn paragraph_scope(id: &str) -> Option<&str> {
    let (paragraph, edit) = id.split_once(':')?;
    let numbered = |value: &str, prefix: char| {
        value
            .strip_prefix(prefix)
            .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
    };
    (numbered(paragraph, 'p') && numbered(edit, 'e')).then_some(paragraph)
}

/// Can the two suggestions' spans be compared at all?
///
/// Spans from two different paragraphs are each relative to their own
/// paragraph, so equal offsets do not mean the same text.
fn share_coordinates(a: &Suggestion, b: &Suggestion) -> bool {
    match (paragraph_scope(&a.id), paragraph_scope(&b.id)) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// Merge local lint suggestions with LLM suggestions.
///
/// * Local suggestions take precedence: when a local lint and an LLM
///   suggestion overlap, the local lint is kept and the LLM one is dropped
///   with a reason.
/// * Among equals the ranking is severity → resolution quality → interior
///   spans → span start.
/// * Overlap is detected on both the UTF-16 and the grapheme axis.
/// * Nothing is dropped silently: every dropped suggestion appears in the
///   returned [`MergeReport`].
///
/// The returned suggestions are in ranked order (best first).
pub fn merge(local: &[Suggestion], llm: &[Suggestion]) -> Vec<Suggestion> {
    merge_with_limit(local, llm, DEFAULT_SUGGESTION_LIMIT).suggestions
}

/// Merge local and LLM suggestions, reporting everything that was dropped.
///
/// This is the report-bearing form of [`merge`]. `limit` caps how many
/// suggestions are kept; `0` means "no cap".
///
/// The document length is unknown here, so only spans at the very start
/// count as edge spans; see [`merge_with_limit_within`].
pub fn merge_with_limit(local: &[Suggestion], llm: &[Suggestion], limit: usize) -> MergeResult {
    merge_with_limit_within(local, llm, limit, None)
}

/// [`merge_with_limit`] for a document whose length is known.
///
/// `document_end` is the document length in UTF-16 units; a span that ends
/// there is an edge span for ranking.
pub fn merge_with_limit_within(
    local: &[Suggestion],
    llm: &[Suggestion],
    limit: usize,
    document_end: Option<usize>,
) -> MergeResult {
    let mut kept: Vec<(Suggestion, SuggestionSource)> = Vec::new();
    let mut dropped: Vec<DroppedSuggestion> = Vec::new();

    // Local lints are resolved first: a verified rule always beats a guess.
    for source in [SuggestionSource::Local, SuggestionSource::Llm] {
        let candidates: Vec<&Suggestion> = match source {
            SuggestionSource::Local => local.iter().collect(),
            SuggestionSource::Llm => llm.iter().collect(),
        };

        // Within one source, the higher-ranked suggestion wins the overlap.
        let mut ordered: Vec<&Suggestion> = candidates;
        ordered.sort_by(|a, b| {
            rank_of(a, document_end)
                .cmp(&rank_of(b, document_end))
                .then_with(|| a.id.cmp(&b.id))
        });

        for suggestion in ordered {
            let conflict = kept.iter().find(|(existing, _)| {
                share_coordinates(existing, suggestion)
                    && (is_duplicate(existing, suggestion)
                        || existing.span.overlaps(&suggestion.span))
            });

            if let Some((existing, _)) = conflict {
                let reason = if is_duplicate(existing, suggestion) {
                    DropReason::Duplicate
                } else {
                    DropReason::Overlap
                };
                dropped.push(DroppedSuggestion {
                    detail: format!(
                        "{} suggestion {} was dropped: it {} (kept {})",
                        source.label(),
                        suggestion.id,
                        reason.explanation(),
                        existing.id
                    ),
                    suggestion: suggestion.clone(),
                    source,
                    reason,
                    kept_id: Some(existing.id.clone()),
                });
                continue;
            }

            kept.push((suggestion.clone(), source));
        }
    }

    // Global ranking for display: severity, resolution, interior, position.
    // Local lints win exact ties, so the order is fully deterministic.
    kept.sort_by(|(a, source_a), (b, source_b)| {
        rank_of(a, document_end)
            .cmp(&rank_of(b, document_end))
            .then_with(|| source_a.cmp(source_b))
            .then_with(|| a.id.cmp(&b.id))
    });

    let kept_suggestions: Vec<Suggestion> = kept.into_iter().map(|(s, _)| s).collect();

    // Apply the display cap last, so the cap never changes which suggestion
    // wins an overlap.
    let (kept_suggestions, capped) = apply_limit(kept_suggestions, limit);
    dropped.extend(capped);

    MergeResult {
        report: MergeReport {
            kept: kept_suggestions.clone(),
            dropped,
        },
        suggestions: kept_suggestions,
    }
}

/// Keep at most `limit` suggestions, reporting the rest as dropped.
///
/// `limit == 0` means "keep everything".
pub fn apply_limit(
    suggestions: Vec<Suggestion>,
    limit: usize,
) -> (Vec<Suggestion>, Vec<DroppedSuggestion>) {
    if limit == 0 || suggestions.len() <= limit {
        return (suggestions, Vec::new());
    }

    let total = suggestions.len();
    let mut kept: Vec<Suggestion> = Vec::with_capacity(limit);
    let mut dropped: Vec<DroppedSuggestion> = Vec::new();

    for (index, suggestion) in suggestions.into_iter().enumerate() {
        if index < limit {
            kept.push(suggestion);
        } else {
            dropped.push(DroppedSuggestion {
                detail: format!(
                    "suggestion {} was dropped: it exceeded the suggestion limit of {limit} (rank {index} of {total})",
                    suggestion.id
                ),
                suggestion,
                source: SuggestionSource::Llm,
                reason: DropReason::OverLimit,
                kept_id: None,
            });
        }
    }

    (kept, dropped)
}

/// Cap a list of suggestions at `limit`, reporting what was dropped.
///
/// The suggestions are assumed to already be in the desired display order;
/// this function does not re-rank them.
///
/// The dropped suggestions are returned as [`RejectedEdit`]s so the existing
/// rejection channel (which the app already renders) can carry them. The
/// reason string explains the drop, so nothing disappears silently.
pub fn cap_suggestions(
    suggestions: Vec<Suggestion>,
    limit: usize,
) -> (Vec<Suggestion>, Vec<RejectedEdit>) {
    if limit == 0 || suggestions.len() <= limit {
        return (suggestions, Vec::new());
    }

    let total = suggestions.len();
    let mut kept: Vec<Suggestion> = Vec::with_capacity(limit);
    let mut rejected: Vec<RejectedEdit> = Vec::new();

    for (index, suggestion) in suggestions.into_iter().enumerate() {
        if index < limit {
            kept.push(suggestion);
        } else {
            rejected.push(RejectedEdit {
                paragraph_id: paragraph_id_of(&suggestion.id),
                original: suggestion.original.clone(),
                reason: format!(
                    "dropped: exceeded the suggestion limit of {limit} (rank {index} of {total})"
                ),
            });
        }
    }

    (kept, rejected)
}

/// Extract the paragraph id from a suggestion id of the form `p3:e7`.
///
/// Returns the whole id when it does not follow that shape.
fn paragraph_id_of(id: &str) -> String {
    match id.split_once(':') {
        Some((paragraph_id, _)) if !paragraph_id.is_empty() => paragraph_id.to_string(),
        _ => id.to_string(),
    }
}

/// The ranking key of a suggestion, exposed so the UI can show *why* a
/// suggestion was ranked where it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankExplanation {
    /// The severity, as reported.
    pub severity: Severity,
    /// The resolution quality, as reported.
    pub resolution: Resolution,
    /// Whether the span is an interior span.
    pub interior: bool,
    /// A one-line, human-readable summary of the ordering key.
    pub reason: String,
}

/// Explain the ranking of a suggestion relative to the document.
pub fn explain_rank(suggestion: &Suggestion, document_end: Option<usize>) -> RankExplanation {
    let interior = is_interior(&suggestion.span, document_end);
    let reason = format!(
        "severity {} > resolution {} > {} span > offset {}",
        match suggestion.severity {
            Severity::High => "high",
            Severity::Medium => "medium",
            Severity::Low => "low",
        },
        match suggestion.resolution {
            Resolution::Exact => "exact",
            Resolution::Normalized => "normalized",
            Resolution::Fuzzy => "fuzzy",
        },
        if interior { "interior" } else { "edge" },
        suggestion.span.utf16_start
    );
    RankExplanation {
        severity: suggestion.severity,
        resolution: suggestion.resolution,
        interior,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elocutio_core::Category;

    fn suggestion(
        id: &str,
        start: usize,
        end: usize,
        severity: Severity,
        resolution: Resolution,
    ) -> Suggestion {
        Suggestion {
            id: id.into(),
            span: TextSpan::new(start, end, start, end),
            original: "x".into(),
            replacement: "y".into(),
            category: Category::Grammar,
            severity,
            explanation: "test".into(),
            resolution,
            confidence: resolution.confidence(),
        }
    }

    #[test]
    fn local_wins_when_it_overlaps_an_llm_suggestion() {
        let local = suggestion("local:1", 0, 5, Severity::Low, Resolution::Fuzzy);
        let llm = suggestion("llm:1", 0, 5, Severity::High, Resolution::Exact);
        let merged = merge(std::slice::from_ref(&local), std::slice::from_ref(&llm));
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "local:1");
    }

    #[test]
    fn llm_fills_gaps_between_local_suggestions() {
        // Neither span starts at offset 0, so both are interior and the
        // order is by position.
        let local = suggestion("local:1", 1, 5, Severity::High, Resolution::Exact);
        let llm = suggestion("llm:1", 10, 15, Severity::High, Resolution::Exact);
        let merged = merge(std::slice::from_ref(&local), std::slice::from_ref(&llm));
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "local:1");
        assert_eq!(merged[1].id, "llm:1");
    }

    #[test]
    fn severity_outranks_resolution_quality() {
        let fuzzy_high = suggestion("a", 0, 5, Severity::High, Resolution::Fuzzy);
        let exact_low = suggestion("b", 10, 15, Severity::Low, Resolution::Exact);
        let merged = merge(&[], &[fuzzy_high, exact_low]);
        assert_eq!(merged[0].id, "a");
    }

    #[test]
    fn resolution_quality_breaks_equal_severity_ties() {
        let fuzzy = suggestion("a", 0, 5, Severity::Medium, Resolution::Fuzzy);
        let exact = suggestion("b", 10, 15, Severity::Medium, Resolution::Exact);
        let merged = merge(&[], &[fuzzy, exact]);
        assert_eq!(merged[0].id, "b");
    }

    #[test]
    fn interior_spans_beat_edge_spans_at_equal_severity_and_resolution() {
        let edge = suggestion("edge", 0, 5, Severity::Medium, Resolution::Exact);
        let interior = suggestion("interior", 10, 15, Severity::Medium, Resolution::Exact);
        // The document is longer than both spans, so only the edge span is
        // anchored to a document boundary.
        let ranked = rank_suggestions_within(&[edge, interior], Some(100));
        assert_eq!(ranked[0].id, "interior");
    }

    #[test]
    fn a_span_at_the_document_end_is_an_edge_span() {
        let interior = suggestion("interior", 10, 15, Severity::Medium, Resolution::Exact);
        let at_end = suggestion("at_end", 20, 25, Severity::Medium, Resolution::Exact);
        let ranked = rank_suggestions_within(&[at_end, interior], Some(25));
        assert_eq!(ranked[0].id, "interior");
    }

    #[test]
    fn span_start_breaks_remaining_ties() {
        let late = suggestion("late", 20, 25, Severity::Medium, Resolution::Exact);
        let early = suggestion("early", 5, 10, Severity::Medium, Resolution::Exact);
        let ranked = rank_suggestions_within(&[late, early], Some(100));
        assert_eq!(ranked[0].id, "early");
    }

    #[test]
    fn overlap_is_detected_on_the_grapheme_axis() {
        let mut local = suggestion("local:1", 0, 0, Severity::High, Resolution::Exact);
        // A span that is empty on UTF-16 but covers two graphemes.
        local.span = TextSpan::new(0, 0, 4, 6);
        let mut llm = suggestion("llm:1", 0, 0, Severity::Low, Resolution::Exact);
        llm.span = TextSpan::new(0, 0, 5, 7);
        let merged = merge(std::slice::from_ref(&local), std::slice::from_ref(&llm));
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "local:1");
    }

    #[test]
    fn adjacent_spans_are_not_overlapping() {
        let first = suggestion("a", 0, 5, Severity::High, Resolution::Exact);
        let second = suggestion("b", 5, 10, Severity::High, Resolution::Exact);
        let merged = merge(&[], &[first, second]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn the_report_names_every_dropped_suggestion() {
        let mut local = suggestion("local:1", 0, 5, Severity::High, Resolution::Exact);
        local.replacement = "local fix".into();
        let mut llm = suggestion("llm:1", 0, 5, Severity::Low, Resolution::Exact);
        llm.replacement = "llm fix".into();
        let result = merge_with_limit(&[local], &[llm], 20);
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.report.dropped.len(), 1);
        let dropped = &result.report.dropped[0];
        assert_eq!(dropped.suggestion.id, "llm:1");
        assert_eq!(dropped.reason, DropReason::Overlap);
        assert_eq!(dropped.kept_id.as_deref(), Some("local:1"));
        assert!(dropped.detail.contains("llm:1"));
    }

    #[test]
    fn exact_duplicates_are_reported_as_duplicates() {
        let first = suggestion("a", 0, 5, Severity::High, Resolution::Exact);
        let second = suggestion("b", 0, 5, Severity::High, Resolution::Exact);
        let result = merge_with_limit(&[first], &[second], 20);
        assert_eq!(result.report.dropped[0].reason, DropReason::Duplicate);
    }

    #[test]
    fn overlapping_llm_suggestions_keep_the_higher_ranked_one() {
        let fuzzy = suggestion("fuzzy", 0, 10, Severity::Medium, Resolution::Fuzzy);
        let exact = suggestion("exact", 0, 10, Severity::Medium, Resolution::Exact);
        let result = merge_with_limit(&[], &[fuzzy, exact], 20);
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].id, "exact");
        assert_eq!(result.report.dropped[0].suggestion.id, "fuzzy");
    }

    #[test]
    fn the_limit_is_applied_after_the_overlap_pass() {
        let suggestions: Vec<Suggestion> = (0..5)
            .map(|index| {
                suggestion(
                    &format!("s{index}"),
                    index * 10,
                    index * 10 + 5,
                    Severity::High,
                    Resolution::Exact,
                )
            })
            .collect();
        let result = merge_with_limit(&[], &suggestions, 3);
        assert_eq!(result.suggestions.len(), 3);
        assert_eq!(result.report.dropped.len(), 2);
        assert!(
            result
                .report
                .dropped
                .iter()
                .all(|entry| entry.reason == DropReason::OverLimit)
        );
    }

    #[test]
    fn cap_suggestions_keeps_the_head_and_reports_the_tail() {
        let suggestions: Vec<Suggestion> = (0..25)
            .map(|index| {
                suggestion(
                    &format!("p1:e{index}"),
                    index * 10,
                    index * 10 + 5,
                    Severity::High,
                    Resolution::Exact,
                )
            })
            .collect();
        let (kept, rejected) = cap_suggestions(suggestions, 20);
        assert_eq!(kept.len(), 20);
        assert_eq!(rejected.len(), 5);
        assert!(rejected.iter().all(|entry| entry.reason.contains("limit")));
        assert_eq!(rejected[0].paragraph_id, "p1");
    }

    #[test]
    fn cap_suggestions_is_a_noop_below_the_limit() {
        let suggestions = vec![suggestion("p1:e1", 0, 5, Severity::High, Resolution::Exact)];
        let (kept, rejected) = cap_suggestions(suggestions, 20);
        assert_eq!(kept.len(), 1);
        assert!(rejected.is_empty());
    }

    #[test]
    fn ranking_is_stable_across_input_orderings() {
        let a = suggestion("a", 0, 5, Severity::High, Resolution::Exact);
        let b = suggestion("b", 10, 15, Severity::Medium, Resolution::Exact);
        let c = suggestion("c", 20, 25, Severity::Low, Resolution::Fuzzy);
        let first = rank_suggestions(&[a.clone(), b.clone(), c.clone()]);
        let second = rank_suggestions(&[c, b, a]);
        assert_eq!(first, second);
    }

    #[test]
    fn rank_explanations_name_the_ordering_key() {
        let suggestion = suggestion("a", 10, 15, Severity::High, Resolution::Exact);
        let explanation = explain_rank(&suggestion, Some(100));
        assert!(explanation.interior);
        assert!(explanation.reason.contains("high"));
        assert!(explanation.reason.contains("exact"));
    }

    #[test]
    fn suggestions_from_different_paragraphs_are_not_compared() {
        // Paragraph-relative spans: "Teh" at 0..3 in p1, "Thier" at 0..5 in p2.
        let first = suggestion("p1:e1", 0, 3, Severity::High, Resolution::Exact);
        let second = suggestion("p2:e1", 0, 5, Severity::High, Resolution::Exact);
        let result = merge_with_limit(&[], &[first, second], 20);
        assert_eq!(result.suggestions.len(), 2);
        assert!(result.report.dropped.is_empty());

        // Within one paragraph, and against unscoped ids, overlap still counts.
        let same = suggestion("p1:e2", 1, 4, Severity::Low, Resolution::Exact);
        let local = suggestion("local:1", 0, 2, Severity::High, Resolution::Exact);
        let base = suggestion("p1:e1", 0, 3, Severity::High, Resolution::Exact);
        assert_eq!(
            merge_with_limit(&[], &[base.clone(), same], 20)
                .suggestions
                .len(),
            1
        );
        assert_eq!(merge_with_limit(&[local], &[base], 20).suggestions.len(), 1);
    }

    #[test]
    fn the_last_suggestion_is_not_an_edge_span_without_a_document_length() {
        let early = suggestion("early", 5, 10, Severity::Medium, Resolution::Exact);
        let last = suggestion("last", 20, 25, Severity::Medium, Resolution::Exact);
        // Rank by position only: neither span touches a known document edge.
        let ranked = rank_suggestions(&[last.clone(), early.clone()]);
        assert_eq!(ranked[0].id, "early");
        let merged = merge_with_limit(&[], &[last.clone(), early.clone()], 20);
        assert_eq!(merged.suggestions[0].id, "early");
        // With the real length, a span at the end is an edge span.
        let merged = merge_with_limit_within(&[], &[last, early], 20, Some(25));
        assert_eq!(merged.suggestions[0].id, "early");
    }

    #[test]
    fn the_document_end_is_compared_on_the_utf16_axis_only() {
        // An emoji-heavy span: 4 graphemes but 8 UTF-16 units, in a document
        // of 20 UTF-16 units. It is interior.
        let mut span = suggestion("emoji", 4, 12, Severity::Medium, Resolution::Exact);
        span.span = TextSpan::new(4, 12, 2, 20);
        assert!(explain_rank(&span, Some(20)).interior);
    }

    #[test]
    fn paragraph_ids_are_extracted_from_suggestion_ids() {
        assert_eq!(paragraph_id_of("p3:e7"), "p3");
        assert_eq!(paragraph_id_of("loose"), "loose");
    }
}
