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
