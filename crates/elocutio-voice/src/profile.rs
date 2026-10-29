//! The voice profile: traits, user overrides, and the learning rules that
//! turn feedback into traits.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::copy_guard::DEFAULT_NGRAM_SIZE;
use crate::feedback::{
    EditOrigin, FeedbackSummary, Rating, SuggestionEvent, SuggestionEventKind, event_weight,
    summarize_suggestions,
};

/// Where a trait value came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraitSource {
    /// Measured from samples or derived from feedback.
    #[default]
    Learned,
    /// Set by the user. Overrides always win.
    Override,
}

/// A learned trait with confidence and evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceTrait {
    /// The effective value: the user's override when there is one, otherwise
    /// the learned value.
    pub value: TraitValue,
    pub confidence: f64,
    #[serde(default)]
    pub evidence: Vec<String>,
    /// Where the effective value came from. Older profiles deserialize as
    /// `Learned`.
    #[serde(default)]
    pub source: TraitSource,
    /// The learned value, kept while the user overrides the trait.
    ///
    /// Clearing an override restores this value instead of dropping the trait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learned: Option<LearnedTrait>,
}

/// The learned value of a trait, kept aside while a user override is active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearnedTrait {
    pub value: TraitValue,
    pub confidence: f64,
}

impl VoiceTrait {
    /// A learned trait.
    pub fn learned(value: TraitValue, confidence: f64, evidence: Vec<String>) -> Self {
        Self {
            value,
            confidence,
            evidence,
            source: TraitSource::Learned,
            learned: None,
        }
    }

    /// A trait the user set by hand.
    pub fn overridden(value: TraitValue, confidence: f64, evidence: Vec<String>) -> Self {
        Self {
            value,
            confidence,
            evidence,
            source: TraitSource::Override,
            learned: None,
        }
    }

    /// Whether the user set this trait by hand.
    pub fn is_override(&self) -> bool {
        self.source == TraitSource::Override
    }
}

/// A single measured trait.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TraitValue {
    Number(f64),
    Bool(bool),
    Text(String),
}

impl TraitValue {
    /// The numeric value, when the trait holds a number.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            TraitValue::Number(value) => Some(*value),
            _ => None,
        }
    }

    /// The boolean value, when the trait holds a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            TraitValue::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The text value, when the trait holds text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            TraitValue::Text(value) => Some(value),
            _ => None,
        }
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
    /// The fingerprints of the sample n-grams, used by the copy guard.
    ///
    /// The sample text itself is never stored; only these hashes are.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub sample_fingerprints: BTreeSet<u64>,
    /// For fingerprints that more than one sample produced: how many samples
    /// beyond the first hold it.
    ///
    /// Removing one sample must not drop an n-gram another sample still
    /// contains, so shared fingerprints are reference counted. Only shared
    /// fingerprints appear here, so a profile whose samples share nothing
    /// serializes exactly as before, and older profiles (which lack the
    /// field) count every fingerprint once.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sample_fingerprint_shares: BTreeMap<u64, u32>,
    /// The n-gram size the fingerprints were computed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint_ngram_size: Option<usize>,
    /// How many voice events have already been folded into this profile.
    ///
    /// Folding is incremental, so the count keeps the fold idempotent: a
    /// second refresh does not apply the same rating twice.
    #[serde(default)]
    pub applied_voice_events: u64,
}

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
            sample_fingerprints: BTreeSet::new(),
            sample_fingerprint_shares: BTreeMap::new(),
            fingerprint_ngram_size: None,
            applied_voice_events: 0,
        }
    }

    /// A profile needs enough signal to be worth injecting.
    pub fn is_usable(&self) -> bool {
        self.confidence >= 0.3 && !self.traits.is_empty()
    }

    pub fn trait_value(&self, key: &str) -> Option<&TraitValue> {
        self.traits.get(key).map(|item| &item.value)
    }

    /// The trait's source, when the trait exists.
    pub fn trait_source(&self, key: &str) -> Option<TraitSource> {
        self.traits.get(key).map(|item| item.source)
    }

    /// Whether the user set this trait by hand.
    pub fn is_overridden(&self, key: &str) -> bool {
        self.traits.get(key).is_some_and(VoiceTrait::is_override)
    }
}

/// Set a trait the user chose by hand.
///
/// The override replaces the effective value for the same key and is marked
/// so the UI can show it as user-set. The learned value is kept aside, so
/// [`clear_trait_override`] can restore it. Overrides are never replaced by
/// learning.
pub fn set_trait_override(profile: &mut VoiceProfile, key: &str, value: TraitValue) {
    let mut entry = match profile.traits.get(key) {
        Some(existing) => existing.clone(),
        None => VoiceTrait {
            value: value.clone(),
            confidence: 1.0,
            evidence: Vec::new(),
            source: TraitSource::Override,
            learned: None,
        },
    };
    if !entry.is_override() {
        // Remember what learning found, so the override can be undone.
        entry.learned = Some(LearnedTrait {
            value: entry.value.clone(),
            confidence: entry.confidence,
        });
    }
    entry.value = value;
    entry.source = TraitSource::Override;
    entry.confidence = 1.0;
    if !entry.evidence.iter().any(|line| line == "set by you") {
        entry.evidence.push("set by you".to_string());
    }
    profile.traits.insert(key.to_string(), entry);
}

/// Remove a user override so the learned value shows again.
///
/// Returns `true` when an override was removed. When there is a remembered
/// learned value it is restored; otherwise the trait is removed entirely,
/// because the override was its only source.
pub fn clear_trait_override(profile: &mut VoiceProfile, key: &str) -> bool {
    let Some(existing) = profile.traits.get(key) else {
        return false;
    };
    if !existing.is_override() {
        return false;
    }
    let evidence: Vec<String> = existing
        .evidence
        .iter()
        .filter(|line| line.as_str() != "set by you")
        .cloned()
        .collect();
    match existing.learned.clone() {
        Some(learned) => {
            let mut entry = existing.clone();
            entry.value = learned.value;
            entry.confidence = learned.confidence;
            entry.source = TraitSource::Learned;
            entry.learned = None;
            entry.evidence = evidence;
            profile.traits.insert(key.to_string(), entry);
        }
        None => {
            if evidence.is_empty() {
                profile.traits.remove(key);
            } else {
                let mut entry = existing.clone();
                entry.evidence = evidence;
                entry.source = TraitSource::Learned;
                entry.confidence = entry.confidence.min(0.9);
                profile.traits.insert(key.to_string(), entry);
            }
        }
    }
    true
}

/// Add the fingerprints of one sample to the profile.
///
/// The sample text is used to compute the hashes and then discarded; only the
/// hashes are kept. A fingerprint another sample already holds is counted
/// again, so removing either sample later keeps it for the other.
pub fn add_sample_guards(profile: &mut VoiceProfile, sample: &str, n: usize) -> usize {
    let n = n.max(1);
    let fingerprints = crate::copy_guard::ngram_fingerprints(sample, n);
    let added = fingerprints.len();
    for fingerprint in fingerprints {
        if !profile.sample_fingerprints.insert(fingerprint) {
            *profile
                .sample_fingerprint_shares
                .entry(fingerprint)
                .or_insert(0) += 1;
        }
    }
    profile.fingerprint_ngram_size = Some(n);
    added
}

/// Remove the fingerprints of one sample from the profile.
///
/// The sample text is used to recompute the hashes; only matching hashes are
/// released, and a hash that another sample also produced stays until that
/// sample is removed too. Returns how many of the sample's hashes were
/// released.
pub fn remove_sample_guards(profile: &mut VoiceProfile, sample: &str, n: usize) -> usize {
    let n = n.max(1);
    let fingerprints = crate::copy_guard::ngram_fingerprints(sample, n);
    let mut removed = 0;
    for fingerprint in fingerprints {
        if let Some(shares) = profile.sample_fingerprint_shares.get_mut(&fingerprint) {
            *shares -= 1;
            if *shares == 0 {
                profile.sample_fingerprint_shares.remove(&fingerprint);
            }
            removed += 1;
        } else if profile.sample_fingerprints.remove(&fingerprint) {
            removed += 1;
        }
    }
    if profile.sample_fingerprints.is_empty() {
        profile.fingerprint_ngram_size = None;
    }
    removed
}

/// The default n-gram size used for sample fingerprints.
pub fn default_fingerprint_size() -> usize {
    DEFAULT_NGRAM_SIZE
}

/// Fold a set of suggestion events into the profile's traits.
///
/// Learned traits are replaced by the new values; user overrides are left
/// alone. Returns the summary of what was learned.
pub fn learn_from_suggestions(
    profile: &mut VoiceProfile,
    events: &[SuggestionEvent],
    now: u64,
    half_life_days: f64,
) -> FeedbackSummary {
    let summary = summarize_suggestions(events, now, half_life_days);
    if summary.events == 0 {
        return summary;
    }

    set_learned_number(
        profile,
        "accepted_edits",
        summary.accepted as f64,
        0.9,
        format!(
            "{} accepted of {} feedback events",
            summary.accepted, summary.events
        ),
    );
    set_learned_number(
        profile,
        "dismissed_edits",
        summary.dismissed as f64,
        0.9,
        format!("{} dismissed suggestions", summary.dismissed),
    );
    set_learned_number(
        profile,
        "edited_edits",
        summary.edited as f64,
        0.9,
        format!("{} edits after accepting", summary.edited),
    );

    if summary.contractions.count() > 0 {
        let prefers = summary.contractions.net() >= 0.0;
        set_learned_bool(
            profile,
            "prefers_contractions",
            prefers,
            summary.contractions.confidence(),
            format!(
                "{} added, {} removed",
                summary.contractions.positive().round(),
                summary.contractions.negative().round()
            ),
        );
    }

    if summary.concise.count() > 0 {
        let concise = summary.concise.net() >= 0.0;
        set_learned_bool(
            profile,
            "prefers_concise",
            concise,
            summary.concise.confidence(),
            format!(
                "{} shortened, {} lengthened",
                summary.concise.positive().round(),
                summary.concise.negative().round()
            ),
        );
    }

    for (category, weight) in &summary.suppressed_categories {
        if *weight >= 1.5 {
            set_learned_bool(
                profile,
                &format!("avoids_{category}_edits"),
                true,
                (weight / 5.0).min(0.9),
                format!("{category} suggestions dismissed {weight:.1} weighted times"),
            );
        }
    }

    for preference in crate::feedback::infer_term_preferences(events) {
        set_learned(
            profile,
            &format!(
                "prefers_{}_over_{}",
                sanitize_key(&preference.to),
                sanitize_key(&preference.from)
            ),
            TraitValue::Text(preference.to.clone()),
            (preference.count as f64 / 6.0).min(0.9),
            format!(
                "replaced \"{}\" with \"{}\" {} times",
                preference.from, preference.to, preference.count
            ),
        );
    }

    apply_rating_confidence(profile, &summary);
    summary
}

/// Turn a word into a trait-key-safe token.
fn sanitize_key(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// Fold a "sounds like me" rating into the profile's confidence.
///
/// A positive rating raises the confidence of the traits the rewrite used; a
/// negative one lowers it.
pub fn apply_rating(profile: &mut VoiceProfile, rating: Rating, now: u64) {
    let decay = crate::feedback::decay_factor(now, now, crate::feedback::FEEDBACK_HALF_LIFE_DAYS);
    let delta = rating.signal() * 0.1 * decay;
    let mut traits = profile.traits.clone();
    for entry in traits.values_mut() {
        if entry.is_override() {
            continue;
        }
        entry.confidence = (entry.confidence + delta).clamp(0.0, 1.0);
    }
    profile.traits = traits;
    profile.confidence = (profile.confidence + delta).clamp(0.0, 1.0);
}

/// Fold the voice events that have not been applied yet into the profile.
///
/// Ratings move the confidence of the learned traits. Overrides are never
/// touched. The number of applied events is remembered, so folding the same
/// log twice has the same effect as folding it once.
pub fn apply_voice_events(profile: &mut VoiceProfile, events: &[crate::feedback::VoiceEvent]) {
    let already = profile.applied_voice_events as usize;
    let pending = events.iter().skip(already);
    let mut applied = already as u64;
    for event in pending {
        if let Some(rating) = event.rating {
            apply_rating(profile, rating, event.timestamp);
        }
        applied += 1;
    }
    profile.applied_voice_events = applied;
}

fn apply_rating_confidence(profile: &mut VoiceProfile, summary: &FeedbackSummary) {
    let signal = summary.rating_signal();
    if signal == 0.0 {
        return;
    }
    let delta = signal * 0.05;
    for entry in profile.traits.values_mut() {
        if entry.is_override() {
            continue;
        }
        entry.confidence = (entry.confidence + delta).clamp(0.0, 1.0);
    }
    profile.confidence = (profile.confidence + delta).clamp(0.0, 1.0);
}

fn set_learned_number(
    profile: &mut VoiceProfile,
    key: &str,
    value: f64,
    confidence: f64,
    evidence: String,
) {
    set_learned(
        profile,
        key,
        TraitValue::Number(value),
        confidence,
        evidence,
    );
}

fn set_learned_bool(
    profile: &mut VoiceProfile,
    key: &str,
    value: bool,
    confidence: f64,
    evidence: String,
) {
    set_learned(profile, key, TraitValue::Bool(value), confidence, evidence);
}

fn set_learned(
    profile: &mut VoiceProfile,
    key: &str,
    value: TraitValue,
    confidence: f64,
    evidence: String,
) {
    if let Some(existing) = profile.traits.get_mut(key)
        && existing.is_override()
    {
        // The user set this trait; learning must not overwrite it. The new
        // learned value is remembered, so clearing the override later shows
        // the freshest measurement rather than a stale one.
        existing.learned = Some(LearnedTrait {
            value,
            confidence: confidence.clamp(0.0, 1.0),
        });
        return;
    }
    let evidence = if evidence.is_empty() {
        Vec::new()
    } else {
        vec![evidence]
    };
    profile.traits.insert(
        key.to_string(),
        VoiceTrait {
            value,
            confidence: confidence.clamp(0.0, 1.0),
            evidence,
            source: TraitSource::Learned,
            learned: None,
        },
    );
}

/// The weight of one accepted edit, exposed for tests and callers that want
/// to reason about the learning signal.
pub fn accepted_edit_weight(
    category: &str,
    origin: EditOrigin,
    timestamp: u64,
    now: u64,
    half_life_days: f64,
) -> f64 {
    event_weight(
        SuggestionEventKind::Accepted,
        category,
        origin,
        timestamp,
        now,
        half_life_days,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feedback::SuggestionEvent;

    const NOW: u64 = 1_700_000_000;

    fn profile() -> VoiceProfile {
        let mut profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        profile.confidence = 0.6;
        profile.strength = VoiceStrength::Balanced;
        profile.traits.insert(
            "avg_sentence_words".into(),
            VoiceTrait::learned(TraitValue::Number(14.0), 0.8, vec![]),
        );
        profile
    }

    #[test]
    fn overrides_replace_learned_values() {
        let mut profile = profile();
        set_trait_override(&mut profile, "avg_sentence_words", TraitValue::Number(8.0));
        assert_eq!(
            profile.trait_value("avg_sentence_words"),
            Some(&TraitValue::Number(8.0))
        );
        assert!(profile.is_overridden("avg_sentence_words"));
    }

    #[test]
    fn overrides_are_marked_as_user_set() {
        let mut profile = profile();
        set_trait_override(&mut profile, "avg_sentence_words", TraitValue::Number(8.0));
        let entry = &profile.traits["avg_sentence_words"];
        assert_eq!(entry.source, TraitSource::Override);
        assert!(entry.evidence.contains(&"set by you".to_string()));
    }

    #[test]
    fn clearing_an_override_restores_the_learned_value() {
        let mut profile = profile();
        set_trait_override(&mut profile, "avg_sentence_words", TraitValue::Number(8.0));
        assert!(clear_trait_override(&mut profile, "avg_sentence_words"));
        assert_eq!(
            profile.trait_value("avg_sentence_words"),
            Some(&TraitValue::Number(14.0))
        );
        assert!(!profile.is_overridden("avg_sentence_words"));
    }

    #[test]
    fn clearing_an_unknown_trait_reports_no_change() {
        let mut profile = profile();
        assert!(!clear_trait_override(&mut profile, "missing"));
    }

    #[test]
    fn learning_never_overwrites_an_override() {
        let mut profile = profile();
        set_trait_override(&mut profile, "avg_sentence_words", TraitValue::Number(8.0));
        let events = vec![SuggestionEvent::new(
            SuggestionEventKind::Accepted,
            "clarity",
            "a long sentence here",
            "a short one",
            EditOrigin::User,
            NOW,
        )];
        learn_from_suggestions(&mut profile, &events, NOW, 180.0);
        assert_eq!(
            profile.trait_value("avg_sentence_words"),
            Some(&TraitValue::Number(8.0))
        );
    }

    #[test]
    fn positive_ratings_raise_confidence() {
        let mut profile = profile();
        let before = profile.traits["avg_sentence_words"].confidence;
        apply_rating(&mut profile, Rating::ThumbsUp, NOW);
        assert!(profile.traits["avg_sentence_words"].confidence > before);
    }

    #[test]
    fn negative_ratings_lower_confidence() {
        let mut profile = profile();
        let before = profile.traits["avg_sentence_words"].confidence;
        apply_rating(&mut profile, Rating::ThumbsDown, NOW);
        assert!(profile.traits["avg_sentence_words"].confidence < before);
    }

    #[test]
    fn ratings_do_not_change_override_confidence() {
        let mut profile = profile();
        set_trait_override(&mut profile, "avg_sentence_words", TraitValue::Number(8.0));
        let before = profile.traits["avg_sentence_words"].confidence;
        apply_rating(&mut profile, Rating::ThumbsUp, NOW);
        assert_eq!(profile.traits["avg_sentence_words"].confidence, before);
    }

    #[test]
    fn sample_guards_are_added_and_removed() {
        let mut profile = profile();
        let sample = "The quick brown fox jumps over the lazy dog and keeps running.";
        let added = add_sample_guards(&mut profile, sample, 8);
        assert!(added > 0);
        assert!(!profile.sample_fingerprints.is_empty());
        let removed = remove_sample_guards(&mut profile, sample, 8);
        assert_eq!(removed, added);
        assert!(profile.sample_fingerprints.is_empty());
    }

    #[test]
    fn learning_records_the_event_counts() {
        let mut profile = profile();
        let events = vec![
            SuggestionEvent::new(
                SuggestionEventKind::Accepted,
                "clarity",
                "we are going",
                "we're going",
                EditOrigin::User,
                NOW,
            ),
            SuggestionEvent::new(
                SuggestionEventKind::Dismissed,
                "word_choice",
                "utilize",
                "use",
                EditOrigin::User,
                NOW,
            ),
        ];
        let summary = learn_from_suggestions(&mut profile, &events, NOW, 180.0);
        assert_eq!(summary.events, 2);
        assert_eq!(summary.accepted, 1);
        assert_eq!(summary.dismissed, 1);
        assert_eq!(
            profile.trait_value("dismissed_edits"),
            Some(&TraitValue::Number(1.0))
        );
        // Dismissals are not acceptances.
        assert_eq!(
            profile.trait_value("accepted_edits"),
            Some(&TraitValue::Number(1.0))
        );
    }

    #[test]
    fn removing_a_sample_keeps_fingerprints_another_sample_shares() {
        let mut profile = profile();
        let shared = "the quick brown fox jumps over the lazy dog";
        let first = format!("{shared} and keeps running.");
        let second = format!("Yesterday {shared} again.");
        add_sample_guards(&mut profile, &first, 8);
        add_sample_guards(&mut profile, &second, 8);
        let shared_prints = crate::copy_guard::ngram_fingerprints(shared, 8);
        assert!(!shared_prints.is_empty());

        remove_sample_guards(&mut profile, &first, 8);
        assert!(shared_prints.is_subset(&profile.sample_fingerprints));
        assert_eq!(
            profile.sample_fingerprints,
            crate::copy_guard::ngram_fingerprints(&second, 8)
        );

        remove_sample_guards(&mut profile, &second, 8);
        assert!(profile.sample_fingerprints.is_empty());
        assert!(profile.sample_fingerprint_shares.is_empty());
    }

    #[test]
    fn fingerprint_shares_round_trip_through_json() {
        let mut profile = profile();
        let sample = "one two three four five six seven eight nine";
        add_sample_guards(&mut profile, sample, 8);
        add_sample_guards(&mut profile, sample, 8);
        let json = serde_json::to_string(&profile).unwrap();
        let mut restored: VoiceProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, profile);
        remove_sample_guards(&mut restored, sample, 8);
        assert!(!restored.sample_fingerprints.is_empty());
    }

    #[test]
    fn profiles_with_new_fields_still_deserialize() {
        let json = r#"{
            "id":"vp1","name":"Work","context":"email","language":"en",
            "strength":"balanced","confidence":0.5,"summary":"",
            "traits":{"avg_sentence_words":{"value":{"kind":"number","value":12.0},"confidence":0.7,"evidence":[]}}
        }"#;
        let profile: VoiceProfile = serde_json::from_str(json).unwrap();
        assert!(profile.sample_fingerprints.is_empty());
        assert_eq!(
            profile.traits["avg_sentence_words"].source,
            TraitSource::Learned
        );
    }

    /// The exact shape of the style card, so the privacy documentation can be
    /// written against a shape that is checked by a test.
    #[test]
    fn the_style_card_has_the_documented_shape() {
        let mut profile = profile();
        add_sample_guards(
            &mut profile,
            "Hi Sam, quick update on the launch. We are on track for Thursday. Thanks!",
            8,
        );
        set_trait_override(&mut profile, "avg_sentence_words", TraitValue::Number(9.0));
        let value = serde_json::to_value(&profile).unwrap();
        assert_eq!(value["id"], "vp1");
        assert_eq!(value["context"], "email");
        assert_eq!(value["strength"], "balanced");
        // The fingerprints are plain u64 numbers, sorted and deduplicated.
        let fingerprints = value["sample_fingerprints"].as_array().unwrap();
        assert!(!fingerprints.is_empty());
        assert!(fingerprints.iter().all(|item| item.is_u64()));
        let mut sorted = fingerprints.clone();
        sorted.sort_by_key(|item| item.as_u64().unwrap());
        assert_eq!(sorted, *fingerprints);
        assert_eq!(value["fingerprint_ngram_size"], 8);
        // The override is marked, and the learned value is kept aside.
        let trait_value = &value["traits"]["avg_sentence_words"];
        assert_eq!(trait_value["source"], "override");
        assert!(trait_value["learned"].is_object());
        // The sample text is nowhere in the file.
        let rendered = serde_json::to_string(&profile).unwrap();
        assert!(!rendered.contains("quick update"));
    }
}
