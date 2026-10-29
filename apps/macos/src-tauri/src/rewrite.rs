//! Explicit system-wide "Rewrite Selection" action.
//!
//! The provider key and network live in the main process; the broker reads the
//! current selection and replaces it with a synthetic paste. This is the first
//! slice of the M1 write ladder: pasteboard + ⌘V with read-back verification.
//!
//! The action has two modes. **Preview** produces the rewrite and shows a
//! word-level diff without touching the field, so the user can see what would
//! change. **Apply** replaces the selection. The preview is cached, so the
//! user can look at the diff and then apply exactly what they saw.

use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::main_thread;

use crate::os_broker::OsBroker;
use crate::{PlatformBroker, openrouter, usage};

const MAX_SELECTION_CHARS: usize = 12_000;

/// A rewrite that has been prepared but not yet applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRewrite {
    /// The text the user had selected.
    pub original: String,
    /// The rewritten text.
    pub rewritten: String,
    /// The word-level diff between the two.
    pub diff: Vec<elocutio_analysis::DiffSegment>,
    /// The model that produced it.
    pub model: String,
    /// When it was prepared, in unix seconds.
    pub created_at: u64,
}

fn pending_path() -> Result<std::path::PathBuf, String> {
    crate::config::Config::folder_path()
        .map(|folder| folder.join("pending-rewrite.json"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())
}

/// Store a prepared rewrite so it can be applied later.
fn save_pending(pending: &PendingRewrite) -> Result<(), String> {
    let path = pending_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!("Could not create the application support directory: {error}")
        })?;
    }
    let json = serde_json::to_vec(pending)
        .map_err(|error| format!("Could not serialize the rewrite: {error}"))?;
    std::fs::write(&path, json).map_err(|error| format!("Could not store the rewrite: {error}"))
}

/// Read the prepared rewrite, if it is still there.
pub fn load_pending() -> Result<Option<PendingRewrite>, String> {
    let path = pending_path()?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read the prepared rewrite: {error}")),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("Invalid prepared rewrite: {error}"))
}

/// Forget the prepared rewrite.
fn clear_pending() {
    if let Ok(path) = pending_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Rewrite the selection and return the result without changing the field.
///
/// The result carries a word-level diff so the user can see exactly what
/// would change before deciding.
pub async fn preview_selection(app: &tauri::AppHandle) -> Result<PendingRewrite, String> {
    let text = selected_text(app).await?;

    let (provider, model, tone, cost_cap, instruction) = {
        let state = app.state::<std::sync::Arc<tokio::sync::Mutex<crate::config::Config>>>();
        let config = state.lock().await;
        if config.local_only {
            return Err("Local-only mode is on. Turn it off to rewrite with a provider.".into());
        }
        (
            openrouter::ProviderConfig {
                kind: config.provider_kind,
                base_url: config.provider_base_url.clone(),
            },
            config.provider_model.clone(),
            config.rewrite_tone,
            config.cost_cap_usd,
            config.custom_instruction.clone(),
        )
    };
    openrouter::check_cost_cap(
        &model,
        text.chars().count(),
        openrouter::REWRITE_MAX_TOKENS,
        cost_cap,
    )?;

    let _ = app.emit("elo-rewrite-state", serde_json::json!({ "active": true }));
    let rewrite_res = openrouter::rewrite_with(
        &provider,
        &text,
        Some(model),
        crate::voice::guidance(),
        tone,
        Some(instruction.as_str()),
    )
    .await;
    let _ = app.emit("elo-rewrite-state", serde_json::json!({ "active": false }));
    let result = rewrite_res?;
    if let Err(error) = usage::record(&result) {
        eprintln!("Could not record usage: {error}");
    }

    let pending = PendingRewrite {
        original: text.clone(),
        rewritten: result.text.clone(),
        diff: elocutio_analysis::diff_words(&text, &result.text),
        model: result.model.clone(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0),
    };
    save_pending(&pending)?;

    Ok(pending)
}

/// Apply the prepared rewrite to the selection.
///
/// The prepared rewrite is removed afterwards, so a second press cannot apply
/// the same text twice.
pub async fn apply_pending(app: &tauri::AppHandle) -> Result<String, String> {
    let pending = load_pending()?.ok_or_else(|| {
        "There is no prepared rewrite to apply. Press the shortcut again to make one.".to_string()
    })?;

    let replacement = pending.rewritten.clone();
    let replacement_for_paste = replacement.clone();
    let verified = main_thread::run_async(app, move |app| {
        let state = app.state::<RwLock<PlatformBroker>>();
        let mut broker = state
            .write()
            .map_err(|error| format!("Could not lock the platform broker: {error}"))?;
        broker.replace_selection_via_paste(&replacement_for_paste)
    })
    .await??;

    clear_pending();

    if !verified {
        return Err(
            "Pasted the rewrite but could not read it back. Check the target app.".to_string(),
        );
    }

    Ok(format!(
        "Replaced the selection with {} characters using {}.",
        replacement.chars().count(),
        pending.model
    ))
}

/// Read the current selection, enforcing the length cap.
async fn selected_text(app: &tauri::AppHandle) -> Result<String, String> {
    let text = main_thread::run_async(app, |app| {
        let state = app.state::<RwLock<PlatformBroker>>();
        let mut broker = state
            .write()
            .map_err(|error| format!("Could not lock the platform broker: {error}"))?;
        broker.selected_text()
    })
    .await??;
    let text = text.ok_or_else(|| "Select text in another app first.".to_string())?;

    if text.chars().count() > MAX_SELECTION_CHARS {
        return Err(format!(
            "Selection is too long ({} characters). Limit is {}.",
            text.chars().count(),
            MAX_SELECTION_CHARS
        ));
    }

    Ok(text)
}

/// Rewrite the selection and apply it straight away.
#[allow(dead_code)]
pub async fn rewrite_selection(app: &tauri::AppHandle) -> Result<String, String> {
    let text = selected_text(app).await?;

    let (provider, model, tone, cost_cap, instruction) = {
        let state = app.state::<std::sync::Arc<tokio::sync::Mutex<crate::config::Config>>>();
        let config = state.lock().await;
        if config.local_only {
            return Err("Local-only mode is on. Turn it off to rewrite with a provider.".into());
        }
        (
            openrouter::ProviderConfig {
                kind: config.provider_kind,
                base_url: config.provider_base_url.clone(),
            },
            config.provider_model.clone(),
            config.rewrite_tone,
            config.cost_cap_usd,
            config.custom_instruction.clone(),
        )
    };
    openrouter::check_cost_cap(
        &model,
        text.chars().count(),
        openrouter::REWRITE_MAX_TOKENS,
        cost_cap,
    )?;
    let _ = app.emit("elo-rewrite-state", serde_json::json!({ "active": true }));
    let rewrite_res = openrouter::rewrite_with(
        &provider,
        &text,
        Some(model),
        crate::voice::guidance(),
        tone,
        Some(instruction.as_str()),
    )
    .await;
    let _ = app.emit("elo-rewrite-state", serde_json::json!({ "active": false }));
    let result = rewrite_res?;
    if let Err(error) = usage::record(&result) {
        eprintln!("Could not record usage: {error}");
    }

    let replacement = result.text;
    let replacement_for_paste = replacement.clone();
    let verified = main_thread::run_async(app, move |app| {
        let state = app.state::<RwLock<PlatformBroker>>();
        let mut broker = state
            .write()
            .map_err(|error| format!("Could not lock the platform broker: {error}"))?;
        broker.replace_selection_via_paste(&replacement_for_paste)
    })
    .await??;

    if !verified {
        return Err(
            "Pasted the rewrite but could not read it back. Check the target app.".to_string(),
        );
    }

    Ok(format!(
        "Replaced the selection with {} characters using {}.",
        replacement.chars().count(),
        result.model
    ))
}

pub fn notify(app: &tauri::AppHandle, title: &str, body: &str) {
    if let Err(error) = app.notification().builder().title(title).body(body).show() {
        eprintln!("Could not show notification: {error}");
    }
}
