//! The voice copy guard: 8-gram overlap detection between a candidate
//! rewrite and the writer's own samples.
//!
//! The app never keeps the raw text of the uploaded samples, so the guard
//! works on *fingerprints*: 64-bit hashes of the sample n-grams. A candidate
//! is split into the same n-grams and its fingerprints are matched against the
//! stored set. A hit means the candidate reproduces sample content, and the
//! rewrite path rejects it.
//!
//! The fingerprint is a copy-detection heuristic, not a security boundary: it
//! answers "do these two texts share an 8-gram?", not "who wrote this?".

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::word_tokens;

/// The n-gram size used by the voice copy guard.
pub const DEFAULT_NGRAM_SIZE: usize = 8;

/// A contiguous phrase that appears in both the candidate and a stored sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlappingSpan {
    /// The shared phrase, normalized to lowercase words.
    pub ngram: String,
    /// The 64-bit fingerprint of the shared phrase.
    pub fingerprint: u64,
    /// Index of the first word of the phrase inside the candidate.
    pub candidate_start: usize,
    /// Index one past the last word of the phrase inside the candidate.
    pub candidate_end: usize,
    /// Index of the sample that holds the phrase, when the sample text is
    /// available. Fingerprint-only checks cannot know the sample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_index: Option<usize>,
}

/// The result of a copy-guard check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyGuardReport {
    /// Every distinct phrase shared with the samples.
    pub overlaps: Vec<OverlappingSpan>,
    /// `true` when nothing was shared.
    pub clean: bool,
}

impl Default for CopyGuardReport {
    fn default() -> Self {
        Self {
            overlaps: Vec::new(),
            clean: true,
        }
    }
}

/// Why a candidate output was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CopyError {
    /// The candidate output repeats content from the writer's samples.
    #[error("the output repeats {matches} phrase(s) from your writing samples")]
    Repeated {
        /// How many distinct phrases were repeated.
        matches: usize,
        /// The first repeated phrase, taken from the candidate output.
        example: String,
    },
}

/// A 64-bit FNV-1a hash of an already normalized n-gram.
pub fn fingerprint_ngram(ngram: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in ngram.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The fingerprints of every contiguous n-gram in `text`.
pub fn ngram_fingerprints(text: &str, n: usize) -> BTreeSet<u64> {
    fingerprints_of_tokens(&word_tokens(text), n)
}

/// The fingerprints of every contiguous n-gram across `samples`.
///
/// Each sample is hashed on its own, so a phrase that only exists across a
/// sample boundary never becomes a fingerprint.
pub fn sample_fingerprints(samples: &[String], n: usize) -> BTreeSet<u64> {
    samples
        .iter()
        .flat_map(|sample| ngram_fingerprints(sample, n))
        .collect()
}

fn fingerprints_of_tokens(tokens: &[String], n: usize) -> BTreeSet<u64> {
    let n = n.max(1);
    if tokens.len() < n {
        return BTreeSet::new();
    }
    tokens
        .windows(n)
        .map(|window| fingerprint_ngram(&window.join(" ")))
        .collect()
}

/// Check a candidate output against the raw text of the samples.
///
/// Matching is case-insensitive and whitespace-normalized: the candidate and
/// the samples are compared as sequences of lowercase words, so punctuation
/// differences do not hide a copied phrase.
pub fn copy_guard(candidate: &str, samples: &[String], n: usize) -> CopyGuardReport {
    let n = n.max(1);
    let candidate_tokens = word_tokens(candidate);
    let mut overlaps: Vec<OverlappingSpan> = Vec::new();
    let mut seen: BTreeSet<u64> = BTreeSet::new();

    for (sample_index, sample) in samples.iter().enumerate() {
        let sample_tokens = word_tokens(sample);
        if sample_tokens.len() < n {
            continue;
        }
        let sample_grams: BTreeSet<String> = sample_tokens
            .windows(n)
            .map(|window| window.join(" "))
            .collect();
        for (start, window) in candidate_tokens.windows(n).enumerate() {
            let gram = window.join(" ");
            if !sample_grams.contains(&gram) {
                continue;
            }
            let fingerprint = fingerprint_ngram(&gram);
            if !seen.insert(fingerprint) {
                continue;
            }
            overlaps.push(OverlappingSpan {
                ngram: gram,
                fingerprint,
                candidate_start: start,
                candidate_end: start + n,
                sample_index: Some(sample_index),
            });
        }
    }

    let clean = overlaps.is_empty();
    CopyGuardReport { overlaps, clean }
}

/// Check a candidate output against stored sample fingerprints.
///
/// This is the check the app uses: the sample text is gone, the fingerprints
/// are not.
pub fn copy_guard_against_fingerprints(
    candidate: &str,
    fingerprints: &BTreeSet<u64>,
    n: usize,
) -> CopyGuardReport {
    let n = n.max(1);
    let tokens = word_tokens(candidate);
    let mut overlaps: Vec<OverlappingSpan> = Vec::new();
    let mut seen: BTreeSet<u64> = BTreeSet::new();

    for (start, window) in tokens.windows(n).enumerate() {
        let gram = window.join(" ");
        let fingerprint = fingerprint_ngram(&gram);
        if !fingerprints.contains(&fingerprint) || !seen.insert(fingerprint) {
            continue;
        }
        overlaps.push(OverlappingSpan {
            ngram: gram,
            fingerprint,
            candidate_start: start,
            candidate_end: start + n,
            sample_index: None,
        });
    }

    let clean = overlaps.is_empty();
    CopyGuardReport { overlaps, clean }
}

/// Reject a candidate output that copies the raw text of the samples.
pub fn assert_no_copy(candidate: &str, samples: &[String]) -> Result<(), CopyError> {
    assert_no_copy_with_size(candidate, samples, DEFAULT_NGRAM_SIZE)
}

/// Reject a candidate output that copies the raw text of the samples, with an
/// explicit n-gram size.
pub fn assert_no_copy_with_size(
    candidate: &str,
    samples: &[String],
    n: usize,
) -> Result<(), CopyError> {
    let report = copy_guard(candidate, samples, n);
    report_to_result(&report)
}

/// Reject a candidate output that copies the writer's samples, using only
/// the stored fingerprints.
pub fn assert_no_copy_against_fingerprints(
    candidate: &str,
    fingerprints: &BTreeSet<u64>,
) -> Result<(), CopyError> {
    assert_no_copy_against_fingerprints_with_size(candidate, fingerprints, DEFAULT_NGRAM_SIZE)
}

/// Reject a candidate output that copies the writer's samples, using only
/// the stored fingerprints and an explicit n-gram size.
pub fn assert_no_copy_against_fingerprints_with_size(
    candidate: &str,
    fingerprints: &BTreeSet<u64>,
    n: usize,
) -> Result<(), CopyError> {
    let report = copy_guard_against_fingerprints(candidate, fingerprints, n);
    report_to_result(&report)
}

fn report_to_result(report: &CopyGuardReport) -> Result<(), CopyError> {
    if report.clean {
        return Ok(());
    }
    let example = report
        .overlaps
        .first()
        .map(|overlap| overlap.ngram.clone())
        .unwrap_or_default();
    Err(CopyError::Repeated {
        matches: report.overlaps.len(),
        example,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<String> {
        vec![
            "Hi Sam, quick update on the launch. We are on track for Thursday. Thanks!".to_string(),
        ]
    }

    #[test]
    fn a_copy_of_a_sample_phrase_is_caught() {
        let candidate = "Hello there. Quick update on the launch. We are on track for Thursday.";
        let report = copy_guard(candidate, &samples(), DEFAULT_NGRAM_SIZE);
        assert!(!report.clean);
        assert!(!report.overlaps.is_empty());
    }

    #[test]
    fn unrelated_output_is_clean() {
        let candidate = "The quarterly numbers look good and we should ship the update tomorrow.";
        assert!(copy_guard(candidate, &samples(), DEFAULT_NGRAM_SIZE).clean);
    }

    #[test]
    fn matching_ignores_case_and_punctuation() {
        let candidate = "QUICK UPDATE, ON THE LAUNCH! We are on track for Thursday.";
        let report = copy_guard(candidate, &samples(), DEFAULT_NGRAM_SIZE);
        assert!(!report.clean);
    }

    #[test]
    fn short_candidates_never_overlap() {
        let candidate = "Quick update on the launch.";
        assert!(copy_guard(candidate, &samples(), DEFAULT_NGRAM_SIZE).clean);
    }

    #[test]
    fn fingerprints_detect_the_same_copy() {
        let candidate = "Hi Sam, quick update on the launch. We are on track for Thursday. Thanks!";
        let fingerprints = sample_fingerprints(&samples(), DEFAULT_NGRAM_SIZE);
        let report = copy_guard_against_fingerprints(candidate, &fingerprints, DEFAULT_NGRAM_SIZE);
        assert!(!report.clean);
        assert_eq!(report.overlaps[0].sample_index, None);
    }

    #[test]
    fn fingerprints_of_other_text_do_not_match() {
        let fingerprints = sample_fingerprints(&samples(), DEFAULT_NGRAM_SIZE);
        let candidate = "Nothing in this sentence appears in the stored sample at all.";
        assert!(
            copy_guard_against_fingerprints(candidate, &fingerprints, DEFAULT_NGRAM_SIZE).clean
        );
    }

    #[test]
    fn assert_no_copy_reports_the_repeated_phrase() {
        let candidate = "Quick update on the launch. We are on track for Thursday.";
        let error = assert_no_copy(candidate, &samples()).unwrap_err();
        match error {
            CopyError::Repeated { matches, example } => {
                assert!(matches >= 1);
                assert!(example.contains("quick update"));
            }
        }
    }

    #[test]
    fn assert_no_copy_passes_clean_output() {
        assert!(assert_no_copy("A completely different sentence.", &samples()).is_ok());
    }

    #[test]
    fn fingerprints_are_stable_and_distinct() {
        assert_eq!(fingerprint_ngram("a b c"), fingerprint_ngram("a b c"));
        assert_ne!(fingerprint_ngram("a b c"), fingerprint_ngram("a b d"));
    }

    #[test]
    fn a_reported_overlap_points_at_the_candidate_words() {
        let candidate =
            "Totally new words. Quick update on the launch. We are on track for Thursday.";
        let report = copy_guard(candidate, &samples(), DEFAULT_NGRAM_SIZE);
        let overlap = &report.overlaps[0];
        assert_eq!(
            overlap.candidate_end - overlap.candidate_start,
            DEFAULT_NGRAM_SIZE
        );
    }

    #[test]
    fn empty_fingerprints_never_match() {
        let report =
            copy_guard_against_fingerprints("Quick update on the launch.", &BTreeSet::new(), 8);
        assert!(report.clean);
    }
}
