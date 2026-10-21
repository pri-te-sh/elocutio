//! Feature extraction: turn raw writing samples into aggregate traits.

use std::collections::BTreeSet;

use crate::profile::{TraitValue, VoiceContext, VoiceProfile, VoiceTrait};

/// Split text into lowercase word tokens.
///
/// Words keep internal apostrophes and hyphens so contractions and
/// hyphenated compounds survive the split.
pub fn word_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.trim_matches(|character: char| {
                !character.is_alphabetic() && character != '\'' && character != '-'
            })
            .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// Build an explainable style card from uploaded writing samples.
///
/// Only aggregate traits are retained; the samples themselves are not stored.
pub fn analyze_samples(
    id: impl Into<String>,
    name: impl Into<String>,
    context: VoiceContext,
    samples: &[String],
) -> VoiceProfile {
    let text = samples.join("\n\n");
    let tokens = word_tokens(&text);
    let sentences = sentence_spans(&text);
    let word_count = tokens.len();
    let sentence_count = sentences.len().max(1);
    let paragraph_count = text
        .split("\n\n")
        .filter(|paragraph| !paragraph.trim().is_empty())
        .count()
        .max(1);

    let mut profile = VoiceProfile::new(id, name, context);
    if word_count == 0 {
        return profile;
    }

    let sentence_lengths: Vec<f64> = sentences
        .iter()
        .map(|sentence| word_tokens(sentence).len() as f64)
        .collect();
    let mean_sentence = sentence_lengths.iter().sum::<f64>() / sentence_count as f64;
    let variance = sentence_lengths
        .iter()
        .map(|length| (length - mean_sentence).powi(2))
        .sum::<f64>()
        / sentence_count as f64;

    let lower = text.to_lowercase();
    let contractions = contractions_count(&lower);
    let first_person = tokens
        .iter()
        .filter(|word| matches!(word.as_str(), "i" | "me" | "my" | "we" | "us" | "our"))
        .count();
    let second_person = tokens
        .iter()
        .filter(|word| matches!(word.as_str(), "you" | "your" | "yours"))
        .count();
    let unique_words = tokens
        .iter()
        .map(|word| word.to_lowercase())
        .collect::<BTreeSet<_>>()
        .len();
    let average_word_length = tokens
        .iter()
        .map(|word| word.chars().count())
        .sum::<usize>() as f64
        / word_count as f64;

    let confidence = ((word_count as f64 / 600.0).clamp(0.0, 1.0) * 0.85
        + (samples.len() as f64 * 0.03).min(0.1))
    .min(0.95);
    profile.confidence = confidence;

    let mut insert = |key: &str, value: TraitValue, evidence: String| {
        profile.traits.insert(
            key.to_string(),
            VoiceTrait::learned(value, confidence, vec![evidence]),
        );
    };

    insert(
        "avg_sentence_words",
        TraitValue::Number(round1(mean_sentence)),
        format!("{sentence_count} sentences"),
    );
    insert(
        "sentence_length_variation",
        TraitValue::Number(round1(variance.sqrt())),
        "standard deviation".into(),
    );
    insert(
        "contractions_per_100_words",
        TraitValue::Number(round1(contractions as f64 / word_count as f64 * 100.0)),
        format!("{contractions} contractions"),
    );
    insert(
        "first_person_per_100_words",
        TraitValue::Number(round1(first_person as f64 / word_count as f64 * 100.0)),
        format!("{first_person} first-person tokens"),
    );
    insert(
        "second_person_per_100_words",
        TraitValue::Number(round1(second_person as f64 / word_count as f64 * 100.0)),
        format!("{second_person} second-person tokens"),
    );
    insert(
        "exclamations_per_1000_words",
        TraitValue::Number(round1(
            text.matches('!').count() as f64 / word_count as f64 * 1000.0,
        )),
        "exclamation marks".into(),
    );
    insert(
        "questions_per_1000_words",
        TraitValue::Number(round1(
            text.matches('?').count() as f64 / word_count as f64 * 1000.0,
        )),
        "question marks".into(),
    );
    insert(
        "semicolons_per_1000_words",
        TraitValue::Number(round1(
            text.matches(';').count() as f64 / word_count as f64 * 1000.0,
        )),
        "semicolons".into(),
    );
    insert(
        "em_dashes_per_1000_words",
        TraitValue::Number(round1(
            (text.matches('—').count() + text.matches(" - ").count()) as f64 / word_count as f64
                * 1000.0,
        )),
        "em dashes".into(),
    );
    insert(
        "lexical_diversity",
        TraitValue::Number(round1(unique_words as f64 / word_count as f64)),
        format!("{unique_words} unique words"),
    );
    insert(
        "avg_word_length",
        TraitValue::Number(round1(average_word_length)),
        "characters per word".into(),
    );
    insert(
        "avg_sentences_per_paragraph",
        TraitValue::Number(round1(sentence_count as f64 / paragraph_count as f64)),
        format!("{paragraph_count} paragraphs"),
    );
    insert(
        "uses_contractions",
        TraitValue::Bool(contractions * 100 >= word_count / 2),
        format!("{contractions} contractions"),
    );

    profile.summary = format!(
        "About {:.0} words per sentence, {:.1} contractions per 100 words, {:.0}% first-person, {:.0}% unique words.",
        mean_sentence,
        contractions as f64 / word_count as f64 * 100.0,
        first_person as f64 / word_count as f64 * 100.0,
        unique_words as f64 / word_count as f64 * 100.0
    );

    profile
}

/// Split text into sentences.
pub fn sentence_spans(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (index, character) in text.char_indices() {
        if matches!(character, '.' | '!' | '?') {
            let end = index + character.len_utf8();
            if !text[start..end].trim().is_empty() {
                sentences.push(&text[start..end]);
            }
            start = end;
        }
    }
    if start < text.len() && !text[start..].trim().is_empty() {
        sentences.push(&text[start..]);
    }
    sentences
}

/// Count contractions in already-lowercased text.
pub fn contractions_count(lower: &str) -> usize {
    lower
        .split_whitespace()
        .filter(|word| {
            let word = word
                .trim_matches(|character: char| !character.is_alphabetic() && character != '\'');
            ["n't", "'ll", "'re", "'ve", "'m", "'d", "'s"]
                .iter()
                .any(|suffix| word.ends_with(suffix))
        })
        .count()
}

/// Round to one decimal place.
pub fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_keep_contractions() {
        let tokens = word_tokens("We're going, aren't we?");
        assert!(tokens.contains(&"we're".to_string()));
        assert!(tokens.contains(&"aren't".to_string()));
    }

    #[test]
    fn tokens_drop_punctuation() {
        let tokens = word_tokens("Hello, world!");
        assert_eq!(tokens, vec!["hello".to_string(), "world".to_string()]);
    }

    #[test]
    fn sentences_split_on_terminators() {
        let sentences = sentence_spans("One. Two! Three? Four");
        assert_eq!(sentences.len(), 4);
    }

    #[test]
    fn contractions_are_counted() {
        assert_eq!(contractions_count("we're it's don't"), 3);
    }
}
