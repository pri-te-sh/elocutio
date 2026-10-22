mod error;
mod integration;

use crate::{PlatformBroker, os_broker::OsBroker};
pub use error::Error;
use harper_core::{
    Dialect, IgnoredLints,
    linting::{FlatConfig, LintGroup},
    spell::{Dictionary, FstDictionary, MergedDictionary, MutableDictionary},
};
use harper_dictionary_wordlist::{load_dict, save_dict};
pub use integration::Integration;
use serde::de::{DeserializeOwned, Error as _};
use std::{fs, io, path::PathBuf, sync::Arc};

/// How many suggestions the card currently has open.
///
/// The highlighter reports this as it works, and the menu bar shows it. It is
/// app state rather than configuration, so it is kept in its own place.
#[derive(Debug, Default)]
pub struct SuggestionCount(std::sync::atomic::AtomicUsize);

/// The shared handle type Tauri uses for managed state.
pub type Shared<T> = std::rc::Rc<T>;

impl SuggestionCount {
    /// The current count.
    pub fn get(&self) -> usize {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Set the current count.
    pub fn set(&self, count: usize) {
        self.0.store(count, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Asks the highlighter to run a card command.
///
/// The command is transient: it lives in the shared configuration in memory
/// and the highlighter fetches it on its fast poll. It is never written to
/// disk, because a keypress is not a setting.
pub async fn set_card_command(app: &tauri::AppHandle, command: CardCommand) -> Result<(), String> {
    use tauri::Manager;
    let state = app.state::<std::sync::Arc<tokio::sync::Mutex<Config>>>();
    let mut config = state.lock().await;
    config.card_command = Some(command);
    Ok(())
}

/// A command for the suggestion card.
///
/// The card lives in the highlighter process, and the card window never takes
/// keyboard focus — activating it would move the caret and break the paste
/// that follows. So the keyboard model is driven from the main process with
/// global hotkeys, and the command travels to the highlighter with the
/// configuration poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardCommand {
    /// Open the card on the first suggestion.
    Focus,
    /// Move to the next suggestion.
    NextSuggestion,
    /// Move to the previous suggestion.
    PreviousSuggestion,
    /// Show the next replacement of the current suggestion.
    NextReplacement,
    /// Show the previous replacement.
    PreviousReplacement,
    /// Apply the shown replacement.
    Accept,
    /// Dismiss the current suggestion.
    Dismiss,
    /// Close the card.
    Close,
}

/// User-controlled app state needed by Tauri commands and the highlighter process.
pub struct Config {
    pub mutable_dictionary: MutableDictionary,
    pub dialect: Dialect,
    pub ignored_lints: IgnoredLints,
    pub lint_config: FlatConfig,
    pub integrations: Vec<Integration>,
    pub auto_enable_new_apps: bool,
    pub onboarding_completed: bool,
    pub debounce_ms: u64,
    /// Unix milliseconds at the start of the last manual or automatic update attempt.
    /// Failed attempts also count toward the automatic 24-hour interval.
    pub last_update_check: Option<u64>,
    pub highlighter_service_enabled: bool,
    /// Provider used by Rewrite Selection and the assistant.
    pub provider_kind: crate::openrouter::ProviderKind,
    /// Base URL for an OpenAI-compatible provider.
    pub provider_base_url: String,
    /// Model id used by Rewrite Selection and the assistant.
    pub provider_model: String,
    /// Analyze the focused field after a typing pause, without a shortcut.
    pub auto_analyze: bool,
    /// Optional spending limit in USD for local usage warnings.
    pub budget_usd: Option<f64>,
    /// The hard ceiling for a single analysis or rewrite, in USD.
    ///
    /// A request whose estimated cost would exceed this is refused before it
    /// is sent. This is the "cost cap" the plan asks for; the softer
    /// `budget_usd` setting only warns after the money is already spent.
    pub cost_cap_usd: Option<f64>,
    /// Whether accepted suggestions may update the local voice profile.
    pub learn_from_edits: bool,
    /// Requested rewrite tone.
    pub rewrite_tone: crate::openrouter::RewriteTone,
    /// A pending command for the highlighter's suggestion card.
    pub card_command: Option<CardCommand>,
    /// How many suggestions the card currently has open.
    ///
    /// The highlighter reports this so the menu bar and the settings window
    /// can say what the card is showing.
    pub card_suggestion_count: usize,
    /// Where the floating widget is pinned, per app.
    ///
    /// An app that is not in the map uses the default position: beside the
    /// first underline, or in the screen corner when there is nothing to
    /// underline.
    pub widget_anchors: crate::widget_anchor::WidgetAnchors,
    /// A free-text instruction the user applies to every request.
    ///
    /// It is sanitized before it reaches a prompt: control characters are
    /// stripped, newlines are collapsed, angle brackets are removed, and it is
    /// length-capped, so it cannot be used to fake a prompt boundary.
    pub custom_instruction: String,
    /// When true, no text may leave the Mac for a provider.
    pub local_only: bool,
    /// Apps Elocutio must never read, even when they are enabled for suggestions.
    ///
    /// A blocklist entry is stronger than a disabled integration: it wins even
    /// when automatic enablement would otherwise turn the app on. This is the
    /// control the privacy docs promise for "apps you exclude".
    pub blocklist: Vec<String>,
}

impl Config {
    pub fn new() -> Self {
        Self {
            mutable_dictionary: MutableDictionary::new(),
            dialect: Self::detect_system_dialect(),
            ignored_lints: IgnoredLints::new(),
            lint_config: FlatConfig::new_curated(),
            integrations: Integration::curated_integrations(),
            auto_enable_new_apps: true,
            onboarding_completed: false,
            debounce_ms: 0,
            last_update_check: None,
            highlighter_service_enabled: true,
            provider_kind: crate::openrouter::ProviderKind::OpenRouter,
            provider_base_url: crate::openrouter::DEFAULT_OPENAI_BASE_URL.to_string(),
            provider_model: crate::openrouter::DEFAULT_MODEL.to_string(),
            auto_analyze: false,
            budget_usd: None,
            cost_cap_usd: None,
            learn_from_edits: false,
            rewrite_tone: crate::openrouter::RewriteTone::Neutral,
            custom_instruction: String::new(),
            card_command: None,
            card_suggestion_count: 0,
            widget_anchors: crate::widget_anchor::WidgetAnchors::default(),
            local_only: false,
            blocklist: Vec::new(),
        }
    }

    fn detect_system_dialect() -> Dialect {
        tauri_plugin_os::locale()
            .and_then(|bcp47| Dialect::try_from_bcp47(&bcp47))
            .unwrap_or(Dialect::American)
    }

    pub fn is_integration_enabled(&self, bundle_id: &str) -> bool {
        let integrations: &[Integration] = &self.integrations;
        integrations
            .iter()
            .any(|integration| integration.bundle_id == bundle_id && integration.enabled)
    }

    /// Whether the app is on the blocklist.
    ///
    /// A blocklisted app is never read, never linted, and never sent to a
    /// provider, even if it is also enabled as an integration.
    pub fn is_blocked(&self, bundle_id: &str) -> bool {
        self.blocklist.iter().any(|blocked| blocked == bundle_id)
    }

    /// Add an app to the blocklist and disable it as an integration.
    ///
    /// Blocking an app must also stop it being enabled, otherwise the two
    /// settings would contradict each other in the UI.
    pub fn block_app(&mut self, bundle_id: &str) {
        let bundle_id = bundle_id.trim();
        if bundle_id.is_empty() {
            return;
        }
        if !self.blocklist.iter().any(|blocked| blocked == bundle_id) {
            self.blocklist.push(bundle_id.to_string());
            self.blocklist.sort();
        }
        self.set_integration_enabled(bundle_id, false);
    }

    /// Remove an app from the blocklist.
    ///
    /// The app stays disabled; the user re-enables it deliberately.
    pub fn unblock_app(&mut self, bundle_id: &str) {
        self.blocklist.retain(|blocked| blocked != bundle_id);
    }

    /// Resolves an app's enabled state, registering unknown apps when automatic enablement is on.
    /// Existing disabled entries are never re-enabled; removed entries can be discovered again.
    /// Elocutio itself is excluded from discovery, but explicitly configured entries are respected.
    /// The caller is responsible for persisting any newly registered integration.
    pub fn resolve_integration(&mut self, bundle_id: &str) -> bool {
        let bundle_id = bundle_id.trim();
        if bundle_id.is_empty() {
            return false;
        }
        // A blocklisted app is never auto-enabled and never read.
        if self.is_blocked(bundle_id) {
            return false;
        }
        if let Some(integration) = self
            .integrations
            .iter()
            .find(|item| item.bundle_id == bundle_id)
        {
            return integration.enabled;
        }
        if !self.auto_enable_new_apps || PlatformBroker::is_elocutio(bundle_id) {
            return false;
        }
        self.add_integration(bundle_id.to_owned());
        true
    }

    pub fn add_integration(&mut self, bundle_id: String) {
        let bundle_id = bundle_id.trim();
        if bundle_id.is_empty()
            || self
                .integrations
                .iter()
                .any(|item| item.bundle_id == bundle_id)
        {
            return;
        }

        self.integrations.push(Integration {
            bundle_id: bundle_id.to_string(),
            enabled: true,
        });
        self.integrations
            .sort_by(|a, b| a.bundle_id.cmp(&b.bundle_id));
    }

    pub fn remove_integration(&mut self, bundle_id: &str) {
        self.integrations
            .retain(|integration| integration.bundle_id != bundle_id);
    }

    pub fn set_integration_enabled(&mut self, bundle_id: &str, enabled: bool) {
        if let Some(integration) = self
            .integrations
            .iter_mut()
            .find(|integration| integration.bundle_id == bundle_id)
        {
            integration.enabled = enabled;
        }
    }

    /// Add newly curated integrations without overwriting existing choices.
    pub fn merge_curated_integrations(&mut self) -> bool {
        let mut changed = false;
        for integration in Integration::curated_integrations() {
            if !self
                .integrations
                .iter()
                .any(|existing| existing.bundle_id == integration.bundle_id)
            {
                self.integrations.push(integration);
                changed = true;
            }
        }
        changed
    }

    pub async fn save_to_system(&self) -> Result<(), Error> {
        #[cfg(test)]
        let folder_path =
            Some(std::env::temp_dir().join("elocutio-test")).ok_or(Error::ConfigDirUnavailable)?;
        #[cfg(not(test))]
        let folder_path = Self::folder_path().ok_or(Error::ConfigDirUnavailable)?;

        let main_path = folder_path.join("config.json");
        let dictionary_path = folder_path.join("user_dictionary.txt");

        fs::create_dir_all(&folder_path)?;
        fs::write(main_path, self.serialize_main()?)?;
        save_dict(&dictionary_path, &self.mutable_dictionary).await?;
        let _ = crate::personalization_db::sync_and_status(self);

        Ok(())
    }

    pub fn main_config_exists() -> Result<bool, Error> {
        let main_path = Self::main_path().ok_or(Error::ConfigDirUnavailable)?;

        match fs::metadata(main_path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn load_from_system() -> Result<Self, Error> {
        let main_path = Self::main_path().ok_or(Error::ConfigDirUnavailable)?;
        let dictionary_path = Self::dictionary_path().ok_or(Error::ConfigDirUnavailable)?;
        let serialized = fs::read_to_string(main_path)?;
        let mut config = Self::deserialize_main(&serialized)?;
        config.lint_config.fill_with_curated();
        let loaded_dict = load_dict(&dictionary_path, config.dialect).await?;
        let mut clean_dict = MutableDictionary::new();
        let mut had_invalid_entries = false;
        for w in loaded_dict.words_iter() {
            let word_str: String = w.iter().collect();
            if crate::personalization_db::is_valid_dictionary_word(&word_str) {
                clean_dict
                    .append_word_str(word_str.trim(), harper_core::DictWordMetadata::default());
            } else {
                had_invalid_entries = true;
            }
        }
        config.mutable_dictionary = clean_dict;
        for db_word in crate::personalization_db::load_dictionary_words_from_db() {
            let trimmed = db_word.trim();
            if crate::personalization_db::is_valid_dictionary_word(trimmed) {
                config
                    .mutable_dictionary
                    .append_word_str(trimmed, harper_core::DictWordMetadata::default());
            }
        }
        if had_invalid_entries {
            let _ = save_dict(&dictionary_path, &config.mutable_dictionary).await;
        }
        if crate::voice::is_learning_enabled() {
            config.learn_from_edits = true;
        }
        let _ = crate::personalization_db::sync_and_status(&config);

        Ok(config)
    }

    pub fn dictionary_from_user_dictionary(
        user_dictionary: MutableDictionary,
    ) -> Arc<MergedDictionary> {
        static BUILTIN_TERMS: std::sync::OnceLock<Arc<MutableDictionary>> =
            std::sync::OnceLock::new();
        let builtin = BUILTIN_TERMS
            .get_or_init(|| {
                let mut dict = MutableDictionary::new();
                for word in [
                    "elo",
                    "Elo",
                    "Elocutio",
                    "Confluence",
                    "confluence",
                    "Jira",
                    "GitHub",
                    "OpenRouter",
                    "Anthropic",
                    "Claude",
                    "ChatGPT",
                    "Supabase",
                    "Postgres",
                    "Kubernetes",
                    "TypeScript",
                    "JavaScript",
                    "Svelte",
                    "Tauri",
                    "upsert",
                    "upserts",
                    "upserted",
                    "webhook",
                    "webhooks",
                    "repo",
                    "repos",
                    "config",
                    "configs",
                    "async",
                    "frontend",
                    "backend",
                    "codebase",
                    "onboarding",
                ] {
                    dict.append_word_str(word, harper_core::DictWordMetadata::default());
                }
                Arc::new(dict)
            })
            .clone();

        let mut dictionary = MergedDictionary::new();
        dictionary.add_dictionary(FstDictionary::curated());
        dictionary.add_dictionary(builtin);
        dictionary.add_dictionary(Arc::new(user_dictionary));

        Arc::new(dictionary)
    }

    fn create_dictionary(&self) -> Arc<MergedDictionary> {
        Self::dictionary_from_user_dictionary(self.mutable_dictionary.clone())
    }

    pub fn create_linter(&self) -> LintGroup {
        LintGroup::new_curated(self.create_dictionary(), self.dialect)
            .with_lint_config(self.lint_config.clone())
    }

    #[allow(dead_code)]
    pub(crate) fn folder_path() -> Option<PathBuf> {
        dirs::config_dir().map(|path| path.join("elocutio"))
    }

    #[allow(dead_code)]
    fn main_path() -> Option<PathBuf> {
        Self::folder_path().map(|path| path.join("config.json"))
    }

    #[allow(dead_code)]
    fn dictionary_path() -> Option<PathBuf> {
        Self::folder_path().map(|path| {
            let user_dict = path.join("user_dictionary.txt");
            if user_dict.exists() {
                user_dict
            } else {
                let legacy = path.join("dictionary.txt");
                if legacy.exists() { legacy } else { user_dict }
            }
        })
    }

    #[allow(dead_code)]
    fn serialize_main(&self) -> serde_json::Result<String> {
        serde_json::to_string(&serde_json::json!({
            "dialect": &self.dialect,
            "ignored_lints": &self.ignored_lints,
            "lint_config": &self.lint_config,
            "integrations": &self.integrations,
            "auto_enable_new_apps": self.auto_enable_new_apps,
            "onboarding_completed": self.onboarding_completed,
            "debounce_ms": self.debounce_ms,
            "last_update_check": self.last_update_check,
            "highlighter_service_enabled": self.highlighter_service_enabled,
            "provider_kind": self.provider_kind,
            "provider_base_url": self.provider_base_url,
            "provider_model": self.provider_model,
            "auto_analyze": self.auto_analyze,
            "budget_usd": self.budget_usd,
            "cost_cap_usd": self.cost_cap_usd,
            "learn_from_edits": self.learn_from_edits,
            "rewrite_tone": self.rewrite_tone,
            "custom_instruction": self.custom_instruction,
            "widget_anchors": self.widget_anchors,
            "local_only": self.local_only,
            "blocklist": self.blocklist,
        }))
    }

    #[allow(dead_code)]
    fn deserialize_main(serialized: &str) -> serde_json::Result<Self> {
        let mut value = serde_json::from_str::<serde_json::Value>(serialized)?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| serde_json::Error::custom("config must be a JSON object"))?;

        Ok(Self {
            mutable_dictionary: MutableDictionary::new(),
            dialect: deserialize_field(object, "dialect")?,
            ignored_lints: deserialize_field(object, "ignored_lints")?,
            lint_config: deserialize_field(object, "lint_config")?,
            integrations: deserialize_optional_field(object, "integrations")?
                .unwrap_or_else(Integration::curated_integrations),
            auto_enable_new_apps: deserialize_optional_field(object, "auto_enable_new_apps")?
                .unwrap_or(true),
            onboarding_completed: deserialize_optional_field(object, "onboarding_completed")?
                .unwrap_or(true),
            debounce_ms: deserialize_optional_field(object, "debounce_ms")?.unwrap_or(0),
            last_update_check: deserialize_optional_field::<Option<u64>>(
                object,
                "last_update_check",
            )?
            .flatten(),
            highlighter_service_enabled: deserialize_optional_field(
                object,
                "highlighter_service_enabled",
            )?
            .unwrap_or(true),
            provider_kind: deserialize_optional_field(object, "provider_kind")?.unwrap_or_default(),
            provider_base_url: deserialize_optional_field(object, "provider_base_url")?
                .unwrap_or_else(|| crate::openrouter::DEFAULT_OPENAI_BASE_URL.to_string()),
            provider_model: deserialize_optional_field(object, "provider_model")?
                .unwrap_or_else(|| crate::openrouter::DEFAULT_MODEL.to_string()),
            auto_analyze: deserialize_optional_field(object, "auto_analyze")?.unwrap_or(false),
            budget_usd: deserialize_optional_field(object, "budget_usd")?.unwrap_or(None),
            cost_cap_usd: deserialize_optional_field(object, "cost_cap_usd")?.unwrap_or(None),
            learn_from_edits: deserialize_optional_field(object, "learn_from_edits")?
                .unwrap_or(false),
            rewrite_tone: deserialize_optional_field(object, "rewrite_tone")?.unwrap_or_default(),
            custom_instruction: deserialize_optional_field(object, "custom_instruction")?
                .unwrap_or_default(),
            // A card command is transient: it is never written to disk, so
            // there is nothing to read back.
            card_command: None,
            card_suggestion_count: 0,
            widget_anchors: deserialize_optional_field(object, "widget_anchors")?
                .unwrap_or_default(),
            local_only: deserialize_optional_field(object, "local_only")?.unwrap_or(false),
            blocklist: deserialize_optional_field(object, "blocklist")?.unwrap_or_default(),
        })
    }
}

#[allow(dead_code)]
fn deserialize_field<T>(
    object: &mut serde_json::Map<String, serde_json::Value>,
    field: &'static str,
) -> serde_json::Result<T>
where
    T: DeserializeOwned,
{
    let value = object
        .remove(field)
        .ok_or_else(|| serde_json::Error::custom(format!("missing config field `{field}`")))?;

    serde_json::from_value(value)
}

#[allow(dead_code)]
fn deserialize_optional_field<T>(
    object: &mut serde_json::Map<String, serde_json::Value>,
    field: &'static str,
) -> serde_json::Result<Option<T>>
where
    T: DeserializeOwned,
{
    let Some(value) = object.remove(field) else {
        return Ok(None);
    };

    serde_json::from_value(value).map(Some)
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, Integration};
    use harper_core::DictWordMetadata;

    #[test]
    fn serialize_main_excludes_dictionary_word_list() {
        let mut config = Config::new();
        config
            .mutable_dictionary
            .append_word_str("blorple", DictWordMetadata::default());

        let serialized = config.serialize_main().unwrap();

        assert!(!serialized.contains("mutable_dictionary"));
        assert!(!serialized.contains("blorple"));
        assert!(serialized.contains("dialect"));
        assert!(serialized.contains("ignored_lints"));
        assert!(serialized.contains("lint_config"));
        assert!(serialized.contains("integrations"));
        assert!(serialized.contains("onboarding_completed"));
        assert!(serialized.contains("debounce_ms"));
        assert!(serialized.contains("last_update_check"));
        assert!(serialized.contains("highlighter_service_enabled"));
        assert!(serialized.contains("auto_analyze"));
        assert!(serialized.contains("budget_usd"));
        assert!(serialized.contains("provider_kind"));
        assert!(serialized.contains("provider_base_url"));
        assert!(serialized.contains("learn_from_edits"));
        assert!(serialized.contains("rewrite_tone"));
        assert!(serialized.contains("local_only"));
    }

    #[test]
    fn deserialize_main_restores_main_serialized_fields() {
        let mut config = Config::new();
        config
            .mutable_dictionary
            .append_word_str("blorple", DictWordMetadata::default());
        let serialized = config.serialize_main().unwrap();

        let deserialized = Config::deserialize_main(&serialized).unwrap();

        assert_eq!(deserialized.dialect, config.dialect);
        assert_eq!(deserialized.lint_config, config.lint_config);
        assert_eq!(deserialized.integrations, config.integrations);
        assert_eq!(
            deserialized.onboarding_completed,
            config.onboarding_completed
        );
        assert_eq!(deserialized.debounce_ms, config.debounce_ms);
        assert_eq!(deserialized.last_update_check, config.last_update_check);
        assert_eq!(
            deserialized.highlighter_service_enabled,
            config.highlighter_service_enabled
        );
        assert_eq!(deserialized.auto_analyze, config.auto_analyze);
        assert_eq!(deserialized.budget_usd, config.budget_usd);
        assert_eq!(deserialized.provider_kind, config.provider_kind);
        assert_eq!(deserialized.provider_base_url, config.provider_base_url);
        assert_eq!(deserialized.learn_from_edits, config.learn_from_edits);
        assert_eq!(deserialized.rewrite_tone, config.rewrite_tone);
        assert_eq!(deserialized.local_only, config.local_only);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&deserialized.serialize_main().unwrap())
                .unwrap(),
            serde_json::from_str::<serde_json::Value>(&serialized).unwrap()
        );
    }

    #[test]
    fn deserialize_main_uses_zero_debounce_when_missing() {
        let config = Config::new();
        let mut value =
            serde_json::from_str::<serde_json::Value>(&config.serialize_main().unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("debounce_ms");

        let deserialized = Config::deserialize_main(&value.to_string()).unwrap();

        assert_eq!(deserialized.debounce_ms, 0);
    }

    #[test]
    fn deserialize_main_uses_default_update_settings_when_missing() {
        let config = Config::new();
        let mut value =
            serde_json::from_str::<serde_json::Value>(&config.serialize_main().unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("last_update_check");

        let deserialized = Config::deserialize_main(&value.to_string()).unwrap();

        assert_eq!(deserialized.last_update_check, None);
    }

    #[test]
    fn deserialize_main_enables_highlighter_service_when_missing() {
        let config = Config::new();
        let mut value =
            serde_json::from_str::<serde_json::Value>(&config.serialize_main().unwrap()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("highlighter_service_enabled");

        let deserialized = Config::deserialize_main(&value.to_string()).unwrap();

        assert!(deserialized.highlighter_service_enabled);
    }

    #[test]
    fn deserialize_main_preserves_integrations() {
        let mut config = Config::new();
        config.integrations = vec![Integration {
            bundle_id: "com.example.Editor".to_string(),
            enabled: false,
        }];

        let deserialized = Config::deserialize_main(&config.serialize_main().unwrap()).unwrap();

        assert_eq!(
            deserialized.integrations,
            vec![Integration {
                bundle_id: "com.example.Editor".to_string(),
                enabled: false,
            }]
        );
    }

    #[test]
    fn integration_helpers_add_remove_enable_and_check() {
        let mut config = Config::new();

        config.add_integration(" com.example.Editor ".to_string());
        config.add_integration("com.example.Editor".to_string());
        assert!(config.is_integration_enabled("com.example.Editor"));
        assert_eq!(
            config
                .integrations
                .iter()
                .filter(|integration| integration.bundle_id == "com.example.Editor")
                .count(),
            1
        );

        config.set_integration_enabled("com.example.Editor", false);
        assert!(!config.is_integration_enabled("com.example.Editor"));

        config.remove_integration("com.example.Editor");

        assert!(
            !config
                .integrations
                .iter()
                .any(|integration| integration.bundle_id == "com.example.Editor")
        );
    }

    #[test]
    fn main_path_points_to_harper_desktop_config_file() {
        let path = Config::main_path().unwrap();

        assert_eq!(path.file_name().unwrap(), "config.json");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "elocutio");
    }

    #[test]
    fn main_config_exists_reports_missing_file() {
        let exists = Config::main_config_exists().unwrap();
        let expected = Config::main_path().unwrap().exists();

        assert_eq!(exists, expected);
    }

    #[test]
    fn dictionary_path_points_to_harper_desktop_dictionary_file() {
        let path = Config::dictionary_path().unwrap();

        assert_eq!(path.file_name().unwrap(), "user_dictionary.txt");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "elocutio");
    }
}
