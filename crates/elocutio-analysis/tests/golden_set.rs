//! Integration test for the golden-set harness.
//!
//! Runs the harness over the seed golden set that ships with the crate and
//! asserts the metrics are sane. The seed set is small but real: it is
//! stratified by error type, length, source, and difficulty, and it includes
//! no-error controls.
//!
//! The plan (§8) asks for 200–500 items in the full set. The seed is the
//! floor, not the goal — it exists so the harness, the JSON format, and the
//! regression gate are exercised in CI from day one.

use std::collections::HashMap;

use elocutio_analysis::golden::{
    GoldenSet, PredictedEdit, Prediction, ResolverPredictions, evaluate, evaluate_resolver,
    load_from_json,
};
use elocutio_core::{Resolution, TextSpan};

const SEED: &str = include_str!("data/seed_golden.json");

fn seed() -> GoldenSet {
    load_from_json(SEED).expect("the seed golden set parses")
}

/// A prediction that reproduces every expected edit exactly.
fn perfect_predictions(set: &GoldenSet) -> Vec<Prediction> {
    set.items
        .iter()
        .map(|item| Prediction {
            item_id: item.id.clone(),
            edits: item
                .expected
                .iter()
                .map(|edit| PredictedEdit {
                    original: edit.original.clone(),
                    replacement: edit.replacement.clone(),
                    utf16_start: edit.utf16_start,
                    utf16_end: edit.utf16_end,
                    resolution: Some(Resolution::Exact),
                })
                .collect(),
            schema_valid: Some(true),
            resolved: Some(true),
        })
        .collect()
}

#[test]
fn the_seed_set_loads_and_is_stratified() {
    let set = seed();
    assert!(set.len() >= 20, "the seed set must have at least 20 items");
    assert!(
        set.control_count() >= 3,
        "the seed set must include no-error controls"
    );

    // Every stratification dimension must be represented.
    let error_types: std::collections::HashSet<_> =
        set.items.iter().map(|item| item.error_type).collect();
    assert!(error_types.len() >= 5, "error types are not stratified");

    let sources: std::collections::HashSet<_> = set.items.iter().map(|item| item.source).collect();
    assert!(sources.len() >= 3, "sources are not stratified");

    let difficulties: std::collections::HashSet<_> =
        set.items.iter().map(|item| item.difficulty).collect();
    assert!(difficulties.len() >= 3, "difficulties are not stratified");

    let lengths: std::collections::HashSet<_> = set.items.iter().map(|item| item.length).collect();
    assert!(lengths.len() >= 2, "lengths are not stratified");
}

#[test]
fn every_expected_edit_points_at_its_quote() {
    let set = seed();
    for item in &set.items {
        for edit in &item.expected {
            let utf16: Vec<u16> = item.text.encode_utf16().collect();
            let slice =
                String::from_utf16(&utf16[edit.utf16_start..edit.utf16_end]).expect("valid utf-16");
            assert_eq!(
                slice, edit.original,
                "{}: the expected span does not contain the quote",
                item.id
            );
        }
    }
}

#[test]
fn a_perfect_run_scores_a_perfect_f0_5() {
    let set = seed();
    let report = evaluate(&set, &perfect_predictions(&set));

    assert_eq!(report.metrics.items, set.len());
    assert_eq!(report.metrics.covered, set.len());
    assert_eq!(report.metrics.f0_5, 1.0, "metrics: {:?}", report.metrics);
    assert_eq!(report.metrics.span_precision, 1.0);
    assert_eq!(report.metrics.span_recall, 1.0);
    assert_eq!(report.metrics.exact_match_rate, 1.0);
    assert_eq!(report.metrics.over_edit_rate, 0.0);
    assert_eq!(report.metrics.schema_failure_rate, 0.0);
    assert_eq!(report.metrics.resolution_rate, 1.0);
    assert!(report.missing.is_empty());
    assert!(report.missed.is_empty());
    assert!(report.over_edited.is_empty());
}

#[test]
fn an_empty_run_scores_zero_and_is_not_a_crash() {
    let set = seed();
    let report = evaluate(&set, &[]);

    assert_eq!(report.metrics.f0_5, 0.0);
    assert_eq!(report.metrics.span_precision, 0.0);
    assert_eq!(report.metrics.span_recall, 0.0);
    assert_eq!(report.missing.len(), set.len());
    assert!(report.metrics.false_negatives > 0);
}

#[test]
fn a_run_that_edits_a_control_is_penalised() {
    let set = seed();
    let mut predictions = perfect_predictions(&set);

    // Add a spurious edit to the first no-error control.
    let control = set
        .items
        .iter()
        .find(|item| item.is_control())
        .expect("the set has a control");
    let edit = predictions
        .iter_mut()
        .find(|prediction| prediction.item_id == control.id)
        .expect("the control has a prediction");
    edit.edits.push(PredictedEdit {
        original: "fine".into(),
        replacement: "great".into(),
        utf16_start: 14,
        utf16_end: 18,
        resolution: Some(Resolution::Exact),
    });

    let report = evaluate(&set, &predictions);
    assert!(
        report.metrics.over_edit_rate > 0.0,
        "the over-edit rate must react to a control edit"
    );
    assert_eq!(report.over_edited, vec![control.id.clone()]);
    assert!(report.metrics.f0_5 < 1.0);
}

#[test]
fn a_run_with_a_wrong_replacement_loses_precision() {
    let set = seed();
    let mut predictions = perfect_predictions(&set);

    // Change one expected replacement to a wrong one.
    let target = set
        .items
        .iter()
        .find(|item| !item.expected.is_empty())
        .expect("the set has an expectation");
    let prediction = predictions
        .iter_mut()
        .find(|prediction| prediction.item_id == target.id)
        .expect("the target has a prediction");
    prediction.edits[0].replacement = "something else".into();

    let report = evaluate(&set, &predictions);
    assert!(report.metrics.span_precision < 1.0);
    assert_eq!(report.metrics.false_positives, 1);
    assert!(report.missed.contains(&target.id));
}

#[test]
fn the_strata_cover_every_item_exactly_once() {
    let set = seed();
    let report = evaluate(&set, &perfect_predictions(&set));

    for strata in [
        &report.by_error_type,
        &report.by_length,
        &report.by_source,
        &report.by_difficulty,
    ] {
        let total: usize = strata.iter().map(|stratum| stratum.items).sum();
        assert_eq!(total, set.len(), "strata do not partition the set");
    }
}

#[test]
fn the_gate_fails_a_regression_and_passes_a_stable_run() {
    let set = seed();
    let baseline = evaluate(&set, &perfect_predictions(&set));

    // The same run against the same set passes.
    assert!(baseline.passes_gate(&baseline));

    // An empty run regresses hard.
    let regressed = evaluate(&set, &[]);
    assert!(!regressed.passes_gate(&baseline));
    assert!(
        regressed
            .gate_failures(&baseline)
            .iter()
            .any(|failure| failure.contains("F0.5"))
    );
    assert!(
        regressed
            .gate_failures(&baseline)
            .iter()
            .any(|failure| failure.contains("resolution rate"))
    );
}

#[test]
fn the_resolver_evaluation_resolves_every_quote() {
    let set = seed();

    // Resolve every expected quote against its own item text and check the
    // resolver lands on the expected span.
    let mut predictions: ResolverPredictions = HashMap::new();
    for item in &set.items {
        let mut spans = Vec::new();
        for edit in &item.expected {
            let utf16: Vec<u16> = item.text.encode_utf16().collect();
            let needle: Vec<u16> = edit.original.encode_utf16().collect();
            let found = find_subsequence(&utf16, &needle);
            spans.push(match found {
                Some(start) => {
                    let end = start + needle.len();
                    TextSpan::new(start, end, 0, 0)
                }
                None => TextSpan::new(0, 0, 0, 0),
            });
        }
        predictions.insert(item.id.clone(), spans);
    }

    let report = evaluate_resolver(&set, &predictions);
    assert_eq!(
        report.resolved, report.quotes,
        "failures: {:?}",
        report.failures
    );
    assert_eq!(report.resolution_rate, 1.0);
    assert_eq!(report.ambiguous, 0);
    assert_eq!(report.ambiguous_rate, 0.0);
}

#[test]
fn the_resolver_evaluation_counts_ambiguous_quotes() {
    // A quote that occurs twice in the text is ambiguous by definition.
    let mut ambiguous_set = seed();
    let text = "the cat saw the dog".to_string();
    ambiguous_set
        .items
        .push(elocutio_analysis::golden::GoldenItem {
            id: "ambiguous-1".into(),
            text,
            expected: vec![elocutio_analysis::golden::ExpectedEdit {
                original: "the".into(),
                replacement: "a".into(),
                utf16_start: 0,
                utf16_end: 3,
                error_type: elocutio_analysis::golden::ErrorType::Grammar,
                required: true,
            }],
            error_type: elocutio_analysis::golden::ErrorType::Grammar,
            length: elocutio_analysis::golden::LengthBucket::Short,
            source: elocutio_analysis::golden::ItemSource::Synthetic,
            difficulty: elocutio_analysis::golden::Difficulty::Easy,
            notes: String::new(),
        });

    let mut predictions: ResolverPredictions = HashMap::new();
    predictions.insert("ambiguous-1".into(), vec![TextSpan::new(0, 3, 0, 3)]);
    let report = evaluate_resolver(&ambiguous_set, &predictions);
    assert!(report.ambiguous >= 1);
    assert!(report.ambiguous_rate > 0.0);
}

/// Find the first index where `needle` occurs in `haystack`.
fn find_subsequence(haystack: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
