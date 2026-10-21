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

/// The shortest gap between two analyses of the same text.
const MIN_ANALYSIS_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
use crate::os_broker::OsBroker;
use crate::{PlatformBroker, llm_cache, main_thread, openrouter, rewrite, usage};

pub async fn analyze_focused_field(app: &AppHandle) -> Result<String, String> {
    analyze_focused_field_with(app, None).await
}

/// Analyze the focused field, optionally sending only what changed.
///
/// `previous_text` enables the diff-only behaviour the plan asks for in live
/// mode: when the caller knows what the field looked like at the last
/// analysis, only the paragraphs that changed since then are sent. The cost
/// of a long document then stops growing with its length.
pub async fn analyze_focused_field_with(
    app: &AppHandle,
    previous_text: Option<&str>,
) -> Result<String, String> {
    let focused = main_thread::run(app, |app| {
        let state = app.state::<RwLock<PlatformBroker>>();
        let mut broker = state
            .write()
            .map_err(|error| format!("Could not lock the platform broker: {error}"))?;
        broker.focused_text()
    })??;

    let focused = focused.ok_or_else(|| "Focus a text field in any app first.".to_string())?;

    let (provider, model) = {
        let config = app.state::<Arc<Mutex<Config>>>();
        let config = config.lock().await;
        if config.local_only {
            return Err(
                "Local-only mode is on. Turn it off to analyze the field with a provider."
                    .to_string(),
            );
        }
        (
            openrouter::ProviderConfig {
                kind: config.provider_kind,
                base_url: config.provider_base_url.clone(),
            },
            config.provider_model.clone(),
        )
    };

    let paragraphs = chunk_paragraphs(&focused.text);
    if paragraphs.is_empty() {
        return Err("The focused field is empty.".into());
    }

    // In live mode only the changed paragraphs are sent. The offsets are still
    // resolved against the full text, so a suggestion lands in the right place
    // even when it came from a partial request.
    let paragraphs: Vec<_> = match previous_text {
        Some(previous) => {
            let changed = crate::auto_analyze::changed_paragraphs(&focused.text, Some(previous));
            paragraphs
                .into_iter()
                .filter(|paragraph| changed.iter().any(|text| text == &paragraph.text))
                .collect()
        }
        None => paragraphs,
    };

    if paragraphs.is_empty() {
        return Ok("Nothing changed since the last analysis.".to_string());
    }

    // Coalesce: the same text, model, and dialect share one request, and a
    // burst of requests for the same key is throttled. A key that arrives
    // while another request is in flight waits for the newer one rather than
    // adding to the storm.
    let key = crate::coalesce::RequestKey::new(&model, &focused.text, "en-US");
    let coalescer = crate::coalesce::shared();
    if !coalescer.allows(&key, MIN_ANALYSIS_INTERVAL) {
        return Ok("That text was just analyzed.".to_string());
    }
    let generation = coalescer.record(&key);

    let mut suggestions = Vec::new();
    for paragraph in paragraphs.iter().take(3) {
        let completion = openrouter::analyze_with(
            &provider,
            &paragraph.id,
            &paragraph.text,
            Some(model.clone()),
            "en-US",
        )
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
            suggestions.push((char_start, char_end, suggestion));
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

    // Rank and cap with the shared merge module, then convert to the cache's
    // character offsets. The merge has to compare spans from different
    // paragraphs, so it works on document-level UTF-16 spans.
    let suggestions = rank_suggestions(&focused.text, suggestions);
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

    // A result that was superseded while the request was in flight describes
    // text the user is no longer looking at, so it is discarded.
    if !coalescer.is_current(&key, generation) {
        return Ok("A newer analysis replaced this one.".to_string());
    }

    if count == 0 {
        Ok("No safe LLM suggestions for the focused field.".to_string())
    } else {
        Ok(format!("{count} LLM suggestion(s) ready in the overlay."))
    }
}

/// Rank the model's suggestions with the shared merge module and cap the list.
///
/// The merge module is the single place that knows how overlapping edits are
/// resolved and how the result is ordered, so the overlay and the analysis
/// panel cannot disagree about what the user sees.
///
/// The merge works on the canonical `Suggestion` type, whose spans are UTF-16
/// offsets into the document. The analysis pipeline resolves spans per
/// paragraph, so the offsets are shifted to document-level ones before the
/// merge and converted back to character offsets afterwards.
pub fn rank_suggestions(
    document: &str,
    suggestions: Vec<(usize, usize, Suggestion)>,
) -> Vec<CachedSuggestion> {
    // Map a character offset in the document to its UTF-16 offset.
    let utf16_prefix: Vec<usize> = document
        .char_indices()
        .map(|(index, _)| document[..index].encode_utf16().count())
        .chain(std::iter::once(document.encode_utf16().count()))
        .collect();

    let char_to_utf16 = |char_offset: usize| -> usize {
        utf16_prefix
            .get(char_offset)
            .copied()
            .unwrap_or_else(|| document.encode_utf16().count())
    };

    // The character offsets are authoritative, so the document-level UTF-16
    // span is rebuilt from them rather than from the paragraph-relative
    // values the resolver produced.
    let canonical: Vec<Suggestion> = suggestions
        .iter()
        .map(|(char_start, char_end, suggestion)| {
            let mut shifted = suggestion.clone();
            shifted.span.utf16_start = char_to_utf16(*char_start);
            shifted.span.utf16_end = char_to_utf16(*char_end);
            shifted.span.grapheme_start = *char_start;
            shifted.span.grapheme_end = *char_end;
            shifted
        })
        .collect();

    let result = elocutio_analysis::merge_with_limit(
        &[],
        &canonical,
        elocutio_analysis::DEFAULT_SUGGESTION_LIMIT,
    );

    for dropped in &result.report.dropped {
        eprintln!(
            "Dropped suggestion {} ({}): {}",
            dropped.suggestion.id,
            dropped.reason.explanation(),
            dropped.detail
        );
    }

    // Map the merged suggestions back onto the character offsets the cache
    // uses, by looking the id up in the original list.
    result
        .suggestions
        .into_iter()
        .filter_map(|suggestion| {
            let original = suggestions
                .iter()
                .find(|(_, _, candidate)| candidate.id == suggestion.id)?;
            Some(cache_suggestion(suggestion, original.0, original.1))
        })
        .collect()
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
