//! Golden-set quality harness.
//!
//! The plan (§8, "Quality evaluation") requires a labelled golden set and a
//! repeatable way to score a run against it. This module provides the
//! harness: the data model, a JSON loader, and a **pure** metric computation
//! so it can be unit-tested without a network, a model, or a clock.
//!
//! The plan's regression gate is:
//!
//! * F0.5 drops by more than 2 points → fail,
//! * resolution rate drops by more than 5 points → fail,
//! * cost increases by more than 20% → fail.
//!
//! [`GoldenReport::passes_gate`] implements the first two (cost needs a
//! price model, so it is left to the caller and reported as a field).
//!
//! The set is stratified by error type, length, source, and difficulty, and
//! includes **no-error controls** — passages that are already correct. A
//! system that suggests an edit on a control passage is producing a
//! false positive, and the over-edit rate exists to catch exactly that.

use elocutio_core::{Category, Resolution, Severity, Suggestion, TextSpan};
use serde::{Deserialize, Serialize};

/// The error class of a golden item.
///
/// Used to stratify the set so one error type cannot dominate the score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    /// The passage has no errors at all — a no-error control.
    #[default]
    None,
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
    /// More than one kind of error in the same passage.
    Mixed,
}

impl ErrorType {
    /// Every error type, in a stable order.
    pub const ALL: [ErrorType; 13] = [
        ErrorType::None,
        ErrorType::Grammar,
        ErrorType::Spelling,
        ErrorType::Punctuation,
        ErrorType::Capitalization,
        ErrorType::Clarity,
        ErrorType::Conciseness,
        ErrorType::WordChoice,
        ErrorType::Consistency,
        ErrorType::Tone,
        ErrorType::Style,
        ErrorType::Formatting,
        ErrorType::Mixed,
    ];

    /// A short label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            ErrorType::None => "none",
            ErrorType::Grammar => "grammar",
            ErrorType::Spelling => "spelling",
            ErrorType::Punctuation => "punctuation",
            ErrorType::Capitalization => "capitalization",
            ErrorType::Clarity => "clarity",
            ErrorType::Conciseness => "conciseness",
            ErrorType::WordChoice => "word_choice",
            ErrorType::Consistency => "consistency",
            ErrorType::Tone => "tone",
            ErrorType::Style => "style",
            ErrorType::Formatting => "formatting",
            ErrorType::Mixed => "mixed",
        }
    }
}

/// The length bucket of a golden item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthBucket {
    /// A single word or short phrase.
    #[default]
    Short,
    /// One sentence.
    Medium,
    /// A paragraph.
    Long,
}

impl LengthBucket {
    /// Every length bucket, in a stable order.
    pub const ALL: [LengthBucket; 3] = [
        LengthBucket::Short,
        LengthBucket::Medium,
        LengthBucket::Long,
    ];

    /// A short label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            LengthBucket::Short => "short",
            LengthBucket::Medium => "medium",
            LengthBucket::Long => "long",
        }
    }
}

/// Where a golden item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemSource {
    /// Written by the project team.
    #[default]
    Curated,
    /// Taken from a real-world document.
    Corpus,
    /// Copied from a bug report.
    BugReport,
    /// Generated to probe a specific weakness.
    Synthetic,
}

impl ItemSource {
    /// Every source, in a stable order.
    pub const ALL: [ItemSource; 4] = [
        ItemSource::Curated,
        ItemSource::Corpus,
        ItemSource::BugReport,
        ItemSource::Synthetic,
    ];

    /// A short label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            ItemSource::Curated => "curated",
            ItemSource::Corpus => "corpus",
            ItemSource::BugReport => "bug_report",
            ItemSource::Synthetic => "synthetic",
        }
    }
}

/// How hard the item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    /// Obvious to any reader.
    #[default]
    Easy,
    /// Needs thought, but unambiguous.
    Medium,
    /// Genuinely subtle; a model may reasonably miss it.
    Hard,
}

impl Difficulty {
    /// Every difficulty, in a stable order.
    pub const ALL: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];

    /// A short label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "easy",
            Difficulty::Medium => "medium",
            Difficulty::Hard => "hard",
        }
    }
}

/// The expected edit for a golden item.
///
/// A golden item is a passage plus the edits a correct system should
/// propose. `utf16_start`/`utf16_end` locate the text in `text`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpectedEdit {
    /// The verbatim text that should be replaced.
    pub original: String,
    /// The text that should replace it.
    pub replacement: String,
    /// Start offset in UTF-16 code units.
    pub utf16_start: usize,
    /// End offset in UTF-16 code units.
    pub utf16_end: usize,
    /// The error class this edit fixes.
    pub error_type: ErrorType,
    /// Whether a correct system should propose exactly this edit.
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

impl ExpectedEdit {
    /// The span of the expected edit, in UTF-16 units.
    pub fn span(&self) -> TextSpan {
        // The grapheme axis is not stored in the golden file: the harness
        // compares on the UTF-16 axis, which every layer can produce.
        TextSpan::new(self.utf16_start, self.utf16_end, 0, 0)
    }
}

/// One labelled item in the golden set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenItem {
    /// A stable identifier, used to match predictions to items.
    pub id: String,
    /// The passage under test.
    pub text: String,
    /// The edits a correct system should propose.
    #[serde(default)]
    pub expected: Vec<ExpectedEdit>,
    /// The error class of the passage.
    pub error_type: ErrorType,
    /// The length bucket of the passage.
    pub length: LengthBucket,
    /// Where the passage came from.
    pub source: ItemSource,
    /// How hard the passage is.
    pub difficulty: Difficulty,
    /// Notes for a human reviewer.
    #[serde(default)]
    pub notes: String,
}

impl GoldenItem {
    /// Is this a no-error control?
    pub fn is_control(&self) -> bool {
        self.error_type == ErrorType::None
    }

    /// The edits a correct system must propose.
    pub fn required_edits(&self) -> Vec<&ExpectedEdit> {
        self.expected.iter().filter(|edit| edit.required).collect()
    }
}

/// A complete golden set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenSet {
    /// A version string, so a report can be tied to a set revision.
    pub version: String,
    /// The items.
    pub items: Vec<GoldenItem>,
}

impl GoldenSet {
    /// How many items the set holds.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Is the set empty?
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// How many no-error controls the set holds.
    pub fn control_count(&self) -> usize {
        self.items.iter().filter(|item| item.is_control()).count()
    }

    /// Look up an item by id.
    pub fn item(&self, id: &str) -> Option<&GoldenItem> {
        self.items.iter().find(|item| item.id == id)
    }
}

/// Load a golden set from a JSON document.
///
/// The document is a [`GoldenSet`]: `{ "version": "...", "items": [...] }`.
/// Returns a human-readable error message on failure, so a malformed file
/// fails a test with a useful message instead of a serde dump.
pub fn load_from_json(json: &str) -> Result<GoldenSet, String> {
    serde_json::from_str(json).map_err(|error| format!("Invalid golden set JSON: {error}"))
}

/// What a system predicted for one golden item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prediction {
    /// The id of the golden item this prediction is for.
    pub item_id: String,
    /// The edits the system proposed.
    #[serde(default)]
    pub edits: Vec<PredictedEdit>,
    /// Whether the model's output parsed against the JSON schema.
    ///
    /// `None` means "not measured"; `Some(false)` is a schema failure.
    #[serde(default)]
    pub schema_valid: Option<bool>,
    /// Whether the quote→offset resolution succeeded for every edit.
    #[serde(default)]
    pub resolved: Option<bool>,
}

impl Prediction {
    /// A prediction with no edits.
    pub fn empty(item_id: &str) -> Self {
        Self {
            item_id: item_id.to_string(),
            edits: Vec::new(),
            schema_valid: None,
            resolved: None,
        }
    }
}

/// An edit a system proposed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredictedEdit {
    /// The text the system proposed to replace.
    pub original: String,
    /// The text the system proposed instead.
    pub replacement: String,
    /// Start offset in UTF-16 code units.
    pub utf16_start: usize,
    /// End offset in UTF-16 code units.
    pub utf16_end: usize,
    /// How the quote was located.
    #[serde(default)]
    pub resolution: Option<Resolution>,
}

impl PredictedEdit {
    /// The span of the predicted edit.
    pub fn span(&self) -> TextSpan {
        TextSpan::new(self.utf16_start, self.utf16_end, 0, 0)
    }
}

/// The metrics the plan requires, computed over a golden set.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GoldenMetrics {
    /// Number of golden items that were scored.
    pub items: usize,
    /// How many items the predictions covered.
    pub covered: usize,
    /// True positives: a predicted edit that matches an expected edit.
    pub true_positives: usize,
    /// False positives: a predicted edit that matches nothing expected.
    pub false_positives: usize,
    /// False negatives: an expected edit that nothing predicted.
    pub false_negatives: usize,
    /// Fraction of predicted edits that were correct, in `0.0..=1.0`.
    pub span_precision: f64,
    /// Fraction of expected edits that were found, in `0.0..=1.0`.
    pub span_recall: f64,
    /// The F0.5 score: precision weighted twice as heavily as recall.
    pub f0_5: f64,
    /// Fraction of items where the predicted set exactly equals the expected
    /// set, in `0.0..=1.0`.
    pub exact_match_rate: f64,
    /// Fraction of *control* items that received no edit, in `0.0..=1.0`.
    ///
    /// This is the false-positive rate the plan caps at 8%.
    pub over_edit_rate: f64,
    /// Fraction of items whose payload failed schema validation.
    pub schema_failure_rate: f64,
    /// Fraction of items whose quotes all resolved to offsets.
    pub resolution_rate: f64,
}

impl GoldenMetrics {
    /// The empty metric set.
    pub const ZERO: GoldenMetrics = GoldenMetrics {
        items: 0,
        covered: 0,
        true_positives: 0,
        false_positives: 0,
        false_negatives: 0,
        span_precision: 0.0,
        span_recall: 0.0,
        f0_5: 0.0,
        exact_match_rate: 0.0,
        over_edit_rate: 0.0,
        schema_failure_rate: 0.0,
        resolution_rate: 0.0,
    };
}

/// The result of scoring a run against a golden set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenReport {
    /// The set that was scored.
    pub version: String,
    /// The overall metrics.
    pub metrics: GoldenMetrics,
    /// The same metrics, broken down by error type.
    pub by_error_type: Vec<StratumMetrics>,
    /// The same metrics, broken down by length bucket.
    pub by_length: Vec<StratumMetrics>,
    /// The same metrics, broken down by source.
    pub by_source: Vec<StratumMetrics>,
    /// The same metrics, broken down by difficulty.
    pub by_difficulty: Vec<StratumMetrics>,
    /// The ids of items that were not predicted at all.
    pub missing: Vec<String>,
    /// The ids of items where a required edit was not found.
    pub missed: Vec<String>,
    /// The ids of items that produced a suggestion on a no-error control.
    pub over_edited: Vec<String>,
}

impl GoldenReport {
    /// Did this run pass the plan's regression gate against `baseline`?
    ///
    /// The gate (plan §8): fail when the F0.5 drops by more than 2 points,
    /// or the resolution rate drops by more than 5 points.
    pub fn passes_gate(&self, baseline: &GoldenReport) -> bool {
        self.gate_failures(baseline).is_empty()
    }

    /// The reasons this run fails the regression gate against `baseline`.
    pub fn gate_failures(&self, baseline: &GoldenReport) -> Vec<String> {
        let mut failures = Vec::new();

        let f0_5_drop = baseline.metrics.f0_5 - self.metrics.f0_5;
        if f0_5_drop > 0.02 {
            failures.push(format!(
                "F0.5 dropped {:.1} points ({:.4} -> {:.4}), limit is 2.0",
                f0_5_drop * 100.0,
                baseline.metrics.f0_5,
                self.metrics.f0_5
            ));
        }

        let resolution_drop = baseline.metrics.resolution_rate - self.metrics.resolution_rate;
        if resolution_drop > 0.05 {
            failures.push(format!(
                "resolution rate dropped {:.1} points ({:.4} -> {:.4}), limit is 5.0",
                resolution_drop * 100.0,
                baseline.metrics.resolution_rate,
                self.metrics.resolution_rate
            ));
        }

        failures
    }
}

/// The metrics for one stratum of the golden set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StratumMetrics {
    /// The stratum name, e.g. `"grammar"` or `"short"`.
    pub name: String,
    /// How many items are in the stratum.
    pub items: usize,
    /// The metrics for the stratum.
    pub metrics: GoldenMetrics,
}

/// Score a run against a golden set.
///
/// `predictions` may omit items (they are then counted as misses) and may
/// contain items that are not in the set (they are ignored). The function is
/// **pure**: it reads no clock, no filesystem, and no network.
///
/// Matching rule: a predicted edit matches an expected edit when the two
/// spans overlap *and* the proposed replacement is identical to the expected
/// one. A predicted edit that overlaps an expected edit but proposes a
/// different replacement is a false positive (an over-edit), not a true
/// positive.
pub fn evaluate(set: &GoldenSet, predictions: &[Prediction]) -> GoldenReport {
    let mut metrics = score_items(set, predictions, |_| true);
    metrics.items = set.len();
    metrics.covered = predictions
        .iter()
        .filter(|prediction| set.item(&prediction.item_id).is_some())
        .count();

    let missing: Vec<String> = set
        .items
        .iter()
        .filter(|item| {
            !predictions
                .iter()
                .any(|prediction| prediction.item_id == item.id)
        })
        .map(|item| item.id.clone())
        .collect();

    let mut missed: Vec<String> = Vec::new();
    let mut over_edited: Vec<String> = Vec::new();
    for item in &set.items {
        let Some(prediction) = predictions
            .iter()
            .find(|prediction| prediction.item_id == item.id)
        else {
            continue;
        };
        if required_edits_missing(item, prediction) {
            missed.push(item.id.clone());
        }
        if item.is_control() && !prediction.edits.is_empty() {
            over_edited.push(item.id.clone());
        }
    }

    let by_error_type = strata(set, predictions, |item| item.error_type.label());
    let by_length = strata(set, predictions, |item| item.length.label());
    let by_source = strata(set, predictions, |item| item.source.label());
    let by_difficulty = strata(set, predictions, |item| item.difficulty.label());

    GoldenReport {
        version: set.version.clone(),
        metrics,
        by_error_type,
        by_length,
        by_source,
        by_difficulty,
        missing,
        missed,
        over_edited,
    }
}

/// Score only the resolver: how often a quote resolves to the right span, and
/// how often a quote is ambiguous.
///
/// The plan (§8) requires the golden set to be usable for the resolver as well
/// as for the model. A resolver prediction is a set of `(quote, span)` pairs
/// per item; a quote that resolves to a span that does not contain it is a
/// resolution failure, and a quote that matches more than one place in the
/// text is ambiguous.
///
/// `predictions` maps an item id to the spans its quotes resolved to, in the
/// same order the quotes appear in `expected`.
pub fn evaluate_resolver(set: &GoldenSet, predictions: &ResolverPredictions) -> ResolverReport {
    let mut total = 0usize;
    let mut resolved = 0usize;
    let mut ambiguous = 0usize;
    let mut wrong = 0usize;
    let mut failed: Vec<String> = Vec::new();

    for item in &set.items {
        let quotes: Vec<&ExpectedEdit> = item.required_edits();
        let Some(spans) = predictions.get(&item.id) else {
            continue;
        };
        for (index, expected) in quotes.iter().enumerate() {
            total += 1;
            match spans.get(index) {
                None => {
                    failed.push(format!("{}: quote {index} did not resolve", item.id));
                }
                Some(span) => {
                    if span.utf16_start == expected.utf16_start
                        && span.utf16_end == expected.utf16_end
                    {
                        resolved += 1;
                    } else {
                        wrong += 1;
                        failed.push(format!(
                            "{}: quote {index} resolved to {}..{}, expected {}..{}",
                            item.id,
                            span.utf16_start,
                            span.utf16_end,
                            expected.utf16_start,
                            expected.utf16_end
                        ));
                    }
                }
            }
        }

        // A quote that occurs more than once in the text is ambiguous by
        // definition: the resolver must refuse to guess.
        for (index, expected) in quotes.iter().enumerate() {
            let occurrences = count_occurrences(&item.text, &expected.original);
            if occurrences > 1 {
                ambiguous += 1;
                let _ = index;
            }
        }
    }

    let resolution_rate = if total == 0 {
        0.0
    } else {
        resolved as f64 / total as f64
    };
    let ambiguous_rate = if total == 0 {
        0.0
    } else {
        ambiguous as f64 / total as f64
    };

    ResolverReport {
        version: set.version.clone(),
        quotes: total,
        resolved,
        ambiguous,
        wrong,
        resolution_rate,
        ambiguous_rate,
        failures: failed,
    }
}

/// The spans a resolver produced for each item, keyed by item id.
pub type ResolverPredictions = std::collections::HashMap<String, Vec<TextSpan>>;

/// The result of scoring the resolver against a golden set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolverReport {
    /// The set that was scored.
    pub version: String,
    /// How many quotes were scored.
    pub quotes: usize,
    /// How many resolved to the expected span.
    pub resolved: usize,
    /// How many quotes are ambiguous in the source text.
    pub ambiguous: usize,
    /// How many resolved to the wrong span.
    pub wrong: usize,
    /// Fraction of quotes that resolved correctly.
    pub resolution_rate: f64,
    /// Fraction of quotes that are ambiguous.
    pub ambiguous_rate: f64,
    /// Human-readable failures.
    pub failures: Vec<String>,
}

/// Score a subset of items, selected by `include`.
fn score_items(
    set: &GoldenSet,
    predictions: &[Prediction],
    include: impl Fn(&GoldenItem) -> bool,
) -> GoldenMetrics {
    let mut true_positives = 0usize;
    let mut false_positives = 0usize;
    let mut false_negatives = 0usize;
    let mut exact = 0usize;
    let mut scored = 0usize;
    let mut controls = 0usize;
    let mut over_edited = 0usize;
    let mut schema_failures = 0usize;
    let mut schema_measured = 0usize;
    let mut resolved = 0usize;
    let mut resolution_measured = 0usize;

    for item in set.items.iter().filter(|item| include(item)) {
        scored += 1;
        let Some(prediction) = predictions
            .iter()
            .find(|prediction| prediction.item_id == item.id)
        else {
            false_negatives += item.required_edits().len();
            continue;
        };

        if let Some(valid) = prediction.schema_valid {
            schema_measured += 1;
            if !valid {
                schema_failures += 1;
            }
        }
        if let Some(ok) = prediction.resolved {
            resolution_measured += 1;
            if ok {
                resolved += 1;
            }
        }

        let expected = item.required_edits();
        let mut matched_expected = vec![false; expected.len()];

        for predicted in &prediction.edits {
            let mut matched = None;
            for (index, candidate) in expected.iter().enumerate() {
                if matched_expected[index] {
                    continue;
                }
                if spans_overlap(predicted.span(), candidate.span())
                    && predicted.replacement == candidate.replacement
                {
                    matched = Some(index);
                    break;
                }
            }
            match matched {
                Some(index) => {
                    matched_expected[index] = true;
                    true_positives += 1;
                }
                None => false_positives += 1,
            }
        }

        false_negatives += matched_expected.iter().filter(|matched| !**matched).count();

        if matched_expected.iter().all(|matched| *matched)
            && prediction.edits.len() == expected.len()
        {
            exact += 1;
        }

        if item.is_control() {
            controls += 1;
            if !prediction.edits.is_empty() {
                over_edited += 1;
            }
        }
    }

    let span_precision = if true_positives + false_positives == 0 {
        0.0
    } else {
        true_positives as f64 / (true_positives + false_positives) as f64
    };
    let span_recall = if true_positives + false_negatives == 0 {
        0.0
    } else {
        true_positives as f64 / (true_positives + false_negatives) as f64
    };
    // F0.5 weights precision twice as heavily as recall:
    //   Fβ = (1 + β²) · (P · R) / (β² · P + R)
    let beta_squared = 0.5_f64 * 0.5;
    let denominator = beta_squared * span_precision + span_recall;
    let f0_5 = if denominator == 0.0 {
        0.0
    } else {
        (1.0 + beta_squared) * span_precision * span_recall / denominator
    };

    GoldenMetrics {
        items: scored,
        covered: 0,
        true_positives,
        false_positives,
        false_negatives,
        span_precision,
        span_recall,
        f0_5,
        exact_match_rate: if scored == 0 {
            0.0
        } else {
            exact as f64 / scored as f64
        },
        over_edit_rate: if controls == 0 {
            0.0
        } else {
            over_edited as f64 / controls as f64
        },
        schema_failure_rate: if schema_measured == 0 {
            0.0
        } else {
            schema_failures as f64 / schema_measured as f64
        },
        resolution_rate: if resolution_measured == 0 {
            0.0
        } else {
            resolved as f64 / resolution_measured as f64
        },
    }
}

/// Compute the metrics for every stratum.
fn strata(
    set: &GoldenSet,
    predictions: &[Prediction],
    name: impl Fn(&GoldenItem) -> &'static str,
) -> Vec<StratumMetrics> {
    let mut labels: Vec<&'static str> = Vec::new();
    for item in &set.items {
        let label = name(item);
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels.sort_unstable();

    labels
        .into_iter()
        .map(|label| {
            let metrics = score_items(set, predictions, |item| name(item) == label);
            StratumMetrics {
                name: label.to_string(),
                items: metrics.items,
                metrics,
            }
        })
        .collect()
}

/// Do two expected edits match a prediction?
fn required_edits_missing(item: &GoldenItem, prediction: &Prediction) -> bool {
    item.required_edits().iter().any(|expected| {
        !prediction.edits.iter().any(|predicted| {
            spans_overlap(predicted.span(), expected.span())
                && predicted.replacement == expected.replacement
        })
    })
}

/// Do two spans overlap? Touching spans do not overlap.
fn spans_overlap(a: TextSpan, b: TextSpan) -> bool {
    a.utf16_start < b.utf16_end && b.utf16_start < a.utf16_end
}

/// How many times does `needle` occur in `haystack`?
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}

/// Build a [`Suggestion`] from a golden item's expected edit, for tests and
/// for turning golden items into resolver fixtures.
pub fn expected_to_suggestion(item: &GoldenItem, expected: &ExpectedEdit) -> Suggestion {
    Suggestion {
        id: format!("{}:{}", item.id, expected.utf16_start),
        span: expected.span(),
        original: expected.original.clone(),
        replacement: expected.replacement.clone(),
        category: Category::Grammar,
        severity: Severity::Medium,
        explanation: "golden expectation".into(),
        resolution: Resolution::Exact,
        confidence: Resolution::Exact.confidence(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> GoldenSet {
        GoldenSet {
            version: "test-1".into(),
            items: vec![
                GoldenItem {
                    id: "g1".into(),
                    text: "Their going to the conference.".into(),
                    expected: vec![ExpectedEdit {
                        original: "Their".into(),
                        replacement: "They're".into(),
                        utf16_start: 0,
                        utf16_end: 5,
                        error_type: ErrorType::Grammar,
                        required: true,
                    }],
                    error_type: ErrorType::Grammar,
                    length: LengthBucket::Short,
                    source: ItemSource::Curated,
                    difficulty: Difficulty::Easy,
                    notes: String::new(),
                },
                GoldenItem {
                    id: "g2".into(),
                    text: "The weather is fine today.".into(),
                    expected: Vec::new(),
                    error_type: ErrorType::None,
                    length: LengthBucket::Short,
                    source: ItemSource::Curated,
                    difficulty: Difficulty::Easy,
                    notes: String::new(),
                },
            ],
        }
    }

    #[test]
    fn a_perfect_run_scores_one() {
        let set = set();
        let predictions = vec![
            Prediction {
                item_id: "g1".into(),
                edits: vec![PredictedEdit {
                    original: "Their".into(),
                    replacement: "They're".into(),
                    utf16_start: 0,
                    utf16_end: 5,
                    resolution: Some(Resolution::Exact),
                }],
                schema_valid: Some(true),
                resolved: Some(true),
            },
            Prediction {
                item_id: "g2".into(),
                edits: Vec::new(),
                schema_valid: Some(true),
                resolved: Some(true),
            },
        ];
        let report = evaluate(&set, &predictions);
        assert_eq!(report.metrics.f0_5, 1.0);
        assert_eq!(report.metrics.span_precision, 1.0);
        assert_eq!(report.metrics.span_recall, 1.0);
        assert_eq!(report.metrics.exact_match_rate, 1.0);
        assert_eq!(report.metrics.over_edit_rate, 0.0);
        assert_eq!(report.metrics.schema_failure_rate, 0.0);
        assert_eq!(report.metrics.resolution_rate, 1.0);
        assert!(report.missed.is_empty());
        assert!(report.over_edited.is_empty());
    }

    #[test]
    fn a_control_with_an_edit_is_an_over_edit() {
        let set = set();
        let predictions = vec![
            Prediction {
                item_id: "g1".into(),
                edits: Vec::new(),
                schema_valid: Some(true),
                resolved: Some(true),
            },
            Prediction {
                item_id: "g2".into(),
                edits: vec![PredictedEdit {
                    original: "fine".into(),
                    replacement: "great".into(),
                    utf16_start: 15,
                    utf16_end: 19,
                    resolution: Some(Resolution::Exact),
                }],
                schema_valid: Some(true),
                resolved: Some(true),
            },
        ];
        let report = evaluate(&set, &predictions);
        assert_eq!(report.metrics.over_edit_rate, 1.0);
        assert_eq!(report.over_edited, vec!["g2".to_string()]);
    }

    #[test]
    fn a_wrong_replacement_is_not_a_true_positive() {
        let set = set();
        let predictions = vec![Prediction {
            item_id: "g1".into(),
            edits: vec![PredictedEdit {
                original: "Their".into(),
                replacement: "They are".into(),
                utf16_start: 0,
                utf16_end: 5,
                resolution: Some(Resolution::Exact),
            }],
            schema_valid: Some(true),
            resolved: Some(true),
        }];
        let report = evaluate(&set, &predictions);
        assert_eq!(report.metrics.true_positives, 0);
        assert_eq!(report.metrics.false_positives, 1);
        assert_eq!(report.metrics.false_negatives, 1);
        assert_eq!(report.missed, vec!["g1".to_string()]);
    }

    #[test]
    fn a_missing_prediction_is_a_miss_not_a_crash() {
        let set = set();
        let report = evaluate(&set, &[]);
        assert_eq!(report.missing.len(), 2);
        assert_eq!(report.metrics.false_negatives, 1);
        assert_eq!(report.metrics.span_recall, 0.0);
    }

    #[test]
    fn schema_failures_are_counted() {
        let set = set();
        let predictions = vec![
            Prediction {
                item_id: "g1".into(),
                edits: Vec::new(),
                schema_valid: Some(false),
                resolved: Some(false),
            },
            Prediction {
                item_id: "g2".into(),
                edits: Vec::new(),
                schema_valid: Some(true),
                resolved: Some(true),
            },
        ];
        let report = evaluate(&set, &predictions);
        assert_eq!(report.metrics.schema_failure_rate, 0.5);
        assert_eq!(report.metrics.resolution_rate, 0.5);
    }

    #[test]
    fn strata_cover_every_dimension() {
        let set = set();
        let report = evaluate(&set, &[]);
        assert!(report.by_error_type.iter().any(|s| s.name == "grammar"));
        assert!(report.by_error_type.iter().any(|s| s.name == "none"));
        assert!(report.by_length.iter().any(|s| s.name == "short"));
        assert!(report.by_source.iter().any(|s| s.name == "curated"));
        assert!(report.by_difficulty.iter().any(|s| s.name == "easy"));
    }

    #[test]
    fn the_gate_fails_on_a_f0_5_drop() {
        let baseline = evaluate(
            &set(),
            &[Prediction {
                item_id: "g1".into(),
                edits: vec![PredictedEdit {
                    original: "Their".into(),
                    replacement: "They're".into(),
                    utf16_start: 0,
                    utf16_end: 5,
                    resolution: Some(Resolution::Exact),
                }],
                schema_valid: Some(true),
                resolved: Some(true),
            }],
        );
        let regressed = evaluate(&set(), &[]);
        assert!(!regressed.passes_gate(&baseline));
        assert!(
            regressed
                .gate_failures(&baseline)
                .iter()
                .any(|failure| failure.contains("F0.5"))
        );
    }

    #[test]
    fn the_gate_allows_a_small_drop() {
        let baseline = evaluate(&set(), &[]);
        let mut regressed_set = set();
        regressed_set.items.push(GoldenItem {
            id: "g3".into(),
            text: "A clean sentence.".into(),
            expected: Vec::new(),
            error_type: ErrorType::None,
            length: LengthBucket::Short,
            source: ItemSource::Curated,
            difficulty: Difficulty::Easy,
            notes: String::new(),
        });
        let regressed = evaluate(&regressed_set, &[]);
        assert!(regressed.passes_gate(&baseline));
    }

    #[test]
    fn the_resolver_report_counts_resolutions() {
        let set = set();
        let mut predictions = ResolverPredictions::new();
        predictions.insert("g1".to_string(), vec![TextSpan::new(0, 5, 0, 5)]);
        let report = evaluate_resolver(&set, &predictions);
        assert_eq!(report.quotes, 1);
        assert_eq!(report.resolved, 1);
        assert_eq!(report.resolution_rate, 1.0);
        assert!(report.failures.is_empty());
    }

    #[test]
    fn the_resolver_report_flags_a_wrong_span() {
        let set = set();
        let mut predictions = ResolverPredictions::new();
        predictions.insert("g1".to_string(), vec![TextSpan::new(6, 11, 6, 11)]);
        let report = evaluate_resolver(&set, &predictions);
        assert_eq!(report.resolved, 0);
        assert_eq!(report.wrong, 1);
        assert!(!report.failures.is_empty());
    }

    #[test]
    fn a_golden_set_round_trips_through_json() {
        let json = serde_json::to_string(&set()).unwrap();
        let decoded: GoldenSet = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, set());
    }

    #[test]
    fn loading_invalid_json_reports_an_error() {
        assert!(load_from_json("{ not json").is_err());
    }
}
