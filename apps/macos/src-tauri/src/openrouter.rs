//! The provider client for M0/M1.
//!
//! The key is stored in the macOS Keychain and never logged. This module
//! keeps the public surface the rest of the app already calls —
//! `rewrite_with`, `analyze_with`, `test_key_for` — and routes the actual
//! request building and error handling through the adapter layer in
//! `elocutio-llm`.
//!
//! Two rules matter here:
//!
//! * **`temperature` is never sent.** The Claude 5 family rejects any
//!   `temperature` other than `1` with an HTTP 400, and the default model is
//!   `anthropic/claude-haiku-5.5`. Omitting the key is valid everywhere.
//! * **Every failure is normalized** to the `elocutio-llm` error taxonomy
//!   before it becomes a user-facing sentence.

use elocutio_llm::{ChatRequest, ErrorKind, Provider, ProviderError, ProviderId, ResponseFormat};
use serde::{Deserialize, Serialize};

#[path = "llm/mod.rs"]
pub mod llm;

use llm::http::Endpoint;

const KEYCHAIN_ACCOUNT: &str = "api_key";
const KEY_URL: &str = "https://openrouter.ai/api/v1/key";
const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

pub const DEFAULT_MODEL: &str = "anthropic/claude-haiku-5.5";
pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// The output budget for a rewrite.
///
/// The default model has adaptive thinking on, and thinking tokens count
/// against `max_tokens`, so a tight cap can cut the response off before any
/// text appears. 2048 leaves headroom for a thinking pass plus a rewritten
/// passage while staying far below the model's output limit. The tradeoff is
/// that a runaway generation can burn up to 2048 tokens before the cap bites
/// instead of 1024; the cost difference is a fraction of a cent.
pub const REWRITE_MAX_TOKENS: u32 = 4096;

/// The output budget for an analysis.
///
/// An analysis returns a JSON array of suggestions, which is larger than a
/// rewrite. 4096 leaves room for the thinking pass, the JSON envelope, and a
/// long list of suggestions. The tradeoff is the same as above: the worst
/// case costs more before the cap stops it.
pub const ANALYSIS_MAX_TOKENS: u32 = 4096;

/// The estimated cost of a request, in USD.
///
/// The estimate uses the catalog price for the model when it is known, and a
/// conservative default when it is not. It is deliberately an over-estimate:
/// a request that is refused because of the cap should never turn out to
/// have been cheap.
pub fn estimate_cost_usd(model: &str, input_chars: usize, output_tokens: u32) -> f64 {
    // A rough 4 characters per token is close enough for a pre-flight check
    // and errs on the high side for English prose.
    let input_tokens = (input_chars as f64 / 4.0).ceil();
    let output_tokens = output_tokens as f64;

    let (input_per_million, output_per_million) = match elocutio_llm::catalog::find(model) {
        Some(entry) => (
            entry.price_input_per_million_usd,
            entry.price_output_per_million_usd,
        ),
        None => (3.0, 15.0),
    };

    (input_tokens * input_per_million + output_tokens * output_per_million) / 1_000_000.0
}

/// Whether a request fits under the user's cost cap.
///
/// A cap of `None` means no cap. The estimate is checked before the request
/// goes out, so a runaway live session cannot spend past the ceiling.
pub fn check_cost_cap(
    model: &str,
    input_chars: usize,
    output_tokens: u32,
    cap: Option<f64>,
) -> Result<(), String> {
    let Some(cap) = cap else {
        return Ok(());
    };
    if !cap.is_finite() || cap < 0.0 {
        return Ok(());
    }

    let estimate = estimate_cost_usd(model, input_chars, output_tokens);
    if estimate > cap {
        return Err(format!(
            "This request would cost about ${estimate:.4}, which is over your ${cap:.2} cost cap. Raise the cap in Settings or analyze less text."
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteTone {
    #[default]
    Neutral,
    Confident,
    Friendly,
    Concise,
    Formal,
}

impl RewriteTone {
    pub fn guidance(self) -> Option<&'static str> {
        match self {
            RewriteTone::Neutral => None,
            RewriteTone::Confident => Some("Use a confident, direct tone without hedging."),
            RewriteTone::Friendly => Some("Use a warm, friendly tone while staying professional."),
            RewriteTone::Concise => Some("Be concise; cut filler and unnecessary qualifiers."),
            RewriteTone::Formal => Some("Use a formal, precise register."),
        }
    }
}

/// Provider wire dialect. OpenRouter adds its own headers and key validation;
/// OpenAI-compatible endpoints use the same Chat Completions shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    OpenRouter,
    OpenAiCompatible,
}

impl From<ProviderKind> for ProviderId {
    fn from(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::OpenRouter => ProviderId::OpenRouter,
            ProviderKind::OpenAiCompatible => ProviderId::OpenAiCompatible,
        }
    }
}

impl ProviderKind {
    pub fn keychain_service(self) -> &'static str {
        match self {
            ProviderKind::OpenRouter => "app.elocutio.provider.openrouter",
            ProviderKind::OpenAiCompatible => "app.elocutio.provider.openai_compatible",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            ProviderKind::OpenRouter => "OpenRouter",
            ProviderKind::OpenAiCompatible => "OpenAI-compatible",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: String,
}

impl ProviderConfig {
    pub fn chat_endpoint(&self) -> String {
        match self.kind {
            ProviderKind::OpenRouter => CHAT_URL.to_string(),
            ProviderKind::OpenAiCompatible => {
                format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
            }
        }
    }

    pub fn models_endpoint(&self) -> String {
        match self.kind {
            ProviderKind::OpenRouter => KEY_URL.to_string(),
            ProviderKind::OpenAiCompatible => {
                format!("{}/models", self.base_url.trim_end_matches('/'))
            }
        }
    }
}

pub const SYSTEM_PROMPT: &str = "You are Elocutio, a precise copy editor. Rewrite the user's text to be clear, correct, and natural while preserving their voice and paragraph breaks. Do NOT introduce Markdown formatting (no **bold**, *italics*, # headings, or backticks) unless the original text already uses Markdown. Return only the rewritten plain text, with no preamble, surrounding quotation marks, or explanation.";

pub const CUSTOM_REWRITE_SYSTEM_PROMPT: &str = "You are Elocutio, a creative and adaptable writing assistant.\n\
Your job is to transform the passage inside <source_text> according to the user's instruction in <rewrite_instruction>.\n\
\n\
Critical rules:\n\
1. Follow <rewrite_instruction> BOLDLY and COMPLETELY. Do NOT do a timid, sentence-by-sentence word swap.\n\
2. Restructure sentences, merge or split paragraphs, adapt the persona, and replace technical or adult jargon with metaphors and concepts that genuinely fit <rewrite_instruction>.\n\
3. If <rewrite_instruction> asks to explain to a young child (e.g., a 5-year-old / ELI5), use warm, playful, story-like language and concrete kid-friendly ideas (like a friendly word-helper, little oopsies, colorful lines, magic buttons, and keeping your secret notebook safe in your room) instead of adult or UI terms like 'middle column', 'conciseness', 'punctuation', 'local engine', or 'the cloud'.\n\
4. Always fix any spelling or grammar mistakes present in <source_text> (such as 'Their are' -> 'There are'). Never copy source typos into your rewrite.\n\
5. Do NOT use Markdown formatting (no **bold**, *italics*, # headings, or `code` backticks) unless <source_text> already uses Markdown or <rewrite_instruction> explicitly asks for Markdown. Use plain text and normal quotation marks (e.g., \"elo\") instead of **bold**.\n\
6. Output ONLY the final rewritten text—no preamble, no surrounding quotation marks, no XML tags, and no commentary.";

fn strip_paired_marker(input: &str, marker: &str, source: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find(marker) {
        out.push_str(&rest[..start]);
        let after_start = &rest[start + marker.len()..];
        if let Some(end) = after_start.find(marker) {
            let inner = &after_start[..end];
            if !inner.is_empty()
                && !inner.contains('\n')
                && !inner.starts_with(char::is_whitespace)
                && !inner.ends_with(char::is_whitespace)
            {
                let quoted_ascii = format!("\"{inner}\"");
                let quoted_curly = format!("“{inner}”");
                if source.contains(&quoted_ascii) || source.contains(&quoted_curly) {
                    out.push('"');
                    out.push_str(inner);
                    out.push('"');
                } else {
                    out.push_str(inner);
                }
                rest = &after_start[end + marker.len()..];
                continue;
            }
        }
        out.push_str(marker);
        rest = after_start;
    }
    out.push_str(rest);
    out
}

/// Strips unrequested Markdown syntax (`**bold**`, `__bold__`, `` `code` ``, `# headings`)
/// that models often emit even when rewriting plain text.
pub fn strip_unrequested_markdown(
    source: &str,
    directive: Option<&str>,
    rewritten: &str,
) -> String {
    let wants_markdown = directive
        .map(|d| {
            let lower = d.to_ascii_lowercase();
            lower.contains("markdown") || lower.contains("md format")
        })
        .unwrap_or(false);
    if wants_markdown {
        return rewritten.to_string();
    }

    let mut out = rewritten.to_string();

    if !source.contains("**") {
        out = strip_paired_marker(&out, "**", source);
    }
    if !source.contains("__") {
        out = strip_paired_marker(&out, "__", source);
    }
    if !source.contains('`') {
        out = strip_paired_marker(&out, "`", source);
    }
    if !source.contains('*') {
        out = strip_paired_marker(&out, "*", source);
    }

    let source_has_headings = source.lines().any(|l| l.trim_start().starts_with('#'));
    if !source_has_headings {
        out = out
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                if trimmed.starts_with('#') {
                    let without_hashes = trimmed.trim_start_matches('#');
                    if without_hashes.starts_with(' ') {
                        return without_hashes.trim_start();
                    }
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n");
    }

    out
}

fn sanitize_rewrite_directive(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| (!c.is_control() || *c == '\t') && *c != '<' && *c != '>')
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated: String = collapsed.chars().take(280).collect();
    let trimmed = truncated.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyStatus {
    pub present: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenRouterStatus {
    pub label: Option<String>,
    pub usage: Option<f64>,
    pub limit: Option<f64>,
    pub is_free_tier: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RewriteResult {
    pub text: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
}

/// The Keychain account a provider configuration's key is saved under.
///
/// OpenRouter has one key. Every OpenAI-compatible preset (OpenAI, Groq,
/// Ollama, the Apple bridge, a custom URL) shares one provider kind, so the
/// key is scoped to the endpoint's host: a key saved for `api.openai.com` is
/// never attached to a request for `api.groq.com` or a local port.
fn key_account(provider: &ProviderConfig) -> String {
    match provider.kind {
        ProviderKind::OpenRouter => KEYCHAIN_ACCOUNT.to_string(),
        ProviderKind::OpenAiCompatible => {
            let host = crate::network_log::host_of(provider.base_url.trim()).to_ascii_lowercase();
            format!("{KEYCHAIN_ACCOUNT}@{host}")
        }
    }
}

/// Keys read from the Keychain this session, so a status check does not hit
/// the Keychain (and possibly an access prompt) on every overlay poll.
fn key_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, Option<String>>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn cache_id(service: &str, account: &str) -> String {
    format!("{service}\u{1f}{account}")
}

/// The secret store: the login Keychain in the app, an in-memory map in tests.
mod secret_store {
    #[cfg(not(test))]
    pub fn get(service: &str, account: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(service, account).map_err(|error| error.to_string())?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    #[cfg(not(test))]
    pub fn set(service: &str, account: &str, secret: &str) -> Result<(), String> {
        keyring::Entry::new(service, account)
            .and_then(|entry| entry.set_password(secret))
            .map_err(|error| error.to_string())
    }

    #[cfg(not(test))]
    pub fn delete(service: &str, account: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(service, account).map_err(|error| error.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }

    #[cfg(test)]
    fn store() -> &'static std::sync::Mutex<std::collections::HashMap<(String, String), String>> {
        static STORE: std::sync::OnceLock<
            std::sync::Mutex<std::collections::HashMap<(String, String), String>>,
        > = std::sync::OnceLock::new();
        STORE.get_or_init(Default::default)
    }

    #[cfg(test)]
    pub fn get(service: &str, account: &str) -> Result<Option<String>, String> {
        let store = store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(store
            .get(&(service.to_string(), account.to_string()))
            .cloned())
    }

    #[cfg(test)]
    pub fn set(service: &str, account: &str, secret: &str) -> Result<(), String> {
        let mut store = store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.insert(
            (service.to_string(), account.to_string()),
            secret.to_string(),
        );
        Ok(())
    }

    #[cfg(test)]
    pub fn delete(service: &str, account: &str) -> Result<(), String> {
        let mut store = store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.remove(&(service.to_string(), account.to_string()));
        Ok(())
    }
}

fn credentials_path() -> Result<std::path::PathBuf, String> {
    #[cfg(test)]
    let folder = Some(std::env::temp_dir().join("elocutio-test"));
    #[cfg(not(test))]
    let folder = crate::config::Config::folder_path();

    let folder = folder.ok_or_else(|| "Configuration directory is unavailable.".to_string())?;
    Ok(folder.join("credentials.json"))
}

fn read_credentials_map() -> Result<std::collections::BTreeMap<String, String>, String> {
    let path = credentials_path()?;
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(std::collections::BTreeMap::new())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn write_credentials_map(map: &std::collections::BTreeMap<String, String>) -> Result<(), String> {
    let path = credentials_path()?;
    if map.is_empty() {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        };
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let serialized = serde_json::to_string_pretty(map).map_err(|error| error.to_string())?;
    std::fs::write(&path, serialized).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// The ids older builds stored a provider kind's single shared key under.
fn legacy_key_ids(kind: ProviderKind) -> [String; 2] {
    [
        format!("{}:{KEYCHAIN_ACCOUNT}", kind.keychain_service()),
        kind.keychain_service().to_string(),
    ]
}

/// Removes every legacy copy of a provider kind's key: the SQLite
/// `encrypted_credentials` rows and the plaintext `credentials.json` entries.
fn purge_legacy_keys(kind: ProviderKind) {
    for id in legacy_key_ids(kind) {
        let _ = crate::personalization_db::clear_encrypted_api_key(&id);
    }
    if let Ok(mut map) = read_credentials_map() {
        let before = map.len();
        for id in legacy_key_ids(kind) {
            map.remove(&id);
        }
        if map.len() != before {
            let _ = write_credentials_map(&map);
        }
    }
}

/// Finds a key saved by an older build, which kept one key per provider kind
/// in SQLite (or, before that, in plaintext).
fn find_legacy_key(kind: ProviderKind) -> Option<String> {
    for id in legacy_key_ids(kind) {
        if let Ok(Some(key)) = crate::personalization_db::load_decrypted_api_key(&id) {
            return Some(key);
        }
    }
    let map = read_credentials_map().ok()?;
    legacy_key_ids(kind)
        .iter()
        .filter_map(|id| map.get(id))
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

pub fn key_status_for(provider: &ProviderConfig) -> Result<KeyStatus, String> {
    let present = load_key_optional(provider)?.is_some();
    Ok(KeyStatus { present })
}

/// The masked key (for example `sk-or-…9f2a`) shown in Settings.
pub fn key_hint_for(provider: &ProviderConfig) -> Option<String> {
    load_key_optional(provider)
        .ok()
        .flatten()
        .map(|key| crate::personalization_db::mask_key_hint(&key))
}

pub fn save_key_for(provider: &ProviderConfig, key: &str) -> Result<KeyStatus, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Enter an API key first.".into());
    }
    let service = provider.kind.keychain_service();
    let account = key_account(provider);
    secret_store::set(service, &account, key)
        .map_err(|error| format!("Could not save the key in the Keychain: {error}"))?;
    key_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(cache_id(service, &account), Some(key.to_string()));

    // A saved key replaces whatever an older build left behind.
    purge_legacy_keys(provider.kind);
    Ok(KeyStatus { present: true })
}

pub fn clear_key_for(provider: &ProviderConfig) -> Result<KeyStatus, String> {
    let service = provider.kind.keychain_service();
    let account = key_account(provider);
    secret_store::delete(service, &account)
        .map_err(|error| format!("Could not remove the key from the Keychain: {error}"))?;
    key_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(cache_id(service, &account), None);
    purge_legacy_keys(provider.kind);
    Ok(KeyStatus { present: false })
}

/// The key for a provider configuration, if one is saved.
pub fn load_key_optional(provider: &ProviderConfig) -> Result<Option<String>, String> {
    let service = provider.kind.keychain_service();
    let account = key_account(provider);
    let id = cache_id(service, &account);
    if let Some(cached) = key_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&id)
    {
        return Ok(cached.clone());
    }

    let key = secret_store::get(service, &account)
        .map_err(|error| format!("Could not read the key from the Keychain: {error}"))?
        .filter(|value| !value.trim().is_empty());
    key_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id, key.clone());
    Ok(key)
}

/// The endpoint a legacy OpenAI-compatible key belongs to, when its prefix
/// says so: Groq keys start `gsk_`, OpenAI keys `sk-` (OpenRouter's `sk-or-`
/// keys live under their own provider kind).
fn base_url_for_key_prefix(key: &str) -> Option<&'static str> {
    if key.starts_with("gsk_") {
        Some("https://api.groq.com/openai/v1")
    } else if key.starts_with("sk-") && !key.starts_with("sk-or-") {
        Some(DEFAULT_OPENAI_BASE_URL)
    } else {
        None
    }
}

/// Moves a key an older build saved into a Keychain slot, then deletes every
/// legacy copy.
///
/// Older builds kept one key per provider kind (in SQLite, and before that in
/// plaintext), shared by every OpenAI-compatible preset, so the key's owner is
/// a guess. A recognizable prefix wins (a Groq key goes to `api.groq.com`
/// even while Ollama is selected); otherwise the configured endpoint is used,
/// unless it is a local port, which never needs a provider key. When neither
/// applies the legacy copy is left alone and migrated on a later call, once a
/// remote endpoint is configured.
pub fn migrate_legacy_key(provider: &ProviderConfig) -> Result<(), String> {
    let Some(legacy) = find_legacy_key(provider.kind) else {
        return Ok(());
    };
    let target = match provider.kind {
        ProviderKind::OpenRouter => provider.clone(),
        ProviderKind::OpenAiCompatible => match base_url_for_key_prefix(&legacy) {
            Some(base_url) => ProviderConfig {
                kind: provider.kind,
                base_url: base_url.to_string(),
            },
            None if is_loopback_url(&provider.base_url) => return Ok(()),
            None => provider.clone(),
        },
    };
    if load_key_optional(&target)?.is_none() {
        save_key_for(&target, &legacy)?;
    } else {
        purge_legacy_keys(provider.kind);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetModelSlug {
    pub slug: String,
    pub label: String,
    pub badge: String,
    pub is_live: bool,
}

/// Returns sensible default model slugs for a provider preset and attempts a fast
/// live fetch against the provider's `/models` (or Ollama `/api/tags`) endpoint.
pub async fn fetch_preset_model_slugs(preset_id: &str, base_url: &str) -> Vec<PresetModelSlug> {
    let curated: Vec<(&str, &str, &str)> = match preset_id {
        "openrouter" => vec![
            (
                "anthropic/claude-haiku-4.5",
                "Claude Haiku 4.5",
                "Fast Editorial",
            ),
            (
                "anthropic/claude-sonnet-4.5",
                "Claude Sonnet 4.5",
                "Deep Rewrite",
            ),
            ("google/gemini-2.5-flash", "Gemini 2.5 Flash", "Low Latency"),
            ("openai/gpt-4.1-mini", "GPT-4.1 Mini", "Balanced"),
            (
                "minimax/minimax-m3.1-flash:free",
                "MiniMax M3.1 Flash",
                "Free",
            ),
            (
                "meta-llama/llama-3.3-70b-instruct:free",
                "Llama 3.3 70B",
                "Free",
            ),
            ("deepseek/deepseek-chat-v3-0324:free", "DeepSeek V3", "Free"),
        ],
        "ollama_local" => vec![
            ("llama3.2:3b", "Llama 3.2 (3B)", "Local Fast"),
            ("qwen2.5:7b", "Qwen 2.5 (7B)", "Local Editorial"),
            ("gemma3:4b", "Gemma 3 (4B)", "Local Compact"),
            ("mistral:7b", "Mistral (7B)", "Local Prose"),
            ("phi4-mini", "Phi-4 Mini", "Local Light"),
        ],
        "ollama_cloud" => vec![
            ("llama3.3:70b", "Llama 3.3 70B", "Ollama Cloud"),
            ("qwen2.5:72b", "Qwen 2.5 72B", "Ollama Cloud"),
            ("gemma3:27b", "Gemma 3 27B", "Ollama Cloud"),
            ("deepseek-v3", "DeepSeek V3", "Ollama Cloud"),
            ("mistral-small3.1", "Mistral Small 3.1", "Ollama Cloud"),
        ],
        "apple_foundation" => vec![
            (
                "apple/foundation-on-device",
                "Apple Foundation 3B",
                "On-Device Neural Engine",
            ),
            (
                "apple/writing-tools-instruct",
                "Apple Writing Tools",
                "Proofread & Rewrite",
            ),
            (
                "apple/foundation-adapter-editorial",
                "Editorial Tone Adapter",
                "macOS Native",
            ),
        ],
        "openai" => vec![
            ("gpt-4.1-mini", "GPT-4.1 Mini", "Fast & Precise"),
            ("gpt-4.1", "GPT-4.1", "Flagship"),
            ("gpt-4.1-nano", "GPT-4.1 Nano", "Ultra-Fast"),
            ("gpt-4o-mini", "GPT-4o Mini", "Balanced"),
        ],
        "groq" => vec![
            ("llama-3.3-70b-versatile", "Llama 3.3 70B", "Groq LPU"),
            ("llama-3.1-8b-instant", "Llama 3.1 8B", "Instant"),
            ("qwen-qwq-32b", "Qwen QwQ 32B", "Reasoning"),
            ("gemma2-9b-it", "Gemma 2 9B", "Compact"),
        ],
        _ => vec![
            ("gpt-4.1-mini", "GPT-4.1 Mini", "Default"),
            ("llama3.2:3b", "Llama 3.2 3B", "Local"),
        ],
    };

    let mut results: Vec<PresetModelSlug> = curated
        .into_iter()
        .map(|(slug, label, badge)| PresetModelSlug {
            slug: slug.to_string(),
            label: label.to_string(),
            badge: badge.to_string(),
            is_live: false,
        })
        .collect();

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(2200))
        .build()
    {
        Ok(c) => c,
        Err(_) => return results,
    };

    // 1. For Ollama Local, check `http://localhost:11434/api/tags` to list locally installed models first
    if preset_id == "ollama_local" {
        let tags_url = format!(
            "{}/api/tags",
            base_url.trim_end_matches('/').trim_end_matches("/v1")
        );
        if let Ok(resp) = client.get(&tags_url).send().await
            && resp.status().is_success()
            && let Ok(json) = resp.json::<serde_json::Value>().await
            && let Some(models) = json.get("models").and_then(|v| v.as_array())
        {
            let mut installed = Vec::new();
            for m in models.iter().take(8) {
                if let Some(name) = m.get("name").and_then(|v| v.as_str()) {
                    installed.push(PresetModelSlug {
                        slug: name.to_string(),
                        label: name.to_string(),
                        badge: "Installed".to_string(),
                        is_live: true,
                    });
                }
            }
            if !installed.is_empty() {
                for cur in results {
                    if !installed.iter().any(|i| i.slug == cur.slug) {
                        installed.push(cur);
                    }
                }
                return installed;
            }
        }
    }

    // 2. For OpenRouter / OpenAI / Groq / Ollama Cloud, query `/models`
    if matches!(preset_id, "openrouter" | "openai" | "groq" | "ollama_cloud") {
        let models_url = format!("{}/models", base_url.trim_end_matches('/'));
        let kind = if preset_id == "openrouter" {
            ProviderKind::OpenRouter
        } else {
            ProviderKind::OpenAiCompatible
        };
        let mut req = client.get(&models_url);
        // The key is looked up by this preset's own host, so a key saved for
        // one provider is never sent to another provider's endpoint.
        let provider = ProviderConfig {
            kind,
            base_url: base_url.to_string(),
        };
        if let Ok(Some(key)) = load_key_optional(&provider) {
            req = req.bearer_auth(key);
        }
        if let Ok(resp) = req.send().await
            && resp.status().is_success()
            && let Ok(json) = resp.json::<serde_json::Value>().await
            && let Some(data) = json.get("data").and_then(|v| v.as_array())
        {
            let mut live_extra = Vec::new();
            for item in data {
                let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                if let Some(existing) = results.iter_mut().find(|r| r.slug == id) {
                    existing.is_live = true;
                    continue;
                }
                // Pick relevant editorial/writing models to keep the list focused
                let lower = id.to_lowercase();
                let is_relevant = lower.contains("claude")
                    || lower.contains("gpt-4")
                    || lower.contains("gemini-2")
                    || lower.contains("llama-3.3")
                    || lower.contains("qwen")
                    || lower.contains("minimax")
                    || lower.ends_with(":free");
                if is_relevant && live_extra.len() < 5 {
                    let name = item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(id)
                        .to_string();
                    live_extra.push(PresetModelSlug {
                        slug: id.to_string(),
                        label: name,
                        badge: if id.ends_with(":free") {
                            "Free · Live".to_string()
                        } else {
                            "Live".to_string()
                        },
                        is_live: true,
                    });
                }
            }
            results.extend(live_extra);
        }
    }

    results
}

/// The endpoint and key for a provider configuration.
async fn endpoint_for(provider: &ProviderConfig) -> Result<Endpoint, String> {
    let key = load_key_optional(provider)?;
    if provider.kind == ProviderKind::OpenRouter && key.is_none() {
        return Err("No OpenRouter key is saved. Add one first.".into());
    }
    Ok(Endpoint {
        url: provider.chat_endpoint(),
        key,
        send_attribution_headers: provider.kind == ProviderKind::OpenRouter,
    })
}

/// Rewrite through a streaming request.
///
/// Streaming matters for long rewrites: the user sees the text appear rather
/// than waiting for the whole response. The copy guard still runs on the
/// finished text, so a streaming response cannot smuggle sample content
/// through.
pub async fn rewrite_with_stream(
    provider: &ProviderConfig,
    text: &str,
    model: Option<String>,
    voice_guidance: Option<String>,
    tone: RewriteTone,
    custom_instruction: Option<&str>,
) -> Result<RewriteResult, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter some text to rewrite first.".into());
    }

    let model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let mut system = SYSTEM_PROMPT.to_string();
    if let Some(guidance) = voice_guidance {
        system.push_str("\n\n");
        system.push_str(&guidance);
    }
    if let Some(guidance) = tone.guidance() {
        system.push_str("\n\n");
        system.push_str(guidance);
    }
    if let Some(instruction) = custom_instruction.and_then(elocutio_analysis::custom_instruction) {
        system.push_str("\n\n");
        system.push_str(&instruction);
    }

    let request = llm::build_chat_request(
        provider.kind,
        &model,
        Some(system),
        text.to_string(),
        Some(REWRITE_MAX_TOKENS),
        ResponseFormat::Text,
    );

    enforce_cost_cap(provider, &request)?;
    let adapter = llm::adapter_for(provider.kind).map_err(|error| llm::user_message(&error))?;
    let endpoint = endpoint_for(provider).await?;
    let streamed = llm::streaming::stream_chat(adapter.as_ref(), &endpoint, &request)
        .await
        .map_err(|error| llm::user_message(&error))?;

    let content = streamed.text.trim().to_string();
    if content.is_empty() {
        return Err("The provider returned no text.".into());
    }

    // The copy guard runs on the finished text, so a streaming response is
    // checked exactly like a non-streaming one.
    crate::voice::check_for_copy(&content)?;

    Ok(RewriteResult {
        text: content,
        model: if streamed.model.is_empty() {
            model
        } else {
            streamed.model
        },
        input_tokens: streamed
            .usage
            .as_ref()
            .map_or(0, |usage| usage.input_tokens),
        output_tokens: streamed
            .usage
            .as_ref()
            .map_or(0, |usage| usage.output_tokens),
        cost_usd: streamed.usage.as_ref().and_then(|usage| usage.cost_usd()),
    })
}

pub async fn test_key_for(provider: &ProviderConfig) -> Result<OpenRouterStatus, String> {
    let key = load_key_optional(provider)?;
    if provider.kind == ProviderKind::OpenRouter && key.is_none() {
        return Err("No OpenRouter key is saved. Add one first.".into());
    }

    let mut request = llm::http::client()?.get(provider.models_endpoint());
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    if provider.kind == ProviderKind::OpenRouter {
        request = request
            .header("HTTP-Referer", llm::http::APP_URL)
            .header("X-Title", llm::http::APP_TITLE);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("Could not reach the provider: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Could not read the provider response: {error}"))?;
    if !status.is_success() {
        // A key check is a network request too, so it is recorded.
        let _ = crate::network_log::record(crate::network_log::NetworkEntry {
            at: crate::network_log::now_seconds(),
            host: crate::network_log::host_of(&provider.models_endpoint()),
            provider: provider.kind.display_name().to_string(),
            model: None,
            kind: crate::network_log::RequestKind::KeyCheck,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: None,
            ok: false,
            status: Some(status.as_u16()),
            error: Some(openrouter_error(status, &body)),
        });
        return Err(openrouter_error(status, &body));
    }

    let _ = crate::network_log::record(crate::network_log::NetworkEntry {
        at: crate::network_log::now_seconds(),
        host: crate::network_log::host_of(&provider.models_endpoint()),
        provider: provider.kind.display_name().to_string(),
        model: None,
        kind: crate::network_log::RequestKind::KeyCheck,
        input_tokens: 0,
        output_tokens: 0,
        cost_usd: None,
        ok: true,
        status: Some(status.as_u16()),
        error: None,
    });

    if provider.kind == ProviderKind::OpenRouter {
        let parsed: KeyEnvelope = serde_json::from_str(&body)
            .map_err(|error| format!("OpenRouter returned an unexpected response: {error}"))?;
        return Ok(OpenRouterStatus {
            label: parsed.data.label,
            usage: parsed.data.usage,
            limit: parsed.data.limit,
            is_free_tier: parsed.data.is_free_tier,
        });
    }

    Ok(OpenRouterStatus {
        label: Some(provider.base_url.clone()),
        usage: None,
        limit: None,
        is_free_tier: false,
    })
}

/// Rewrite the text with the provider.
///
/// The custom instruction is sanitized and wrapped as data, so it cannot be used
/// to fake a prompt boundary.
pub async fn rewrite_with(
    provider: &ProviderConfig,
    text: &str,
    model: Option<String>,
    voice_guidance: Option<String>,
    tone: RewriteTone,
    custom_instruction: Option<&str>,
) -> Result<RewriteResult, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter some text to rewrite first.".into());
    }

    let model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    let custom_directive = custom_instruction.and_then(sanitize_rewrite_directive);
    let (system, user_message) = if let Some(directive) = custom_directive {
        let mut sys = CUSTOM_REWRITE_SYSTEM_PROMPT.to_string();
        if let Some(guidance) = tone.guidance() {
            sys.push_str("\n\nAdditional tone preference: ");
            sys.push_str(guidance);
        }
        let user_msg = format!(
            "<source_text>\n{text}\n</source_text>\n\n\
             <rewrite_instruction>\n{directive}\n</rewrite_instruction>\n\n\
             Rewrite the <source_text> above so that it fully embodies: \"{directive}\". \
             Do not merely paraphrase sentence-by-sentence—transform the vocabulary, imagery, reading level, and structure to genuinely match the request, and fix any spelling or grammar mistakes from the original. Return only the rewritten text."
        );
        (sys, user_msg)
    } else {
        let mut sys = SYSTEM_PROMPT.to_string();
        if let Some(guidance) = voice_guidance {
            sys.push_str("\n\n");
            sys.push_str(&guidance);
        }
        if let Some(guidance) = tone.guidance() {
            sys.push_str("\n\n");
            sys.push_str(guidance);
        }
        (sys, text.to_string())
    };

    // `temperature` is deliberately left unset: the Claude 5 family rejects
    // any value other than 1, and omitting the key is valid for every model.
    let request = llm::build_chat_request(
        provider.kind,
        &model,
        Some(system),
        user_message,
        Some(REWRITE_MAX_TOKENS),
        ResponseFormat::Text,
    );

    let response = send_request(provider, &request).await?;
    let mut content = response.text.trim().to_string();
    if let Some(inner) = content
        .strip_prefix("<source_text>")
        .and_then(|s| s.strip_suffix("</source_text>"))
    {
        content = inner.trim().to_string();
    }
    content = strip_unrequested_markdown(text, custom_instruction, &content);
    if content.is_empty() {
        return Err("The provider returned no text.".into());
    }

    // Voice copy guard: refuse output that repeats the writer's own sample
    // content. The guard lives here rather than at each call site so every
    // provider path fails closed, not just the system-wide rewrite action.
    // See docs/VOICE-LEARNING.md §11.
    crate::voice::check_for_copy(&content)?;

    Ok(RewriteResult {
        text: content,
        model: model_name(&response, &request),
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        cost_usd: response.usage.cost_usd(),
    })
}

#[derive(Debug, Deserialize)]
struct KeyEnvelope {
    data: KeyData,
}

#[derive(Debug, Deserialize)]
struct KeyData {
    label: Option<String>,
    usage: Option<f64>,
    limit: Option<f64>,
    #[serde(default)]
    is_free_tier: bool,
}

/// The model to report for a completed request.
///
/// Providers normally echo the model they used. A few OpenAI-compatible
/// endpoints omit it, so the requested id is used as the fallback and the
/// usage log never records a blank model.
fn model_name(response: &elocutio_llm::ChatResponse, request: &ChatRequest) -> String {
    let reported = response.model.trim();
    if reported.is_empty() {
        request.model.model.clone()
    } else {
        reported.to_string()
    }
}

/// The cost cap from the saved settings.
///
/// It is enforced where every request leaves the app, so no path (live
/// analysis, the overlay's rewrite, a future command) can skip it.
fn active_cost_cap() -> &'static std::sync::Mutex<Option<f64>> {
    static CAP: std::sync::OnceLock<std::sync::Mutex<Option<f64>>> = std::sync::OnceLock::new();
    CAP.get_or_init(Default::default)
}

/// Records the cost cap from the configuration; called whenever it loads or saves.
pub fn set_active_cost_cap(cap: Option<f64>) {
    *active_cost_cap()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = cap;
}

/// True when a URL points at this Mac (`localhost`, `127.0.0.1`, `[::1]`).
pub fn is_loopback_url(url: &str) -> bool {
    let authority = crate::network_log::host_of(url).to_ascii_lowercase();
    let host = match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Refuses a request whose estimated cost is over the cap.
///
/// The estimate covers the whole prompt (system prompt, voice guidance and
/// user text), not just the user's text. A loopback endpoint (Ollama, the
/// Apple bridge) costs nothing, so it is never refused.
fn enforce_cost_cap(provider: &ProviderConfig, request: &ChatRequest) -> Result<(), String> {
    if is_loopback_url(&provider.chat_endpoint()) {
        return Ok(());
    }
    let cap = *active_cost_cap()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let input_chars = request
        .system
        .as_ref()
        .map_or(0, |system| system.chars().count())
        + request
            .messages
            .iter()
            .map(|message| message.content.chars().count())
            .sum::<usize>();
    check_cost_cap(
        &request.model.model,
        input_chars,
        request.max_output_tokens.unwrap_or(REWRITE_MAX_TOKENS),
        cap,
    )
}

/// Why a request did not produce a response.
enum RequestFailure {
    /// Refused before it was sent: no key, or over the cost cap.
    Local(String),
    /// The provider (or the network) failed, normalized to the taxonomy.
    Provider(ProviderError),
}

impl RequestFailure {
    fn into_message(self) -> String {
        match self {
            RequestFailure::Local(message) => message,
            RequestFailure::Provider(error) => llm::user_message(&error),
        }
    }
}

/// Send a canonical request through the provider's adapter.
async fn send_request(
    provider: &ProviderConfig,
    request: &ChatRequest,
) -> Result<elocutio_llm::ChatResponse, String> {
    send_request_raw(provider, request)
        .await
        .map_err(RequestFailure::into_message)
}

/// [`send_request`], keeping the normalized error so a caller can decide
/// what to do about its kind.
async fn send_request_raw(
    provider: &ProviderConfig,
    request: &ChatRequest,
) -> Result<elocutio_llm::ChatResponse, RequestFailure> {
    enforce_cost_cap(provider, request).map_err(RequestFailure::Local)?;
    let adapter = llm::adapter_for(provider.kind).map_err(RequestFailure::Provider)?;
    let endpoint = endpoint_for(provider)
        .await
        .map_err(RequestFailure::Local)?;
    llm::http::send_chat(adapter.as_ref(), &endpoint, request)
        .await
        .map_err(RequestFailure::Provider)
}

/// Structured analysis with a custom instruction and a voice style card.
///
/// The instruction and the style card are both sanitized and both marked as
/// data, so neither can turn into a prompt boundary.
pub async fn analyze_with_context(
    provider: &ProviderConfig,
    paragraph_id: &str,
    paragraph: &str,
    model: Option<String>,
    dialect: &str,
    custom_instruction: Option<&str>,
) -> Result<AnalyzeCompletion, String> {
    let model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    let mut system = elocutio_analysis::system_prompt().to_string();
    let voice_guidance = crate::voice::guidance();
    if let Some(guidance) = &voice_guidance {
        system.push_str("\n\n");
        system.push_str(guidance);
    }

    let user_prompt = elocutio_analysis::build_user_prompt_with_context(
        paragraph_id,
        paragraph,
        dialect,
        custom_instruction,
        voice_guidance.as_deref(),
    );

    // Try strict JSON-schema structured output first; if the upstream model/provider
    // rejects `response_format: json_schema`, fall back to plain text JSON output.
    let request = llm::build_chat_request(
        provider.kind,
        &model,
        Some(system.clone()),
        user_prompt.clone(),
        Some(ANALYSIS_MAX_TOKENS),
        ResponseFormat::JsonSchema {
            name: "suggestions".to_string(),
            schema: elocutio_analysis::json_schema(),
            strict: true,
        },
    );

    // Fall back only when the structured request came back empty or was
    // rejected as a request the endpoint does not support. A rate limit, a bad
    // key, or an outage would fail the same way again, so retrying it as text
    // would only double the attempts (and, for a timeout, the bill).
    let response = match send_request_raw(provider, &request).await {
        Ok(resp) if !resp.text.trim().is_empty() => Some(resp),
        Ok(_) => None,
        Err(RequestFailure::Provider(error))
            if matches!(
                error.kind,
                ErrorKind::InvalidRequest | ErrorKind::Unsupported
            ) =>
        {
            None
        }
        Err(failure) => return Err(failure.into_message()),
    };
    let response = match response {
        Some(resp) => resp,
        None => {
            let fallback_request = llm::build_chat_request(
                provider.kind,
                &model,
                Some(system),
                user_prompt,
                Some(ANALYSIS_MAX_TOKENS),
                ResponseFormat::Text,
            );
            send_request(provider, &fallback_request).await?
        }
    };
    let content = response.text.clone();
    if content.trim().is_empty() {
        return Err("The provider returned no analysis text.".into());
    }

    Ok(AnalyzeCompletion {
        content,
        model: model_name(&response, &request),
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        cost_usd: response.usage.cost_usd(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalyzeCompletion {
    pub content: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
}

// The functions below are the provider layer's public surface for the rest of
// the app. They have no caller yet — the model picker and the error surface
// are wired up in a later milestone — so they are allowed to sit unused.

/// The models Elocutio offers for a provider.
///
/// The list is the curated catalog, not a live query: it is the set of models
/// the app is willing to recommend, with the context window and price needed
/// to show a cost estimate.
#[allow(dead_code)]
pub fn list_models(provider: &ProviderConfig) -> Vec<llm::ModelOption> {
    llm::list_models(provider.kind)
}

/// The models Elocutio offers for a provider kind.
#[allow(dead_code)]
pub fn list_models_for(kind: ProviderKind) -> Vec<llm::ModelOption> {
    llm::list_models(kind)
}

/// Turn a provider failure into the sentence the user sees.
///
/// The taxonomy decides the headline; the provider's own message is appended
/// as a short detail.
#[allow(dead_code)]
pub fn provider_error_message(error: &ProviderError) -> String {
    llm::user_message(error)
}

/// True when a failure is worth retrying.
#[allow(dead_code)]
pub fn is_retryable(error: &ProviderError) -> bool {
    llm::is_retryable(error)
}

/// The user-facing message for an HTTP failure.
///
/// The wording per status is preserved from the original client, and the
/// status is mapped through the shared taxonomy so the app has one place that
/// knows what a 402 means.
fn openrouter_error(status: reqwest::StatusCode, body: &str) -> String {
    let adapter = elocutio_llm::OpenRouterAdapter;
    let error = adapter.map_http_error(status.as_u16(), body);
    let kind = error.kind;
    let detail = error.message.trim();

    match kind {
        ErrorKind::Auth => format!(
            "The provider rejected the API key (HTTP {status}). Check it in Settings. ({detail})"
        ),
        ErrorKind::Quota => format!(
            "The provider says this account is out of credits (HTTP 402). Add credits or use another key. ({detail})"
        ),
        ErrorKind::RateLimit => format!(
            "The provider rate limit was reached (HTTP 429). Wait a moment and try again. ({detail})"
        ),
        ErrorKind::InvalidRequest if status == reqwest::StatusCode::NOT_FOUND => format!(
            "The selected model was not found (HTTP 404). Check the model id in Settings. ({detail})"
        ),
        ErrorKind::InvalidRequest => format!("The provider rejected the request: {detail}"),
        ErrorKind::Overloaded => format!(
            "The provider is having trouble right now (HTTP {status}). Try again shortly. ({detail})"
        ),
        ErrorKind::Server => format!(
            "The provider is having trouble right now (HTTP {status}). Try again shortly. ({detail})"
        ),
        _ => format!("Provider error (HTTP {status}): {detail}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn keys_are_scoped_to_the_endpoint_host() {
        let groq = ProviderConfig {
            kind: ProviderKind::OpenAiCompatible,
            base_url: "https://api.groq.com/openai/v1".to_string(),
        };
        let ollama = ProviderConfig {
            kind: ProviderKind::OpenAiCompatible,
            base_url: "http://localhost:11434/v1".to_string(),
        };
        assert_eq!(key_account(&groq), "api_key@api.groq.com");
        assert_eq!(key_account(&ollama), "api_key@localhost:11434");
        assert_ne!(key_account(&groq), key_account(&ollama));
    }

    #[test]
    fn a_legacy_key_follows_its_prefix_not_the_selected_preset() {
        assert_eq!(
            base_url_for_key_prefix("gsk_abc"),
            Some("https://api.groq.com/openai/v1")
        );
        assert_eq!(
            base_url_for_key_prefix("sk-proj-abc"),
            Some(DEFAULT_OPENAI_BASE_URL)
        );
        assert_eq!(base_url_for_key_prefix("sk-or-v1-abc"), None);
        assert_eq!(base_url_for_key_prefix("ollama-key"), None);
    }

    #[test]
    fn loopback_urls_are_recognized() {
        assert!(is_loopback_url("http://localhost:11434/v1"));
        assert!(is_loopback_url("http://127.0.0.1:11535/v1"));
        assert!(is_loopback_url("http://[::1]:8080/v1"));
        assert!(!is_loopback_url("https://api.openai.com/v1"));
        assert!(!is_loopback_url("http://localhost.evil.example/v1"));
    }

    use super::*;
    use elocutio_llm::{Message, ModelRef};

    #[test]
    fn default_model_is_set() {
        assert!(!DEFAULT_MODEL.is_empty());
        assert!(DEFAULT_MODEL.contains('/'));
    }

    #[test]
    fn only_transient_statuses_are_retried() {
        // The taxonomy is the single source of truth for the retry policy.
        let retryable = |status: reqwest::StatusCode| {
            elocutio_llm::ErrorKind::from_status(status.as_u16()).is_retryable()
        };
        assert!(retryable(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(retryable(reqwest::StatusCode::BAD_GATEWAY));
        assert!(!retryable(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!retryable(reqwest::StatusCode::BAD_REQUEST));
    }

    #[test]
    fn tone_guidance_is_optional_and_specific() {
        assert!(RewriteTone::Neutral.guidance().is_none());
        assert!(
            RewriteTone::Confident
                .guidance()
                .unwrap()
                .contains("confident")
        );
        assert!(RewriteTone::Concise.guidance().unwrap().contains("concise"));
    }

    #[test]
    fn provider_endpoints_are_built_from_the_base_url() {
        let openrouter = ProviderConfig {
            kind: ProviderKind::OpenRouter,
            base_url: "https://api.openai.com/v1".into(),
        };
        assert_eq!(openrouter.chat_endpoint(), CHAT_URL);
        assert_eq!(openrouter.models_endpoint(), KEY_URL);

        let compatible = ProviderConfig {
            kind: ProviderKind::OpenAiCompatible,
            base_url: "https://api.openai.com/v1/".into(),
        };
        assert_eq!(
            compatible.chat_endpoint(),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            compatible.models_endpoint(),
            "https://api.openai.com/v1/models"
        );
    }

    #[test]
    fn openrouter_error_extracts_message() {
        let message = openrouter_error(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"No auth credentials found","code":401}}"#,
        );
        assert!(message.contains("No auth credentials found"));
        assert!(message.contains("401"));
    }

    #[test]
    fn openrouter_error_explains_auth_and_quota() {
        let auth = openrouter_error(reqwest::StatusCode::UNAUTHORIZED, "denied");
        assert!(auth.contains("API key"));
        let quota = openrouter_error(reqwest::StatusCode::PAYMENT_REQUIRED, "no credits");
        assert!(quota.contains("out of credits"));
        let rate = openrouter_error(reqwest::StatusCode::TOO_MANY_REQUESTS, "slow down");
        assert!(rate.contains("rate limit"));
        let model = openrouter_error(reqwest::StatusCode::NOT_FOUND, "missing");
        assert!(model.contains("model was not found"));
    }

    #[test]
    fn openrouter_error_falls_back_to_body() {
        let message = openrouter_error(reqwest::StatusCode::BAD_GATEWAY, "upstream failed");
        assert!(message.contains("upstream failed"));
    }

    #[test]
    fn the_rewrite_body_has_no_temperature_key() {
        // The Claude 5 family rejects any `temperature` other than 1, so the
        // key must be absent from the wire format for the default model.
        let request = ChatRequest::new(
            ModelRef::new(ProviderId::OpenRouter, DEFAULT_MODEL),
            vec![Message::user("Proofread this.")],
        )
        .with_system("You are a precise copy editor.")
        .with_max_output_tokens(REWRITE_MAX_TOKENS);
        assert_eq!(request.temperature, None);

        let adapter = elocutio_llm::OpenRouterAdapter;
        let body = adapter.build_request(&request);
        let object = body.as_object().expect("the body is an object");
        assert!(
            !object.contains_key("temperature"),
            "the default model rejects a temperature key"
        );
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["max_tokens"], REWRITE_MAX_TOKENS);
    }

    #[test]
    fn the_wire_format_is_unchanged_except_for_the_missing_temperature() {
        // The pre-change client sent exactly these keys. The only difference
        // is that `temperature` is gone, which is the fix.
        let request = ChatRequest::new(
            ModelRef::new(ProviderId::OpenRouter, DEFAULT_MODEL),
            vec![Message::user("Proofread this.")],
        )
        .with_system("You are a precise copy editor.")
        .with_max_output_tokens(REWRITE_MAX_TOKENS);

        let body = elocutio_llm::OpenRouterAdapter.build_request(&request);
        let keys: Vec<&str> = body
            .as_object()
            .expect("the body is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert!(keys.contains(&"model"));
        assert!(keys.contains(&"messages"));
        assert!(keys.contains(&"max_tokens"));
        assert!(keys.contains(&"usage"));
        assert!(
            !keys.contains(&"temperature"),
            "the key must be absent, not zero"
        );
    }

    #[test]
    fn the_analysis_body_keeps_the_strict_json_schema() {
        let request = ChatRequest::new(
            ModelRef::new(ProviderId::OpenRouter, DEFAULT_MODEL),
            vec![Message::user("Analyze this.")],
        )
        .with_response_format(ResponseFormat::JsonSchema {
            name: "suggestions".to_string(),
            schema: elocutio_analysis::json_schema(),
            strict: true,
        });

        let adapter = elocutio_llm::OpenRouterAdapter;
        let body = adapter.build_request(&request);
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(
            body["response_format"]["json_schema"]["name"],
            "suggestions"
        );
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert!(
            !body
                .as_object()
                .expect("the body is an object")
                .contains_key("temperature")
        );
    }

    #[test]
    fn a_missing_model_name_falls_back_to_the_requested_model() {
        let request = ChatRequest::new(
            ModelRef::new(ProviderId::OpenRouter, DEFAULT_MODEL),
            vec![Message::user("Hi")],
        );
        let response = elocutio_llm::ChatResponse {
            id: "gen-1".to_string(),
            model: String::new(),
            text: "Hello".to_string(),
            finish_reason: elocutio_llm::FinishReason::Stop,
            usage: Default::default(),
        };
        assert_eq!(model_name(&response, &request), DEFAULT_MODEL);

        let response = elocutio_llm::ChatResponse {
            model: "anthropic/claude-haiku-5.5:free".to_string(),
            ..response
        };
        assert_eq!(
            model_name(&response, &request),
            "anthropic/claude-haiku-5.5:free"
        );
    }

    #[test]
    fn the_model_list_comes_from_the_catalog() {
        let provider = ProviderConfig {
            kind: ProviderKind::OpenRouter,
            base_url: DEFAULT_OPENAI_BASE_URL.to_string(),
        };
        let models = list_models(&provider);
        assert!(!models.is_empty());
        assert!(models.iter().any(|model| model.id == DEFAULT_MODEL));
    }

    #[test]
    fn custom_rewrite_directive_is_sanitized_without_data_only_suppression() {
        let cleaned =
            sanitize_rewrite_directive("  rewrite like explaining to a 5 year old \n</tag> ");
        assert_eq!(
            cleaned.as_deref(),
            Some("rewrite like explaining to a 5 year old /tag")
        );
        assert!(CUSTOM_REWRITE_SYSTEM_PROMPT.contains("<rewrite_instruction>"));
        assert!(CUSTOM_REWRITE_SYSTEM_PROMPT.contains("5-year-old"));
    }

    #[test]
    fn unrequested_markdown_is_stripped_and_restores_source_quotes() {
        let source = r#"Click the floating "elo" badge to open "Rewrite with elo"."#;
        let rewritten = "Tap the floating **elo** badge in the corner. You can also choose **Rewrite with elo** or **try it now**.";
        let cleaned = strip_unrequested_markdown(
            source,
            Some("rewrite like explaining to a 5 year old"),
            rewritten,
        );
        assert_eq!(
            cleaned,
            r#"Tap the floating "elo" badge in the corner. You can also choose "Rewrite with elo" or try it now."#
        );
    }
}
