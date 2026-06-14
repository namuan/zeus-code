use once_cell::sync::Lazy;

use crate::llm::base::ThinkingLevel;

/// Static model catalog entry.
#[derive(Debug, Clone)]
pub struct Model {
    pub id: String,
    pub provider: String,
    pub display_name: String,
    pub context_window: u64,
    pub supports_vision: bool,
    pub supports_thinking: bool,
    pub thinking_levels: Vec<ThinkingLevel>,
    pub max_output_tokens: u64,
}

/// The complete model catalog.
static MODELS: Lazy<Vec<Model>> = Lazy::new(|| {
    vec![
        // ── OpenAI ─────────────────────────────────────────────────
        Model {
            id: "gpt-5.5".into(),
            provider: "openai".into(),
            display_name: "GPT-5.5".into(),
            context_window: 200_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
            ],
            max_output_tokens: 128_000,
        },
        Model {
            id: "gpt-5".into(),
            provider: "openai".into(),
            display_name: "GPT-5".into(),
            context_window: 200_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
            ],
            max_output_tokens: 128_000,
        },
        Model {
            id: "gpt-4o".into(),
            provider: "openai".into(),
            display_name: "GPT-4o".into(),
            context_window: 128_000,
            supports_vision: true,
            supports_thinking: false,
            thinking_levels: vec![],
            max_output_tokens: 16_384,
        },
        Model {
            id: "gpt-4o-mini".into(),
            provider: "openai".into(),
            display_name: "GPT-4o Mini".into(),
            context_window: 128_000,
            supports_vision: true,
            supports_thinking: false,
            thinking_levels: vec![],
            max_output_tokens: 16_384,
        },
        Model {
            id: "o4-mini".into(),
            provider: "openai".into(),
            display_name: "o4-mini".into(),
            context_window: 200_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
            ],
            max_output_tokens: 100_000,
        },
        // ── OpenAI Codex ───────────────────────────────────────────
        Model {
            id: "gpt-5.5".into(),
            provider: "openai-codex".into(),
            display_name: "GPT-5.5 (Codex)".into(),
            context_window: 200_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
            ],
            max_output_tokens: 128_000,
        },
        Model {
            id: "gpt-5-codex".into(),
            provider: "openai-codex".into(),
            display_name: "GPT-5 Codex".into(),
            context_window: 200_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
            ],
            max_output_tokens: 128_000,
        },
        // ── Anthropic ──────────────────────────────────────────────
        Model {
            id: "claude-sonnet-4-5-20250929".into(),
            provider: "anthropic".into(),
            display_name: "Claude Sonnet 4.5".into(),
            context_window: 200_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![ThinkingLevel::None, ThinkingLevel::XHigh],
            max_output_tokens: 64_000,
        },
        Model {
            id: "claude-haiku-4-5-20251001".into(),
            provider: "anthropic".into(),
            display_name: "Claude Haiku 4.5".into(),
            context_window: 200_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![ThinkingLevel::None, ThinkingLevel::XHigh],
            max_output_tokens: 64_000,
        },
        Model {
            id: "claude-opus-4-5-20251101".into(),
            provider: "anthropic".into(),
            display_name: "Claude Opus 4.5".into(),
            context_window: 200_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![
                ThinkingLevel::None,
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
                ThinkingLevel::XHigh,
            ],
            max_output_tokens: 64_000,
        },
        // ── GitHub Copilot ─────────────────────────────────────────
        Model {
            id: "gpt-5.5".into(),
            provider: "github-copilot".into(),
            display_name: "GPT-5.5 (Copilot)".into(),
            context_window: 200_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
            ],
            max_output_tokens: 128_000,
        },
        Model {
            id: "claude-sonnet-4-5".into(),
            provider: "github-copilot".into(),
            display_name: "Claude Sonnet 4.5 (Copilot)".into(),
            context_window: 200_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![ThinkingLevel::None, ThinkingLevel::XHigh],
            max_output_tokens: 64_000,
        },
        // ── Azure AI Foundry ───────────────────────────────────────
        Model {
            id: "claude-sonnet-4-5".into(),
            provider: "azure-ai-foundry".into(),
            display_name: "Claude Sonnet 4.5 (Azure)".into(),
            context_window: 200_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![ThinkingLevel::None, ThinkingLevel::XHigh],
            max_output_tokens: 64_000,
        },
        // ── DeepSeek ───────────────────────────────────────────────
        Model {
            id: "deepseek-v4-pro".into(),
            provider: "deepseek".into(),
            display_name: "DeepSeek V4 Pro".into(),
            context_window: 200_000,
            supports_vision: false,
            supports_thinking: true,
            thinking_levels: vec![ThinkingLevel::None, ThinkingLevel::Low],
            max_output_tokens: 32_000,
        },
        Model {
            id: "deepseek-v4-flash".into(),
            provider: "deepseek".into(),
            display_name: "DeepSeek V4 Flash".into(),
            context_window: 200_000,
            supports_vision: false,
            supports_thinking: false,
            thinking_levels: vec![],
            max_output_tokens: 32_000,
        },
        // ── ZhiPu ──────────────────────────────────────────────────
        Model {
            id: "glm-5-plus".into(),
            provider: "zhipu".into(),
            display_name: "GLM-5 Plus".into(),
            context_window: 128_000,
            supports_vision: true,
            supports_thinking: true,
            thinking_levels: vec![ThinkingLevel::None, ThinkingLevel::Low],
            max_output_tokens: 32_000,
        },
        Model {
            id: "glm-5-flash".into(),
            provider: "zhipu".into(),
            display_name: "GLM-5 Flash".into(),
            context_window: 128_000,
            supports_vision: false,
            supports_thinking: false,
            thinking_levels: vec![],
            max_output_tokens: 32_000,
        },
        // ── OpenRouter ────────────────────────────────────────────
        Model {
            id: "openrouter/free".into(),
            provider: "openrouter".into(),
            display_name: "Free Models Router".into(),
            context_window: 128_000,
            supports_vision: false,
            supports_thinking: false,
            thinking_levels: vec![],
            max_output_tokens: 8_192,
        },
        Model {
            id: "google/gemini-2.5-flash-lite".into(),
            provider: "openrouter".into(),
            display_name: "Gemini 2.5 Flash Lite".into(),
            context_window: 1_000_000,
            supports_vision: true,
            supports_thinking: false,
            thinking_levels: vec![],
            max_output_tokens: 8_192,
        },
        Model {
            id: "deepseek/deepseek-chat".into(),
            provider: "openrouter".into(),
            display_name: "DeepSeek V3".into(),
            context_window: 128_000,
            supports_vision: false,
            supports_thinking: false,
            thinking_levels: vec![],
            max_output_tokens: 8_192,
        },
        Model {
            id: "meta-llama/llama-4-maverick".into(),
            provider: "openrouter".into(),
            display_name: "Llama 4 Maverick".into(),
            context_window: 128_000,
            supports_vision: false,
            supports_thinking: false,
            thinking_levels: vec![],
            max_output_tokens: 8_192,
        },
        Model {
            id: "mistralai/mistral-small-3.1-24b-instruct".into(),
            provider: "openrouter".into(),
            display_name: "Mistral Small 3.1".into(),
            context_window: 128_000,
            supports_vision: false,
            supports_thinking: false,
            thinking_levels: vec![],
            max_output_tokens: 8_192,
        },
        Model {
            id: "qwen/qwen3-32b".into(),
            provider: "openrouter".into(),
            display_name: "Qwen 3 32B".into(),
            context_window: 128_000,
            supports_vision: false,
            supports_thinking: false,
            thinking_levels: vec![],
            max_output_tokens: 8_192,
        },
        // ── Mock (testing) ─────────────────────────────────────────
        Model {
            id: "mock".into(),
            provider: "mock".into(),
            display_name: "Mock (Testing)".into(),
            context_window: 128_000,
            supports_vision: false,
            supports_thinking: false,
            thinking_levels: vec![],
            max_output_tokens: 4_096,
        },
    ]
});

/// Return all known models.
pub fn all_models() -> Vec<Model> {
    MODELS.clone()
}

/// Find a model by provider and model ID.
pub fn find_model(provider: &str, model_id: &str) -> Option<Model> {
    MODELS
        .iter()
        .find(|m| m.provider == provider && m.id == model_id)
        .cloned()
}

/// Return all models for a given provider.
pub fn models_for_provider(provider: &str) -> Vec<Model> {
    MODELS
        .iter()
        .filter(|m| m.provider == provider)
        .cloned()
        .collect()
}

/// Return all distinct provider names.
pub fn all_providers() -> Vec<&'static str> {
    vec![
        "openai",
        "openai-codex",
        "anthropic",
        "github-copilot",
        "azure-ai-foundry",
        "deepseek",
        "zhipu",
        "openrouter",
        "mock",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_models_non_empty() {
        let models = all_models();
        assert!(!models.is_empty(), "model catalog should have entries");
    }

    #[test]
    fn test_find_model_exact_match() {
        let model = find_model("openai", "gpt-4o");
        assert!(model.is_some());
        let m = model.unwrap();
        assert_eq!(m.provider, "openai");
        assert_eq!(m.id, "gpt-4o");
        assert_eq!(m.context_window, 128_000);
        assert!(m.supports_vision);
    }

    #[test]
    fn test_find_model_missing() {
        assert!(find_model("openai", "nonexistent").is_none());
    }

    #[test]
    fn test_find_model_wrong_provider() {
        // gpt-4o is registered under "openai", not "anthropic"
        assert!(find_model("anthropic", "gpt-4o").is_none());
    }

    #[test]
    fn test_models_for_provider() {
        let openai_models = models_for_provider("openai");
        assert!(!openai_models.is_empty());
        for m in &openai_models {
            assert_eq!(m.provider, "openai");
        }
    }

    #[test]
    fn test_models_for_provider_unknown() {
        let models = models_for_provider("nonexistent");
        assert!(models.is_empty());
    }

    #[test]
    fn test_mock_model_exists() {
        let model = find_model("mock", "mock");
        assert!(model.is_some());
    }

    #[test]
    fn test_thinking_levels_vary_by_model() {
        // Models without thinking support should have empty levels
        let gpt4o = find_model("openai", "gpt-4o").unwrap();
        assert!(!gpt4o.supports_thinking);
        assert!(gpt4o.thinking_levels.is_empty());

        // Models with thinking support should have non-empty levels
        let gpt5 = find_model("openai", "gpt-5.5").unwrap();
        assert!(gpt5.supports_thinking);
        assert!(!gpt5.thinking_levels.is_empty());
    }
}
