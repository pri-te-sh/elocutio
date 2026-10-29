#![forbid(unsafe_code)]

//! A curated, static catalog of the models Elocutio offers.
//!
//! The catalog is intentionally small and hand-checked rather than a live
//! mirror of a provider's model list: it is the curated set the app is willing
//! to offer in a picker, with the context window and price needed to show a
//! cost estimate before a long analysis.
//!
//! Prices are **approximate** USD per 1M tokens and are only a planning aid.
//! The provider's own reported cost always wins at runtime.

use serde::Serialize;

use crate::{ModelRef, ProviderId};

/// The model Elocutio uses when the user has not chosen one.
pub const DEFAULT_MODEL_ID: &str = "anthropic/claude-haiku-5.5";

/// A model Elocutio knows about.
///
/// The catalog is a compile-time static table, so it is only ever serialized
/// outward (to the app) and never deserialized.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CatalogEntry {
    /// The provider-native model id, e.g. `anthropic/claude-haiku-5.5`.
    pub id: &'static str,
    /// The name to show in a picker, e.g. `Claude Haiku 5.5`.
    pub display_name: &'static str,
    /// The providers that serve this model.
    ///
    /// OpenRouter serves the canonical id (`anthropic/claude-haiku-5.5`); an
    /// OpenAI-compatible endpoint serves the same family by its short id
    /// (`claude-haiku-5.5`), because those endpoints do not accept a vendor
    /// prefix.
    pub providers: &'static [ProviderId; 2],
    /// The context window in tokens.
    pub context_window: u32,
    /// Approximate USD per 1M input tokens.
    pub price_input_per_million_usd: f64,
    /// Approximate USD per 1M output tokens.
    pub price_output_per_million_usd: f64,
    /// One line on what this model is good for.
    pub best_for: &'static str,
}

impl CatalogEntry {
    /// The catalog id without any provider prefix, e.g. `claude-haiku-5.5`.
    pub fn short_id(&self) -> &str {
        self.id.rsplit('/').next().unwrap_or(self.id)
    }

    /// True when this provider serves the model.
    pub fn serves_provider(&self, provider: ProviderId) -> bool {
        self.providers.contains(&provider)
    }

    /// True when this model accepts a `temperature` parameter.
    ///
    /// The Claude 5 family rejects any `temperature` other than `1` with an
    /// HTTP 400, so the parameter must be omitted entirely for those models.
    pub fn supports_temperature(&self) -> bool {
        supports_temperature(self.id)
    }

    /// The approximate cost of one call, in USD, for the given token counts.
    pub fn estimate_cost_usd(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        let input = input_tokens as f64 / 1_000_000.0 * self.price_input_per_million_usd;
        let output = output_tokens as f64 / 1_000_000.0 * self.price_output_per_million_usd;
        input + output
    }
}

/// Every curated model, in the order a picker should show them.
///
/// Ordering is by recommendation: the default first, then the cheap and
/// reliable options, then the escalation targets.
pub static CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "anthropic/claude-haiku-5.5",
        display_name: "Claude Haiku 5.5",
        providers: &[ProviderId::OpenRouter, ProviderId::OpenAiCompatible],
        context_window: 1_000_000,
        price_input_per_million_usd: 0.10,
        price_output_per_million_usd: 0.50,
        best_for: "Default: fast, cheap, and strong enough for everyday proofreading.",
    },
    CatalogEntry {
        id: "deepseek/deepseek-v4.1-flash",
        display_name: "DeepSeek V4.1 Flash",
        providers: &[ProviderId::OpenRouter, ProviderId::OpenAiCompatible],
        context_window: 1_000_000,
        price_input_per_million_usd: 0.034,
        price_output_per_million_usd: 0.08,
        best_for: "Budget mode: the cheapest credible option, text only.",
    },
    CatalogEntry {
        id: "openai/gpt-5-mini",
        display_name: "GPT-5 mini",
        providers: &[ProviderId::OpenRouter, ProviderId::OpenAiCompatible],
        context_window: 400_000,
        price_input_per_million_usd: 0.25,
        price_output_per_million_usd: 2.00,
        best_for: "Reliable structured output when you want a second opinion.",
    },
    CatalogEntry {
        id: "anthropic/claude-sonnet-5",
        display_name: "Claude Sonnet 5",
        providers: &[ProviderId::OpenRouter, ProviderId::OpenAiCompatible],
        context_window: 1_000_000,
        price_input_per_million_usd: 2.00,
        price_output_per_million_usd: 10.00,
        best_for: "Escalation: stronger writing quality for rewrites and long documents.",
    },
    CatalogEntry {
        id: "openai/gpt-5.6-sol",
        display_name: "GPT-5.6 sol",
        providers: &[ProviderId::OpenRouter, ProviderId::OpenAiCompatible],
        context_window: 1_050_000,
        price_input_per_million_usd: 4.00,
        price_output_per_million_usd: 20.00,
        best_for: "Frontier quality for a deep review; use sparingly.",
    },
];

/// Look a model up by its full catalog id.
pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    let id = id.trim();
    CATALOG.iter().find(|entry| entry.id == id)
}

/// Look a model up by a model id, tolerating a missing provider prefix.
///
/// `claude-haiku-5.5` and `anthropic/claude-haiku-5.5` resolve to the same
/// entry, which is how people actually type model ids.
pub fn find_loose(id: &str) -> Option<&'static CatalogEntry> {
    let id = id.trim();
    find(id).or_else(|| {
        let short = id.rsplit('/').next().unwrap_or(id);
        CATALOG.iter().find(|entry| entry.short_id() == short)
    })
}

/// Every model offered by a provider.
pub fn for_provider(provider: ProviderId) -> Vec<&'static CatalogEntry> {
    CATALOG
        .iter()
        .filter(|entry| entry.serves_provider(provider))
        .collect()
}

/// The model to use when the user has not chosen one.
pub fn default_model() -> &'static CatalogEntry {
    find(DEFAULT_MODEL_ID).expect("the default model is in the catalog")
}

/// The default model as a [`ModelRef`] for a provider.
///
/// OpenRouter serves the default by its canonical id. An OpenAI-compatible
/// endpoint serves the same family by its short id, because those endpoints do
/// not accept the `vendor/` prefix.
pub fn default_model_ref(provider: ProviderId) -> ModelRef {
    let entry = default_model();
    let model = match provider {
        ProviderId::OpenRouter => entry.id.to_string(),
        _ => entry.short_id().to_string(),
    };
    ModelRef::new(provider, model)
}

/// True when the model accepts a `temperature` parameter.
///
/// The whole Claude 5 family (Haiku, Sonnet, Opus, Fable) accepts only
/// `temperature: 1` and returns HTTP 400 for anything else, so the parameter
/// must be left out of the request entirely. Everything else in the catalog
/// takes an optional `temperature` in the usual `0..=2` range.
///
/// The id is matched by family, not against a fixed list, so provider
/// prefixes (`anthropic/`, `us.anthropic.`), variant suffixes (`:thinking`),
/// point releases, and dated snapshots are all covered.
pub fn supports_temperature(model_id: &str) -> bool {
    let id = model_id.trim().to_ascii_lowercase();
    // Drop a `:variant` suffix, then any `vendor/` prefix.
    let id = id.split(':').next().unwrap_or_default();
    let id = id.rsplit('/').next().unwrap_or_default();
    // Drop a dotted provider prefix such as Bedrock's `us.anthropic.`.
    let id = id.find("claude-").map_or(id, |start| &id[start..]);
    !is_claude_five(id)
}

/// Is `id` (already stripped of prefixes and suffixes) a Claude 5 model?
///
/// Matches `claude-<tier>-5` followed by the end of the id or a version or
/// snapshot separator (`claude-sonnet-5`, `claude-haiku-5.5`,
/// `claude-opus-5-20260101`), and the `claude-5…` naming.
fn is_claude_five(id: &str) -> bool {
    if id.starts_with("claude-5") {
        return true;
    }
    ["haiku", "sonnet", "opus", "fable"].iter().any(|tier| {
        id.strip_prefix("claude-")
            .and_then(|rest| rest.strip_prefix(tier))
            .and_then(|rest| rest.strip_prefix("-5"))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '-', '@']))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_ids_are_unique() {
        let mut seen = HashSet::new();
        for entry in CATALOG {
            assert!(seen.insert(entry.id), "duplicate catalog id: {}", entry.id);
        }
    }

    #[test]
    fn the_default_model_is_present_and_is_the_first_entry() {
        let default = default_model();
        assert_eq!(default.id, DEFAULT_MODEL_ID);
        assert_eq!(CATALOG[0].id, DEFAULT_MODEL_ID);
    }

    #[test]
    fn every_entry_has_a_usable_description() {
        for entry in CATALOG {
            assert!(!entry.display_name.is_empty(), "{} has no name", entry.id);
            assert!(!entry.best_for.is_empty(), "{} has no blurb", entry.id);
            assert!(entry.best_for.len() < 120, "{} blurb is too long", entry.id);
        }
    }

    #[test]
    fn prices_are_sane() {
        for entry in CATALOG {
            assert!(
                entry.price_input_per_million_usd > 0.0,
                "{} has no input price",
                entry.id
            );
            assert!(
                entry.price_output_per_million_usd > 0.0,
                "{} has no output price",
                entry.id
            );
            assert!(
                entry.price_output_per_million_usd >= entry.price_input_per_million_usd,
                "{} prices output below input",
                entry.id
            );
            assert!(
                entry.price_input_per_million_usd < 100.0,
                "{} input price looks wrong",
                entry.id
            );
            assert!(
                entry.price_output_per_million_usd < 500.0,
                "{} output price looks wrong",
                entry.id
            );
        }
    }

    #[test]
    fn context_windows_are_plausible() {
        for entry in CATALOG {
            assert!(
                entry.context_window >= 8_000,
                "{} has a tiny context window",
                entry.id
            );
            assert!(
                entry.context_window <= 2_000_000,
                "{} has an implausible context window",
                entry.id
            );
        }
    }

    #[test]
    fn find_resolves_exact_ids() {
        let entry = find("anthropic/claude-haiku-5.5").expect("default is in the catalog");
        assert_eq!(entry.display_name, "Claude Haiku 5.5");
        assert!(find("nope/nope").is_none());
    }

    #[test]
    fn find_loose_accepts_a_missing_vendor_prefix() {
        let loose = find_loose("claude-haiku-5.5").expect("short id resolves");
        assert_eq!(loose.id, "anthropic/claude-haiku-5.5");
        assert_eq!(
            find_loose("anthropic/claude-haiku-5.5").unwrap().id,
            loose.id
        );
    }

    #[test]
    fn for_provider_filters_the_catalog() {
        let openrouter = for_provider(ProviderId::OpenRouter);
        assert!(!openrouter.is_empty());
        assert!(
            openrouter
                .iter()
                .all(|entry| entry.serves_provider(ProviderId::OpenRouter))
        );
        assert!(for_provider(ProviderId::Ollama).is_empty());
    }

    #[test]
    fn default_model_ref_uses_the_short_id_off_openrouter() {
        let openrouter = default_model_ref(ProviderId::OpenRouter);
        assert_eq!(openrouter.model, "anthropic/claude-haiku-5.5");
        assert_eq!(openrouter.provider, ProviderId::OpenRouter);

        let compatible = default_model_ref(ProviderId::OpenAiCompatible);
        assert_eq!(compatible.model, "claude-haiku-5.5");
        assert_eq!(compatible.provider, ProviderId::OpenAiCompatible);
    }

    #[test]
    fn cost_estimates_scale_with_tokens() {
        let entry = default_model();
        let one_million = entry.estimate_cost_usd(1_000_000, 0);
        assert!((one_million - 0.10).abs() < 1e-9);
        let both = entry.estimate_cost_usd(1_000_000, 1_000_000);
        assert!((both - 0.60).abs() < 1e-9);
        assert_eq!(entry.estimate_cost_usd(0, 0), 0.0);
    }

    #[test]
    fn claude_five_models_reject_a_temperature() {
        assert!(!supports_temperature("anthropic/claude-haiku-5.5"));
        assert!(!supports_temperature("claude-sonnet-5"));
        assert!(!supports_temperature("anthropic/claude-opus-5.5"));
        assert!(supports_temperature("openai/gpt-5.6-sol"));
        assert!(supports_temperature("deepseek/deepseek-v4.1-flash"));
        assert!(!default_model().supports_temperature());
    }

    #[test]
    fn the_whole_claude_five_family_rejects_a_temperature() {
        for id in [
            "anthropic/claude-fable-5",
            "anthropic/claude-sonnet-5.5",
            "anthropic/claude-haiku-5.5:thinking",
            "claude-opus-5-20260301",
            "us.anthropic.claude-sonnet-5-v1:0",
            "  Anthropic/Claude-Opus-5.5  ",
            "claude-haiku-5",
        ] {
            assert!(!supports_temperature(id), "{id} should reject temperature");
        }
        for id in [
            "anthropic/claude-sonnet-4.5",
            "anthropic/claude-3.5-haiku",
            "claude-opus-50",
            "openai/gpt-5.6-sol",
            "",
        ] {
            assert!(supports_temperature(id), "{id} should accept temperature");
        }
    }
}
