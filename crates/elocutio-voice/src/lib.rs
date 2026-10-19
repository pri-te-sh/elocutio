#![forbid(unsafe_code)]

//! Local voice profiles.
//!
//! A profile is an explainable style card, not a fine-tuned model. It is built
//! from uploaded samples and feedback, stored locally, and injected into
//! rewrite/compose prompts. See `docs/VOICE-LEARNING.md` for the design.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How strongly a voice profile influences a task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceStrength {
    Off,
    Light,
    #[default]
    Balanced,
    Strong,
}

/// The writing context a profile belongs to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceContext {
    Email,
    Chat,
    Document,
    Social,
    Support,
    #[default]
    General,
}

/// A single measured trait.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TraitValue {
    Number(f64),
    Bool(bool),
    Text(String),
}

/// A learned trait with confidence and evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceTrait {
    pub value: TraitValue,
    pub confidence: f64,
    pub evidence: Vec<String>,
}

/// Confidence bands shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceBand {
    Low,
    Medium,
    High,
    VeryHigh,
}

/// Map sample size to a user-facing confidence band.
pub fn confidence_band(sample_words: u64) -> ConfidenceBand {
    match sample_words {
        0..=299 => ConfidenceBand::Low,
        300..=1_499 => ConfidenceBand::Medium,
        1_500..=4_999 => ConfidenceBand::High,
        _ => ConfidenceBand::VeryHigh,
    }
}

/// An explainable, local voice profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceProfile {
    pub id: String,
    pub name: String,
    pub context: VoiceContext,
    pub language: String,
    pub strength: VoiceStrength,
    pub confidence: f64,
    pub summary: String,
    /// Trait key -> learned value, e.g. "avg_sentence_words".
    pub traits: BTreeMap<String, VoiceTrait>,
}

impl VoiceProfile {
    pub fn new(id: impl Into<String>, name: impl Into<String>, context: VoiceContext) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            context,
            language: "en".into(),
            strength: VoiceStrength::Off,
            confidence: 0.0,
            summary: String::new(),
            traits: BTreeMap::new(),
        }
    }

    /// A profile needs enough signal to be worth injecting.
    pub fn is_usable(&self) -> bool {
        self.confidence >= 0.3 && !self.traits.is_empty()
    }

    pub fn trait_value(&self, key: &str) -> Option<&TraitValue> {
        self.traits.get(key).map(|item| &item.value)
    }
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
        .collect::<std::collections::BTreeSet<_>>()
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
            VoiceTrait {
                value,
                confidence,
                evidence: vec![evidence],
            },
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

/// Compact prompt guidance derived only from confident profile traits.
pub fn voice_prompt_guidance(profile: &VoiceProfile) -> Option<String> {
    if !profile.is_usable() || profile.strength == VoiceStrength::Off {
        return None;
    }

    let mut lines = Vec::new();
    if let Some(TraitValue::Number(value)) = profile.trait_value("avg_sentence_words") {
        lines.push(format!(
            "Keep sentences around {value:.0} words on average."
        ));
    }
    if let Some(TraitValue::Number(value)) = profile.trait_value("contractions_per_100_words") {
        if *value >= 1.0 {
            lines.push("Use contractions naturally.".to_string());
        } else {
            lines.push("Avoid contractions unless they are already natural.".to_string());
        }
    }
    if let Some(TraitValue::Number(value)) = profile.trait_value("first_person_per_100_words")
        && *value >= 2.0
    {
        lines.push("First person is part of this voice.".to_string());
    }
    if let Some(TraitValue::Number(value)) = profile.trait_value("questions_per_1000_words")
        && *value >= 15.0
    {
        lines.push("Occasional rhetorical questions fit this voice.".to_string());
    }
    if let Some(TraitValue::Number(value)) = profile.trait_value("avg_word_length")
        && *value < 4.2
    {
        lines.push("Prefer short, plain words.".to_string());
    }

    if lines.is_empty() {
        return None;
    }

    Some(format!(
        "Match the writer's voice. Do not copy their content or mention this guidance.\n{}",
        lines.join("\n")
    ))
}

fn word_tokens(text: &str) -> Vec<String> {
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

fn sentence_spans(text: &str) -> Vec<&str> {
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

fn contractions_count(lower: &str) -> usize {
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

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_bands_are_monotonic() {
        assert_eq!(confidence_band(0), ConfidenceBand::Low);
        assert_eq!(confidence_band(299), ConfidenceBand::Low);
        assert_eq!(confidence_band(300), ConfidenceBand::Medium);
        assert_eq!(confidence_band(1_499), ConfidenceBand::Medium);
        assert_eq!(confidence_band(1_500), ConfidenceBand::High);
        assert_eq!(confidence_band(5_000), ConfidenceBand::VeryHigh);
    }

    #[test]
    fn new_profiles_are_not_usable_until_trained() {
        let profile = VoiceProfile::new("vp1", "Work email", VoiceContext::Email);
        assert!(!profile.is_usable());
        assert_eq!(profile.strength, VoiceStrength::Off);
    }

    #[test]
    fn profile_round_trips_through_json() {
        let mut profile = VoiceProfile::new("vp1", "Work email", VoiceContext::Email);
        profile.confidence = 0.82;
        profile.traits.insert(
            "contractions".into(),
            VoiceTrait {
                value: TraitValue::Text("frequent".into()),
                confidence: 0.9,
                evidence: vec!["12 emails".into()],
            },
        );
        let json = serde_json::to_string(&profile).unwrap();
        let decoded: VoiceProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(profile, decoded);
    }

    #[test]
    fn analyzer_extracts_rhythm_and_diction_without_storing_samples() {
        let base = [
            "I think we can ship this today. It isn't perfect, but it's useful!\n\nCan you take a look?".to_string(),
            "I'd rather keep it simple. We don't need a long memo; a short note is better.\n\nThanks!".to_string(),
        ];
        let samples = (0..10)
            .flat_map(|_| base.iter().cloned())
            .collect::<Vec<_>>();
        let profile = analyze_samples("vp1", "Work", VoiceContext::Email, &samples);
        assert!(profile.is_usable());
        assert!(profile.traits.contains_key("avg_sentence_words"));
        assert!(profile.traits.contains_key("contractions_per_100_words"));
        assert!(profile.summary.contains("words per sentence"));
    }

    #[test]
    fn guidance_is_only_emitted_for_usable_profiles() {
        let mut profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        assert!(voice_prompt_guidance(&profile).is_none());
        profile.confidence = 0.5;
        profile.strength = VoiceStrength::Balanced;
        profile.traits.insert(
            "avg_sentence_words".into(),
            VoiceTrait {
                value: TraitValue::Number(14.0),
                confidence: 0.8,
                evidence: vec![],
            },
        );
        let guidance = voice_prompt_guidance(&profile).unwrap();
        assert!(guidance.contains("14 words"));
    }

    #[test]
    fn usable_profiles_need_confidence_and_traits() {
        let mut profile = VoiceProfile::new("vp1", "Work email", VoiceContext::Email);
        profile.confidence = 0.5;
        assert!(!profile.is_usable());
        profile.traits.insert(
            "formality".into(),
            VoiceTrait {
                value: TraitValue::Number(3.2),
                confidence: 0.7,
                evidence: vec![],
            },
        );
        assert!(profile.is_usable());
    }
}
