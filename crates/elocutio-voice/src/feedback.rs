//! Feedback events, weights, and decay.
//!
//! The profile learns from three kinds of behaviour: suggestions the user
//! accepted, suggestions the user dismissed, and rewrites the user edited
//! after accepting. Each event carries a weight, and weights decay with a
//! 180-day half-life so the profile follows the writer instead of freezing on
//! their first week.
//!
//! Two rules protect the profile from feedback loops:
//!
//! * AI-generated text that the user accepted unchanged is down-weighted, so
//!   the profile does not converge on the model's voice.
//! * The `Model` origin always carries less weight than the `User` origin for
//!   the same event kind.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The half-life of a feedback event, in days.
pub const FEEDBACK_HALF_LIFE_DAYS: f64 = 180.0;

/// The seconds in one day, as a float.
const SECONDS_PER_DAY: f64 = 86_400.0;

/// Where a piece of text came from.
///
/// `User` is text the writer produced. `Model` is text a language model
/// produced, which is a much weaker signal about the writer's own voice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditOrigin {
    /// The writer's own words.
    #[default]
    User,
    /// Model-generated text.
    Model,
}

impl EditOrigin {
    /// The multiplier applied to the event weight for this origin.
    pub fn weight_multiplier(self) -> f64 {
        match self {
            EditOrigin::User => 1.0,
            EditOrigin::Model => 0.1,
        }
    }
}

/// What the user did with a suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionEventKind {
    /// The user accepted the suggestion.
    Accepted,
    /// The user rejected the suggestion.
    Dismissed,
    /// The user accepted the suggestion and then changed the wording.
    EditedAfterAccept,
}

impl SuggestionEventKind {
    /// The base weight of the event, before decay and origin scaling.
    pub fn base_weight(self) -> f64 {
        match self {
            SuggestionEventKind::Accepted => 0.1,
            SuggestionEventKind::Dismissed => 0.5,
            SuggestionEventKind::EditedAfterAccept => 0.7,
        }
    }

    /// The sign of the event: dismissals push traits down.
    pub fn is_negative(self) -> bool {
        matches!(self, SuggestionEventKind::Dismissed)
    }

    /// The stable name used on disk.
    pub fn as_str(self) -> &'static str {
        match self {
            SuggestionEventKind::Accepted => "accepted",
            SuggestionEventKind::Dismissed => "dismissed",
            SuggestionEventKind::EditedAfterAccept => "edited_after_accept",
        }
    }

    /// Parse the stable on-disk name.
    pub fn from_str_name(value: &str) -> Option<Self> {
        match value {
            "accepted" => Some(SuggestionEventKind::Accepted),
            "dismissed" => Some(SuggestionEventKind::Dismissed),
            "edited_after_accept" => Some(SuggestionEventKind::EditedAfterAccept),
            _ => None,
        }
    }
}

/// The kind of a voice event recorded in the local feedback log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceEventKind {
    /// The user rated a rewrite "sounds like me" or "does not sound like me".
    Rated,
    /// The user picked a tone for a context.
    ToneSelected,
    /// The user set a trait override.
    Override,
}

impl VoiceEventKind {
    /// The stable name used on disk and over IPC.
    pub fn as_str(self) -> &'static str {
        match self {
            VoiceEventKind::Rated => "rated",
            VoiceEventKind::ToneSelected => "tone_selected",
            VoiceEventKind::Override => "override",
        }
    }

    /// Parse the stable on-disk name.
    pub fn from_str_name(value: &str) -> Option<Self> {
        match value {
            "rated" => Some(VoiceEventKind::Rated),
            "tone_selected" => Some(VoiceEventKind::ToneSelected),
            "override" => Some(VoiceEventKind::Override),
            _ => None,
        }
    }
}

/// A "sounds like me" rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rating {
    /// The rewrite sounds like the writer.
    ThumbsUp,
    /// The rewrite does not sound like the writer.
    ThumbsDown,
}

impl Rating {
    /// The signed signal of the rating, in `[-1.0, 1.0]`.
    pub fn signal(self) -> f64 {
        match self {
            Rating::ThumbsUp => 1.0,
            Rating::ThumbsDown => -1.0,
        }
    }

    /// The stable name used on disk.
    pub fn as_str(self) -> &'static str {
        match self {
            Rating::ThumbsUp => "thumbs_up",
            Rating::ThumbsDown => "thumbs_down",
        }
    }

    /// Parse the stable on-disk name.
    pub fn from_str_name(value: &str) -> Option<Self> {
        match value {
            "thumbs_up" | "up" | "positive" => Some(Rating::ThumbsUp),
            "thumbs_down" | "down" | "negative" => Some(Rating::ThumbsDown),
            _ => None,
        }
    }
}

/// A suggestion event recorded by the app.
///
/// The shape is the `suggestion_event` message from `docs/ARCHITECTURE.md`
/// §10.2, with the local additions the voice profile needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuggestionEvent {
    /// The event kind.
    pub kind: SuggestionEventKind,
    /// The category of the suggestion, e.g. "clarity".
    #[serde(default)]
    pub category: String,
    /// The original text the suggestion replaced.
    #[serde(default)]
    pub original: String,
    /// The text the suggestion proposed.
    #[serde(default)]
    pub replacement: String,
    /// The text the writer ended up with, when the event was an edit.
    #[serde(default)]
    pub final_text: String,
    /// Where the replacement text came from.
    #[serde(default)]
    pub origin: EditOrigin,
    /// The app the suggestion came from.
    #[serde(default)]
    pub app: String,
    /// When the event happened, in seconds since the Unix epoch.
    pub timestamp: u64,
}

impl SuggestionEvent {
    /// Build an event with the current wall-clock time.
    pub fn new(
        kind: SuggestionEventKind,
        category: impl Into<String>,
        original: impl Into<String>,
        replacement: impl Into<String>,
        origin: EditOrigin,
        timestamp: u64,
    ) -> Self {
        Self {
            kind,
            category: category.into(),
            original: original.into(),
            replacement: replacement.into(),
            final_text: String::new(),
            origin,
            app: String::new(),
            timestamp,
        }
    }

    /// The text the writer actually ended up with.
    pub fn effective_text(&self) -> &str {
        if self.kind == SuggestionEventKind::EditedAfterAccept && !self.final_text.is_empty() {
            &self.final_text
        } else {
            &self.replacement
        }
    }
}

/// A voice event recorded by the app: a rating, a tone choice, or an override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceEvent {
    /// The event kind.
    pub kind: VoiceEventKind,
    /// The trait key the event applies to, when it applies to one.
    #[serde(default)]
    pub trait_key: String,
    /// The value the trait was set to, for overrides.
    #[serde(default)]
    pub value: Option<f64>,
    /// The rating, for `Rated` events.
    #[serde(default)]
    pub rating: Option<Rating>,
    /// The tone name, for `ToneSelected` events.
    #[serde(default)]
    pub tone: Option<String>,
    /// The context the event applies to.
    #[serde(default)]
    pub context: Option<String>,
    /// When the event happened, in seconds since the Unix epoch.
    pub timestamp: u64,
}

impl VoiceEvent {
    /// Build a rating event.
    pub fn rated(rating: Rating, context: Option<String>, timestamp: u64) -> Self {
        Self {
            kind: VoiceEventKind::Rated,
            trait_key: String::new(),
            value: None,
            rating: Some(rating),
            tone: None,
            context,
            timestamp,
        }
    }

    /// Build a tone-selection event.
    pub fn tone_selected(tone: impl Into<String>, context: Option<String>, timestamp: u64) -> Self {
        Self {
            kind: VoiceEventKind::ToneSelected,
            trait_key: String::new(),
            value: None,
            rating: None,
            tone: Some(tone.into()),
            context,
            timestamp,
        }
    }

    /// Build an override event.
    pub fn override_set(
        trait_key: impl Into<String>,
        value: f64,
        context: Option<String>,
        timestamp: u64,
    ) -> Self {
        Self {
            kind: VoiceEventKind::Override,
            trait_key: trait_key.into(),
            value: Some(value),
            rating: None,
            tone: None,
            context,
            timestamp,
        }
    }
}

/// The decay factor of an event at a point in time.
///
/// `weight = 0.5 ^ (days_since_event / half_life)`. Events in the future are
/// treated as fresh.
pub fn decay_factor(event_timestamp: u64, now: u64, half_life_days: f64) -> f64 {
    if half_life_days <= 0.0 {
        return 1.0;
    }
    let elapsed = now.saturating_sub(event_timestamp) as f64 / SECONDS_PER_DAY;
    0.5f64.powf(elapsed / half_life_days)
}

/// The full weight of an event: base weight, category multiplier, origin
/// multiplier, and time decay.
pub fn event_weight(
    kind: SuggestionEventKind,
    category: &str,
    origin: EditOrigin,
    event_timestamp: u64,
    now: u64,
    half_life_days: f64,
) -> f64 {
    kind.base_weight()
        * category_weight(category)
        * origin.weight_multiplier()
        * decay_factor(event_timestamp, now, half_life_days)
}

/// The weight multiplier for a suggestion category.
///
/// Spelling and punctuation are correctness, not style, so they teach much
/// less about the writer's voice than a word-choice or tone edit does.
pub fn category_weight(category: &str) -> f64 {
    match category {
        "spelling" | "punctuation" | "capitalization" | "formatting" => 0.3,
        "grammar" => 0.4,
        "clarity" | "conciseness" => 0.7,
        "word_choice" | "style" | "tone" | "consistency" => 1.0,
        _ => 0.5,
    }
}

/// A weighted tally used to derive traits from feedback.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeightedTally {
    positive: f64,
    negative: f64,
    count: u64,
}

impl WeightedTally {
    /// Add a signed amount to the tally.
    pub fn add(&mut self, amount: f64) {
        if amount >= 0.0 {
            self.positive += amount;
        } else {
            self.negative += -amount;
        }
        self.count += 1;
    }

    /// The total positive weight.
    pub fn positive(&self) -> f64 {
        self.positive
    }

    /// The total negative weight.
    pub fn negative(&self) -> f64 {
        self.negative
    }

    /// The signed net weight.
    pub fn net(&self) -> f64 {
        self.positive - self.negative
    }

    /// How many events contributed.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The confidence of a derived trait, in `[0, 1]`.
    ///
    /// Confidence grows with the amount of evidence and shrinks with the
    /// disagreement between positive and negative evidence.
    pub fn confidence(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let total = self.positive + self.negative;
        if total <= 0.0 {
            return 0.0;
        }
        let agreement = (self.positive - self.negative).abs() / total;
        let volume = (total / 3.0).min(1.0);
        (agreement * volume).clamp(0.0, 1.0)
    }
}

/// A term the writer consistently replaces with another term.
///
/// The design calls for three consistent events before a term pair is
/// inferred, so a single slip never becomes a rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermPreference {
    /// The term the writer removed.
    pub from: String,
    /// The term the writer used instead.
    pub to: String,
    /// How many consistent events backed this pair.
    pub count: u64,
}

/// How many consistent events a term pair needs before it is inferred.
pub const TERM_PREFERENCE_THRESHOLD: u64 = 3;

/// How many dismissals a category needs before it is suppressed.
pub const CATEGORY_SUPPRESSION_THRESHOLD: u64 = 5;

/// Infer the term pairs the writer consistently prefers.
///
/// Only single-word replacements are considered: a phrase-level replacement
/// is harder to read as a style rule.
pub fn infer_term_preferences(events: &[SuggestionEvent]) -> Vec<TermPreference> {
    let mut counts: BTreeMap<(String, String), u64> = BTreeMap::new();
    for event in events {
        if event.kind == SuggestionEventKind::Dismissed {
            continue;
        }
        let original = event.original.trim();
        let replacement = event.effective_text().trim();
        if original == replacement {
            continue;
        }
        if original.split_whitespace().count() != 1 || replacement.split_whitespace().count() != 1 {
            continue;
        }
        let from = original.to_lowercase();
        let to = replacement.to_lowercase();
        if from == to {
            continue;
        }
        *counts.entry((from, to)).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count >= TERM_PREFERENCE_THRESHOLD)
        .map(|((from, to), count)| TermPreference { from, to, count })
        .collect()
}

/// The categories the writer dismissed often enough to suppress.
///
/// Dismissals of correctness categories are kept separate: the design treats
/// them as rule preferences, not voice evidence.
pub fn suppressed_categories(events: &[SuggestionEvent]) -> BTreeMap<String, u64> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for event in events {
        if event.kind != SuggestionEventKind::Dismissed {
            continue;
        }
        *counts.entry(event.category.clone()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count >= CATEGORY_SUPPRESSION_THRESHOLD)
        .collect()
}

/// The result of folding feedback events into a profile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FeedbackSummary {
    /// How many events were considered.
    pub events: u64,
    /// How many events were accepted edits.
    pub accepted: u64,
    /// How many events were dismissals.
    pub dismissed: u64,
    /// How many events were edits after accepting.
    pub edited: u64,
    /// The summed weights of all events.
    pub total_weight: f64,
    /// The net preference for contractions.
    pub contractions: WeightedTally,
    /// The net preference for shorter phrasing.
    pub concise: WeightedTally,
    /// The net preference for a warmer, plainer register.
    pub warm: WeightedTally,
    /// Categories the user rejected, with their negative weight.
    pub suppressed_categories: BTreeMap<String, f64>,
    /// How many thumbs-up ratings were recorded.
    pub positive_ratings: u64,
    /// How many thumbs-down ratings were recorded.
    pub negative_ratings: u64,
}

impl FeedbackSummary {
    /// The net rating signal, in `[-1.0, 1.0]`.
    pub fn rating_signal(&self) -> f64 {
        let total = self.positive_ratings + self.negative_ratings;
        if total == 0 {
            return 0.0;
        }
        (self.positive_ratings as f64 - self.negative_ratings as f64) / total as f64
    }
}

/// Fold suggestion events into a summary of what the profile learned.
///
/// `now` and `half_life_days` control the time decay. Events that are older
/// than the retention window should be pruned before calling this.
pub fn summarize_suggestions(
    events: &[SuggestionEvent],
    now: u64,
    half_life_days: f64,
) -> FeedbackSummary {
    let mut summary = FeedbackSummary::default();
    for event in events {
        let weight = event_weight(
            event.kind,
            &event.category,
            event.origin,
            event.timestamp,
            now,
            half_life_days,
        );
        summary.events += 1;
        summary.total_weight += weight.abs();
        match event.kind {
            SuggestionEventKind::Accepted => summary.accepted += 1,
            SuggestionEventKind::Dismissed => summary.dismissed += 1,
            SuggestionEventKind::EditedAfterAccept => summary.edited += 1,
        }

        if event.kind == SuggestionEventKind::Dismissed {
            *summary
                .suppressed_categories
                .entry(event.category.clone())
                .or_insert(0.0) += weight.abs();
            continue;
        }

        let original = event.original.to_lowercase();
        let effective = event.effective_text().to_lowercase();
        let original_has = has_contraction(&original);
        let effective_has = has_contraction(&effective);
        if !original_has && effective_has {
            summary.contractions.add(weight);
        } else if original_has && !effective_has {
            summary.contractions.add(-weight);
        }

        let shorter = effective.chars().count() < original.chars().count();
        let longer = effective.chars().count() > original.chars().count();
        if shorter {
            summary.concise.add(weight);
        } else if longer {
            summary.concise.add(-weight);
        }

        if effective_has && !original_has {
            summary.warm.add(weight * 0.5);
        } else if original_has && !effective_has {
            summary.warm.add(-weight * 0.5);
        }
    }
    summary
}

/// Whether a text contains a contraction.
pub fn has_contraction(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        let word =
            word.trim_matches(|character: char| !character.is_alphabetic() && character != '\'');
        ["n't", "'ll", "'re", "'ve", "'m", "'d", "'s"]
            .iter()
            .any(|suffix| word.ends_with(suffix))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_700_000_000;
    const DAY: u64 = 86_400;

    fn event(
        kind: SuggestionEventKind,
        category: &str,
        origin: EditOrigin,
        days_ago: u64,
    ) -> SuggestionEvent {
        let mut event = SuggestionEvent::new(
            kind,
            category,
            "we are going",
            "we're going",
            origin,
            NOW - days_ago * DAY,
        );
        event.final_text = "we're going".into();
        event
    }

    #[test]
    fn decay_halves_at_the_half_life() {
        let factor = decay_factor(NOW - 180 * DAY, NOW, FEEDBACK_HALF_LIFE_DAYS);
        assert!((factor - 0.5).abs() < 1e-9);
    }

    #[test]
    fn fresh_events_have_no_decay() {
        assert_eq!(decay_factor(NOW, NOW, FEEDBACK_HALF_LIFE_DAYS), 1.0);
    }

    #[test]
    fn future_events_are_treated_as_fresh() {
        assert_eq!(
            decay_factor(NOW + 10 * DAY, NOW, FEEDBACK_HALF_LIFE_DAYS),
            1.0
        );
    }

    #[test]
    fn model_text_is_down_weighted_versus_user_text() {
        let user = event_weight(
            SuggestionEventKind::Accepted,
            "clarity",
            EditOrigin::User,
            NOW,
            NOW,
            FEEDBACK_HALF_LIFE_DAYS,
        );
        let model = event_weight(
            SuggestionEventKind::Accepted,
            "clarity",
            EditOrigin::Model,
            NOW,
            NOW,
            FEEDBACK_HALF_LIFE_DAYS,
        );
        assert!((user - 0.07).abs() < 1e-9, "user weight was {user}");
        assert!((model - 0.007).abs() < 1e-9, "model weight was {model}");
    }

    #[test]
    fn category_weights_follow_the_design() {
        assert_eq!(category_weight("spelling"), 0.3);
        assert_eq!(category_weight("grammar"), 0.4);
        assert_eq!(category_weight("clarity"), 0.7);
        assert_eq!(category_weight("word_choice"), 1.0);
        assert_eq!(category_weight("tone"), 1.0);
    }

    #[test]
    fn dismissals_suppress_their_category() {
        let events = vec![event(
            SuggestionEventKind::Dismissed,
            "word_choice",
            EditOrigin::User,
            0,
        )];
        let summary = summarize_suggestions(&events, NOW, FEEDBACK_HALF_LIFE_DAYS);
        assert_eq!(summary.dismissed, 1);
        assert!(summary.suppressed_categories.contains_key("word_choice"));
    }

    #[test]
    fn edited_after_accept_counts_as_an_edited_event() {
        let events = vec![event(
            SuggestionEventKind::EditedAfterAccept,
            "style",
            EditOrigin::User,
            0,
        )];
        let summary = summarize_suggestions(&events, NOW, FEEDBACK_HALF_LIFE_DAYS);
        assert_eq!(summary.edited, 1);
        assert_eq!(summary.accepted, 0);
    }

    #[test]
    fn old_events_contribute_less_than_new_ones() {
        let fresh = vec![event(
            SuggestionEventKind::Accepted,
            "clarity",
            EditOrigin::User,
            0,
        )];
        let old = vec![event(
            SuggestionEventKind::Accepted,
            "clarity",
            EditOrigin::User,
            360,
        )];
        let fresh_summary = summarize_suggestions(&fresh, NOW, FEEDBACK_HALF_LIFE_DAYS);
        let old_summary = summarize_suggestions(&old, NOW, FEEDBACK_HALF_LIFE_DAYS);
        assert!(old_summary.total_weight < fresh_summary.total_weight);
    }

    #[test]
    fn contractive_edits_raise_the_contraction_tally() {
        let events = vec![event(
            SuggestionEventKind::Accepted,
            "style",
            EditOrigin::User,
            0,
        )];
        let summary = summarize_suggestions(&events, NOW, FEEDBACK_HALF_LIFE_DAYS);
        assert!(summary.contractions.positive() > 0.0);
    }

    #[test]
    fn rating_signal_is_signed() {
        let summary = FeedbackSummary {
            positive_ratings: 3,
            negative_ratings: 1,
            ..FeedbackSummary::default()
        };
        assert!((summary.rating_signal() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn event_kind_names_round_trip() {
        for kind in [
            SuggestionEventKind::Accepted,
            SuggestionEventKind::Dismissed,
            SuggestionEventKind::EditedAfterAccept,
        ] {
            assert_eq!(
                SuggestionEventKind::from_str_name(kind.as_str()),
                Some(kind)
            );
        }
    }

    #[test]
    fn voice_event_kind_names_round_trip() {
        for kind in [
            VoiceEventKind::Rated,
            VoiceEventKind::ToneSelected,
            VoiceEventKind::Override,
        ] {
            assert_eq!(VoiceEventKind::from_str_name(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn rating_names_round_trip() {
        assert_eq!(Rating::from_str_name("thumbs_up"), Some(Rating::ThumbsUp));
        assert_eq!(
            Rating::from_str_name("thumbs_down"),
            Some(Rating::ThumbsDown)
        );
        assert_eq!(Rating::from_str_name("nonsense"), None);
    }

    #[test]
    fn tally_confidence_grows_with_evidence() {
        let mut small = WeightedTally::default();
        small.add(1.0);
        let mut large = WeightedTally::default();
        for _ in 0..10 {
            large.add(1.0);
        }
        assert!(large.confidence() > small.confidence());
        assert!(large.confidence() <= 1.0);
    }

    #[test]
    fn tally_confidence_drops_with_disagreement() {
        let mut agreed = WeightedTally::default();
        agreed.add(1.0);
        agreed.add(1.0);
        let mut mixed = WeightedTally::default();
        mixed.add(1.0);
        mixed.add(-1.0);
        assert!(mixed.confidence() < agreed.confidence());
    }

    #[test]
    fn term_preferences_need_three_consistent_events() {
        let events: Vec<SuggestionEvent> = (0..2)
            .map(|_| {
                SuggestionEvent::new(
                    SuggestionEventKind::Accepted,
                    "word_choice",
                    "utilize",
                    "use",
                    EditOrigin::User,
                    NOW,
                )
            })
            .collect();
        assert!(infer_term_preferences(&events).is_empty());

        let events: Vec<SuggestionEvent> = (0..3)
            .map(|_| {
                SuggestionEvent::new(
                    SuggestionEventKind::Accepted,
                    "word_choice",
                    "utilize",
                    "use",
                    EditOrigin::User,
                    NOW,
                )
            })
            .collect();
        let preferences = infer_term_preferences(&events);
        assert_eq!(preferences.len(), 1);
        assert_eq!(preferences[0].from, "utilize");
        assert_eq!(preferences[0].to, "use");
        assert_eq!(preferences[0].count, 3);
    }

    #[test]
    fn term_preferences_ignore_multi_word_replacements() {
        let events: Vec<SuggestionEvent> = (0..5)
            .map(|_| {
                SuggestionEvent::new(
                    SuggestionEventKind::Accepted,
                    "clarity",
                    "in order to",
                    "to",
                    EditOrigin::User,
                    NOW,
                )
            })
            .collect();
        assert!(infer_term_preferences(&events).is_empty());
    }

    #[test]
    fn category_suppression_needs_five_dismissals() {
        let events: Vec<SuggestionEvent> = (0..4)
            .map(|_| {
                SuggestionEvent::new(
                    SuggestionEventKind::Dismissed,
                    "word_choice",
                    "utilize",
                    "use",
                    EditOrigin::User,
                    NOW,
                )
            })
            .collect();
        assert!(suppressed_categories(&events).is_empty());

        let events: Vec<SuggestionEvent> = (0..5)
            .map(|_| {
                SuggestionEvent::new(
                    SuggestionEventKind::Dismissed,
                    "word_choice",
                    "utilize",
                    "use",
                    EditOrigin::User,
                    NOW,
                )
            })
            .collect();
        let suppressed = suppressed_categories(&events);
        assert_eq!(suppressed.get("word_choice"), Some(&5));
    }
}
