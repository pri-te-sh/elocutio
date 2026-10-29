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
use elocutio_analysis::{GoldenReport, MergeResult, Suggestion, Tone, ToneSignal};
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
        get_dialect_name,
        set_dialect_name,
        get_lint_rules,
        get_debounce_ms,
        set_debounce_ms,
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
        list_provider_models,
        fetch_provider_preset_models,
        get_widget_anchors,
        get_network_log,
        get_app_matrix,
        get_app_matrix_record,
        get_app_matrix_summary,
        record_app_matrix_check,
        preview_request,
        get_network_log_summary,
        clear_network_log,
        preview_rewrite_selection,
        apply_pending_rewrite,
        get_pending_rewrite,
        discard_pending_rewrite,
        set_widget_anchor,
        get_widget_anchor_corners,
        get_coalescer_state,
        run_m1_gate,
        clear_coalescer_state,
        get_custom_instruction,
        rewrite_text_streamed,
        set_custom_instruction,
        get_default_model,
        get_auto_analyze,
        set_auto_analyze,
        get_rewrite_tone,
        set_rewrite_tone,
        get_local_only,
        set_local_only,
        get_budget,
        set_budget,
        get_cost_cap,
        set_cost_cap,
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
        remove_from_dictionary,
        get_integrations,
        get_auto_enable_new_apps,
        set_auto_enable_new_apps,
        get_blocklist,
        set_blocked,
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
        get_voice_guidance,
        rate_voice_rewrite,
        set_voice_trait_override,
        clear_voice_trait_override,
        get_voice_traits,
        get_voice_trait_overrides,
        get_voice_event_count,
        get_copy_guard_report,
        get_voice_profile_summary,
        get_voice_trait_list,
        record_voice_rating,
        get_feedback_fingerprint_count,
        set_voice_trait_value,
        get_learn_from_edits,
        set_learn_from_edits,
        get_voice_feedback_count,
        get_suggestion_events_enabled,
        set_suggestion_events_enabled,
        get_suggestion_event_summary,
        clear_suggestion_events,
        detect_text_tone,
        merge_suggestions,
        evaluate_golden_set,
        run_resolver_quality_check,
        start_highlighter_service,
        stop_highlighter_service,
        launch_app,
        search_apps,
        check_text_local,
        analyze_studio_text,
        get_elo_overlay_snapshot,
        set_elo_overlay_regions,
        save_elo_widget_offset,
        apply_elo_alert,
        dismiss_elo_alert,
        disable_elo_rule,
        set_lint_rule_enabled,
        get_personalization_db_status,
        get_writing_analytics,
        record_studio_edit_feedback,
        open_elocutio_studio,
        take_studio_launch_state,
        reset_and_request_accessibility,
        elo_rewrite_focused_text,
        apply_elo_field_rewrite,
        set_elo_overlay_input_focus,
    ]
}

#[tauri::command]
async fn get_lint_config(config: State<'_, Arc<Mutex<Config>>>) -> Result<FlatConfig, String> {
    let mut lint_config = config.lock().await.lint_config.clone();
    lint_config.fill_with_curated();

    Ok(lint_config)
}

/// The dialect, as the abbreviation the UI can show.
///
/// The `Dialect` enum serializes as a number, which is fine for the config
/// file but not for a settings window. This is the string form.
#[tauri::command]
async fn get_dialect_name(config: State<'_, Arc<Mutex<Config>>>) -> Result<String, String> {
    Ok(dialect_abbr(config.lock().await.dialect))
}

/// Set the dialect by abbreviation.
#[tauri::command]
async fn set_dialect_name(
    dialect: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<String, String> {
    let mut config = config.lock().await;
    config.dialect = Dialect::try_from_abbr(dialect.trim())
        .ok_or_else(|| "Unknown dialect. Use one of: US, CA, AU, GB, IN.".to_string())?;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    Ok(dialect_abbr(config.dialect))
}

/// The abbreviation for a dialect.
fn dialect_abbr(dialect: Dialect) -> String {
    match dialect {
        Dialect::American => "US".to_string(),
        Dialect::Canadian => "CA".to_string(),
        Dialect::Australian => "AU".to_string(),
        Dialect::British => "GB".to_string(),
        Dialect::Indian => "IN".to_string(),
    }
}

/// The rules the local engine can run, and whether each is on.
///
/// The settings window needs the list to show a rule picker; the local config
/// alone is a map the UI cannot read.
#[tauri::command]
async fn get_lint_rules(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<Vec<LintRuleView>, String> {
    let lint_config = config.lock().await.lint_config.clone();
    // The config serializes as an ordered map, so the UI gets a stable order.
    let serialized = serde_json::to_value(&lint_config)
        .map_err(|error| format!("Could not read the rule list: {error}"))?;
    let rules = serialized
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(key, value)| LintRuleView {
                    key: key.clone(),
                    enabled: value.as_bool().unwrap_or(true),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(rules)
}

/// One rule in the local engine.
#[derive(serde::Serialize, Clone)]
struct LintRuleView {
    key: String,
    enabled: bool,
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
fn get_current_version<R: Runtime>(app: tauri::AppHandle<R>) -> String {
    app.package_info().version.to_string()
}

#[derive(serde::Serialize)]
struct ProviderSettings {
    kind: ProviderKind,
    base_url: String,
    model: String,
    key_present: bool,
    key_hint: Option<String>,
}

/// The provider configuration the saved settings point at.
fn provider_from(config: &Config) -> ProviderConfig {
    ProviderConfig {
        kind: config.provider_kind,
        base_url: config.provider_base_url.clone(),
    }
}

fn provider_settings_from(config: &Config) -> Result<ProviderSettings, String> {
    let provider = provider_from(config);
    Ok(ProviderSettings {
        kind: config.provider_kind,
        base_url: config.provider_base_url.clone(),
        model: config.provider_model.clone(),
        key_present: openrouter::key_status_for(&provider)?.present,
        key_hint: openrouter::key_hint_for(&provider),
    })
}

#[tauri::command]
async fn get_provider_settings(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<ProviderSettings, String> {
    provider_settings_from(&*config.lock().await)
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
    // A legacy key left unmigrated while a local endpoint was selected finds
    // its slot once a remote endpoint is configured.
    openrouter::spawn_legacy_key_migration(provider_from(&config));
    provider_settings_from(&config)
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
    // The API key travels in a header, so plain http is only allowed to this
    // Mac (Ollama, the Apple bridge); anywhere else it would go in cleartext.
    if base_url.starts_with("http://") && !openrouter::is_loopback_url(&base_url) {
        return Err(
            "Use an https:// base URL. Plain http:// is only allowed for localhost.".into(),
        );
    }
    let mut config = config.lock().await;
    config.provider_base_url = base_url.clone();
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    openrouter::spawn_legacy_key_migration(provider_from(&config));
    Ok(base_url)
}

#[tauri::command]
async fn get_openrouter_key_status(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<KeyStatus, String> {
    let provider = provider_from(&*config.lock().await);
    openrouter::key_status_for(&provider)
}

#[tauri::command]
async fn set_openrouter_key(
    key: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<KeyStatus, String> {
    let provider = provider_from(&*config.lock().await);
    openrouter::save_key_for(&provider, &key)
}

#[tauri::command]
async fn clear_openrouter_key(config: State<'_, Arc<Mutex<Config>>>) -> Result<KeyStatus, String> {
    let provider = provider_from(&*config.lock().await);
    openrouter::clear_key_for(&provider)
}

#[tauri::command]
async fn test_openrouter_key(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<OpenRouterStatus, String> {
    // The lock is released before the network call, so a slow key check does
    // not stall every other command that reads the configuration.
    let provider = provider_from(&*config.lock().await);
    openrouter::test_key_for(&provider).await
}

#[tauri::command]
async fn rewrite_text(
    text: String,
    model: Option<String>,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<RewriteResult, String> {
    let (provider, model, tone, cost_cap, instruction) = {
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
    let (provider, model, tone, cost_cap, instruction) = {
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

/// Rewrite through a streaming request.
///
/// The response arrives in pieces rather than all at once, which matters for
/// long rewrites. The copy guard still runs on the finished text.
#[tauri::command]
async fn rewrite_text_streamed(
    text: String,
    model: Option<String>,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<RewriteResult, String> {
    let (provider, model, tone, cost_cap, instruction) = {
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
    let result = openrouter::rewrite_with_stream(
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
    Ok(result)
}

/// The custom instruction applied to every request.
#[tauri::command]
async fn get_custom_instruction(config: State<'_, Arc<Mutex<Config>>>) -> Result<String, String> {
    Ok(config.lock().await.custom_instruction.clone())
}

/// Set the custom instruction applied to every request.
///
/// The instruction is sanitized before it is stored, so it cannot be used to
/// fake a prompt boundary.
#[tauri::command]
async fn set_custom_instruction(
    instruction: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<String, String> {
    let sanitized = elocutio_analysis::custom_instruction(&instruction)
        .map(|wrapped| {
            // The stored value is the plain text; the wrapper is added at
            // request time.
            wrapped
                .trim_start_matches("<custom_instruction data=\"true\">")
                .trim_end_matches("</custom_instruction>")
                .to_string()
        })
        .unwrap_or_default();

    let mut config = config.lock().await;
    config.custom_instruction = sanitized;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    Ok(config.custom_instruction.clone())
}

/// Run the M1 gate and report every check.
///
/// The automatic checks read the repository and the running configuration. The
/// checks that need a human are reported as needing one rather than quietly
/// passing, so the gate never claims more than it can show.
#[tauri::command(async)]
fn run_m1_gate() -> crate::gate::GateReport {
    crate::gate::run_m1_gate()
}

/// The number of analysis requests the coalescer is tracking.
///
/// This is a diagnostic: it shows how many distinct texts have been analyzed
/// recently, and lets the user clear the tracking.
#[tauri::command]
fn get_coalescer_state() -> serde_json::Value {
    let coalescer = crate::coalesce::shared();
    serde_json::json!({
        "tracked": coalescer.len(),
        "empty": coalescer.is_empty(),
    })
}

/// Forget every tracked analysis request.
#[tauri::command]
fn clear_coalescer_state() -> usize {
    let coalescer = crate::coalesce::shared();
    coalescer.clear();
    coalescer.len()
}

/// Prepare a rewrite of the current selection and show what would change.
///
/// The rewrite is not applied. The user sees the diff and then applies it,
/// so an edit they have not looked at never lands in their document.
#[tauri::command]
async fn preview_rewrite_selection(
    app: State<'_, tauri::AppHandle>,
) -> Result<crate::rewrite::PendingRewrite, String> {
    crate::rewrite::preview_selection(&app).await
}

/// Apply the prepared rewrite to the selection.
#[tauri::command]
async fn apply_pending_rewrite(app: State<'_, tauri::AppHandle>) -> Result<String, String> {
    crate::rewrite::apply_pending(&app).await
}

/// The prepared rewrite, if there is one.
#[tauri::command(async)]
fn get_pending_rewrite() -> Result<Option<crate::rewrite::PendingRewrite>, String> {
    crate::rewrite::load_pending()
}

/// Discard the prepared rewrite.
#[tauri::command(async)]
fn discard_pending_rewrite() -> Result<(), String> {
    let path = crate::config::Config::folder_path()
        .map(|folder| folder.join("pending-rewrite.json"))
        .ok_or_else(|| "Could not find the application support directory.".to_string())?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not discard the rewrite: {error}")),
    }
}

/// A preview of the request the app is about to send.
///
/// The key is redacted and the text is shown exactly as it will be sent, so
/// the user can check what leaves their Mac without reading the source.
#[tauri::command]
async fn preview_request(
    text: String,
    model: Option<String>,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<crate::preview::RequestPreview, String> {
    let (provider, model, system) = {
        let config = config.lock().await;
        let model = match model.filter(|value| !value.trim().is_empty()) {
            Some(model) => model,
            None => config.provider_model.clone(),
        };
        let mut system = crate::openrouter::SYSTEM_PROMPT.to_string();
        if let Some(guidance) = crate::voice::guidance() {
            system.push_str("\n\n");
            system.push_str(&guidance);
        }
        (
            ProviderConfig {
                kind: config.provider_kind,
                base_url: config.provider_base_url.clone(),
            },
            model,
            system,
        )
    };

    let has_key = crate::openrouter::key_status_for(&provider)
        .map(|status| status.present)
        .unwrap_or(false);

    Ok(crate::preview::chat_preview(
        &crate::network_log::host_of(&provider.chat_endpoint()),
        provider.kind.display_name(),
        Some(&model),
        &system,
        &text,
        has_key,
    ))
}

/// The recorded matrix checks, so the window can show what was already done.
///
/// Without this, a cell checked in an earlier session reads as unverified.
#[tauri::command(async)]
fn get_app_matrix_record() -> Result<crate::matrix::MatrixRecord, String> {
    crate::matrix::load_record()
}

/// The Tier A app matrix: the apps, the conditions, and what has been checked.
///
/// A manual matrix cannot be automated, because it needs a human looking at
/// a screen. What this returns is the list and the record, so the matrix is
/// something you can run rather than something you remember.
#[tauri::command]
fn get_app_matrix() -> serde_json::Value {
    serde_json::json!({
        "tierA": crate::matrix::TIER_A_APPS,
        "tierB": crate::matrix::TIER_B_APPS,
        "unsupported": crate::matrix::UNSUPPORTED_APPS
            .iter()
            .map(|(bundle_id, reason)| serde_json::json!({
                "bundleId": bundle_id,
                "reason": reason,
            }))
            .collect::<Vec<_>>(),
        "conditions": crate::matrix::ALL_CONDITIONS
            .iter()
            .map(|condition| serde_json::json!({
                "name": crate::matrix::condition_key(*condition),
                "label": condition.label(),
            }))
            .collect::<Vec<_>>(),
    })
}

/// The summary of what has been verified in the app matrix.
#[tauri::command]
fn get_app_matrix_summary() -> crate::matrix::MatrixSummary {
    crate::matrix::MatrixSummary::summarize(&crate::matrix::load_record().unwrap_or_default())
}

/// Record the result of one matrix check.
#[tauri::command(async)]
fn record_app_matrix_check(
    bundle_id: String,
    condition: String,
    state: String,
) -> Result<crate::matrix::MatrixSummary, String> {
    let mut record = crate::matrix::load_record()?;
    let condition = crate::matrix::condition_from_key(&condition)
        .ok_or_else(|| format!("Unknown matrix condition: {condition}"))?;
    let state = match state.as_str() {
        "pass" => crate::matrix::CheckState::Pass,
        "fail" => crate::matrix::CheckState::Fail,
        "unverified" => crate::matrix::CheckState::Unverified,
        other => return Err(format!("Unknown check state: {other}")),
    };
    record.record(&bundle_id, condition, state);
    crate::matrix::save_record(&record)?;
    Ok(crate::matrix::MatrixSummary::summarize(&record))
}

/// The network log: every host the app has contacted.
///
/// The log holds metadata only — the host, the model, the token counts, the
/// cost — and never the text that was sent.
#[tauri::command(async)]
fn get_network_log() -> Result<Vec<crate::network_log::NetworkEntry>, String> {
    crate::network_log::read()
}

/// A summary of the network log.
#[tauri::command(async)]
fn get_network_log_summary() -> Result<crate::network_log::NetworkSummary, String> {
    let entries = crate::network_log::read()?;
    Ok(crate::network_log::NetworkSummary::summarize(&entries))
}

/// Delete the network log.
#[tauri::command(async)]
fn clear_network_log() -> Result<crate::network_log::NetworkSummary, String> {
    crate::network_log::clear()?;
    Ok(crate::network_log::NetworkSummary::default())
}

/// The widgets the user has pinned.
#[tauri::command]
async fn get_widget_anchors(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<Vec<(String, crate::widget_anchor::WidgetAnchor)>, String> {
    Ok(config.lock().await.widget_anchors.entries())
}

/// Pin the widget to a corner for an app, or clear the pin.
///
/// An app that is not pinned uses the default position: beside the first
/// underline, or in the screen corner when there is nothing to underline.
#[tauri::command]
async fn set_widget_anchor(
    bundle_id: String,
    anchor: Option<crate::widget_anchor::WidgetAnchor>,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<Vec<(String, crate::widget_anchor::WidgetAnchor)>, String> {
    let mut config = config.lock().await;
    match anchor {
        Some(anchor) => config.widget_anchors.set(&bundle_id, anchor),
        None => config.widget_anchors.clear(&bundle_id),
    }
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    Ok(config.widget_anchors.entries())
}

/// The corners the widget can be pinned to.
#[tauri::command]
fn get_widget_anchor_corners() -> Vec<crate::widget_anchor::AnchorCorner> {
    crate::widget_anchor::AnchorCorner::ALL.to_vec()
}

/// The models the configured provider can serve.
///
/// The catalog is curated and local, so the picker works offline and does not
/// need a provider round trip.
#[tauri::command]
async fn list_provider_models(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<Vec<crate::openrouter::llm::ModelOption>, String> {
    let config = config.lock().await;
    Ok(crate::openrouter::list_models(
        &crate::openrouter::ProviderConfig {
            kind: config.provider_kind,
            base_url: config.provider_base_url.clone(),
        },
    ))
}

#[tauri::command]
async fn fetch_provider_preset_models(
    preset_id: String,
    base_url: String,
) -> Result<Vec<crate::openrouter::PresetModelSlug>, String> {
    Ok(crate::openrouter::fetch_preset_model_slugs(&preset_id, &base_url).await)
}

/// The default model id for the configured provider.
#[tauri::command]
async fn get_default_model(config: State<'_, Arc<Mutex<Config>>>) -> Result<String, String> {
    let config = config.lock().await;
    Ok(crate::openrouter::llm::default_model_id(
        config.provider_kind,
    ))
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

    // Choosing a tone is a preference, so the voice profile learns it.
    if let Err(error) = crate::voice::record_tone_selected(
        &format!("{:?}", config.rewrite_tone).to_lowercase(),
        None,
    ) {
        eprintln!("Could not record the tone selection: {error}");
    }

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

/// The hard cost ceiling for a single request, in USD.
///
/// Unlike the budget warning, this refuses a request before it is sent.
#[tauri::command]
async fn get_cost_cap(config: State<'_, Arc<Mutex<Config>>>) -> Result<Option<f64>, String> {
    Ok(config.lock().await.cost_cap_usd)
}

#[tauri::command]
async fn set_cost_cap(
    cost_cap_usd: Option<f64>,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<Option<f64>, String> {
    if let Some(value) = cost_cap_usd
        && (!value.is_finite() || value < 0.0)
    {
        return Err("Enter a non-negative USD cap, or leave it blank.".into());
    }

    let mut config = config.lock().await;
    config.cost_cap_usd = cost_cap_usd;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    Ok(config.cost_cap_usd)
}

#[tauri::command(async)]
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

    let (provider, model, cost_cap, instruction) = {
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
            config.cost_cap_usd,
            config.custom_instruction.clone(),
        )
    };
    let paragraphs = elocutio_analysis::chunk_paragraphs(text);
    if paragraphs.is_empty() {
        return Err("No paragraphs found.".into());
    }

    // The cap covers the whole analysis, which sends up to three paragraphs.
    let analyzed_chars: usize = paragraphs
        .iter()
        .take(3)
        .map(|paragraph| paragraph.text.chars().count())
        .sum();
    openrouter::check_cost_cap(
        &model,
        analyzed_chars,
        openrouter::ANALYSIS_MAX_TOKENS * 3,
        cost_cap,
    )?;

    let mut suggestions = Vec::new();
    let mut rejected = Vec::new();
    let mut input_tokens = 0;
    let mut output_tokens = 0;
    let mut cost_usd = 0.0;
    let mut response_model = model.clone();

    for paragraph in paragraphs.iter().take(3) {
        let completion = openrouter::analyze_with_context(
            &provider,
            &paragraph.id,
            &paragraph.text,
            Some(model.clone()),
            "en-US",
            Some(instruction.as_str()),
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

    // Rank and cap the suggestions with the shared merge module, so the
    // analysis panel and the overlay agree on what is shown and why.
    let merge = elocutio_analysis::merge_with_limit(
        &[],
        &suggestions,
        elocutio_analysis::DEFAULT_SUGGESTION_LIMIT,
    );
    for dropped in &merge.report.dropped {
        rejected.push(elocutio_analysis::RejectedEdit {
            paragraph_id: String::new(),
            original: dropped.suggestion.original.clone(),
            reason: format!("{} ({})", dropped.reason.explanation(), dropped.detail),
        });
    }

    Ok(AnalyzeResponse {
        suggestions: merge.suggestions,
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
        .filter(|word| crate::personalization_db::is_valid_dictionary_word(word))
        .collect::<Vec<_>>();
    words.sort();
    words.dedup();

    Ok(words)
}

#[tauri::command]
async fn set_dictionary(
    words: Vec<String>,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut dictionary = MutableDictionary::new();
    dictionary.extend_words(
        words
            .into_iter()
            .filter(|word| crate::personalization_db::is_valid_dictionary_word(word))
            .map(|word| {
                (
                    word.trim().chars().collect::<Vec<_>>(),
                    DictWordMetadata::default(),
                )
            }),
    );

    let mut config = config.lock().await;
    config.mutable_dictionary = dictionary;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;

    Ok(())
}

/// Removes one word from the personal dictionary.
///
/// The removal happens under the configuration lock, so a word added from the
/// overlay window at the same moment is never lost, as it could be with a
/// read-modify-write through `set_dictionary` from the frontend.
#[tauri::command]
async fn remove_from_dictionary(
    word: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let target = word.trim();
    let mut config = config.lock().await;
    let mut dictionary = MutableDictionary::new();
    dictionary.extend_words(
        config
            .mutable_dictionary
            .words_iter()
            .map(|word| word.iter().collect::<String>())
            .filter(|word| word.trim() != target)
            .map(|word| {
                (
                    word.chars().collect::<Vec<_>>(),
                    DictWordMetadata::default(),
                )
            }),
    );
    config.mutable_dictionary = dictionary;
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())
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
    let trimmed = word.trim();
    if !crate::personalization_db::is_valid_dictionary_word(trimmed) {
        return Err(
            "Only single words (not phrases or sentences) can be added to the personal dictionary."
                .to_string(),
        );
    }
    let mut config = config.lock().await;
    config
        .mutable_dictionary
        .append_word_str(trimmed, DictWordMetadata::default());
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
async fn get_blocklist(config: State<'_, Arc<Mutex<Config>>>) -> Result<Vec<String>, String> {
    Ok(config.lock().await.blocklist.clone())
}

#[tauri::command]
async fn set_blocked(
    bundle_id: String,
    blocked: bool,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<Vec<String>, String> {
    let mut config = config.lock().await;
    if blocked {
        config.block_app(&bundle_id);
    } else {
        config.unblock_app(&bundle_id);
    }
    config
        .save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    Ok(config.blocklist.clone())
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

#[tauri::command(async)]
fn get_voice_profile() -> Result<Option<elocutio_voice::VoiceProfile>, String> {
    crate::voice::refresh_feedback()
}

#[tauri::command(async)]
fn learn_voice_samples(
    text: String,
    name: Option<String>,
) -> Result<elocutio_voice::VoiceProfile, String> {
    crate::voice::learn(&text, name)
}

#[tauri::command(async)]
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

#[tauri::command(async)]
fn get_voice_guidance() -> Result<Option<String>, String> {
    Ok(crate::voice::guidance())
}

/// Rate a rewrite: "sounds like me" or "does not sound like me".
///
/// The rating feeds back into the profile's confidence, so the app learns
/// whether the voice it applied was right.
#[tauri::command(async)]
fn rate_voice_rewrite(rating: String) -> Result<Option<elocutio_voice::VoiceProfile>, String> {
    crate::voice::rate_rewrite_by_name(&rating)
}

/// Set a trait the user chose by hand. Overrides always win over learned values.
#[tauri::command(async)]
fn set_voice_trait_override(
    key: String,
    value: crate::voice::TraitValueJson,
) -> Result<Option<elocutio_voice::VoiceProfile>, String> {
    crate::voice::set_trait_override_json(&key, value)
}

/// Remove a user override so the learned value shows again.
#[tauri::command(async)]
fn clear_voice_trait_override(key: String) -> Result<Option<elocutio_voice::VoiceProfile>, String> {
    crate::voice::clear_trait_override(&key)
}

/// The learned traits, with their confidence and whether the user set them.
#[tauri::command(async)]
fn get_voice_traits() -> Vec<crate::voice::TraitView> {
    crate::voice::traits_for_ui()
}

/// The traits the user set by hand.
#[tauri::command(async)]
fn get_voice_trait_overrides() -> Vec<(String, elocutio_voice::TraitValue)> {
    crate::voice::trait_overrides()
}

/// How many voice events are stored locally.
#[tauri::command(async)]
fn get_voice_event_count() -> usize {
    crate::voice::voice_event_count()
}

/// The copy-guard report for a candidate rewrite.
///
/// This is the diagnostic behind the guard: it says which sample n-grams a
/// candidate shares, without revealing the sample text.
#[tauri::command(async)]
fn get_copy_guard_report(
    candidate: String,
) -> Result<Option<crate::voice::CopyGuardReport>, String> {
    Ok(crate::voice::copy_guard_report(&candidate))
}

/// A summary of the stored voice profile for the Settings UI.
#[tauri::command(async)]
fn get_voice_profile_summary() -> Option<crate::voice::VoiceProfileSummary> {
    crate::voice::profile_summary()
}

/// The traits of the profile, with their confidence and override state.
#[tauri::command(async)]
fn get_voice_trait_list() -> Vec<crate::voice::TraitOverride> {
    crate::voice::trait_overrides_for_ui()
}

/// Record a rating without folding it into the profile yet.
///
/// Used when the user rates something other than the last rewrite.
#[tauri::command(async)]
fn record_voice_rating(rating: String, context: Option<String>) -> Result<(), String> {
    crate::voice::record_rating_by_name(&rating, context)
}

/// The fingerprints of the writer's own edits, for the copy guard.
#[tauri::command(async)]
fn get_feedback_fingerprint_count() -> usize {
    crate::voice::feedback_fingerprint_set().len()
}

/// Set a trait from a plain value, choosing the right trait type.
///
/// The UI sends a number, a boolean, or a string; the trait type follows the
/// value, so the user does not have to pick a type as well as a value.
#[tauri::command(async)]
fn set_voice_trait_value(
    key: String,
    value: serde_json::Value,
) -> Result<Option<elocutio_voice::VoiceProfile>, String> {
    let trait_value = match value {
        serde_json::Value::Number(number) => {
            crate::voice::numeric_trait(number.as_f64().unwrap_or_default())
        }
        serde_json::Value::Bool(flag) => crate::voice::boolean_trait(flag),
        serde_json::Value::String(text) => crate::voice::text_trait(&text),
        other => {
            return Err(format!(
                "A trait value must be a number, a boolean, or text, not {}.",
                match other {
                    serde_json::Value::Null => "null",
                    serde_json::Value::Array(_) => "a list",
                    serde_json::Value::Object(_) => "an object",
                    _ => "an unsupported value",
                }
            ));
        }
    };
    crate::voice::set_trait_override(&key, trait_value)
}

#[tauri::command(async)]
fn get_suggestion_events_enabled() -> bool {
    crate::events::is_enabled()
}

#[tauri::command(async)]
fn set_suggestion_events_enabled(enabled: bool) -> Result<bool, String> {
    crate::events::set_enabled(enabled)?;
    Ok(crate::events::is_enabled())
}

#[tauri::command(async)]
fn get_suggestion_event_summary() -> Result<crate::events::EventSummary, String> {
    crate::events::summarize()
}

#[tauri::command(async)]
fn clear_suggestion_events() -> Result<crate::events::EventSummary, String> {
    crate::events::clear()?;
    crate::events::summarize()
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

#[tauri::command(async)]
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

/// The detected tone of a passage, with the evidence behind it.
#[derive(serde::Serialize, Clone)]
struct ToneResponse {
    tone: Tone,
    label: String,
    confidence: f64,
    evidence: Vec<String>,
    guidance: Option<String>,
}

/// A suggestion that did not make it into the merged list, and why.
#[derive(serde::Serialize, Clone)]
struct DroppedEntry {
    id: String,
    original: String,
    replacement: String,
    source: String,
    reason: String,
    detail: String,
    kept_id: Option<String>,
}

/// Why one suggestion was ranked above another.
#[derive(serde::Serialize, Clone)]
struct RankingEntry {
    id: String,
    severity: String,
    resolution: String,
    interior: bool,
    reason: String,
}

/// The merged suggestion list, what it dropped, and how it was ranked.
#[derive(serde::Serialize, Clone)]
struct MergeResponse {
    suggestions: Vec<Suggestion>,
    dropped: Vec<DroppedEntry>,
    ranking: Vec<RankingEntry>,
}

/// Detect the register of a passage without sending it anywhere.
///
/// The detector is local and deterministic, so this works in local-only mode
/// and produces the same answer for the same text every time.
#[tauri::command(async)]
fn detect_text_tone(text: String) -> Result<ToneResponse, String> {
    let signal: ToneSignal = elocutio_analysis::detect_tone(&text);
    Ok(ToneResponse {
        tone: signal.tone,
        label: signal.tone.label().to_string(),
        confidence: signal.confidence,
        evidence: signal.evidence,
        guidance: elocutio_analysis::tone_guidance(signal.tone).map(str::to_string),
    })
}

/// Merge local lints with model suggestions and explain the result.
#[tauri::command(async)]
fn merge_suggestions(
    local: Vec<Suggestion>,
    llm: Vec<Suggestion>,
) -> Result<MergeResponse, String> {
    let result: MergeResult = elocutio_analysis::merge_with_limit(
        &local,
        &llm,
        elocutio_analysis::DEFAULT_SUGGESTION_LIMIT,
    );
    // The caller sends suggestions, not the document, so its length is
    // unknown. The furthest suggestion end is not the document end, and
    // treating it as one wrongly marks the last suggestion as an edge span.
    let document_end = None;

    let ranking = result
        .suggestions
        .iter()
        .map(|s| {
            let explanation = elocutio_analysis::merge::explain_rank(s, document_end);
            RankingEntry {
                id: s.id.clone(),
                severity: format!("{:?}", s.severity).to_lowercase(),
                resolution: format!("{:?}", s.resolution).to_lowercase(),
                interior: explanation.interior,
                reason: explanation.reason,
            }
        })
        .collect();

    let dropped = result
        .report
        .dropped
        .iter()
        .map(|entry| DroppedEntry {
            id: entry.suggestion.id.clone(),
            original: entry.suggestion.original.clone(),
            replacement: entry.suggestion.replacement.clone(),
            source: entry.source.label().to_string(),
            reason: entry.reason.explanation().to_string(),
            detail: entry.detail.clone(),
            kept_id: entry.kept_id.clone(),
        })
        .collect();

    Ok(MergeResponse {
        suggestions: result.suggestions,
        dropped,
        ranking,
    })
}

/// Score a run against the labelled golden set.
///
/// The harness is pure, so this is the same code the tests use. The seed set
/// ships with the app so the metrics can be reproduced on demand.
#[tauri::command(async)]
fn evaluate_golden_set(
    golden_json: String,
    predictions_json: String,
) -> Result<GoldenReport, String> {
    let set = elocutio_analysis::load_from_json(&golden_json)?;
    let predictions: Vec<elocutio_analysis::golden::Prediction> =
        serde_json::from_str(&predictions_json)
            .map_err(|error| format!("Invalid predictions JSON: {error}"))?;
    Ok(elocutio_analysis::evaluate(&set, &predictions))
}

/// Run the resolver against the golden set that ships with the app.
///
/// This measures the component that decides where an edit lands, which is the
/// part of the pipeline that has to be right for an accepted suggestion to be
/// safe.
#[tauri::command(async)]
fn run_resolver_quality_check() -> Result<crate::quality::ResolverQuality, String> {
    crate::quality::run_resolver_check()
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

#[derive(serde::Serialize, Clone)]
struct LocalLintSuggestion {
    kind: String,
    text: String,
    label: String,
}

#[derive(serde::Serialize, Clone)]
struct LocalLintItem {
    id: String,
    rule_name: String,
    kind: String,
    category: String,
    category_label: String,
    subcategory: String,
    color: String,
    start: usize,
    end: usize,
    original: String,
    message: String,
    context_before: String,
    context_after: String,
    suggestions: Vec<LocalLintSuggestion>,
}

/// Runs the local Harper linter on `text` for the in-app Inline Writing Studio.
#[tauri::command]
async fn analyze_studio_text(
    text: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<usize, String> {
    if text.trim().chars().count() < 12 {
        return Ok(0);
    }
    let current_hash = crate::llm_cache::text_hash(&text);
    if let Ok(Some(cached)) = crate::llm_cache::read()
        && cached.text_hash == current_hash
    {
        return Ok(cached.suggestions.len());
    }
    let focused = crate::os_broker::FocusedText {
        pid: std::process::id() as i32,
        bundle_id: "app.elocutio.desktop".to_string(),
        text,
    };
    crate::llm_analysis::analyze_text_with_config(config.inner(), focused, None).await
}

#[tauri::command]
async fn check_text_local(
    text: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<Vec<LocalLintItem>, String> {
    if text.is_empty() {
        return Ok(Vec::new());
    }

    type StudioLinterCacheEntry = (
        harper_core::Dialect,
        usize,
        usize,
        harper_core::linting::LintGroup,
        Arc<harper_core::spell::MergedDictionary>,
    );
    static CACHED_STUDIO_LINTER: std::sync::OnceLock<
        std::sync::Mutex<Option<StudioLinterCacheEntry>>,
    > = std::sync::OnceLock::new();

    let ignored_lints = {
        use harper_core::spell::Dictionary;
        let cfg = config.lock().await;
        // The linter is rebuilt when the dictionary's contents change, not
        // just its size: adding one word and removing another keeps the
        // count but must still refresh the linter. The per-word hashes are
        // combined order-independently, so iteration order does not matter.
        let dict_fingerprint = cfg
            .mutable_dictionary
            .words_iter()
            .fold(0usize, |acc, word| {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::hash::DefaultHasher::new();
                word.hash(&mut hasher);
                acc.wrapping_add(hasher.finish() as usize)
            });
        let dialect = cfg.dialect;
        let rule_epoch = LINTER_RULE_EPOCH.load(std::sync::atomic::Ordering::Relaxed);
        let cache_mutex = CACHED_STUDIO_LINTER.get_or_init(|| std::sync::Mutex::new(None));
        let mut guard = cache_mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let needs_rebuild = guard.as_ref().is_none_or(|(d, fingerprint, ep, _, _)| {
            *d != dialect || *fingerprint != dict_fingerprint || *ep != rule_epoch
        });
        if needs_rebuild {
            let dict = Config::dictionary_from_user_dictionary(cfg.mutable_dictionary.clone());
            let linter = cfg.create_linter();
            *guard = Some((dialect, dict_fingerprint, rule_epoch, linter, dict));
        }
        cfg.ignored_lints.clone()
    };

    // Linting a long document takes real CPU time, so it runs on the blocking
    // pool rather than an async worker. A panic inside the linter poisons the
    // cache mutex; the poisoned entry is dropped and rebuilt on the next call
    // instead of failing every check until the app restarts.
    let lint_text = text.clone();
    let linted = tauri::async_runtime::spawn_blocking(move || {
        let cache_mutex = CACHED_STUDIO_LINTER.get_or_init(|| std::sync::Mutex::new(None));
        let mut guard = cache_mutex.lock().unwrap_or_else(|poisoned| {
            let mut guard = poisoned.into_inner();
            *guard = None;
            guard
        });
        let (_, _, _, linter, dictionary) = guard.as_mut()?;
        let doc = harper_core::Document::new_markdown_default(&lint_text, dictionary);
        let organized = linter.organized_lints(&doc);
        Some((organized, doc))
    })
    .await
    .map_err(|error| format!("The local check failed: {error}"))?;
    let Some((mut organized_lints, doc)) = linted else {
        return Ok(Vec::new());
    };

    for lints in organized_lints.values_mut() {
        ignored_lints.remove_ignored(lints, &doc);
    }

    let source_chars = doc.get_source();
    let mut raw_items: Vec<LocalLintItem> = Vec::new();

    // 1. Include cached AI suggestions first when fresh for this exact text hash
    let current_hash = crate::llm_cache::text_hash(&text);
    let cached_ai = crate::llm_cache::read()
        .ok()
        .flatten()
        .filter(|c| c.text_hash == current_hash);
    let has_fresh_llm_cache = cached_ai.is_some();

    if let Some(cached) = cached_ai {
        for suggestion in cached.suggestions {
            if suggestion.char_end > source_chars.len()
                || suggestion.char_start >= suggestion.char_end
                || raw_items.iter().any(|existing| {
                    existing.start < suggestion.char_end && suggestion.char_start < existing.end
                })
            {
                continue;
            }

            let original: String = source_chars[suggestion.char_start..suggestion.char_end]
                .iter()
                .collect();
            let (category, category_label, subcategory, color_hex) = match suggestion.category {
                elocutio_core::Category::Spelling => {
                    ("correctness", "Correctness", "Spelling", "#E5484D")
                }
                elocutio_core::Category::Punctuation => {
                    ("correctness", "Correctness", "Punctuation", "#E5484D")
                }
                elocutio_core::Category::Capitalization => {
                    ("correctness", "Correctness", "Capitalization", "#E5484D")
                }
                elocutio_core::Category::Grammar => {
                    ("correctness", "Correctness", "Grammar", "#E5484D")
                }
                elocutio_core::Category::Conciseness | elocutio_core::Category::Clarity => {
                    ("clarity", "Clarity", "Conciseness", "#2563EB")
                }
                elocutio_core::Category::WordChoice => {
                    ("engagement", "Engagement", "Word choice", "#10B981")
                }
                elocutio_core::Category::Tone => ("delivery", "Delivery", "Tone", "#7C3AED"),
                elocutio_core::Category::Style
                | elocutio_core::Category::Consistency
                | elocutio_core::Category::Formatting => {
                    ("style", "Style guide", "Style", "#191F70")
                }
            };

            let before_start = suggestion.char_start.saturating_sub(42);
            let after_end = (suggestion.char_end + 42).min(source_chars.len());
            let context_before: String = source_chars[before_start..suggestion.char_start]
                .iter()
                .collect::<String>()
                .replace('\n', " ");
            let context_after: String = source_chars[suggestion.char_end..after_end]
                .iter()
                .collect::<String>()
                .replace('\n', " ");

            let repl = suggestion.replacement.clone();
            let suggestions = vec![if repl.is_empty() {
                LocalLintSuggestion {
                    kind: "remove".to_string(),
                    label: "Remove".to_string(),
                    text: String::new(),
                }
            } else {
                LocalLintSuggestion {
                    kind: "replace".to_string(),
                    label: repl.clone(),
                    text: repl,
                }
            }];

            raw_items.push(LocalLintItem {
                id: format!(
                    "llm:{}:{}:{}",
                    category, suggestion.char_start, suggestion.char_end
                ),
                rule_name: format!("llm:{category}"),
                kind: subcategory.to_string(),
                category: category.to_string(),
                category_label: category_label.to_string(),
                subcategory: subcategory.to_string(),
                color: color_hex.to_string(),
                start: suggestion.char_start,
                end: suggestion.char_end,
                original,
                message: suggestion.explanation,
                context_before,
                context_after,
                suggestions,
            });
        }
    }

    // 2. Local Harper lints (non-overlapping, with context-blind SpellCheck suppressed when AI cache is loaded)
    for (rule_name, lints) in organized_lints {
        if has_fresh_llm_cache && rule_name == "SpellCheck" {
            continue;
        }
        for mut lint in lints {
            if lint.span.is_empty() || lint.span.end > source_chars.len() {
                continue;
            }
            if raw_items
                .iter()
                .any(|existing| existing.start < lint.span.end && lint.span.start < existing.end)
            {
                continue;
            }
            if rule_name == "SpellCheck" && lint.suggestions.len() > 3 {
                lint.suggestions.truncate(3);
            }
            let original = lint.get_str(source_chars);
            let (category, category_label, subcategory, color_hex) = match lint.lint_kind {
                harper_core::linting::LintKind::Spelling | harper_core::linting::LintKind::Typo => {
                    ("correctness", "Correctness", "Spelling", "#E5484D")
                }
                harper_core::linting::LintKind::Punctuation => {
                    ("correctness", "Correctness", "Punctuation", "#E5484D")
                }
                harper_core::linting::LintKind::Capitalization => {
                    ("correctness", "Correctness", "Capitalization", "#E5484D")
                }
                harper_core::linting::LintKind::Grammar
                | harper_core::linting::LintKind::Agreement
                | harper_core::linting::LintKind::BoundaryError
                | harper_core::linting::LintKind::Eggcorn
                | harper_core::linting::LintKind::Malapropism
                | harper_core::linting::LintKind::Nonstandard
                | harper_core::linting::LintKind::WordOrder => {
                    ("correctness", "Correctness", "Grammar", "#E5484D")
                }
                harper_core::linting::LintKind::Redundancy
                | harper_core::linting::LintKind::Repetition => {
                    ("clarity", "Clarity", "Conciseness", "#2563EB")
                }
                harper_core::linting::LintKind::Readability => {
                    ("clarity", "Clarity", "Readability", "#2563EB")
                }
                harper_core::linting::LintKind::WordChoice
                | harper_core::linting::LintKind::Enhancement => {
                    ("engagement", "Engagement", "Vocabulary", "#10B981")
                }
                harper_core::linting::LintKind::Usage
                | harper_core::linting::LintKind::Regionalism => {
                    ("delivery", "Delivery", "Conventions", "#7C3AED")
                }
                harper_core::linting::LintKind::Style
                | harper_core::linting::LintKind::Formatting
                | harper_core::linting::LintKind::Miscellaneous => {
                    ("style", "Style guide", "Style", "#191F70")
                }
            };

            let before_start = lint.span.start.saturating_sub(42);
            let after_end = (lint.span.end + 42).min(source_chars.len());
            let context_before: String = source_chars[before_start..lint.span.start]
                .iter()
                .collect::<String>()
                .replace('\n', " ");
            let context_after: String = source_chars[lint.span.end..after_end]
                .iter()
                .collect::<String>()
                .replace('\n', " ");

            let suggestions = lint
                .suggestions
                .iter()
                .map(|suggestion| match suggestion {
                    harper_core::linting::Suggestion::ReplaceWith(chars) => {
                        let repl: String = chars.iter().collect();
                        LocalLintSuggestion {
                            kind: "replace".to_string(),
                            label: if repl.is_empty() {
                                "Remove".to_string()
                            } else {
                                repl.clone()
                            },
                            text: repl,
                        }
                    }
                    harper_core::linting::Suggestion::InsertAfter(chars) => {
                        let suffix: String = chars.iter().collect();
                        LocalLintSuggestion {
                            kind: "insert_after".to_string(),
                            label: format!("{original}{suffix}"),
                            text: format!("{original}{suffix}"),
                        }
                    }
                    harper_core::linting::Suggestion::Remove => LocalLintSuggestion {
                        kind: "remove".to_string(),
                        label: "Remove".to_string(),
                        text: String::new(),
                    },
                })
                .collect();

            raw_items.push(LocalLintItem {
                id: format!("{}:{}:{}", rule_name, lint.span.start, lint.span.end),
                rule_name: rule_name.clone(),
                kind: lint.lint_kind.to_string(),
                category: category.to_string(),
                category_label: category_label.to_string(),
                subcategory: subcategory.to_string(),
                color: color_hex.to_string(),
                start: lint.span.start,
                end: lint.span.end,
                original,
                message: lint.message,
                context_before,
                context_after,
                suggestions,
            });
        }
    }

    raw_items.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.end.cmp(&b.end)));

    let mut deduped: Vec<LocalLintItem> = Vec::with_capacity(raw_items.len());
    for item in raw_items {
        if let Some(prev) = deduped.last()
            && item.start < prev.end
        {
            continue;
        }
        deduped.push(item);
    }

    Ok(deduped)
}

#[tauri::command]
fn get_elo_overlay_snapshot() -> crate::elo_overlay::EloOverlaySnapshot {
    crate::elo_overlay::current_snapshot()
}

#[tauri::command]
fn set_elo_overlay_regions(
    widget_rect: Option<crate::elo_overlay::EloRect>,
    card_rect: Option<crate::elo_overlay::EloRect>,
    is_dragging: bool,
) {
    crate::elo_overlay::update_interactive_regions(widget_rect, card_rect, is_dragging);
}

#[tauri::command]
fn save_elo_widget_offset(bundle_id: String, offset_x: f64, offset_y: f64) {
    crate::elo_overlay::save_widget_offset(&bundle_id, offset_x, offset_y);
}

#[tauri::command]
fn apply_elo_alert(alert_id: String, suggestion_index: usize) -> Result<bool, String> {
    crate::elo_overlay::request_apply_alert(alert_id, suggestion_index)
}

#[tauri::command]
fn dismiss_elo_alert(alert_id: String) -> Result<bool, String> {
    crate::elo_overlay::request_dismiss_alert(alert_id)
}

pub static LINTER_RULE_EPOCH: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(1);

#[tauri::command]
async fn disable_elo_rule(
    rule_name: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut cfg = config.lock().await;
    cfg.lint_config.set_rule_enabled(rule_name, false);
    cfg.save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    LINTER_RULE_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    crate::elo_overlay::request_refresh();
    Ok(())
}

#[tauri::command]
async fn set_lint_rule_enabled(
    rule_name: String,
    enabled: bool,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<(), String> {
    let mut cfg = config.lock().await;
    cfg.lint_config.set_rule_enabled(rule_name, enabled);
    cfg.save_to_system()
        .await
        .map_err(|error| error.to_string())?;
    LINTER_RULE_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    crate::elo_overlay::request_refresh();
    Ok(())
}

#[tauri::command]
async fn get_personalization_db_status(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<crate::personalization_db::PersonalizationDbStatus, String> {
    let cfg = config.lock().await;
    crate::personalization_db::sync_and_status(&cfg)
}

#[tauri::command]
async fn get_writing_analytics(
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<crate::personalization_db::WritingAnalyticsSnapshot, String> {
    let cfg = config.lock().await;
    crate::personalization_db::get_writing_analytics(&cfg)
}

#[tauri::command]
async fn record_studio_edit_feedback(
    kind: String,
    category: String,
    original: String,
    replacement: String,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<crate::personalization_db::PersonalizationDbStatus, String> {
    match kind.as_str() {
        "dismissed" => {
            let _ = crate::voice::record_dismissed(&original, &replacement, &category);
        }
        _ => {
            let _ = crate::voice::record_accepted(&original, &replacement, &category);
        }
    }
    let _ = crate::voice::refresh_feedback();
    let cfg = config.lock().await;
    crate::personalization_db::sync_and_status(&cfg)
}

#[tauri::command]
fn open_elocutio_studio(
    tab: Option<String>,
    import_focused_text: Option<bool>,
) -> Result<(), String> {
    crate::elo_overlay::open_studio_window(tab, import_focused_text.unwrap_or(false))
}

#[tauri::command]
fn take_studio_launch_state() -> crate::elo_overlay::StudioLaunchState {
    crate::elo_overlay::take_studio_launch_state()
}

#[tauri::command]
fn reset_and_request_accessibility(
    broker: State<'_, RwLock<PlatformBroker>>,
) -> AccessibilityPermissionStatus {
    crate::elo_overlay::reset_and_request_accessibility(&broker)
}

#[derive(serde::Serialize)]
struct EloRewriteResponse {
    original: String,
    rewritten: String,
    mode_label: String,
    engine: String,
}

/// Applies all local Harper suggestions and tone-specific refinements so "Rewrite with elo"
/// works instantaneously even without a cloud API key or in Local-Only mode.
fn local_editorial_rewrite(
    text: &str,
    mode: &str,
    custom_prompt: Option<&str>,
    cfg: &Config,
) -> String {
    let mut linter = cfg.create_linter();
    let dictionary = Config::dictionary_from_user_dictionary(cfg.mutable_dictionary.clone());
    let doc = harper_core::Document::new_markdown_default(text, &dictionary);
    let mut organized = linter.organized_lints(&doc);
    for lints in organized.values_mut() {
        cfg.ignored_lints.remove_ignored(lints, &doc);
    }

    let mut all_lints: Vec<harper_core::linting::Lint> =
        organized.into_values().flatten().collect();
    all_lints.sort_by(|a, b| {
        a.span
            .start
            .cmp(&b.span.start)
            .then_with(|| a.span.end.cmp(&b.span.end))
    });

    // Dedup overlapping spans, then apply from right to left
    let mut non_overlapping: Vec<harper_core::linting::Lint> = Vec::new();
    for lint in all_lints {
        if lint.span.is_empty() || lint.suggestions.is_empty() {
            continue;
        }
        if let Some(prev) = non_overlapping.last()
            && lint.span.start < prev.span.end
        {
            continue;
        }
        non_overlapping.push(lint);
    }

    let mut chars: Vec<char> = text.chars().collect();
    for lint in non_overlapping.into_iter().rev() {
        if lint.span.end <= chars.len()
            && let Some(first) = lint.suggestions.first()
        {
            first.apply(lint.span, &mut chars);
        }
    }

    let mut result: String = chars.into_iter().collect();

    // Common misspellings of product/brand terms
    for (wrong, right) in [
        ("grammerly", "Grammarly"),
        ("Grammerly", "Grammarly"),
        ("elocuto", "Elocutio"),
    ] {
        result = result.replace(wrong, right);
    }

    let effective_mode = if let Some(prompt) = custom_prompt.map(str::trim)
        && !prompt.is_empty()
    {
        let lower = prompt.to_lowercase();
        if lower.contains("confident") || lower.contains("assertive") || lower.contains("strong") {
            "confident"
        } else if lower.contains("short") || lower.contains("concise") || lower.contains("brief") {
            "concise"
        } else if lower.contains("formal") || lower.contains("professional") {
            "formal"
        } else if lower.contains("friend") || lower.contains("warm") {
            "friendly"
        } else if lower.contains("bullet") || lower.contains("list") {
            "bullets"
        } else {
            mode
        }
    } else {
        mode
    };

    match effective_mode {
        "confident" => {
            for (weak, strong) in [
                (" just to see ", " to demonstrate "),
                (" just ", " "),
                ("I think ", ""),
                ("I feel like ", ""),
                ("maybe ", ""),
                ("hopefully ", ""),
                ("sort of ", ""),
                ("kind of ", ""),
                ("I was wondering if ", "Please let me know if "),
            ] {
                result = result.replace(weak, strong);
            }
        }
        "concise" => {
            for (wordy, tight) in [
                (" just to see how ", " to test how "),
                ("in order to ", "to "),
                ("due to the fact that ", "because "),
                ("at this point in time", "now"),
                ("for the purpose of ", "for "),
                ("very unique", "unique"),
                (" just ", " "),
            ] {
                result = result.replace(wordy, tight);
            }
        }
        "formal" => {
            for (casual, formal) in [
                (" just to see how ", " to evaluate how "),
                (" can't ", " cannot "),
                (" won't ", " will not "),
                (" don't ", " do not "),
                (" isn't ", " is not "),
                (" doesn't ", " does not "),
            ] {
                result = result.replace(casual, formal);
            }
        }
        "friendly" => {
            if !result.ends_with('!') && !result.ends_with('?') {
                result = result.trim_end_matches('.').to_string();
                result.push('.');
            }
        }
        "bullets" => {
            let sentences: Vec<&str> = result
                .split(['.', '\n'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            if sentences.len() > 1 {
                result = sentences
                    .into_iter()
                    .map(|s| format!("• {s}."))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        _ => {}
    }

    // Capitalize first letter and ensure ending punctuation
    let trimmed = result.trim();
    if !trimmed.is_empty() {
        let mut out_chars: Vec<char> = trimmed.chars().collect();
        if let Some(first) = out_chars.first_mut() {
            *first = first.to_uppercase().next().unwrap_or(*first);
        }
        let mut final_str: String = out_chars.into_iter().collect();
        if !final_str.ends_with(['.', '!', '?']) {
            final_str.push('.');
        }
        return final_str;
    }

    result
}

#[tauri::command]
async fn elo_rewrite_focused_text(
    mode: String,
    custom_prompt: Option<String>,
    source_override: Option<String>,
    config: State<'_, Arc<Mutex<Config>>>,
) -> Result<EloRewriteResponse, String> {
    let snap = crate::elo_overlay::current_snapshot();
    let original = source_override
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| snap.source_text.clone());

    if original.trim().is_empty() {
        return Err("Type some text in the field first.".into());
    }

    let mode_label = match mode.as_str() {
        "improve" => "Improve it",
        "confident" => "Sound confident",
        "concise" => "Make it concise",
        "formal" => "Sound formal",
        "friendly" => "Sound friendly",
        "bullets" => "Bullet points",
        _ => custom_prompt.as_deref().unwrap_or("Rewrite"),
    }
    .to_string();

    let mode_directive = match mode.as_str() {
        "improve" => Some(
            "Improve clarity, flow, grammar, and word choice while keeping the meaning natural and engaging.",
        ),
        "confident" => Some(
            "Rewrite in a direct, confident, assertive tone. Remove hedging, filler, and tentative phrasing, and use strong active verbs.",
        ),
        "concise" => Some(
            "Make this significantly more concise and tight. Cut filler words, redundancies, and wordy phrasing so every word counts while keeping the core message.",
        ),
        "formal" => Some(
            "Rewrite in a polished, formal, professional register suitable for executive or official communication. Avoid slang, casual phrasing, and conversational contractions.",
        ),
        "friendly" => Some(
            "Rewrite in a warm, approachable, friendly, and conversational tone that feels welcoming and personable while keeping the message clear.",
        ),
        "bullets" => Some(
            "Rewrite and organize the key points into a clean, easy-to-scan bulleted list using '• ' bullets.",
        ),
        _ => None,
    };

    let has_explicit_directive = custom_prompt
        .as_deref()
        .map(|p| !p.trim().is_empty())
        .unwrap_or(false)
        || mode_directive.is_some();

    let (can_try_cloud, provider, model, tone, instruction) = {
        let cfg = config.lock().await;
        let is_local_endpoint = cfg.provider_kind == ProviderKind::OpenAiCompatible
            && (cfg.provider_base_url.contains("localhost")
                || cfg.provider_base_url.contains("127.0.0.1"));
        let key_ok = is_local_endpoint
            || openrouter::key_status_for(&provider_from(&cfg))
                .map(|s| s.present)
                .unwrap_or(false);
        let tone = match mode.as_str() {
            "concise" => openrouter::RewriteTone::Concise,
            "formal" => openrouter::RewriteTone::Formal,
            "friendly" => openrouter::RewriteTone::Friendly,
            "confident" => openrouter::RewriteTone::Confident,
            "bullets" | "custom" => openrouter::RewriteTone::Neutral,
            _ => cfg.rewrite_tone,
        };
        let extra_inst = if let Some(p) = custom_prompt.as_deref().filter(|p| !p.trim().is_empty())
        {
            p.trim().to_string()
        } else if let Some(dir) = mode_directive {
            dir.to_string()
        } else {
            cfg.custom_instruction.clone()
        };
        (
            !cfg.local_only && key_ok,
            ProviderConfig {
                kind: cfg.provider_kind,
                base_url: cfg.provider_base_url.clone(),
            },
            cfg.provider_model.clone(),
            tone,
            extra_inst,
        )
    };

    if can_try_cloud {
        let is_apple_on_device = model.starts_with("apple/") || provider.base_url.contains("11535");
        let voice_guidance = if has_explicit_directive {
            None
        } else {
            crate::voice::guidance()
        };
        let cloud_attempt = openrouter::rewrite_with(
            &provider,
            &original,
            Some(model),
            voice_guidance,
            tone,
            Some(instruction.as_str()),
        )
        .await;

        match cloud_attempt {
            Ok(cloud) => {
                let _ = usage::record(&cloud);
                return Ok(EloRewriteResponse {
                    original,
                    rewritten: cloud.text,
                    mode_label,
                    engine: "AI Companion".to_string(),
                });
            }
            Err(err) if has_explicit_directive && !is_apple_on_device => {
                return Err(err);
            }
            Err(_) => {}
        }
    }

    let local_fallback = {
        let cfg = config.lock().await;
        local_editorial_rewrite(&original, &mode, custom_prompt.as_deref(), &cfg)
    };

    Ok(EloRewriteResponse {
        original,
        rewritten: local_fallback,
        mode_label,
        engine: "Local Engine".to_string(),
    })
}

#[tauri::command]
fn apply_elo_field_rewrite(text: String) -> Result<bool, String> {
    crate::elo_overlay::request_replace_field_text(text)
}

#[tauri::command]
fn set_elo_overlay_input_focus(focused: bool) {
    crate::elo_overlay::set_overlay_input_focus(focused);
}
