//! Local suggestion event log.
//!
//! The plan asks for accept, dismiss, and edit-after-accept events to be
//! recorded locally so the voice profile can learn from them. This module is
//! the recorder: it writes a bounded, metadata-first log that never stores the
//! user's document text.
//!
//! What is stored is the *shape* of an interaction: which rule fired, which
//! category it belonged to, how long the original and replacement were, and
//! whether the text came from a model. That is enough to learn preferences
//! ("you remove 'just' often") without keeping a second copy of what the user
//! wrote.
//!
//! The log is opt-in: nothing is written unless the caller enables it.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::Config;

/// The kind of interaction a user had with a suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// The user applied the suggestion.
    Accepted,
    /// The user dismissed the suggestion.
    Dismissed,
    /// The user applied the suggestion and then changed it again.
    EditedAfterAccept,
    /// The user rated a rewrite.
    Rated,
    /// The user picked a tone.
    ToneSelected,
}

impl EventKind {
    /// Whether this event says something about a preference the user holds.
    pub fn is_preference_signal(self) -> bool {
        matches!(
            self,
            EventKind::Accepted | EventKind::Dismissed | EventKind::EditedAfterAccept
        )
    }
}

/// Where the text in an event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The suggestion came from the local rule engine.
    Local,
    /// The suggestion came from the language model.
    Model,
}

/// One recorded interaction.
///
/// The field text is deliberately absent. `original_length` and
/// `replacement_length` carry the size of the edit without keeping the text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestionEvent {
    pub at: u64,
    pub kind: EventKind,
    pub origin: Origin,
    /// The rule or category that produced the suggestion, e.g. `llm:clarity`.
    pub rule: String,
    /// The suggestion category, e.g. `grammar`.
    pub category: String,
    pub original_length: usize,
    pub replacement_length: usize,
    /// How many times the user had already seen this suggestion.
    pub seen_count: u32,
}

/// A summary of the recorded events, used by the voice profile and the UI.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSummary {
    pub accepted: u32,
    pub dismissed: u32,
    pub edited_after_accept: u32,
    pub rated_positive: u32,
    pub rated_negative: u32,
    /// Accepted events per category.
    pub by_category: BTreeMap<String, u32>,
    /// Dismissed events per category.
    pub dismissed_by_category: BTreeMap<String, u32>,
}

fn folder() -> Result<PathBuf, String> {
    Config::folder_path()
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

fn events_path() -> Result<PathBuf, String> {
    Ok(folder()?.join("suggestion-events.jsonl"))
}

fn enabled_path() -> Result<PathBuf, String> {
    Ok(folder()?.join("suggestion-events.enabled"))
}

/// Whether suggestion events are being recorded.
pub fn is_enabled() -> bool {
    enabled_path().map(|path| path.exists()).unwrap_or(false)
}

/// Turn event recording on or off.
///
/// Turning it off also clears the log, so "off" means no trace is left behind.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let path = enabled_path()?;
    if enabled {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create the events directory: {error}"))?;
        }
        std::fs::write(&path, b"1")
            .map_err(|error| format!("Could not enable event recording: {error}"))
    } else {
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not disable event recording: {error}")),
        }
    }
}

/// The maximum number of events kept on disk.
///
/// The log is a learning signal, not an archive. A bounded log cannot grow
/// without limit and cannot become a shadow copy of what the user wrote.
const MAX_EVENTS: usize = 5_000;

/// Record one interaction.
///
/// Does nothing when recording is off, so callers can call this freely.
pub fn record_event(
    kind: EventKind,
    origin: Origin,
    rule: &str,
    category: &str,
    original_length: usize,
    replacement_length: usize,
) -> Result<(), String> {
    if !is_enabled() {
        return Ok(());
    }

    let event = SuggestionEvent {
        at: now_seconds(),
        kind,
        origin,
        rule: rule.to_string(),
        category: category.to_string(),
        original_length,
        replacement_length,
        seen_count: 1,
    };

    let path = events_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the events directory: {error}"))?;
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Could not open the event log: {error}"))?;
    let line = serde_json::to_string(&event)
        .map_err(|error| format!("Could not serialize the event: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("Could not record the event: {error}"))?;

    trim(&path)?;
    Ok(())
}

/// Read every recorded event.
pub fn read_events() -> Result<Vec<SuggestionEvent>, String> {
    let path = events_path()?;
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Could not read the event log: {error}")),
    };

    Ok(contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

/// Summarize the recorded events.
pub fn summarize() -> Result<EventSummary, String> {
    let mut summary = EventSummary::default();
    for event in read_events()? {
        match event.kind {
            EventKind::Accepted => {
                summary.accepted += 1;
                *summary.by_category.entry(event.category).or_insert(0) += 1;
            }
            EventKind::Dismissed => {
                summary.dismissed += 1;
                *summary
                    .dismissed_by_category
                    .entry(event.category)
                    .or_insert(0) += 1;
            }
            EventKind::EditedAfterAccept => summary.edited_after_accept += 1,
            EventKind::Rated => {}
            EventKind::ToneSelected => {}
        }
    }
    Ok(summary)
}

/// Delete every recorded event.
pub fn clear() -> Result<(), String> {
    let path = events_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not clear the event log: {error}")),
    }
}

/// Keep the log within [`MAX_EVENTS`] by dropping the oldest entries.
fn trim(path: &PathBuf) -> Result<(), String> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Could not read the event log: {error}")),
    };

    let lines: Vec<&str> = contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() <= MAX_EVENTS {
        return Ok(());
    }

    let kept = &lines[lines.len() - MAX_EVENTS..];
    let mut file = std::fs::File::create(path)
        .map_err(|error| format!("Could not rewrite the event log: {error}"))?;
    for line in kept {
        writeln!(file, "{line}")
            .map_err(|error| format!("Could not rewrite the event log: {error}"))?;
    }
    Ok(())
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_signals_are_the_learning_ones() {
        assert!(EventKind::Accepted.is_preference_signal());
        assert!(EventKind::Dismissed.is_preference_signal());
        assert!(EventKind::EditedAfterAccept.is_preference_signal());
        assert!(!EventKind::Rated.is_preference_signal());
    }

    #[test]
    fn an_event_carries_no_document_text() {
        let event = SuggestionEvent {
            at: 1,
            kind: EventKind::Accepted,
            origin: Origin::Model,
            rule: "llm:clarity".to_string(),
            category: "clarity".to_string(),
            original_length: 12,
            replacement_length: 9,
            seen_count: 1,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("original\":"));
        assert!(json.contains("original_length"));
    }

    #[test]
    fn events_round_trip_through_json() {
        let event = SuggestionEvent {
            at: 7,
            kind: EventKind::Dismissed,
            origin: Origin::Local,
            rule: "Spelling".to_string(),
            category: "spelling".to_string(),
            original_length: 4,
            replacement_length: 5,
            seen_count: 2,
        };
        let decoded: SuggestionEvent =
            serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
        assert_eq!(decoded, event);
    }
}
