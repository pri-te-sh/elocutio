//! Explicit "Analyze Field" action: read the focused field, analyze the first
//! paragraphs with the provider, and publish validated suggestions for the
//! highlighter to render.

use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use elocutio_analysis::{chunk_paragraphs, parse_response};
use elocutio_core::Suggestion;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::config::Config;
use crate::llm_cache::{CachedAnalysis, CachedSuggestion, text_hash};
use crate::os_broker::OsBroker;
use crate::{PlatformBroker, llm_cache, main_thread, openrouter, rewrite, usage};

pub async fn analyze_focused_field(app: &AppHandle) -> Result<String, String> {
    let focused = main_thread::run(app, |app| {
        let state = app.state::<RwLock<PlatformBroker>>();
        let mut broker = state
            .write()
            .map_err(|error| format!("Could not lock the platform broker: {error}"))?;
        broker.focused_text()
    })??;

    let focused = focused
        .ok_or_else(|| "Focus a text field in TextEdit, Notes, or Safari first.".to_string())?;

    let model = {
        let config = app.state::<Arc<Mutex<Config>>>();
        config.lock().await.provider_model.clone()
    };

    let paragraphs = chunk_paragraphs(&focused.text);
    if paragraphs.is_empty() {
        return Err("The focused field is empty.".into());
    }

    let mut suggestions = Vec::new();
    for paragraph in paragraphs.iter().take(3) {
        let completion =
            openrouter::analyze(&paragraph.id, &paragraph.text, Some(model.clone()), "en-US")
                .await?;
        let parsed = parse_response(&completion.content, &paragraphs)?;
        eprintln!(
            "Analyze {}: model={} content_chars={} suggestions={} rejected={}",
            paragraph.id,
            completion.model,
            completion.content.chars().count(),
            parsed.suggestions.len(),
            parsed.rejected.len()
        );
        for rejected in &parsed.rejected {
            eprintln!("Rejected {}: {}", rejected.paragraph_id, rejected.reason);
        }

        let paragraph_char_start = focused.text[..paragraph.byte_start].chars().count();
        for suggestion in parsed.suggestions {
            let char_start =
                paragraph_char_start + utf16_to_char(&paragraph.text, suggestion.span.utf16_start);
            let char_end =
                paragraph_char_start + utf16_to_char(&paragraph.text, suggestion.span.utf16_end);
            suggestions.push(cache_suggestion(suggestion, char_start, char_end));
        }

        if let Err(error) = usage::record_parts(
            &completion.model,
            completion.input_tokens,
            completion.output_tokens,
            completion.cost_usd,
        ) {
            eprintln!("Could not record usage: {error}");
        }
    }

    let count = suggestions.len();
    let cache = CachedAnalysis {
        pid: focused.pid,
        bundle_id: focused.bundle_id,
        text_hash: text_hash(&focused.text),
        model: model.clone(),
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs(),
        suggestions,
    };
    llm_cache::write(&cache)?;

    if count == 0 {
        Ok("No safe LLM suggestions for the focused field.".to_string())
    } else {
        Ok(format!("{count} LLM suggestion(s) ready in the overlay."))
    }
}

fn cache_suggestion(
    suggestion: Suggestion,
    char_start: usize,
    char_end: usize,
) -> CachedSuggestion {
    CachedSuggestion {
        id: format!("llm:{}", suggestion.id),
        char_start,
        char_end,
        original: suggestion.original,
        replacement: suggestion.replacement,
        category: suggestion.category,
        severity: suggestion.severity,
        explanation: suggestion.explanation,
        resolution: suggestion.resolution,
    }
}

fn utf16_to_char(text: &str, utf16_index: usize) -> usize {
    let mut utf16 = 0;
    for (char_index, character) in text.chars().enumerate() {
        if utf16 >= utf16_index {
            return char_index;
        }
        utf16 += character.len_utf16();
    }
    text.chars().count()
}

pub fn notify_result(app: &AppHandle, result: Result<String, String>) {
    match result {
        Ok(message) => rewrite::notify(app, "Elocutio analysis", &message),
        Err(error) => rewrite::notify(app, "Analysis failed", &error),
    }
}
