//! Resolution logic for the `/model` slash command.
//!
//! Pure functions — no I/O, no async, no state. Independently testable.

use crate::config::default_model_for_provider;
use crate::llm::models::{self, Model, all_providers, find_models_by_id};

// ── Outcome type ─────────────────────────────────────────────────────────

/// Outcome of resolving a `/model <arg>` string.
#[derive(Debug, PartialEq, Eq)]
pub enum ModelSwitch {
    /// Successfully resolved to a provider + model.
    Switch {
        provider: String,
        model_id: String,
        model_info: Model,
    },
    /// No argument — show the catalog and current model.
    ShowCatalog,
    /// Model name matched multiple providers.
    Ambiguous {
        name: String,
        candidates: Vec<Model>,
    },
    /// Provider or model not found in the catalog.
    NotFound { arg: String, reason: String },
    /// Provider is known in the catalog but not yet implemented.
    ProviderNotImplemented { provider: String },
}

// ── Resolution ───────────────────────────────────────────────────────────

/// Resolve a `/model <arg>` string into a [`ModelSwitch`].
///
/// Resolution order:
/// 1. Empty / whitespace-only arg → `ShowCatalog`.
/// 2. Arg contains `/` → split into `provider/model`.
///    - Validate provider exists in the catalog.
///    - Validate model exists for that provider.
///    - Check whether the provider is implemented.
/// 3. Arg matches a known provider name → switch provider, use the
///    provider's default model.
/// 4. Arg matches a model ID in exactly one provider → switch to that
///    provider + model.
/// 5. Arg matches a model ID in multiple providers → `Ambiguous`.
/// 6. Nothing matches → `NotFound`.
pub fn resolve_model_switch(arg: &str) -> ModelSwitch {
    let arg = arg.trim();
    if arg.is_empty() {
        return ModelSwitch::ShowCatalog;
    }

    // 2. provider/model
    if let Some((provider, model)) = arg.split_once('/') {
        let provider = provider.trim();
        let model = model.trim();
        if provider.is_empty() || model.is_empty() {
            return ModelSwitch::NotFound {
                arg: arg.to_string(),
                reason: "expected format: provider/model (e.g. openai/gpt-4o)".into(),
            };
        }

        // Validate provider existence in the catalog.
        if !all_providers().contains(&provider) {
            return ModelSwitch::NotFound {
                arg: arg.to_string(),
                reason: format!("unknown provider '{provider}'"),
            };
        }

        // Check implementation.
        if !is_provider_implemented(provider) {
            return ModelSwitch::ProviderNotImplemented {
                provider: provider.to_string(),
            };
        }

        // Validate model for this provider.
        return match models::find_model(provider, model) {
            Some(model_info) => ModelSwitch::Switch {
                provider: provider.to_string(),
                model_id: model.to_string(),
                model_info,
            },
            None => ModelSwitch::NotFound {
                arg: arg.to_string(),
                reason: format!("model '{model}' not found for provider '{provider}'"),
            },
        };
    }

    // 3. Arg is a provider name.
    if all_providers().contains(&arg) {
        if !is_provider_implemented(arg) {
            return ModelSwitch::ProviderNotImplemented {
                provider: arg.to_string(),
            };
        }
        let model_id = default_model_for_provider(arg);
        // By construction, the default model exists for the provider.
        let model_info =
            models::find_model(arg, &model_id).expect("default model must exist in catalog");
        return ModelSwitch::Switch {
            provider: arg.to_string(),
            model_id,
            model_info,
        };
    }

    // 4-5. Search for the model ID across all providers.
    let candidates = find_models_by_id(arg);
    match candidates.len() {
        0 => ModelSwitch::NotFound {
            arg: arg.to_string(),
            reason: format!("unknown model or provider '{arg}' — use /model to browse"),
        },
        1 => {
            let m = &candidates[0];
            if !is_provider_implemented(&m.provider) {
                return ModelSwitch::ProviderNotImplemented {
                    provider: m.provider.clone(),
                };
            }
            ModelSwitch::Switch {
                provider: m.provider.clone(),
                model_id: m.id.clone(),
                model_info: m.clone(),
            }
        }
        _ => ModelSwitch::Ambiguous {
            name: arg.to_string(),
            candidates,
        },
    }
}

/// Returns `true` if the provider is implemented (not a stub returning
/// `KonError::Provider("... not yet implemented"))`.
///
/// When a new provider graduates from stub to real implementation, add
/// its name here to make it selectable via `/model`.
pub(crate) fn is_provider_implemented(provider_name: &str) -> bool {
    matches!(
        provider_name,
        "mock" | "openai" | "deepseek" | "zhipu" | "github-copilot" | "openrouter"
    )
}

// ── Formatting ───────────────────────────────────────────────────────────

/// Format the catalog for display when `/model` is invoked with no arguments.
pub fn format_catalog() -> String {
    let providers = all_providers();
    let mut out = String::new();
    for provider in &providers {
        let models = models::models_for_provider(provider);
        if models.is_empty() {
            continue;
        }
        let names: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        let implemented = if is_provider_implemented(provider) {
            ""
        } else {
            " (not yet implemented)"
        };
        out.push_str(&format!(
            "  {provider}{implemented}: {}\n",
            names.join(", ")
        ));
    }
    out
}

/// Format an individual model for the switch result.
fn format_model_info(m: &Model) -> String {
    let mut parts = Vec::new();
    parts.push(format!("Context: {}", format_tokens(m.context_window)));
    parts.push(format!(
        "Vision: {}",
        if m.supports_vision { "yes" } else { "no" }
    ));
    if m.supports_thinking && !m.thinking_levels.is_empty() {
        let levels: Vec<&str> = m
            .thinking_levels
            .iter()
            .filter(|l| **l != crate::llm::base::ThinkingLevel::None)
            .map(|l| thinking_level_to_short(l))
            .collect();
        if !levels.is_empty() {
            parts.push(format!("Thinking: {}", levels.join("/")));
        }
    }
    let joined = parts.join(" | ");
    format!("  {joined}")
}

/// Format the outcome of a model switch for display in the chat.
pub fn format_switch_result(result: &ModelSwitch) -> String {
    match result {
        ModelSwitch::Switch {
            provider,
            model_id,
            model_info,
        } => {
            let model_line = format_model_info(model_info);
            format!("✓ Switched to {provider}/{model_id}\n{model_line}")
        }
        ModelSwitch::ShowCatalog => {
            // Handled by the caller — includes current model.
            String::new()
        }
        ModelSwitch::Ambiguous { name, candidates } => {
            let mut out = format!(
                "Ambiguous model '{name}' — found in {} providers:\n",
                candidates.len()
            );
            for m in candidates {
                let implemented = if is_provider_implemented(&m.provider) {
                    ""
                } else {
                    " — not yet implemented"
                };
                out.push_str(&format!(
                    "  {provider}/{id} ({display}){implemented}\n",
                    provider = m.provider,
                    id = m.id,
                    display = m.display_name,
                ));
            }
            out.push_str("Use /model provider/model to be specific.");
            out
        }
        ModelSwitch::NotFound { arg, reason } => {
            format!("Unknown model '{arg}': {reason}")
        }
        ModelSwitch::ProviderNotImplemented { provider } => {
            let implemented: Vec<&str> = all_providers()
                .into_iter()
                .filter(|p| is_provider_implemented(p))
                .collect();
            format!(
                "Provider '{provider}' is not yet implemented.\n\
                 Available providers: {}",
                implemented.join(", ")
            )
        }
    }
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.0}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{}K", n / 1_000)
    } else {
        n.to_string()
    }
}

fn thinking_level_to_short(level: &crate::llm::base::ThinkingLevel) -> &'static str {
    match level {
        crate::llm::base::ThinkingLevel::None => "off",
        crate::llm::base::ThinkingLevel::Minimal => "min",
        crate::llm::base::ThinkingLevel::Low => "low",
        crate::llm::base::ThinkingLevel::Medium => "med",
        crate::llm::base::ThinkingLevel::High => "high",
        crate::llm::base::ThinkingLevel::XHigh => "xhi",
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_model_switch ─────────────────────────────────────────

    #[test]
    fn test_resolve_empty_returns_show_catalog() {
        assert_eq!(resolve_model_switch(""), ModelSwitch::ShowCatalog);
    }

    #[test]
    fn test_resolve_whitespace_returns_show_catalog() {
        assert_eq!(resolve_model_switch("   "), ModelSwitch::ShowCatalog);
    }

    #[test]
    fn test_resolve_provider_only() {
        let result = resolve_model_switch("deepseek");
        match result {
            ModelSwitch::Switch {
                provider, model_id, ..
            } => {
                assert_eq!(provider, "deepseek");
                // default_model_for_provider("deepseek") is "deepseek-v4-pro"
                assert_eq!(model_id, "deepseek-v4-pro");
            }
            other => panic!("expected Switch, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_provider_slash_model_not_found() {
        let result = resolve_model_switch("openai/nonexistent");
        assert!(matches!(result, ModelSwitch::NotFound { .. }));
    }

    #[test]
    fn test_resolve_provider_slash_model_empty_parts() {
        let result = resolve_model_switch("openai/");
        assert!(matches!(result, ModelSwitch::NotFound { .. }));
        let result = resolve_model_switch("/gpt-4o");
        assert!(matches!(result, ModelSwitch::NotFound { .. }));
    }

    #[test]
    fn test_resolve_unknown_provider() {
        let result = resolve_model_switch("nonexistent");
        assert!(matches!(result, ModelSwitch::NotFound { .. }));
    }

    #[test]
    fn test_resolve_model_only_unique() {
        let result = resolve_model_switch("gpt-4o");
        match result {
            ModelSwitch::Switch {
                provider, model_id, ..
            } => {
                assert_eq!(provider, "openai");
                assert_eq!(model_id, "gpt-4o");
            }
            other => panic!("expected Switch, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_model_only_ambiguous() {
        // "gpt-5.5" exists for openai, openai-codex, and github-copilot
        let result = resolve_model_switch("gpt-5.5");
        match result {
            ModelSwitch::Ambiguous { name, candidates } => {
                assert_eq!(name, "gpt-5.5");
                assert_eq!(candidates.len(), 3);
                let providers: Vec<&str> = candidates.iter().map(|m| m.provider.as_str()).collect();
                assert!(providers.contains(&"openai"));
                assert!(providers.contains(&"openai-codex"));
                assert!(providers.contains(&"github-copilot"));
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_provider_not_implemented() {
        // anthropic is in the catalog but not implemented
        let result = resolve_model_switch("anthropic/claude-sonnet-4-5-20250929");
        match result {
            ModelSwitch::ProviderNotImplemented { ref provider } => {
                assert_eq!(provider, "anthropic");
            }
            other => panic!("expected ProviderNotImplemented, got {other:?}"),
        }

        // provider-only form also reports not implemented
        let result = resolve_model_switch("anthropic");
        match result {
            ModelSwitch::ProviderNotImplemented { ref provider } => {
                assert_eq!(provider, "anthropic");
            }
            other => panic!("expected ProviderNotImplemented, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_model_only_not_implemented_provider() {
        // "claude-sonnet-4-5-20250929" only exists in anthropic (not implemented)
        let result = resolve_model_switch("claude-sonnet-4-5-20250929");
        assert!(matches!(result, ModelSwitch::ProviderNotImplemented { .. }));
    }

    #[test]
    fn test_resolve_deepseek_model_by_id() {
        let result = resolve_model_switch("deepseek-v4-flash");
        match result {
            ModelSwitch::Switch {
                provider, model_id, ..
            } => {
                assert_eq!(provider, "deepseek");
                assert_eq!(model_id, "deepseek-v4-flash");
            }
            other => panic!("expected Switch, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_mock() {
        let result = resolve_model_switch("mock");
        match result {
            ModelSwitch::Switch {
                provider, model_id, ..
            } => {
                assert_eq!(provider, "mock");
                assert_eq!(model_id, "mock");
            }
            other => panic!("expected Switch, got {other:?}"),
        }
    }

    // ── format helpers ───────────────────────────────────────────────

    #[test]
    fn test_format_catalog_contains_providers() {
        let catalog = format_catalog();
        assert!(catalog.contains("openai"), "catalog: {catalog}");
        assert!(catalog.contains("deepseek"), "catalog: {catalog}");
        assert!(catalog.contains("mock"), "catalog: {catalog}");
    }

    #[test]
    fn test_format_switch_result_contains_model_info() {
        let model = models::find_model("openai", "gpt-4o").unwrap();
        let result = ModelSwitch::Switch {
            provider: "openai".into(),
            model_id: "gpt-4o".into(),
            model_info: model,
        };
        let text = format_switch_result(&result);
        assert!(text.contains("openai/gpt-4o"));
        assert!(text.contains("128K"));
        assert!(text.contains("yes")); // vision
    }

    #[test]
    fn test_format_switch_result_not_found() {
        let result = ModelSwitch::NotFound {
            arg: "bad-model".into(),
            reason: "not in catalog".into(),
        };
        let text = format_switch_result(&result);
        assert!(text.contains("bad-model"));
        assert!(text.contains("not in catalog"));
    }

    #[test]
    fn test_format_switch_result_ambiguous() {
        let candidates = vec![
            models::find_model("openai", "gpt-5.5").unwrap(),
            models::find_model("openai-codex", "gpt-5.5").unwrap(),
            models::find_model("github-copilot", "gpt-5.5").unwrap(),
        ];
        let result = ModelSwitch::Ambiguous {
            name: "gpt-5.5".into(),
            candidates,
        };
        let text = format_switch_result(&result);
        assert!(text.contains("gpt-5.5"));
        assert!(text.contains("openai/gpt-5.5"));
        assert!(text.contains("github-copilot/gpt-5.5"));
        assert!(text.contains("not yet implemented"));
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(128_000), "128K");
        assert_eq!(format_tokens(1_000_000), "1M");
    }
}
