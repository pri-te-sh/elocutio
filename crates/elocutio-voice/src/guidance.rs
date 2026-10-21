//! Prompt guidance: turn a profile into the short style card that is sent
//! to a provider.

use crate::profile::{TraitValue, VoiceProfile, VoiceStrength};

/// Compact prompt guidance derived only from confident profile traits.
///
/// User overrides win over learned values, because `trait_value` resolves the
/// override first.
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
    if let Some(TraitValue::Bool(value)) = profile.trait_value("prefers_concise")
        && *value
    {
        lines.push("Prefer concise phrasing; trim filler.".to_string());
    }
    if let Some(TraitValue::Bool(value)) = profile.trait_value("prefers_contractions") {
        if *value {
            lines.push("Use contractions where natural.".to_string());
        } else {
            lines.push("Avoid contractions unless already present.".to_string());
        }
    }
    if let Some(TraitValue::Bool(value)) = profile.trait_value("uses_contractions") {
        if *value {
            lines.push("Contractions are natural in this voice.".to_string());
        } else {
            lines.push("Write out contractions in full.".to_string());
        }
    }

    if lines.is_empty() {
        return None;
    }

    Some(format!(
        "Match the writer's voice. Do not copy their content or mention this guidance.\n{}",
        lines.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{VoiceContext, VoiceTrait, set_trait_override};

    fn profile_with_trait(key: &str, value: TraitValue) -> VoiceProfile {
        let mut profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        profile.confidence = 0.5;
        profile.strength = VoiceStrength::Balanced;
        profile
            .traits
            .insert(key.into(), VoiceTrait::learned(value, 0.8, vec![]));
        profile
    }

    #[test]
    fn guidance_reflects_the_trait_values() {
        let profile = profile_with_trait("avg_sentence_words", TraitValue::Number(14.0));
        let guidance = voice_prompt_guidance(&profile).unwrap();
        assert!(guidance.contains("14 words"));
    }

    #[test]
    fn guidance_is_none_for_unusable_profiles() {
        let profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        assert!(voice_prompt_guidance(&profile).is_none());
    }

    #[test]
    fn guidance_is_none_when_the_strength_is_off() {
        let mut profile = profile_with_trait("avg_sentence_words", TraitValue::Number(14.0));
        profile.strength = VoiceStrength::Off;
        assert!(voice_prompt_guidance(&profile).is_none());
    }

    #[test]
    fn overrides_win_over_learned_values_in_the_guidance() {
        let mut profile = profile_with_trait("avg_sentence_words", TraitValue::Number(14.0));
        set_trait_override(&mut profile, "avg_sentence_words", TraitValue::Number(6.0));
        let guidance = voice_prompt_guidance(&profile).unwrap();
        assert!(guidance.contains("6 words"));
        assert!(!guidance.contains("14 words"));
    }

    #[test]
    fn contraction_traits_produce_a_line() {
        let profile = profile_with_trait("prefers_contractions", TraitValue::Bool(true));
        let guidance = voice_prompt_guidance(&profile).unwrap();
        assert!(guidance.contains("contractions"));
    }
}
