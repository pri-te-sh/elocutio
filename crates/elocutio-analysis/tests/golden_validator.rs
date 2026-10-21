//! The golden-set validator.
//!
//! The golden set is *ground truth*: every `expected` edit is an edit a
//! careful human copy editor would actually make, and every
//! `utf16_start`/`utf16_end` pair must point at exactly that quote inside
//! the item's `text`.
//!
//! A wrong offset is not a false positive — it is a false *failure*. The
//! harness compares spans, so an off-by-one offset makes a correct system
//! look wrong, and the regression gate then fires on a healthy build. That
//! is the worst possible failure mode for a golden set, and it is silent:
//! the JSON still parses, the tests still pass, and the score quietly rots.
//!
//! So the offsets are never trusted. This file re-derives every span from
//! the quote and the text and asserts the result, and it checks the
//! structural invariants the harness depends on.

use std::collections::{BTreeMap, HashMap, HashSet};

use elocutio_analysis::golden::{
    Difficulty, ErrorType, GoldenItem, GoldenSet, ItemSource, LengthBucket, load_from_json,
};

const SEED: &str = include_str!("data/seed_golden.json");

fn set() -> GoldenSet {
    load_from_json(SEED).expect("the golden set parses")
}

/// The UTF-16 code units of `text`.
fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// The byte offset of the `index`-th occurrence of `needle` in `text`.
///
/// Returns `None` when the needle does not occur that many times.
fn occurrence_byte_offset(text: &str, needle: &str, index: usize) -> Option<usize> {
    let mut found = 0usize;
    let mut start = 0usize;
    while let Some(offset) = text[start..].find(needle) {
        let absolute = start + offset;
        if found == index {
            return Some(absolute);
        }
        found += 1;
        start = absolute + 1;
    }
    None
}

/// How many times `needle` occurs in `text`.
fn occurrence_count(text: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    text.matches(needle).count()
}

/// The UTF-16 span of the `index`-th occurrence of `needle` in `text`.
fn expected_span(text: &str, needle: &str, index: usize) -> Option<(usize, usize)> {
    let byte_start = occurrence_byte_offset(text, needle, index)?;
    let byte_end = byte_start + needle.len();
    let utf16_start = text[..byte_start].encode_utf16().count();
    let utf16_end = text[..byte_end].encode_utf16().count();
    Some((utf16_start, utf16_end))
}

// =====================================================================
// The core validator
// =====================================================================

/// Every expected edit must point at its quote, and the quote must really
/// be there.
///
/// The span is re-derived from the text rather than read from the file, so
/// a hand-edited or mis-serialised offset fails here.
#[test]
fn every_expected_span_is_exactly_where_the_quote_is() {
    let set = set();
    let mut checked = 0usize;

    for item in &set.items {
        let units = utf16(&item.text);

        for edit in &item.expected {
            assert!(
                !edit.original.is_empty(),
                "{}: an expected edit has an empty quote",
                item.id
            );

            // The span must be inside the text.
            assert!(
                edit.utf16_start <= edit.utf16_end,
                "{}: span {}..{} is inverted",
                item.id,
                edit.utf16_start,
                edit.utf16_end
            );
            assert!(
                edit.utf16_end <= units.len(),
                "{}: span {}..{} runs past the end of the text ({} UTF-16 units)",
                item.id,
                edit.utf16_start,
                edit.utf16_end,
                units.len()
            );

            // The span must contain exactly the quote.
            let slice = String::from_utf16(&units[edit.utf16_start..edit.utf16_end])
                .expect("the span is a valid UTF-16 sequence");
            assert_eq!(
                slice, edit.original,
                "{}: the span {}..{} contains {slice:?}, not the quote {:?}",
                item.id, edit.utf16_start, edit.utf16_end, edit.original
            );

            // The span must be the *first* place the quote occurs, unless
            // the item deliberately exercises an ambiguous quote. A golden
            // edit that claims an offset somewhere other than the first
            // occurrence is almost always a hand-typed mistake.
            if occurrence_count(&item.text, &edit.original) == 1 {
                let (start, end) = expected_span(&item.text, &edit.original, 0)
                    .expect("the quote occurs once, so it has one span");
                assert_eq!(
                    (edit.utf16_start, edit.utf16_end),
                    (start, end),
                    "{}: the span does not match the derived span for {:?}",
                    item.id,
                    edit.original
                );
            } else {
                // An ambiguous quote must still land on *one* of the
                // occurrences, never between them.
                let spans: Vec<(usize, usize)> = (0..occurrence_count(&item.text, &edit.original))
                    .filter_map(|index| expected_span(&item.text, &edit.original, index))
                    .collect();
                assert!(
                    spans.contains(&(edit.utf16_start, edit.utf16_end)),
                    "{}: the span {}..{} is not one of the {} occurrences of {:?}",
                    item.id,
                    edit.utf16_start,
                    edit.utf16_end,
                    spans.len(),
                    edit.original
                );
            }

            checked += 1;
        }
    }

    assert!(checked > 0, "the golden set has no expected edits at all");
}

/// Every quote must really occur in its text.
#[test]
fn every_quote_occurs_in_its_text() {
    let set = set();
    for item in &set.items {
        for edit in &item.expected {
            assert!(
                item.text.contains(&edit.original),
                "{}: the quote {:?} does not occur in the text",
                item.id,
                edit.original
            );
        }
    }
}

/// No edit may be a no-op.
///
/// A no-op edit can never be applied: the resolver rejects it outright
/// with `ResolveError::NoChange`, so it can never be a true positive and
/// it silently caps the achievable F0.5. The old seed set carried three
/// of these.
#[test]
fn no_expected_edit_is_a_no_op() {
    let set = set();
    for item in &set.items {
        for edit in &item.expected {
            assert_ne!(
                edit.original, edit.replacement,
                "{}: the edit replaces {:?} with itself",
                item.id, edit.original
            );
        }
    }
}

/// Item ids must be unique.
///
/// The harness matches predictions to items by id, so a duplicate id
/// means one item is scored twice and another is never scored.
#[test]
fn item_ids_are_unique() {
    let set = set();
    let mut seen = HashSet::new();
    for item in &set.items {
        assert!(
            seen.insert(item.id.as_str()),
            "duplicate item id: {}",
            item.id
        );
    }
    assert_eq!(seen.len(), set.len());
}

/// The harness computes a stratum's F0.5 over the edits in that stratum.
/// An item with no expected edits contributes nothing to either the true
/// positive or the false negative count, so a stratum that contains one
/// reports F0.5 = 0 no matter how well the system does.
///
/// So: a no-error control must have no expected edits, and an item with
/// an error must have at least one.
#[test]
fn controls_have_no_edits_and_error_items_have_edits() {
    let set = set();
    for item in &set.items {
        if item.error_type == ErrorType::None {
            assert!(
                item.expected.is_empty(),
                "{}: a no-error control must not carry expected edits",
                item.id
            );
        } else {
            assert!(
                !item.expected.is_empty(),
                "{}: an item with error type {:?} must carry at least one expected edit",
                item.id,
                item.error_type
            );
        }
    }
}

/// The edit's own error type must agree with the item's error type.
///
/// A mismatch means the item is scored under one stratum while its edit
/// claims another, which makes the per-stratum report meaningless.
#[test]
fn an_edits_error_type_matches_its_item() {
    let set = set();
    for item in &set.items {
        for edit in &item.expected {
            assert_eq!(
                edit.error_type, item.error_type,
                "{}: the edit {:?} is tagged {:?} but the item is tagged {:?}",
                item.id, edit.original, edit.error_type, item.error_type
            );
        }
    }
}

/// No two items may share a passage.
///
/// A duplicated passage lets the same text be scored under two strata
/// and makes the per-stratum counts misleading.
#[test]
fn no_two_items_share_a_passage() {
    let set = set();
    let mut seen: HashMap<&str, &str> = HashMap::new();
    for item in &set.items {
        if let Some(previous) = seen.insert(item.text.as_str(), item.id.as_str()) {
            panic!(
                "the passage {:?} appears in both {} and {}",
                &item.text[..item.text.len().min(60)],
                previous,
                item.id
            );
        }
    }
}

/// The set must actually be big enough to be worth measuring.
///
/// The plan (§8, "Quality evaluation") asks for 200-500 items. A set that
/// silently shrinks to a handful of items would still pass every other
/// test in this file, and would quietly weaken every metric the gate
/// depends on.
#[test]
fn the_set_is_sized_for_the_plan() {
    let set = set();
    assert!(
        (200..=500).contains(&set.len()),
        "the plan asks for 200-500 golden items, the set has {}",
        set.len()
    );
}

// =====================================================================
// The stratification targets
// =====================================================================

/// The minimum number of items every error type must carry.
const MIN_PER_ERROR_TYPE: usize = 8;

/// The minimum share of no-error controls, as a percentage.
///
/// The plan requires no-error controls so the over-edit rate is measured
/// against a real baseline rather than against nothing.
const MIN_CONTROL_PERCENT: usize = 15;

/// Every error type must be represented, and the common ones well covered.
#[test]
fn every_error_type_is_well_covered() {
    let set = set();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for item in &set.items {
        *counts.entry(item.error_type.label()).or_default() += 1;
    }

    for error_type in ErrorType::ALL {
        let label = error_type.label();
        let count = counts.get(label).copied().unwrap_or(0);
        assert!(
            count >= MIN_PER_ERROR_TYPE,
            "the error type {label:?} has only {count} items, the floor is {MIN_PER_ERROR_TYPE}\n\
             full distribution: {counts:?}"
        );
    }

    // The common types carry a good deal more than the floor.
    for label in ["grammar", "spelling", "punctuation", "capitalization"] {
        let count = counts.get(label).copied().unwrap_or(0);
        assert!(
            count >= 20,
            "the common error type {label:?} has only {count} items\n\
             full distribution: {counts:?}"
        );
    }
}

/// The set must contain enough no-error controls to measure the
/// over-edit rate.
#[test]
fn the_set_has_enough_no_error_controls() {
    let set = set();
    let controls = set.control_count();
    let percent = 100 * controls / set.len();

    assert!(
        controls > 0,
        "the set has no no-error controls, so the over-edit rate is unmeasurable"
    );
    assert!(
        percent >= MIN_CONTROL_PERCENT,
        "the set has {controls} no-error controls ({percent}%), the floor is {MIN_CONTROL_PERCENT}%\n\
         full distribution: {counts:?}",
        counts = {
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for item in &set.items {
                *counts.entry(item.error_type.label()).or_default() += 1;
            }
            counts
        }
    );
}

/// Every stratification axis must be populated.
#[test]
fn every_stratification_axis_is_populated() {
    let set = set();

    for error_type in ErrorType::ALL {
        assert!(
            set.items.iter().any(|item| item.error_type == error_type),
            "no item has the error type {error_type:?}"
        );
    }
    for length in LengthBucket::ALL {
        assert!(
            set.items.iter().any(|item| item.length == length),
            "no item has the length bucket {length:?}"
        );
    }
    for source in ItemSource::ALL {
        assert!(
            set.items.iter().any(|item| item.source == source),
            "no item has the source {source:?}"
        );
    }
    for difficulty in Difficulty::ALL {
        assert!(
            set.items.iter().any(|item| item.difficulty == difficulty),
            "no item has the difficulty {difficulty:?}"
        );
    }
}

/// The set must contain the hard cases the harness has to survive:
/// ambiguous quotes, deletions, punctuation-only changes, contractions
/// and repeated words.
///
/// These are the cases that break a naive resolver or a naive matcher, so
/// a set without them would not exercise the parts of the code they were
/// written for.
#[test]
fn the_set_contains_the_hard_cases() {
    let set = set();

    // A quote that occurs more than once is ambiguous: the resolver must
    // refuse to guess which occurrence was meant.
    let ambiguous = set
        .items
        .iter()
        .flat_map(|item| {
            item.expected
                .iter()
                .filter(move |edit| occurrence_count(&item.text, &edit.original) > 1)
                .map(move |_| item.id.as_str())
        })
        .collect::<HashSet<_>>();
    assert!(
        ambiguous.len() >= 5,
        "the set has only {} items with an ambiguous quote, the floor is 5",
        ambiguous.len()
    );

    // A deletion: the replacement is empty.
    let deletions = set
        .items
        .iter()
        .filter(|item| item.expected.iter().any(|edit| edit.replacement.is_empty()))
        .count();
    assert!(
        deletions >= 5,
        "the set has only {deletions} items with a deletion, the floor is 5"
    );

    // A change that touches only punctuation.
    let punctuation_only = set
        .items
        .iter()
        .flat_map(|item| {
            item.expected.iter().filter(move |edit| {
                strip_punctuation(&edit.original) == strip_punctuation(&edit.replacement)
                    && edit.original != edit.replacement
                    && !edit.original.is_empty()
            })
        })
        .count();
    assert!(
        punctuation_only >= 10,
        "the set has only {punctuation_only} punctuation-only edits, the floor is 10"
    );

    // A contraction, in either direction.
    let contractions = set
        .items
        .iter()
        .flat_map(|item| {
            item.expected
                .iter()
                .filter(move |edit| edit.original.contains('\'') || edit.replacement.contains('\''))
        })
        .count();
    assert!(
        contractions >= 10,
        "the set has only {contractions} contraction edits, the floor is 10"
    );

    // A repeated word.
    let repeated = set
        .items
        .iter()
        .flat_map(|item| {
            item.expected.iter().filter(move |edit| {
                let words: Vec<&str> = edit.original.split_whitespace().collect();
                words.len() >= 2 && words[0] == words[1]
            })
        })
        .count();
    assert!(
        repeated >= 5,
        "the set has only {repeated} repeated-word edits, the floor is 5"
    );
}

/// Strip everything that is not a letter or a digit.
fn strip_punctuation(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .collect()
}

/// Every item must be a real passage, not filler.
///
/// The set is scored on prose that resembles what a person actually
/// writes, so an item that is a single word, or a string of repeated
/// tokens, would inflate the score without testing anything. The one
/// exception is the very shortest item, which exists on purpose as the
/// extreme case of the short bucket; it is named here so that adding
/// another one-word item cannot slip through.
#[test]
fn every_item_is_real_prose() {
    const ONE_WORD_EXCEPTIONS: [&str; 1] = ["g-punct-01"];

    let set = set();
    for item in &set.items {
        let words: Vec<&str> = item.text.split_whitespace().collect();
        if !ONE_WORD_EXCEPTIONS.contains(&item.id.as_str()) {
            assert!(
                words.len() >= 3,
                "{}: the passage has only {} words: {:?}",
                item.id,
                words.len(),
                item.text
            );
        }

        // No word may repeat three times in a row.
        let mut run = 1usize;
        for pair in words.windows(2) {
            if pair[0] == pair[1] {
                run += 1;
                assert!(
                    run < 3,
                    "{}: the passage repeats {:?} in a row: {:?}",
                    item.id,
                    pair[0],
                    item.text
                );
            } else {
                run = 1;
            }
        }
    }
}

/// A golden item must not be a *copy* of another item's text with a
/// different label, and the passage must not be a substring of another
/// passage, which would make the two items near-duplicates.
#[test]
fn no_passage_is_a_prefix_of_another() {
    let set = set();
    let texts: Vec<&str> = set.items.iter().map(|item| item.text.as_str()).collect();
    for (index, text) in texts.iter().enumerate() {
        for (other_index, other) in texts.iter().enumerate() {
            if index == other_index {
                continue;
            }
            assert!(
                !other.starts_with(text),
                "the passage {:?} is a prefix of another passage",
                &text[..text.len().min(60)]
            );
        }
    }
}

// =====================================================================
// The stratification report
// =====================================================================

/// A labelling function for one stratification axis.
type AxisLabel = fn(&GoldenItem) -> &str;

/// Print the distribution of the set across every axis.
///
/// The output is the human-readable form of the same numbers the
/// assertions above pin down, so a reviewer can see the shape of the set
/// without running a script.
#[test]
fn the_stratification_report() {
    let set = set();
    let total = set.len();

    let mut report = String::new();
    report.push_str(&format!(
        "\nGolden set v{}: {total} items, {} expected edits\n",
        set.version,
        set.items
            .iter()
            .map(|item| item.expected.len())
            .sum::<usize>()
    ));

    let axes: [(&str, AxisLabel); 4] = [
        ("error type", |item| item.error_type.label()),
        ("length", |item| item.length.label()),
        ("source", |item| item.source.label()),
        ("difficulty", |item| item.difficulty.label()),
    ];

    for (axis_name, label) in axes {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for item in &set.items {
            *counts.entry(label(item)).or_default() += 1;
        }
        report.push_str(&format!("\n{axis_name}:\n"));
        for (name, count) in &counts {
            report.push_str(&format!(
                "  {name:<16} {count:>4}  {:>5.1}%\n",
                100.0 * *count as f64 / total as f64
            ));
        }
        let sum: usize = counts.values().sum();
        assert_eq!(
            sum, total,
            "the {axis_name} axis does not partition the set"
        );
    }

    // Every axis must cover every value of its enum.
    assert_eq!(
        set.items
            .iter()
            .map(|item| item.error_type.label())
            .collect::<HashSet<_>>()
            .len(),
        ErrorType::ALL.len(),
        "the error type axis does not cover every variant"
    );
    assert_eq!(
        set.items
            .iter()
            .map(|item| item.length.label())
            .collect::<HashSet<_>>()
            .len(),
        LengthBucket::ALL.len(),
        "the length axis does not cover every bucket"
    );
    assert_eq!(
        set.items
            .iter()
            .map(|item| item.source.label())
            .collect::<HashSet<_>>()
            .len(),
        ItemSource::ALL.len(),
        "the source axis does not cover every source"
    );
    assert_eq!(
        set.items
            .iter()
            .map(|item| item.difficulty.label())
            .collect::<HashSet<_>>()
            .len(),
        Difficulty::ALL.len(),
        "the difficulty axis does not cover every difficulty"
    );

    // Short items dominate, but the long ones must exist: a resolver bug
    // that only shows up on a paragraph would go unnoticed otherwise.
    let short = set
        .items
        .iter()
        .filter(|item| item.length == LengthBucket::Short)
        .count();
    let long = set
        .items
        .iter()
        .filter(|item| item.length == LengthBucket::Long)
        .count();
    assert!(
        short > total / 3,
        "only {short} of {total} items are short, short items should dominate"
    );
    assert!(long >= 20, "only {long} items are long, the floor is 20");

    // The set must hold a spread of difficulties, and the hard ones must
    // be a meaningful share.
    let hard = set
        .items
        .iter()
        .filter(|item| item.difficulty == Difficulty::Hard)
        .count();
    assert!(hard >= 40, "only {hard} items are hard, the floor is 40");

    println!("{report}");
}
