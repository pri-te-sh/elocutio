//! This module centralizes all the Rust commands to be made available to the JavaScript
//! environment. Use [`application_message_handler`] to load them into the Tauri runtime.

use crate::config::Config;
use crate::highlighter_service::HighlighterService;
use crate::openrouter::{
    self, KeyStatus, OpenRouterStatus, ProviderConfig, ProviderKind, RewriteResult,
};
use crate::os_broker::{AccessibilityPermissionStatus, AppSearchResult, OsBroker};
use crate::usage::{self, UsageSummary};
use crate::{IntegrationView, PlatformBroker};
use base64::{Engine as _, engine::general_purpose};
use harper_core::{
    Dialect, DictWordMetadata, IgnoredLints,
    linting::FlatConfig,
    spell::{Dictionary, MutableDictionary},
};
use std::sync::{Arc, RwLock};
use tauri::ipc::Invoke;
use tauri::{Manager, Runtime, State};
use tokio::sync::Mutex;

pub fn application_message_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool {
    tauri::generate_handler![
        get_lint_config,
        get_dialect,
        get_debounce_ms,
        set_debounce_ms,
        get_auto_update,
        set_auto_update,
        get_current_version,
        get_provider_settings,
        set_provider_kind,
        set_provider_base_url,
        get_openrouter_key_status,
        set_openrouter_key,
        clear_openrouter_key,
        test_openrouter_key,
        rewrite_text,
        rewrite_preview,
        get_provider_model,
        set_provider_model,
        get_auto_analyze,
        set_auto_analyze,
        get_rewrite_tone,
        set_rewrite_tone,
        get_local_only,
        set_local_only,
        get_budget,
        set_budget,
        get_usage_summary,
        analyze_text,
        get_onboarding_completed,
        set_onboarding_completed,
        set_dialect,
        set_lint_config,
        get_dictionary,
        set_dictionary,
        ignore_lint,
        add_to_dictionary,
        get_integrations,
        get_auto_enable_new_apps,
        set_auto_enable_new_apps,
        add_integration,
        remove_integration,
        set_integration_enabled,
        get_application_icon_data_url,
        get_accessibility_permission_status,
        request_accessibility_permission,
        get_highlighter_service_status,
        undo_last_edit,
        get_voice_profile,
        learn_voice_samples,
        set_voice_enabled,
        clear_voice_profile,
        get_learn_from_edits,
        set_learn_from_edits,
        get_voice_feedback_count,
        start_highlighter_service,
        stop_highlighter_service,
        launch_app,
        search_apps,
    ]
}

#[tauri::command]
async fn get_lint_config(config: State<'_, Arc<Mutex<Config>>>) -> Result<FlatConfig, String> {
    let mut lint_config = config.lock().await.lint_config.clone();
    lint_config.fill_with_curated();

    Ok(lint_config)
}

#[tauri::command]
async fn get_dialect(config: State<'_, Arc<Mutex<Config>>>) -> Result<Dialect, String> {
    Ok(config.lock().await.dialect)
}

#[tauri::command]
async fn get_debounce_ms(config: State<'_, Arc<Mutex<Config>>>) -> Result<u64, String> {
    Ok(config.lock().await.debounce_ms)
}

#[tauri::command]
async fn set_debounce_ms(
    debounce_ms: u64,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut config = config.lock().await;
    config.debounce_ms = debounce_ms;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
async fn get_auto_update(config: State<'_, Arc<Mutex<Config>>>) -> Result<bool, String> {
    Ok(config.lock().await.auto_update)
}

#[tauri::command]
async fn set_auto_update(
    auto_update: bool,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut config = config.lock().await;
    config.auto_update = auto_update;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
fn get_current_version<R: Runtime>(app: tauri::AppHandle<R>) -> String {
    app.package_info().version.to_string()
}

#[derive(serde::Serialize)]
struct ProviderSettings {
    kind: ProviderKind,
    base_url: String,
    model: String,
    key_present: bool,
}

#[tauri::command]
async fn get_provider_settings(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<ProviderSettings, String> {
    let config = config.lock().await;
    let key_present = openrouter::key_status_for(config.provider_kind)?.present;
    Ok(ProviderSettings {
        kind: config.provider_kind,
        base_url: config.provider_base_url.clone(),
        model: config.provider_model.clone(),
        key_present,
    })
}

#[tauri::command]
async fn set_provider_kind(
    kind: ProviderKind,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<ProviderSettings, String> {
    let mut config = config.lock().await;
    config.provider_kind = kind;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    let key_present = openrouter::key_status_for(config.provider_kind)?.present;
    Ok(ProviderSettings {
        kind: config.provider_kind,
        base_url: config.provider_base_url.clone(),
        model: config.provider_model.clone(),
        key_present,
    })
}

#[tauri::command]
async fn set_provider_base_url(
    base_url: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<String, String> {
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    if base_url.is_empty() || !base_url.starts_with("http") {
        return Err("Enter an http(s) base URL, for example https://api.openai.com/v1.".into());
    }
    let mut config = config.lock().await;
    config.provider_base_url = base_url.clone();
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    Ok(base_url)
}

#[tauri::command]
async fn get_openrouter_key_status(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<KeyStatus, String> {
    openrouter::key_status_for(config.lock().await.provider_kind)
}

#[tauri::command]
async fn set_openrouter_key(
    key: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<KeyStatus, String> {
    openrouter::save_key_for(config.lock().await.provider_kind, &key)
}

#[tauri::command]
async fn clear_openrouter_key(config: State<'_, Arc<Mutex<Config>>>) -> Result<KeyStatus, String> {
    openrouter::clear_key_for(config.lock().await.provider_kind)
}

#[tauri::command]
async fn test_openrouter_key(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<OpenRouterStatus, String> {
    let config = config.lock().await;
    let provider = ProviderConfig {
        kind: config.provider_kind,
        base_url: config.provider_base_url.clone(),
    };
    openrouter::test_key_for(&provider).await
}

#[tauri::command]
async fn rewrite_text(
    text: String,
    model: Option<String>,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<RewriteResult, String> {
    let (provider, model, tone) = {
        let config = config.lock().await;
        if config.local_only {
            return Err("Local-only mode is on. Turn it off to use a provider.".into());
        }
        (
            ProviderConfig {
                kind: config.provider_kind,
                base_url: config.provider_base_url.clone(),
            },
            match model.filter(|value| !value.trim().is_empty()) {
                Some(model) => model,
                None => config.provider_model.clone(),
            },
            config.rewrite_tone,
        )
    };
    let result = openrouter::rewrite_with(
        &provider,
        &text,
        Some(model),
        crate::voice::guidance(),
        tone,
    )
    .await?;
    if let Err(error) = usage::record(&result) {
        eprintln!("Could not record usage: {error}");
    }
    Ok(result)
}

#[derive(serde::Serialize)]
struct RewritePreview {
    original: String,
    rewritten: String,
    diff: Vec<elocutio_analysis::DiffSegment>,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: Option<f64>,
}

#[tauri::command]
async fn rewrite_preview(
    text: String,
    model: Option<String>,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<RewritePreview, String> {
    let (provider, model, tone) = {
        let config = config.lock().await;
        if config.local_only {
            return Err("Local-only mode is on. Turn it off to use a provider.".into());
        }
        (
            ProviderConfig {
                kind: config.provider_kind,
                base_url: config.provider_base_url.clone(),
            },
            match model.filter(|value| !value.trim().is_empty()) {
                Some(model) => model,
                None => config.provider_model.clone(),
            },
            config.rewrite_tone,
        )
    };
    let result = openrouter::rewrite_with(
        &provider,
        &text,
        Some(model),
        crate::voice::guidance(),
        tone,
    )
    .await?;
    if let Err(error) = usage::record(&result) {
        eprintln!("Could not record usage: {error}");
    }
    Ok(RewritePreview {
        original: text.trim().to_string(),
        rewritten: result.text.clone(),
        diff: elocutio_analysis::diff_words(text.trim(), &result.text),
        model: result.model,
        input_tokens: result.input_tokens,
        output_tokens: result.output_tokens,
        cost_usd: result.cost_usd,
    })
}

#[tauri::command]
async fn get_provider_model(config: State<'_, Arc<Mutex<Config>>>) -> Result<String, String> {
    Ok(config.lock().await.provider_model.clone())
}

#[tauri::command]
async fn set_provider_model(
    model: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<String, String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("Enter a model id, for example anthropic/claude-haiku-5.5.".into());
    }
    let mut config = config.lock().await;
    config.provider_model = model.to_string();
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    Ok(config.provider_model.clone())
}

#[tauri::command]
async fn get_auto_analyze(config: State<'_, Arc<Mutex<Config>>>) -> Result<bool, String> {
    Ok(config.lock().await.auto_analyze)
}

#[tauri::command]
async fn set_auto_analyze(
    enabled: bool,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<bool, String> {
    let mut config = config.lock().await;
    config.auto_analyze = enabled;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    Ok(config.auto_analyze)
}

#[tauri::command]
async fn get_rewrite_tone(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<openrouter::RewriteTone, String> {
    Ok(config.lock().await.rewrite_tone)
}

#[tauri::command]
async fn set_rewrite_tone(
    tone: openrouter::RewriteTone,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<openrouter::RewriteTone, String> {
    let mut config = config.lock().await;
    config.rewrite_tone = tone;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    Ok(config.rewrite_tone)
}

#[tauri::command]
async fn get_local_only(config: State<'_, Arc<Mutex<Config>>>) -> Result<bool, String> {
    Ok(config.lock().await.local_only)
}

#[tauri::command]
async fn set_local_only(
    enabled: bool,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<bool, String> {
    let mut config = config.lock().await;
    config.local_only = enabled;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    Ok(config.local_only)
}

#[tauri::command]
async fn get_budget(config: State<'_, Arc<Mutex<Config>>>) -> Result<Option<f64>, String> {
    Ok(config.lock().await.budget_usd)
}

#[tauri::command]
async fn set_budget(
    budget_usd: Option<f64>,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<Option<f64>, String> {
    if let Some(value) = budget_usd
        && (!value.is_finite() || value < 0.0)
    {
        return Err("Enter a non-negative USD budget, or leave it blank.".into());
    }

    let mut config = config.lock().await;
    config.budget_usd = budget_usd;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    Ok(config.budget_usd)
}

#[tauri::command]
fn get_usage_summary(limit: Option<usize>) -> Result<UsageSummary, String> {
    usage::summary(limit.unwrap_or(10).min(100))
}

#[derive(serde::Serialize)]
struct AnalyzeResponse {
    suggestions: Vec<elocutio_core::Suggestion>,
    rejected: Vec<elocutio_analysis::RejectedEdit>,
    paragraphs: Vec<elocutio_analysis::Paragraph>,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: Option<f64>,
}

#[tauri::command]
async fn analyze_text(
    text: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<AnalyzeResponse, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter some text to analyze first.".into());
    }

    let (provider, model) = {
        let config = config.lock().await;
        if config.local_only {
            return Err("Local-only mode is on. Turn it off to use a provider.".into());
        }
        (
            ProviderConfig {
                kind: config.provider_kind,
                base_url: config.provider_base_url.clone(),
            },
            config.provider_model.clone(),
        )
    };
    let paragraphs = elocutio_analysis::chunk_paragraphs(text);
    if paragraphs.is_empty() {
        return Err("No paragraphs found.".into());
    }

    let mut suggestions = Vec::new();
    let mut rejected = Vec::new();
    let mut input_tokens = 0;
    let mut output_tokens = 0;
    let mut cost_usd = 0.0;
    let mut response_model = model.clone();

    for paragraph in paragraphs.iter().take(3) {
        let completion = openrouter::analyze_with(
            &provider,
            &paragraph.id,
            &paragraph.text,
            Some(model.clone()),
            "en-US",
        )
        .await?;
        input_tokens += completion.input_tokens;
        output_tokens += completion.output_tokens;
        if let Some(cost) = completion.cost_usd {
            cost_usd += cost;
        }
        response_model = completion.model.clone();

        match elocutio_analysis::parse_response(&completion.content, &paragraphs) {
            Ok(result) => {
                suggestions.extend(result.suggestions);
                rejected.extend(result.rejected);
            }
            Err(error) => {
                rejected.push(elocutio_analysis::RejectedEdit {
                    paragraph_id: paragraph.id.clone(),
                    original: String::new(),
                    reason: error,
                });
            }
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

    Ok(AnalyzeResponse {
        suggestions,
        rejected,
        paragraphs: paragraphs.into_iter().take(3).collect(),
        model: response_model,
        input_tokens,
        output_tokens,
        cost_usd: (cost_usd > 0.0).then_some(cost_usd),
    })
}

#[tauri::command]
async fn get_onboarding_completed(config: State<'_, Arc<Mutex<Config>>>) -> Result<bool, String> {
    Ok(config.lock().await.onboarding_completed)
}

#[tauri::command]
async fn set_onboarding_completed(
    onboarding_completed: bool,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut config = config.lock().await;
    let previous_value = config.onboarding_completed;
    config.onboarding_completed = onboarding_completed;

    if let Err(error) = config.save_to_system().await {
        config.onboarding_completed = previous_value;
        return Err(error.to_string());
    }

    Ok(())
}

#[tauri::command]
async fn set_dialect(
    dialect: Dialect,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut config = config.lock().await;
    config.dialect = dialect;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
async fn set_lint_config(
    lint_config: FlatConfig,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut lint_config = lint_config;
    lint_config.fill_with_curated();

    let mut config = config.lock().await;
    config.lint_config = lint_config;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
async fn get_dictionary(config: State<'_, Arc<Mutex<Config>>>) -> Result<Vec<String>, String> {
    let mut words = config
        .lock()
        .await
        .mutable_dictionary
        .words_iter()
        .map(|word| word.iter().collect::<String>())
        .collect::<Vec<_>>();
    words.sort();

    Ok(words)
}

#[tauri::command]
async fn set_dictionary(
    words: Vec<String>,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut dictionary = MutableDictionary::new();
    dictionary.extend_words(words.into_iter().map(|word| {
        (
            word.chars().collect::<Vec<_>>(),
            DictWordMetadata::default(),
        )
    }));

    let mut config = config.lock().await;
    config.mutable_dictionary = dictionary;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
async fn ignore_lint(
    ignored_lints: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let ignored_lints =
        serde_json::from_str::<IgnoredLints>(&ignored_lints).map_err(|error| error.to_string())?;

    let mut config = config.lock().await;
    config.ignored_lints.append(ignored_lints);
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
async fn add_to_dictionary(
    word: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut config = config.lock().await;
    config
        .mutable_dictionary
        .append_word_str(&word, DictWordMetadata::default());
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
async fn get_integrations(
    config: State<'_, Arc<Mutex<Config>>>,
    broker: State<'_, RwLock<PlatformBroker>>,
) -> Result<Vec<IntegrationView>, String> {
    let integrations = config.lock().await.integrations.clone();
    let broker = broker
        .read()
        .map_err(|error| format!("Failed to read platform broker: {error}"))?;

    Ok(integrations
        .into_iter()
        .map(|integration| IntegrationView {
            display_name: broker.integration_display_name(&integration.bundle_id),
            bundle_id: integration.bundle_id,
            enabled: integration.enabled,
        })
        .collect())
}

#[tauri::command]
async fn get_auto_enable_new_apps(config: State<'_, Arc<Mutex<Config>>>) -> Result<bool, String> {
    Ok(config.lock().await.auto_enable_new_apps)
}

#[tauri::command]
async fn set_auto_enable_new_apps(
    auto_enable_new_apps: bool,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut config = config.lock().await;
    let previous = config.auto_enable_new_apps;
    config.auto_enable_new_apps = auto_enable_new_apps;
    if let Err(error) = config.save_to_system().await {
        config.auto_enable_new_apps = previous;
        return Err(error.to_string());
    }
    Ok(())
}

#[tauri::command]
async fn add_integration(
    bundle_id: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut config = config.lock().await;
    config.add_integration(bundle_id);
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
async fn remove_integration(
    bundle_id: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut config = config.lock().await;
    config.remove_integration(&bundle_id);
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
async fn set_integration_enabled(
    bundle_id: String,
    enabled: bool,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut config = config.lock().await;
    config.set_integration_enabled(&bundle_id, enabled);
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[tauri::command]
async fn get_application_icon_data_url<R: Runtime>(
    bundle_id: String,
    app_handle: tauri::AppHandle<R>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let broker = app_handle.state::<RwLock<PlatformBroker>>();
        let icon_png = broker
            .read()
            .map_err(|error| format!("Failed to read platform broker: {error}"))?
            .application_icon_png(&bundle_id)?;
        let encoded = general_purpose::STANDARD.encode(icon_png);

        Ok(format!("data:image/png;base64,{encoded}"))
    })
    .await
    .map_err(|error| format!("Failed to load application icon: {error}"))?
}

#[tauri::command]
fn get_accessibility_permission_status(
    broker: State<'_, RwLock<PlatformBroker>>,
) -> AccessibilityPermissionStatus {
    match broker.read() {
        Ok(broker) => broker.accessibility_permission_status(),
        Err(error) => {
            eprintln!("Failed to read platform broker: {error}");
            AccessibilityPermissionStatus::Unsupported
        }
    }
}

#[tauri::command]
fn request_accessibility_permission(
    broker: State<'_, RwLock<PlatformBroker>>,
) -> AccessibilityPermissionStatus {
    match broker.read() {
        Ok(broker) => broker.request_accessibility_permission(),
        Err(error) => {
            eprintln!("Failed to read platform broker: {error}");
            AccessibilityPermissionStatus::Unsupported
        }
    }
}

#[tauri::command]
fn get_voice_profile() -> Result<Option<elocutio_voice::VoiceProfile>, String> {
    crate::voice::refresh_feedback()
}

#[tauri::command]
fn learn_voice_samples(
    text: String,
    name: Option<String>,
) -> Result<elocutio_voice::VoiceProfile, String> {
    crate::voice::learn(&text, name)
}

#[tauri::command]
fn set_voice_enabled(enabled: bool) -> Result<Option<elocutio_voice::VoiceProfile>, String> {
    let Some(mut profile) = crate::voice::load()? else {
        return Err("Teach Elocutio your voice first.".into());
    };
    profile.strength = if enabled {
        elocutio_voice::VoiceStrength::Balanced
    } else {
        elocutio_voice::VoiceStrength::Off
    };
    crate::voice::save(&profile)?;
    Ok(Some(profile))
}

#[tauri::command]
async fn clear_voice_profile(config: State<'_, Arc<Mutex<Config>>>) -> Result<(), String> {
    crate::voice::clear()?;
    let mut config = config.lock().await;
    config.learn_from_edits = false;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_learn_from_edits(config: State<'_, Arc<Mutex<Config>>>) -> Result<bool, String> {
    Ok(config.lock().await.learn_from_edits)
}

#[tauri::command]
async fn set_learn_from_edits(
    enabled: bool,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<bool, String> {
    let mut config = config.lock().await;
    config.learn_from_edits = enabled;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    crate::voice::set_learning_enabled(enabled)?;
    if enabled {
        let _ = crate::voice::refresh_feedback();
    }
    Ok(enabled)
}

#[tauri::command]
fn get_voice_feedback_count() -> usize {
    crate::voice::feedback_count()
}

#[tauri::command]
async fn undo_last_edit<R: Runtime>(app: tauri::AppHandle<R>) -> Result<String, String> {
    crate::undo::undo_last_edit(&app).await
}

#[tauri::command]
pub(crate) fn get_highlighter_service_status(
    highlighter_service: State<'_, HighlighterService>,
) -> Result<bool, String> {
    Ok(highlighter_service.is_running())
}

#[tauri::command]
pub(crate) async fn start_highlighter_service(
    config: State<'_, Arc<Mutex<Config>>>,
    highlighter_service: State<'_, HighlighterService>,
) -> Result<bool, String> {
    {
        let mut config = config.lock().await;
        config.highlighter_service_enabled = true;
        config
            .save_to_system()
            .await
            .map_err(|error| error.to_string())?;
    }

    highlighter_service
        .start()
        .map_err(|error| error.to_string())?;

    Ok(highlighter_service.is_running())
}

#[tauri::command]
pub(crate) async fn stop_highlighter_service(
    config: State<'_, Arc<Mutex<Config>>>,
    highlighter_service: State<'_, HighlighterService>,
) -> Result<bool, String> {
    {
        let mut config = config.lock().await;
        config.highlighter_service_enabled = false;
        config
            .save_to_system()
            .await
            .map_err(|error| error.to_string())?;
    }

    Ok(highlighter_service.stop())
}

#[tauri::command]
fn launch_app(bundle_id: String, broker: State<'_, RwLock<PlatformBroker>>) -> Result<(), String> {
    broker
        .read()
        .map_err(|error| format!("Failed to read platform broker: {error}"))?
        .launch_app_bundle(&bundle_id)
}

#[tauri::command]
fn search_apps(
    query: String,
    broker: State<'_, RwLock<PlatformBroker>>,
) -> Result<Vec<AppSearchResult>, String> {
    broker
        .read()
        .map_err(|error| format!("Failed to read platform broker: {error}"))?
        .search_apps(&query)
}
