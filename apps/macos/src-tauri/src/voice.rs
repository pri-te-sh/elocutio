//! Local voice profile storage. Only the aggregate style card is written; raw
//! uploaded samples are discarded after analysis.
//!
//! Three files live in the application support folder:
//!
//! | File | Contents |
//! | --- | --- |
//! | `voice-profile.json` | The style card: traits, overrides, and the sample fingerprints the copy guard uses. |
//! | `voice-feedback.jsonl` | Suggestion events (`accepted`, `dismissed`, `edited_after_accept`), one JSON object per line. |
//! | `voice-events.jsonl` | Voice events (`rated`, `tone_selected`, `override`), one JSON object per line. |
//! | `voice-learning.enabled` | The opt-in flag. Its presence means learning is on. |
//!
//! The privacy invariant holds across all of them: no raw uploaded sample is
//! ever written. The copy guard works on 64-bit fingerprints of the sample
//! n-grams, and the feedback log stores only the truncated text of the
//! writer's own edits.

use std::io::Write;
use std::path::PathBuf;

use elocutio_voice::{
    EditOrigin, FEEDBACK_HALF_LIFE_DAYS, FEEDBACK_RETENTION_DAYS, Rating, StoredSuggestionEvent,
    SuggestionEvent, SuggestionEventKind, TraitValue, VoiceContext, VoiceEvent, VoiceProfile,
    VoiceStrength, add_sample_guards, analyze_samples, apply_voice_events,
    assert_no_copy_against_fingerprints, default_fingerprint_size, feedback_fingerprints,
    learn_from_suggestions, prune_events, voice_prompt_guidance,
};
use elocutio_voice::{clear_trait_override as clear_override, set_trait_override as set_override};
use serde::{Deserialize, Serialize};

/// The result of a copy-guard check, re-exported for the Tauri commands.
pub use elocutio_voice::CopyGuardReport;

use crate::config::Config;

fn path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("voice-profile.json"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

fn learning_flag_path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("voice-learning.enabled"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

fn feedback_path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("voice-feedback.jsonl"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

fn events_path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("voice-events.jsonl"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn set_learning_enabled(enabled: bool) -> Result<(), String> {
    let path = learning_flag_path()?;
    if enabled {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create the voice directory: {error}"))?;
        }
        std::fs::write(&path, b"1")
            .map_err(|error| format!("Could not enable edit learning: {error}"))
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not disable edit learning: {error}")),
        }
    }
}

pub fn is_learning_enabled() -> bool {
    learning_flag_path()
        .map(|path| path.exists())
        .unwrap_or(false)
}

/// Record an accepted suggestion.
///
/// The replacement came from the local rule engine, so it counts as
/// user-authored text. Use [`record_suggestion_event`] when the replacement
/// came from a language model.
pub fn record_accepted(original: &str, replacement: &str, category: &str) -> Result<(), String> {
    record_suggestion_event(
        SuggestionEventKind::Accepted,
        original,
        replacement,
        "",
        category,
        EditOrigin::User,
    )
}

/// Record a suggestion the user rejected.
pub fn record_dismissed(original: &str, replacement: &str, category: &str) -> Result<(), String> {
    record_suggestion_event(
        SuggestionEventKind::Dismissed,
        original,
        replacement,
        "",
        category,
        EditOrigin::User,
    )
}

/// Record a suggestion the user accepted and then changed.
///
/// This is the strongest signal in the design: the writer disagreed with the
/// proposed wording and wrote their own.
pub fn record_edited_after_accept(
    original: &str,
    replacement: &str,
    final_text: &str,
    category: &str,
) -> Result<(), String> {
    record_suggestion_event(
        SuggestionEventKind::EditedAfterAccept,
        original,
        replacement,
        final_text,
        category,
        EditOrigin::User,
    )
}

/// Record any suggestion event.
pub fn record_suggestion_event(
    kind: SuggestionEventKind,
    original: &str,
    replacement: &str,
    final_text: &str,
    category: &str,
    origin: EditOrigin,
) -> Result<(), String> {
    if !is_learning_enabled() {
        return Ok(());
    }
    let original = original.trim();
    let replacement = replacement.trim();
    let final_text = final_text.trim();
    if kind == SuggestionEventKind::Accepted && original == replacement {
        return Ok(());
    }
    if kind == SuggestionEventKind::EditedAfterAccept && final_text.is_empty() {
        return Ok(());
    }

    let path = feedback_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the voice directory: {error}"))?;
    }

    let event = SuggestionEvent {
        kind,
        category: category.to_string(),
        original: original.to_string(),
        replacement: replacement.to_string(),
        final_text: final_text.to_string(),
        origin,
        app: String::new(),
        timestamp: now_seconds(),
    };
    let stored = StoredSuggestionEvent::from_event(&event);
    let line = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Could not open voice feedback: {error}"))?;
    writeln!(file, "{line}")
        .map_err(|error| format!("Could not record the feedback event: {error}"))
}

/// Record a voice event: a rating, a tone choice, or an override.
pub fn record_voice_event(event: &VoiceEvent) -> Result<(), String> {
    if !is_learning_enabled() {
        return Ok(());
    }
    let path = events_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the voice directory: {error}"))?;
    }
    let line = serde_json::to_string(event).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Could not open voice events: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("Could not record the voice event: {error}"))
}

/// Record a "sounds like me" rating for a rewrite.
pub fn record_rating(rating: Rating, context: Option<String>) -> Result<(), String> {
    let event = VoiceEvent::rated(rating, context, now_seconds());
    record_voice_event(&event)
}

/// Record a rating given as a string, e.g. from the settings UI.
///
/// Accepts `thumbs_up`/`up`/`positive` and `thumbs_down`/`down`/`negative`.
pub fn record_rating_by_name(name: &str, context: Option<String>) -> Result<(), String> {
    let rating = Rating::from_str_name(name.trim())
        .ok_or_else(|| format!("Unknown rating \"{name}\". Use thumbs_up or thumbs_down."))?;
    record_rating(rating, context)
}

/// Record a rating and fold it into the stored profile.
///
/// The rating is stored in the event log and applied to the profile exactly
/// once, even if the profile is refreshed later.
pub fn rate_rewrite(rating: Rating) -> Result<Option<VoiceProfile>, String> {
    record_rating(rating, None)?;
    let Some(mut profile) = load()? else {
        return Ok(None);
    };
    let events = read_voice_events();
    apply_voice_events(&mut profile, &events);
    save(&profile)?;
    Ok(Some(profile))
}

/// Record a rating given as a string and fold it into the stored profile.
pub fn rate_rewrite_by_name(name: &str) -> Result<Option<VoiceProfile>, String> {
    let rating = Rating::from_str_name(name.trim())
        .ok_or_else(|| format!("Unknown rating \"{name}\". Use thumbs_up or thumbs_down."))?;
    rate_rewrite(rating)
}

/// Record a tone selection for a context.
pub fn record_tone_selected(tone: &str, context: Option<String>) -> Result<(), String> {
    let event = VoiceEvent::tone_selected(tone, context, now_seconds());
    record_voice_event(&event)
}

/// Set a trait the user chose by hand.
pub fn set_trait_override(key: &str, value: TraitValue) -> Result<Option<VoiceProfile>, String> {
    let Some(mut profile) = load()? else {
        return Err("Teach Elocutio your voice first.".into());
    };
    set_override(&mut profile, key, value);
    save(&profile)?;
    let event = VoiceEvent::override_set(key, 0.0, None, now_seconds());
    let _ = record_voice_event(&event);
    Ok(Some(profile))
}

/// Set a trait override from a plain JSON value, for the settings UI.
pub fn set_trait_override_json(
    key: &str,
    value: TraitValueJson,
) -> Result<Option<VoiceProfile>, String> {
    set_trait_override(key, value.into())
}

/// Remove a user override so the learned value shows again.
pub fn clear_trait_override(key: &str) -> Result<Option<VoiceProfile>, String> {
    let Some(mut profile) = load()? else {
        return Err("Teach Elocutio your voice first.".into());
    };
    if !clear_override(&mut profile, key) {
        return Err("That trait has no override to clear.".into());
    }
    save(&profile)?;
    Ok(Some(profile))
}

/// The traits the user set by hand.
pub fn trait_overrides() -> Vec<(String, TraitValue)> {
    let Some(profile) = load().ok().flatten() else {
        return Vec::new();
    };
    profile
        .traits
        .iter()
        .filter(|(_, entry)| entry.is_override())
        .map(|(key, entry)| (key.clone(), entry.value.clone()))
        .collect()
}

/// Check a candidate rewrite against the writer's samples.
///
/// Returns `Err` with a user-facing message when the output repeats sample
/// content. Call this before applying a provider response.
pub fn check_for_copy(candidate: &str) -> Result<(), String> {
    let Some(profile) = load().ok().flatten() else {
        return Ok(());
    };
    check_for_copy_against(&profile, candidate)
}

/// Check a candidate rewrite against a given profile's sample fingerprints.
pub fn check_for_copy_against(profile: &VoiceProfile, candidate: &str) -> Result<(), String> {
    if profile.sample_fingerprints.is_empty() {
        return Ok(());
    }
    assert_no_copy_against_fingerprints(candidate, &profile.sample_fingerprints).map_err(|error| {
        format!(
            "The rewrite repeats content from your writing samples, so it was not applied: {error}"
        )
    })
}

/// The copy-guard report for a candidate rewrite, for diagnostics and tests.
pub fn copy_guard_report(candidate: &str) -> Option<CopyGuardReport> {
    let profile = load().ok().flatten()?;
    Some(elocutio_voice::copy_guard_against_fingerprints(
        candidate,
        &profile.sample_fingerprints,
        default_fingerprint_size(),
    ))
}

/// Re-read the feedback log, fold it into the profile, and prune the log.
pub fn refresh_feedback() -> Result<Option<VoiceProfile>, String> {
    let Some(mut profile) = load()? else {
        return Ok(None);
    };
    let (events, dropped) = read_feedback()?;
    if dropped > 0 {
        write_feedback(&events)?;
    }
    if !events.is_empty() {
        let live: Vec<SuggestionEvent> =
            events.iter().map(StoredSuggestionEvent::to_event).collect();
        learn_from_suggestions(&mut profile, &live, now_seconds(), FEEDBACK_HALF_LIFE_DAYS);
    }
    let voice_events = read_voice_events();
    apply_voice_events(&mut profile, &voice_events);
    save(&profile)?;
    Ok(Some(profile))
}

/// How many suggestion events are stored.
pub fn feedback_count() -> usize {
    feedback_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|contents| {
            contents
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count()
        })
        .unwrap_or(0)
}

/// How many voice events are stored.
pub fn voice_event_count() -> usize {
    events_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|contents| {
            contents
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count()
        })
        .unwrap_or(0)
}

/// The fingerprints of the writer's own edits, for the copy guard.
pub fn feedback_fingerprint_set() -> std::collections::BTreeSet<u64> {
    let (events, _) = read_feedback().unwrap_or_default();
    feedback_fingerprints(&events, default_fingerprint_size())
}

fn read_feedback() -> Result<(Vec<StoredSuggestionEvent>, usize), String> {
    let path = feedback_path()?;
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(error) => return Err(format!("Could not read voice feedback: {error}")),
    };
    let events: Vec<StoredSuggestionEvent> = contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let (kept, dropped) = prune_events(&events, now_seconds(), FEEDBACK_RETENTION_DAYS);
    Ok((kept, dropped))
}

fn write_feedback(events: &[StoredSuggestionEvent]) -> Result<(), String> {
    let path = feedback_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the voice directory: {error}"))?;
    }
    let mut body = String::new();
    for event in events {
        let line = serde_json::to_string(event).map_err(|error| error.to_string())?;
        body.push_str(&line);
        body.push('\n');
    }
    std::fs::write(&path, body)
        .map_err(|error| format!("Could not write the voice feedback: {error}"))
}

fn read_voice_events() -> Vec<VoiceEvent> {
    events_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|contents| {
            contents
                .lines()
                .filter(|line| !line.trim().is_empty())
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

pub fn load() -> Result<Option<VoiceProfile>, String> {
    let path = path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read the voice profile: {error}")),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("Invalid voice profile: {error}"))
}

pub fn save(profile: &VoiceProfile) -> Result<(), String> {
    let path = path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the voice directory: {error}"))?;
    }
    std::fs::write(
        &path,
        serde_json::to_vec(profile).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("Could not write the voice profile: {error}"))
}

pub fn clear() -> Result<(), String> {
    let profile = path()?;
    match std::fs::remove_file(&profile) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not clear the voice profile: {error}")),
    }
    if let Ok(feedback) = feedback_path() {
        let _ = std::fs::remove_file(feedback);
    }
    if let Ok(events) = events_path() {
        let _ = std::fs::remove_file(events);
    }
    crate::personalization_db::clear_learned_voice_data()?;
    let _ = set_learning_enabled(false);
    Ok(())
}

/// Learn a voice profile from pasted writing.
///
/// The sample text is analyzed in memory and then discarded. Only the
/// aggregate traits and the sample fingerprints are written to disk.
pub fn learn(text: &str, name: Option<String>) -> Result<VoiceProfile, String> {
    if text.trim().is_empty() {
        return Err("Paste at least a few paragraphs of your writing first.".into());
    }
    let samples = vec![text.to_string()];
    let mut profile = analyze_samples(
        format!("voice-{}", now_seconds()),
        name.unwrap_or_else(|| "My voice".to_string()),
        VoiceContext::General,
        &samples,
    );
    if !profile.is_usable() {
        return Err(
            "That sample is too short to learn a reliable voice. Add at least 300 words.".into(),
        );
    }
    profile.strength = VoiceStrength::Balanced;
    // The copy guard needs a reference. The sample text is hashed into
    // fingerprints and then dropped; the text itself is never written.
    add_sample_guards(&mut profile, text, default_fingerprint_size());
    let _ = learn_from_feedback(&mut profile);
    save(&profile)?;
    Ok(profile)
}

/// Fold the stored feedback into a profile without touching the disk.
fn learn_from_feedback(profile: &mut VoiceProfile) -> Result<usize, String> {
    let (events, _) = read_feedback()?;
    if events.is_empty() {
        return Ok(0);
    }
    let live: Vec<SuggestionEvent> = events.iter().map(StoredSuggestionEvent::to_event).collect();
    let summary = learn_from_suggestions(profile, &live, now_seconds(), FEEDBACK_HALF_LIFE_DAYS);
    Ok(summary.events as usize)
}

/// The exact style card that will be sent to a provider.
pub fn guidance() -> Option<String> {
    load()
        .ok()
        .flatten()
        .filter(|profile| profile.is_usable())
        .and_then(|profile| voice_prompt_guidance(&profile))
}

/// A summary of the profile for the Settings UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceProfileSummary {
    pub id: String,
    pub name: String,
    pub strength: String,
    pub confidence: f64,
    pub summary: String,
    pub trait_count: usize,
    pub override_count: usize,
    pub feedback_count: usize,
    pub sample_guards: usize,
    pub learning_enabled: bool,
}

/// A summary of the stored profile, for the Settings UI.
pub fn profile_summary() -> Option<VoiceProfileSummary> {
    let profile = load().ok().flatten()?;
    Some(VoiceProfileSummary {
        id: profile.id.clone(),
        name: profile.name.clone(),
        strength: format!("{:?}", profile.strength).to_lowercase(),
        confidence: profile.confidence,
        summary: profile.summary.clone(),
        trait_count: profile.traits.len(),
        override_count: profile
            .traits
            .values()
            .filter(|entry| entry.is_override())
            .count(),
        feedback_count: feedback_count(),
        sample_guards: profile.sample_fingerprints.len(),
        learning_enabled: is_learning_enabled(),
    })
}

/// A trait the user can override, as the UI sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitOverride {
    pub key: String,
    pub value: TraitValue,
    pub confidence: f64,
    pub is_override: bool,
    pub evidence: Vec<String>,
}

/// The traits of the profile, ready for the Settings UI.
pub fn trait_overrides_for_ui() -> Vec<TraitOverride> {
    let Some(profile) = load().ok().flatten() else {
        return Vec::new();
    };
    profile
        .traits
        .iter()
        .map(|(key, entry)| TraitOverride {
            key: key.clone(),
            value: entry.value.clone(),
            confidence: entry.confidence,
            is_override: entry.is_override(),
            evidence: entry.evidence.clone(),
        })
        .collect()
}

/// Build a trait value from a plain number, for the override UI.
pub fn numeric_trait(value: f64) -> TraitValue {
    TraitValue::Number(value)
}

/// Build a trait value from a plain boolean, for the override UI.
pub fn boolean_trait(value: bool) -> TraitValue {
    TraitValue::Bool(value)
}

/// Build a trait value from plain text, for the override UI.
pub fn text_trait(value: &str) -> TraitValue {
    TraitValue::Text(value.to_string())
}

/// A trait value in the shape the settings UI expects.
///
/// The UI works with plain JSON values, not the tagged enum the crate uses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TraitValueJson {
    Number(f64),
    Bool(bool),
    Text(String),
}

impl From<&TraitValue> for TraitValueJson {
    fn from(value: &TraitValue) -> Self {
        match value {
            TraitValue::Number(number) => TraitValueJson::Number(*number),
            TraitValue::Bool(boolean) => TraitValueJson::Bool(*boolean),
            TraitValue::Text(text) => TraitValueJson::Text(text.clone()),
        }
    }
}

impl From<TraitValueJson> for TraitValue {
    fn from(value: TraitValueJson) -> Self {
        match value {
            TraitValueJson::Number(number) => TraitValue::Number(number),
            TraitValueJson::Bool(boolean) => TraitValue::Bool(boolean),
            TraitValueJson::Text(text) => TraitValue::Text(text),
        }
    }
}

/// A trait as the settings UI sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitView {
    pub key: String,
    pub value: TraitValueJson,
    pub confidence: f64,
    /// Whether the user set this trait by hand.
    pub is_override: bool,
    pub evidence: Vec<String>,
}

/// The traits of the profile, ready for the settings UI.
pub fn traits_for_ui() -> Vec<TraitView> {
    let Some(profile) = load().ok().flatten() else {
        return Vec::new();
    };
    profile
        .traits
        .iter()
        .map(|(key, entry)| TraitView {
            key: key.clone(),
            value: TraitValueJson::from(&entry.value),
            confidence: entry.confidence,
            is_override: entry.is_override(),
            evidence: entry.evidence.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_samples_are_rejected() {
        let error = learn("I write a little.", None).unwrap_err();
        assert!(error.contains("too short"));
    }

    #[test]
    fn stored_events_round_trip_through_the_log_format() {
        let event = SuggestionEvent {
            kind: SuggestionEventKind::EditedAfterAccept,
            category: "clarity".into(),
            original: "we are going".into(),
            replacement: "we're going".into(),
            final_text: "we'll go".into(),
            origin: EditOrigin::Model,
            app: "com.apple.mail".into(),
            timestamp: 1_700_000_000,
        };
        let stored = StoredSuggestionEvent::from_event(&event);
        let line = serde_json::to_string(&stored).unwrap();
        let parsed: StoredSuggestionEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed.to_event(), event);
    }

    #[test]
    fn legacy_log_lines_still_parse() {
        let line =
            r#"{"original":"a","replacement":"b","category":"clarity","timestamp":1700000000}"#;
        let parsed: StoredSuggestionEvent = serde_json::from_str(line).unwrap();
        assert_eq!(parsed.kind, "accepted");
        assert_eq!(parsed.origin, "user");
    }

    #[test]
    fn the_stored_log_row_has_the_documented_shape() {
        let event = SuggestionEvent {
            kind: SuggestionEventKind::Dismissed,
            category: "word_choice".into(),
            original: "utilize".into(),
            replacement: "use".into(),
            final_text: String::new(),
            origin: EditOrigin::User,
            app: "com.apple.mail".into(),
            timestamp: 1_700_000_000,
        };
        let value = serde_json::to_value(StoredSuggestionEvent::from_event(&event)).unwrap();
        assert_eq!(value["kind"], "dismissed");
        assert_eq!(value["category"], "word_choice");
        assert_eq!(value["origin"], "user");
        assert_eq!(value["timestamp"], 1_700_000_000u64);
        assert_eq!(value["original"], "utilize");
        assert_eq!(value["replacement"], "use");
    }

    #[test]
    fn stored_text_is_truncated_to_the_retention_limit() {
        let event = SuggestionEvent {
            kind: SuggestionEventKind::Accepted,
            category: "clarity".into(),
            original: "x".repeat(1_000),
            replacement: "y".repeat(1_000),
            final_text: String::new(),
            origin: EditOrigin::User,
            app: String::new(),
            timestamp: 1_700_000_000,
        };
        let stored = StoredSuggestionEvent::from_event(&event);
        assert_eq!(
            stored.original.chars().count(),
            elocutio_voice::MAX_STORED_TEXT_CHARS + 1
        );
        assert_eq!(
            stored.replacement.chars().count(),
            elocutio_voice::MAX_STORED_TEXT_CHARS + 1
        );
    }

    #[test]
    fn the_copy_guard_rejects_a_copied_sample_phrase() {
        let samples = vec![
            "The quarterly planning meeting moved to Thursday afternoon this week.".to_string(),
        ];
        let mut profile = VoiceProfile::new("vp1", "Work", VoiceContext::Email);
        for sample in &samples {
            add_sample_guards(&mut profile, sample, default_fingerprint_size());
        }
        let copy = "I should note that the quarterly planning meeting moved to Thursday afternoon.";
        assert!(check_for_copy_against(&profile, copy).is_err());
        let fresh = "A completely unrelated sentence about the weather today.";
        assert!(check_for_copy_against(&profile, fresh).is_ok());
    }
}
