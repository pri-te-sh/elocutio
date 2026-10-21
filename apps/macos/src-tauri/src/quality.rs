//! The golden-set quality check, runnable from the app.
//!
//! The plan asks for a golden set with span precision/recall, a false-positive
//! rate, and a regression gate. The harness lives in `elocutio-analysis`, and
//! this module runs it against the seed set that ships with the app, so the
//! quality numbers can be reproduced on demand rather than only in CI.
//!
//! The resolver check is the important one: it asks the real quote-to-offset
//! resolver to find every quote in the golden set and reports the fraction it
//! got right. That measures the component that decides where an edit lands.

use elocutio_analysis::golden::{GoldenSet, ResolverPredictions};
use elocutio_analysis::{RawEdit, resolve_edit};
use elocutio_core::TextSpan;
use serde::Serialize;

/// The seed golden set, shipped with the app.
pub const SEED_GOLDEN_JSON: &str = include_str!("../resources/seed_golden.json");

/// The outcome of running the resolver against the golden set.
#[derive(Debug, Clone, Serialize)]
pub struct ResolverQuality {
    pub version: String,
    pub quotes: usize,
    pub resolved: usize,
    pub ambiguous: usize,
    pub wrong: usize,
    pub resolution_rate: f64,
    pub ambiguous_rate: f64,
    pub failures: Vec<String>,
}

/// The outcome of running a prediction set against the golden set.
#[derive(Debug, Clone, Serialize)]
pub struct GoldenQuality {
    pub version: String,
    pub items: usize,
    pub covered: usize,
    pub span_precision: f64,
    pub span_recall: f64,
    pub f0_5: f64,
    pub exact_match_rate: f64,
    pub over_edit_rate: f64,
    pub schema_failure_rate: f64,
    pub resolution_rate: f64,
}

/// Run the resolver against every quote in the golden set.
///
/// The resolver is asked to find the same quote the golden set expects. A
/// quote that appears more than once is ambiguous by design, so the resolver
/// is expected to refuse it rather than guess; that is reported separately
/// from a wrong span.
pub fn run_resolver_check() -> Result<ResolverQuality, String> {
    let set: GoldenSet = elocutio_analysis::load_from_json(SEED_GOLDEN_JSON)?;
    let mut predictions: ResolverPredictions = ResolverPredictions::new();

    for item in &set.items {
        let mut spans = Vec::new();
        for expected in item.required_edits() {
            let edit = RawEdit {
                paragraph_id: item.id.clone(),
                original: expected.original.clone(),
                replacement: expected.replacement.clone(),
                category: elocutio_core::Category::Grammar,
                severity: elocutio_core::Severity::High,
                explanation: "golden set check".into(),
            };
            if let Ok(suggestion) = resolve_edit(&item.id, &item.text, &edit, "e1") {
                spans.push(suggestion.span);
            }
        }
        predictions.insert(item.id.clone(), spans);
    }

    let report = elocutio_analysis::evaluate_resolver(&set, &predictions);

    Ok(ResolverQuality {
        version: report.version,
        quotes: report.quotes,
        resolved: report.resolved,
        ambiguous: report.ambiguous,
        wrong: report.wrong,
        resolution_rate: report.resolution_rate,
        ambiguous_rate: report.ambiguous_rate,
        failures: report.failures,
    })
}

/// Score a prediction set against the golden set.
pub fn run_golden_check(predictions_json: &str) -> Result<GoldenQuality, String> {
    let set: GoldenSet = elocutio_analysis::load_from_json(SEED_GOLDEN_JSON)?;
    let predictions: Vec<elocutio_analysis::golden::Prediction> =
        serde_json::from_str(predictions_json)
            .map_err(|error| format!("Invalid predictions JSON: {error}"))?;
    let report = elocutio_analysis::evaluate(&set, &predictions);

    Ok(GoldenQuality {
        version: report.version,
        items: report.metrics.items,
        covered: report.metrics.covered,
        span_precision: report.metrics.span_precision,
        span_recall: report.metrics.span_recall,
        f0_5: report.metrics.f0_5,
        exact_match_rate: report.metrics.exact_match_rate,
        over_edit_rate: report.metrics.over_edit_rate,
        schema_failure_rate: report.metrics.schema_failure_rate,
        resolution_rate: report.metrics.resolution_rate,
    })
}

/// A convenience for callers that only need the span type.
pub fn empty_span() -> TextSpan {
    TextSpan::new(0, 0, 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resolver_finds_every_unique_quote_in_the_seed_set() {
        let quality = run_resolver_check().expect("the seed set should load");
        assert!(quality.quotes > 0, "the seed set should have quotes");
        assert!(
            quality.resolution_rate >= 0.9,
            "the resolver should find nearly every quote, got {}",
            quality.resolution_rate
        );
        assert_eq!(quality.wrong, 0, "no quote should resolve to a wrong span");
    }

    #[test]
    fn the_seed_set_contains_no_error_controls() {
        let set: GoldenSet = elocutio_analysis::load_from_json(SEED_GOLDEN_JSON).unwrap();
        assert!(
            set.control_count() > 0,
            "the set needs no-error controls to measure false positives"
        );
    }
}
