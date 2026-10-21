#![forbid(unsafe_code)]

//! Local voice profiles.
//!
//! A profile is an explainable style card, not a fine-tuned model. It is built
//! from uploaded samples and feedback, stored locally, and injected into
//! rewrite/compose prompts. See `docs/VOICE-LEARNING.md` for the design.
//!
//! The crate covers four concerns:
//!
//! * [`copy_guard`] — 8-gram overlap detection, so a rewrite never repeats
//!   the writer's sample content.
//! * [`profile`] — the profile itself, trait overrides, and the learning
//!   rules.
//! * [`feedback`] — the event model, weights, and 180-day decay.
//! * [`guidance`] — the short style card that is sent to a provider.

pub mod copy_guard;
pub mod features;
pub mod feedback;
pub mod guidance;
pub mod profile;
pub mod retention;

pub use copy_guard::{
    CopyError, CopyGuardReport, DEFAULT_NGRAM_SIZE, OverlappingSpan, assert_no_copy,
    assert_no_copy_against_fingerprints, assert_no_copy_against_fingerprints_with_size,
    assert_no_copy_with_size, copy_guard, copy_guard_against_fingerprints, fingerprint_ngram,
    ngram_fingerprints, sample_fingerprints,
};
pub use features::{analyze_samples, contractions_count, round1, sentence_spans, word_tokens};
pub use feedback::{
    CATEGORY_SUPPRESSION_THRESHOLD, EditOrigin, FEEDBACK_HALF_LIFE_DAYS, FeedbackSummary, Rating,
    SuggestionEvent, SuggestionEventKind, TERM_PREFERENCE_THRESHOLD, TermPreference, VoiceEvent,
    VoiceEventKind, WeightedTally, category_weight, decay_factor, event_weight, has_contraction,
    infer_term_preferences, summarize_suggestions, suppressed_categories,
};
pub use guidance::voice_prompt_guidance;
pub use profile::{
    LearnedTrait, TraitSource, TraitValue, VoiceContext, VoiceProfile, VoiceStrength, VoiceTrait,
    accepted_edit_weight, add_sample_guards, apply_rating, apply_voice_events,
    clear_trait_override, default_fingerprint_size, learn_from_suggestions, remove_sample_guards,
    set_trait_override,
};
pub use retention::{
    FEEDBACK_RETENTION_DAYS, MAX_STORED_TEXT_CHARS, StoredSuggestionEvent, feedback_fingerprints,
    is_within_retention, prune_events, truncate,
};

/// Confidence bands shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
            VoiceTrait::learned(
                TraitValue::Text("frequent".into()),
                0.9,
                vec!["12 emails".into()],
            ),
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
    fn the_analyzer_never_stores_the_sample_text() {
        let samples = vec![
            "This is a very distinctive sentence that should never appear in the profile."
                .to_string(),
        ];
        let profile = analyze_samples("vp1", "Work", VoiceContext::Email, &samples);
        let json = serde_json::to_string(&profile).unwrap();
        assert!(!json.contains("distinctive sentence"));
    }

    #[test]
    fn guidance_is_only_emitted_for_usable_profiles() {
        let mut profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        assert!(voice_prompt_guidance(&profile).is_none());
        profile.confidence = 0.5;
        profile.strength = VoiceStrength::Balanced;
        profile.traits.insert(
            "avg_sentence_words".into(),
            VoiceTrait::learned(TraitValue::Number(14.0), 0.8, vec![]),
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
            VoiceTrait::learned(TraitValue::Number(3.2), 0.7, vec![]),
        );
        assert!(profile.is_usable());
    }
}
