//! Explicit system-wide "Rewrite Selection" action.
//!
//! The provider key and network live in the main process; the broker reads the
//! current selection and replaces it with a synthetic paste. This is the first
//! slice of the M1 write ladder: pasteboard + ⌘V with read-back verification.

use std::sync::RwLock;

use tauri::Manager;
use tauri_plugin_notification::NotificationExt;

use crate::main_thread;

use crate::os_broker::OsBroker;
use crate::{PlatformBroker, openrouter, usage};

const MAX_SELECTION_CHARS: usize = 12_000;

pub async fn rewrite_selection(app: &tauri::AppHandle) -> Result<String, String> {
    let text = main_thread::run(app, |app| {
        let state = app.state::<RwLock<PlatformBroker>>();
        let mut broker = state
            .write()
            .map_err(|error| format!("Could not lock the platform broker: {error}"))?;
        broker.selected_text()
    })??;
    let text = text.ok_or_else(|| "Select text in another app first.".to_string())?;

    if text.chars().count() > MAX_SELECTION_CHARS {
        return Err(format!(
            "Selection is too long ({} characters). Limit is {}.",
            text.chars().count(),
            MAX_SELECTION_CHARS
        ));
    }

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
    // The system-wide rewrite honours the user's custom instruction, so it
    // goes through the context-aware path.
    let result = openrouter::rewrite_with(
        &provider,
        &text,
        Some(model),
        crate::voice::guidance(),
        tone,
        Some(instruction.as_str()),
    )
    .await?;
    if let Err(error) = usage::record(&result) {
        eprintln!("Could not record usage: {error}");
    }

    let replacement = result.text;
    let replacement_for_paste = replacement.clone();
    let verified = main_thread::run(app, move |app| {
        let state = app.state::<RwLock<PlatformBroker>>();
        let mut broker = state
            .write()
            .map_err(|error| format!("Could not lock the platform broker: {error}"))?;
        broker.replace_selection_via_paste(&replacement_for_paste)
    })??;

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
