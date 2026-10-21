//! Retention for the local feedback log.
//!
//! The log grows without bound if nothing prunes it, and it stores the text
//! of the writer's edits. Two rules keep it honest:
//!
//! * Events older than the retention window are dropped.
//! * The stored text is truncated to the shortest useful length, so the log
//!   holds the signal and not a copy of everything the user typed.
//!
//! The half-life (180 days) and the retention window (365 days) are separate:
//! decay decides how much an old event counts, retention decides how long it
//! is kept at all.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// How long feedback events are kept on disk, in days.
pub const FEEDBACK_RETENTION_DAYS: u64 = 365;

/// The longest text kept per event, in characters.
pub const MAX_STORED_TEXT_CHARS: usize = 240;

/// A stored suggestion event, as written to `voice-feedback.jsonl`.
///
/// The shape is backward compatible with the first version of the file, which
/// stored `original`, `replacement`, `category`, and `timestamp`. The newer
/// fields all have defaults, so old lines still parse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSuggestionEvent {
    /// The event kind: `accepted`, `dismissed`, or `edited_after_accept`.
    #[serde(default = "default_kind")]
    pub kind: String,
    /// The original text, truncated.
    #[serde(default)]
    pub original: String,
    /// The proposed text, truncated.
    #[serde(default)]
    pub replacement: String,
    /// The text the writer ended up with, truncated. Only set for
    /// `edited_after_accept`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub final_text: String,
    /// The suggestion category, e.g. "clarity".
    #[serde(default)]
    pub category: String,
    /// Where the replacement text came from: `user` or `model`.
    ///
    /// Lines written before this field existed default to `user`.
    #[serde(default = "default_origin")]
    pub origin: String,
    /// The app the suggestion came from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub app: String,
    /// When the event happened, in seconds since the Unix epoch.
    #[serde(default)]
    pub timestamp: u64,
}

fn default_kind() -> String {
    "accepted".to_string()
}

fn default_origin() -> String {
    "user".to_string()
}

impl StoredSuggestionEvent {
    /// Build a stored event from a live event, truncating the text.
    pub fn from_event(event: &crate::feedback::SuggestionEvent) -> Self {
        Self {
            kind: event.kind.as_str().to_string(),
            original: truncate(&event.original),
            replacement: truncate(&event.replacement),
            final_text: truncate(&event.final_text),
            category: event.category.clone(),
            origin: match event.origin {
                crate::feedback::EditOrigin::User => "user".to_string(),
                crate::feedback::EditOrigin::Model => "model".to_string(),
            },
            app: event.app.clone(),
            timestamp: event.timestamp,
        }
    }

    /// Convert back into a live event.
    ///
    /// Unknown kinds and unknown origins fall back to the safe defaults:
    /// an accepted edit from the user.
    pub fn to_event(&self) -> crate::feedback::SuggestionEvent {
        let kind = crate::feedback::SuggestionEventKind::from_str_name(&self.kind)
            .unwrap_or(crate::feedback::SuggestionEventKind::Accepted);
        let origin = match self.origin.as_str() {
            "model" => crate::feedback::EditOrigin::Model,
            _ => crate::feedback::EditOrigin::User,
        };
        crate::feedback::SuggestionEvent {
            kind,
            category: self.category.clone(),
            original: self.original.clone(),
            replacement: self.replacement.clone(),
            final_text: self.final_text.clone(),
            origin,
            app: self.app.clone(),
            timestamp: self.timestamp,
        }
    }
}

/// Truncate stored text to the retention limit.
pub fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_STORED_TEXT_CHARS {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(MAX_STORED_TEXT_CHARS).collect();
    truncated.push('\u{2026}');
    truncated
}

/// Whether an event is still inside the retention window.
pub fn is_within_retention(timestamp: u64, now: u64, retention_days: u64) -> bool {
    let max_age = retention_days.saturating_mul(86_400);
    now.saturating_sub(timestamp) <= max_age
}

/// Prune events that fell outside the retention window.
///
/// Returns the kept events and how many were dropped.
pub fn prune_events(
    events: &[StoredSuggestionEvent],
    now: u64,
    retention_days: u64,
) -> (Vec<StoredSuggestionEvent>, usize) {
    let mut kept = Vec::with_capacity(events.len());
    let mut dropped = 0;
    for event in events {
        if is_within_retention(event.timestamp, now, retention_days) {
            kept.push(event.clone());
        } else {
            dropped += 1;
        }
    }
    (kept, dropped)
}

/// The distinct n-gram fingerprints of the stored text of a set of events.
///
/// The copy guard needs to know what the writer's own edits look like, so the
/// guard can reject a rewrite that copies them. The fingerprints are hashes:
/// the text itself is not recoverable from them.
pub fn feedback_fingerprints(events: &[StoredSuggestionEvent], n: usize) -> BTreeSet<u64> {
    events
        .iter()
        .flat_map(|event| {
            let mut text = event.original.clone();
            text.push(' ');
            text.push_str(&event.replacement);
            crate::copy_guard::ngram_fingerprints(&text, n)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feedback::{EditOrigin, SuggestionEvent, SuggestionEventKind};

    const NOW: u64 = 1_700_000_000;
    const DAY: u64 = 86_400;

    #[test]
    fn old_lines_still_parse() {
        let line =
            r#"{"original":"a","replacement":"b","category":"clarity","timestamp":1700000000}"#;
        let stored: StoredSuggestionEvent = serde_json::from_str(line).unwrap();
        assert_eq!(stored.kind, "accepted");
        assert_eq!(stored.origin, "user");
        assert_eq!(stored.to_event().kind, SuggestionEventKind::Accepted);
    }

    #[test]
    fn new_lines_round_trip() {
        let event = SuggestionEvent::new(
            SuggestionEventKind::EditedAfterAccept,
            "style",
            "we are going",
            "we're going",
            EditOrigin::Model,
            NOW,
        );
        let stored = StoredSuggestionEvent::from_event(&event);
        let restored = stored.to_event();
        assert_eq!(restored.kind, SuggestionEventKind::EditedAfterAccept);
        assert_eq!(restored.origin, EditOrigin::Model);
    }

    /// The exact shape of a feedback row, so the privacy documentation can be
    /// written against a shape that is checked by a test.
    #[test]
    fn a_feedback_row_has_the_documented_shape() {
        let event = crate::feedback::SuggestionEvent {
            kind: SuggestionEventKind::Accepted,
            category: "clarity".into(),
            original: "we are going".into(),
            replacement: "we're going".into(),
            final_text: String::new(),
            origin: EditOrigin::User,
            app: "com.apple.mail".into(),
            timestamp: 1_700_000_000,
        };
        let value = serde_json::to_value(StoredSuggestionEvent::from_event(&event)).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "kind": "accepted",
                "original": "we are going",
                "replacement": "we're going",
                "category": "clarity",
                "origin": "user",
                "app": "com.apple.mail",
                "timestamp": 1_700_000_000u64,
            })
        );
    }

    #[test]
    fn long_text_is_truncated() {
        let long = "x".repeat(500);
        let stored = truncate(&long);
        assert_eq!(stored.chars().count(), MAX_STORED_TEXT_CHARS + 1);
    }

    #[test]
    fn short_text_is_untouched() {
        assert_eq!(truncate("short"), "short");
    }

    #[test]
    fn events_outside_the_window_are_dropped() {
        let events = vec![
            StoredSuggestionEvent {
                kind: "accepted".into(),
                original: "a".into(),
                replacement: "b".into(),
                final_text: String::new(),
                category: "clarity".into(),
                origin: "user".into(),
                app: String::new(),
                timestamp: NOW - 400 * DAY,
            },
            StoredSuggestionEvent {
                kind: "accepted".into(),
                original: "a".into(),
                replacement: "b".into(),
                final_text: String::new(),
                category: "clarity".into(),
                origin: "user".into(),
                app: String::new(),
                timestamp: NOW - 10 * DAY,
            },
        ];
        let (kept, dropped) = prune_events(&events, NOW, FEEDBACK_RETENTION_DAYS);
        assert_eq!(kept.len(), 1);
        assert_eq!(dropped, 1);
    }

    #[test]
    fn feedback_fingerprints_do_not_expose_the_text() {
        let events = vec![StoredSuggestionEvent {
            kind: "accepted".into(),
            original: "a very distinctive original phrase".into(),
            replacement: "another distinctive replacement phrase".into(),
            final_text: String::new(),
            category: "clarity".into(),
            origin: "user".into(),
            app: String::new(),
            timestamp: NOW,
        }];
        let fingerprints = feedback_fingerprints(&events, 4);
        assert!(!fingerprints.is_empty());
        let rendered: Vec<String> = fingerprints.iter().map(|value| value.to_string()).collect();
        assert!(!rendered.iter().any(|value| value.contains("distinctive")));
    }
}
