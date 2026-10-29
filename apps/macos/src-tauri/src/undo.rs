//! One-shot undo for an accepted Elocutio suggestion.
//!
//! The highlighter writes the field's before/after text after a successful
//! apply. The main process restores it only if the focused field still matches
//! the after text. The record is deleted after use.

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::config::Config;
use crate::os_broker::OsBroker;
use crate::{PlatformBroker, main_thread};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoRecord {
    pub pid: i32,
    pub before: String,
    pub after: String,
    pub created_at: u64,
}

fn path() -> Result<PathBuf, String> {
    Config::folder_path()
        .map(|folder| folder.join("last-edit-undo.json"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

fn can_restore(record: &UndoRecord, pid: i32, text: &str) -> bool {
    record.pid == pid && record.after == text
}

pub fn record(pid: i32, before: &str, after: &str) -> Result<(), String> {
    let record = UndoRecord {
        pid,
        before: before.to_string(),
        after: after.to_string(),
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs(),
    };
    let path = path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the undo directory: {error}"))?;
    }
    std::fs::write(
        &path,
        serde_json::to_vec(&record).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("Could not write the undo record: {error}"))
}

pub fn take() -> Result<Option<UndoRecord>, String> {
    let path = path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read the undo record: {error}")),
    };
    let _ = std::fs::remove_file(&path);
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("Invalid undo record: {error}"))
}

pub async fn undo_last_edit<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
    let record = take()?.ok_or_else(|| "There is no Elocutio edit to undo.".to_string())?;

    let focused = main_thread::run_async(app, |app| {
        let state = app.state::<RwLock<PlatformBroker>>();
        let mut broker = state
            .write()
            .map_err(|error| format!("Could not lock the platform broker: {error}"))?;
        broker.focused_text()
    })
    .await??;

    let Some(focused) = focused else {
        return Err("Focus the edited field again before undoing.".into());
    };
    if !can_restore(&record, focused.pid, &focused.text) {
        return Err("The field changed after the edit, so Elocutio did not overwrite it.".into());
    }

    let before = record.before;
    let after = record.after;
    let restored = main_thread::run_async(app, {
        let before = before.clone();
        move |app| {
            let state = app.state::<RwLock<PlatformBroker>>();
            let mut broker = state
                .write()
                .map_err(|error| format!("Could not lock the platform broker: {error}"))?;
            broker.replace_focused_text(&before)
        }
    })
    .await??;

    if restored {
        // Undoing an accepted suggestion is the strongest preference signal
        // there is: the writer disagreed with the proposed wording. The voice
        // profile learns from it, so the same edit is not proposed again.
        if let Err(error) = crate::voice::record_edited_after_accept(
            &before,
            &after,
            &before,
            "edited_after_accept",
        ) {
            eprintln!("Could not record the edited-after-accept event: {error}");
        }

        Ok("Reverted the last Elocutio edit.".to_string())
    } else {
        Err("Could not restore the previous field text.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_only_applies_to_the_same_pid_and_text() {
        let record = UndoRecord {
            pid: 7,
            before: "before".into(),
            after: "after".into(),
            created_at: 1,
        };
        assert!(can_restore(&record, 7, "after"));
        assert!(!can_restore(&record, 8, "after"));
        assert!(!can_restore(&record, 7, "changed"));
    }

    #[test]
    fn undo_record_round_trips() {
        let path =
            std::env::temp_dir().join(format!("elocutio-undo-test-{}.json", std::process::id()));
        let record = UndoRecord {
            pid: 42,
            before: "before".into(),
            after: "after".into(),
            created_at: 1,
        };
        std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let decoded: UndoRecord = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.before, "before");
        assert_eq!(decoded.after, "after");
        let _ = std::fs::remove_file(path);
    }
}
