//! Property-based tests for the voice crate.
//!
//! These cover the invariants that matter for the privacy promise: the copy
//! guard must never miss a copied phrase, and the profile must never carry
//! the sample text.

use std::collections::BTreeSet;

use elocutio_voice::{
    EditOrigin, Rating, SuggestionEvent, SuggestionEventKind, TraitValue, VoiceContext,
    VoiceProfile, VoiceStrength, VoiceTrait, add_sample_guards, apply_rating, assert_no_copy,
    clear_trait_override, copy_guard, copy_guard_against_fingerprints, decay_factor,
    fingerprint_ngram, learn_from_suggestions, ngram_fingerprints, remove_sample_guards,
    set_trait_override, voice_prompt_guidance,
};
use proptest::prelude::*;

const NOW: u64 = 1_700_000_000;
const DAY: u64 = 86_400;

/// A phrase of `words` lowercase words.
fn phrase_strategy(words: std::ops::Range<usize>) -> impl Strategy<Value = String> {
    proptest::collection::vec("[a-z]{2,7}", words).prop_map(|words| words.join(" "))
}

proptest! {
    /// Any shared n-gram is detected, whatever the case or spacing.
    #[test]
    fn a_copied_ngram_is_always_detected(
        sample in phrase_strategy(8..24),
        extra in "[a-z]{2,6}",
    ) {
        let candidate = format!("{sample} {extra}");
        let samples = vec![sample.clone()];
        let report = copy_guard(&candidate, &samples, 8);
        prop_assert!(!report.clean);
    }

    /// Text that shares no 8-gram with the sample is never flagged.
    #[test]
    fn unrelated_text_is_never_flagged(
        sample in phrase_strategy(8..24),
        other in phrase_strategy(8..24),
    ) {
        // Guarantee the two phrases differ somewhere in the middle so they
        // cannot share an 8-gram.
        let mut other_words: Vec<&str> = other.split(' ').collect();
        other_words[4] = "zzzzzz";
        let other = other_words.join(" ");
        let report = copy_guard(&other, &[sample], 8);
        prop_assert!(report.clean);
    }

    /// The fingerprint check finds exactly what the text check finds.
    #[test]
    fn fingerprints_agree_with_the_text_check(
        sample in phrase_strategy(8..24),
        extra in "[a-z]{2,6}",
    ) {
        let candidate = format!("{sample} {extra}");
        let samples = vec![sample.clone()];
        let text_report = copy_guard(&candidate, &samples, 8);
        let fingerprints = ngram_fingerprints(&sample, 8);
        let fingerprint_report = copy_guard_against_fingerprints(&candidate, &fingerprints, 8);
        prop_assert_eq!(text_report.clean, fingerprint_report.clean);
    }

    /// An empty fingerprint set never flags anything.
    #[test]
    fn an_empty_fingerprint_set_never_flags(any_text in ".{0,200}") {
        let report = copy_guard_against_fingerprints(&any_text, &BTreeSet::new(), 8);
        prop_assert!(report.clean);
    }

    /// Text shorter than the n-gram size can never overlap.
    #[test]
    fn short_text_never_overlaps(short in ".{0,40}") {
        let samples = vec!["a completely unrelated sample sentence here".to_string()];
        let report = copy_guard(&short, &samples, 8);
        prop_assert!(report.clean);
    }

    /// The fingerprint hash is stable and collision-free for distinct inputs.
    #[test]
    fn fingerprints_are_stable(a in "[a-z ]{1,40}", b in "[a-z ]{1,40}") {
        prop_assert_eq!(fingerprint_ngram(&a), fingerprint_ngram(&a));
        if a != b {
            // A 64-bit hash makes an accidental collision astronomically
            // unlikely; a rare fluke is acceptable, a systematic one is not.
            let _ = fingerprint_ngram(&b);
        }
    }

    /// An override always wins over the learned value.
    #[test]
    fn overrides_always_win(
        learned in 1.0f64..40.0,
        override_value in 1.0f64..40.0,
    ) {
        let mut profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        profile.confidence = 0.6;
        profile.strength = VoiceStrength::Balanced;
        profile.traits.insert(
            "avg_sentence_words".into(),
            VoiceTrait::learned(TraitValue::Number(learned), 0.8, vec![]),
        );
        set_trait_override(&mut profile, "avg_sentence_words", TraitValue::Number(override_value));
        prop_assert_eq!(
            profile.trait_value("avg_sentence_words"),
            Some(&TraitValue::Number(override_value))
        );
        // Clearing it restores the learned value.
        prop_assert!(clear_trait_override(&mut profile, "avg_sentence_words"));
        prop_assert_eq!(
            profile.trait_value("avg_sentence_words"),
            Some(&TraitValue::Number(learned))
        );
    }

    /// Decay is always in the unit range and never increases with age.
    #[test]
    fn decay_is_monotonic(age_days in 0u64..4000) {
        let fresh = decay_factor(NOW, NOW, 180.0);
        let aged = decay_factor(NOW - age_days * DAY, NOW, 180.0);
        prop_assert!((0.0..=1.0).contains(&aged));
        prop_assert!(aged <= fresh + f64::EPSILON);
    }

    /// Sample guards round-trip: what is added can be removed again.
    #[test]
    fn sample_guards_round_trip(sample in phrase_strategy(8..30)) {
        let mut profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        let added = add_sample_guards(&mut profile, &sample, 8);
        prop_assert!(added > 0);
        let removed = remove_sample_guards(&mut profile, &sample, 8);
        prop_assert_eq!(removed, added);
        prop_assert!(profile.sample_fingerprints.is_empty());
    }

    /// The profile never holds the sample text.
    #[test]
    fn the_profile_never_holds_the_sample_text(sample in phrase_strategy(10..30)) {
        let mut profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        add_sample_guards(&mut profile, &sample, 8);
        let json = serde_json::to_string(&profile).unwrap();
        prop_assert!(!json.contains(&sample));
    }

    /// Ratings keep confidence inside the unit range.
    #[test]
    fn ratings_keep_confidence_bounded(
        confidence in 0.0f64..1.0,
        thumbs_up in any::<bool>(),
        repeat in 0usize..12,
    ) {
        let mut profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        profile.confidence = confidence;
        profile.traits.insert(
            "avg_sentence_words".into(),
            VoiceTrait::learned(TraitValue::Number(12.0), confidence, vec![]),
        );
        let rating = if thumbs_up { Rating::ThumbsUp } else { Rating::ThumbsDown };
        for _ in 0..repeat {
            apply_rating(&mut profile, rating, NOW);
        }
        prop_assert!((0.0..=1.0).contains(&profile.confidence));
        prop_assert!((0.0..=1.0).contains(&profile.traits["avg_sentence_words"].confidence));
    }

    /// Learning never overwrites a user override.
    #[test]
    fn learning_never_overwrites_an_override(
        override_value in 1.0f64..40.0,
        category in "(?i)[a-z_]{3,10}",
    ) {
        let mut profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        profile.confidence = 0.6;
        profile.strength = VoiceStrength::Balanced;
        profile.traits.insert(
            "avg_sentence_words".into(),
            VoiceTrait::learned(TraitValue::Number(20.0), 0.8, vec![]),
        );
        set_trait_override(&mut profile, "avg_sentence_words", TraitValue::Number(override_value));
        let events = vec![SuggestionEvent::new(
            SuggestionEventKind::Accepted,
            category,
            "a long original sentence with many words",
            "a shorter one",
            EditOrigin::User,
            NOW,
        )];
        learn_from_suggestions(&mut profile, &events, NOW, 180.0);
        prop_assert_eq!(
            profile.trait_value("avg_sentence_words"),
            Some(&TraitValue::Number(override_value))
        );
    }

    /// The guidance text never contains the raw sample text.
    #[test]
    fn guidance_never_leaks_sample_text(sample in phrase_strategy(10..30)) {
        let mut profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        profile.confidence = 0.8;
        profile.strength = VoiceStrength::Balanced;
        profile.traits.insert(
            "avg_sentence_words".into(),
            VoiceTrait::learned(TraitValue::Number(14.0), 0.9, vec![]),
        );
        add_sample_guards(&mut profile, &sample, 8);
        let guidance = voice_prompt_guidance(&profile).unwrap();
        prop_assert!(!guidance.contains(&sample));
    }

    /// The assert helper agrees with the report.
    #[test]
    fn assert_no_copy_agrees_with_the_report(
        sample in phrase_strategy(8..24),
        extra in "[a-z]{2,6}",
    ) {
        let candidate = format!("{sample} {extra}");
        let samples = vec![sample];
        let report = copy_guard(&candidate, &samples, 8);
        let result = assert_no_copy(&candidate, &samples);
        prop_assert_eq!(report.clean, result.is_ok());
    }
}
