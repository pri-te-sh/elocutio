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
const MIN_ANALYSIS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1_200);
use crate::os_broker::{FocusedText, OsBroker};
use crate::{PlatformBroker, llm_cache, main_thread, openrouter, rewrite, usage};

pub async fn analyze_focused_field(app: &AppHandle) -> Result<String, String> {
    analyze_focused_field_with(app, None).await
}

/// Reads the active text field from the live `"elo"` overlay snapshot first, falling back
/// to the main-thread `PlatformBroker` when needed.
pub fn current_focused_text(app: &AppHandle) -> Result<Option<FocusedText>, String> {
    let snap = crate::elo_overlay::current_snapshot();
    if snap.field_active && snap.pid > 0 && !snap.source_text.trim().is_empty() {
        return Ok(Some(FocusedText {
            pid: snap.pid,
            bundle_id: snap.app_bundle_id,
            text: snap.source_text,
        }));
    }
    if crate::elo_overlay::is_running() {
        return Ok(None);
    }
    main_thread::run(app, |app| {
        let state = app.state::<RwLock<PlatformBroker>>();
        let mut broker = state
            .write()
            .map_err(|error| format!("Could not lock the platform broker: {error}"))?;
        broker.focused_text()
    })?
}

/// Analyze the focused field, optionally sending only what changed.
///
/// `previous_text` enables the diff-only behaviour the plan asks for in live
/// mode: when the caller knows what the field looked like at the last
/// analysis, only the paragraphs that changed since then are sent. The cost
/// of a long document then stops growing with its length.
pub async fn analyze_text_with_config(
    config: &Arc<Mutex<Config>>,
    focused: FocusedText,
    previous_text: Option<&str>,
) -> Result<usize, String> {
    let (provider, model, custom_instruction, dialect_code) = {
        let config = config.lock().await;
        if config.local_only {
            return Err(
                "Local-only mode is on. Turn it off to analyze the field with a provider."
                    .to_string(),
            );
        }
        let dialect_str = match config.dialect {
            harper_core::Dialect::British => "en-GB",
            harper_core::Dialect::Canadian => "en-CA",
            harper_core::Dialect::Australian => "en-AU",
            harper_core::Dialect::Indian => "en-IN",
            harper_core::Dialect::American => "en-US",
        };
        (
            openrouter::ProviderConfig {
                kind: config.provider_kind,
                base_url: config.provider_base_url.clone(),
            },
            config.provider_model.clone(),
            config.custom_instruction.clone(),
            dialect_str,
        )
    };

    let paragraphs = chunk_paragraphs(&focused.text);
    if paragraphs.is_empty() {
        return Err("The focused field is empty.".into());
    }

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
        return Ok(0);
    }

    let key = crate::coalesce::RequestKey::new(&model, &focused.text, dialect_code);
    let coalescer = crate::coalesce::shared();
    if !coalescer.allows(&key, MIN_ANALYSIS_INTERVAL) {
        return Ok(0);
    }
    let generation = coalescer.record(&key);

    let custom_opt = (!custom_instruction.trim().is_empty()).then_some(custom_instruction.as_str());
    let mut suggestions = Vec::new();
    for paragraph in paragraphs.iter().take(4) {
        let completion = openrouter::analyze_with_context(
            &provider,
            &paragraph.id,
            &paragraph.text,
            Some(model.clone()),
            dialect_code,
            custom_opt,
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

    if !coalescer.is_current(&key, generation) {
        return Ok(0);
    }

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
    crate::elo_overlay::request_refresh();

    Ok(count)
}

pub async fn analyze_focused_field_with(
    app: &AppHandle,
    previous_text: Option<&str>,
) -> Result<String, String> {
    let focused = current_focused_text(app)?;
    let focused = focused.ok_or_else(|| "Focus a text field in any app first.".to_string())?;
    let config = app.state::<Arc<Mutex<Config>>>();
    let count = analyze_text_with_config(&config, focused, previous_text).await?;
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
    let (trimmed_start, trimmed_end, trimmed_orig, trimmed_repl) = trim_unchanged_context(
        char_start,
        char_end,
        &suggestion.original,
        &suggestion.replacement,
    );
    CachedSuggestion {
        id: format!("llm:{}", suggestion.id),
        char_start: trimmed_start,
        char_end: trimmed_end,
        original: trimmed_orig,
        replacement: trimmed_repl,
        category: suggestion.category,
        severity: suggestion.severity,
        explanation: suggestion.explanation,
        resolution: suggestion.resolution,
    }
}

/// Trims unchanged leading/trailing whole-word context that the LLM included to
/// disambiguate `original` within a paragraph (e.g. `"corrections does"` -> `"corrections do"`
/// trims to `"does"` -> `"do"`), so the inline underline and replacement pill highlight
/// only the exact changed word(s).
fn trim_unchanged_context(
    mut char_start: usize,
    mut char_end: usize,
    original: &str,
    replacement: &str,
) -> (usize, usize, String, String) {
    if original.is_empty()
        || replacement.is_empty()
        || char_start >= char_end
        || original.chars().count() != char_end - char_start
    {
        return (
            char_start,
            char_end,
            original.to_string(),
            replacement.to_string(),
        );
    }

    let mut orig = original;
    let mut repl = replacement;

    // 1. Strip identical trailing whitespace + word tokens (`" word"`)
    while let Some(ws_byte) = orig.rfind(char::is_whitespace) {
        let suffix = &orig[ws_byte..];
        if !repl.ends_with(suffix) {
            break;
        }
        let next_orig = &orig[..ws_byte];
        let next_repl = &repl[..repl.len() - suffix.len()];
        if next_orig.is_empty()
            || next_repl.is_empty()
            || next_orig == next_repl
            || next_orig.ends_with(char::is_whitespace)
            || next_repl.ends_with(char::is_whitespace)
            || !next_orig.chars().any(char::is_alphanumeric)
        {
            break;
        }
        char_end = char_end.saturating_sub(suffix.chars().count());
        orig = next_orig;
        repl = next_repl;
    }

    // 2. Strip identical leading word + whitespace tokens (`"word "`)
    while let Some(ws_byte) = orig.find(char::is_whitespace) {
        let ws_char_len = orig[ws_byte..]
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(char::len_utf8)
            .sum::<usize>();
        let prefix_end = ws_byte + ws_char_len;
        let prefix = &orig[..prefix_end];
        if !repl.starts_with(prefix) {
            break;
        }
        let next_orig = &orig[prefix_end..];
        let next_repl = &repl[prefix_end..];
        if next_orig.is_empty()
            || next_repl.is_empty()
            || next_orig == next_repl
            || next_orig.starts_with(char::is_whitespace)
            || next_repl.starts_with(char::is_whitespace)
            || !next_orig.chars().any(char::is_alphanumeric)
        {
            break;
        }
        char_start += prefix.chars().count();
        orig = next_orig;
        repl = next_repl;
    }

    // 3. Strip identical trailing punctuation character (e.g. `"spell,"` -> `"spelling,"`)
    if let (Some(o_last), Some(r_last)) = (orig.chars().last(), repl.chars().last())
        && o_last == r_last
        && matches!(o_last, ',' | '.' | ';' | ':' | '!' | '?')
    {
        let next_orig = &orig[..orig.len() - o_last.len_utf8()];
        let next_repl = &repl[..repl.len() - r_last.len_utf8()];
        if !next_orig.is_empty()
            && !next_repl.is_empty()
            && next_orig != next_repl
            && next_orig.chars().any(char::is_alphanumeric)
        {
            char_end = char_end.saturating_sub(1);
            orig = next_orig;
            repl = next_repl;
        }
    }

    (char_start, char_end, orig.to_string(), repl.to_string())
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
