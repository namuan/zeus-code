use crate::core::errors::KonResult;
use crate::llm::base::{Provider, ThinkingLevel};
use crate::llm::models;
use crate::llm::providers::mock::MockProvider;
use crate::llm::providers::openai_completions::OpenAICompletionsProvider;

pub mod anthropic;
pub mod azure_ai_foundry;
pub mod copilot;
pub mod mock;
pub mod openai_codex_responses;
pub mod openai_completions;
pub mod openai_responses;

/// Configuration needed to construct any provider.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub provider_name: String,
    pub model_id: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub thinking_level: ThinkingLevel,
    pub max_tokens: Option<u64>,
    pub request_timeout_secs: u64,
    pub insecure_skip_verify: bool,
}

impl ProviderConfig {
    /// Create a config with sensible defaults for a given provider/model pair.
    pub fn new(provider_name: &str, model_id: &str, api_key: &str) -> Self {
        let model = models::find_model(provider_name, model_id);
        Self {
            provider_name: provider_name.into(),
            model_id: model_id.into(),
            api_key: api_key.into(),
            base_url: None,
            thinking_level: ThinkingLevel::None,
            max_tokens: model.as_ref().map(|m| m.max_output_tokens),
            request_timeout_secs: 600,
            insecure_skip_verify: false,
        }
    }

    /// Create a config for the always-available mock provider.
    pub fn mock() -> Self {
        Self::new("mock", "mock", "")
    }

    /// Set the base URL override (fluent).
    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = Some(url);
        self
    }

    /// Enable or disable TLS certificate verification (fluent).
    pub fn with_insecure_skip_verify(mut self, skip: bool) -> Self {
        self.insecure_skip_verify = skip;
        self
    }
}

/// Create a provider instance from configuration.
///
/// Providers are looked up by the `provider_name` field. The `mock` provider
/// is always available; others will be added in later phases.
pub fn create_provider(config: &ProviderConfig) -> KonResult<Box<dyn Provider>> {
    match config.provider_name.as_str() {
        "mock" => Ok(Box::new(MockProvider::new(config.model_id.clone()))),
        "openai" | "deepseek" | "zhipu" | "github-copilot" | "openrouter" => {
            Ok(Box::new(OpenAICompletionsProvider::new(config.clone())?))
        }
        "openai-codex" => Err(crate::core::errors::KonError::Provider(
            "openai-codex provider not yet implemented".into(),
        )),
        "openai-responses" => Err(crate::core::errors::KonError::Provider(
            "openai-responses provider not yet implemented".into(),
        )),
        "anthropic" => Err(crate::core::errors::KonError::Provider(
            "anthropic provider not yet implemented".into(),
        )),
        "azure-ai-foundry" => Err(crate::core::errors::KonError::Provider(
            "azure-ai-foundry provider not yet implemented".into(),
        )),
        other => Err(crate::core::errors::KonError::Provider(format!(
            "unknown provider: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_mock_provider() {
        let config = ProviderConfig::mock();
        let provider = create_provider(&config);
        assert!(provider.is_ok());
        let p = provider.unwrap();
        assert_eq!(p.name(), "mock");
        assert_eq!(p.model(), "mock");
    }

    #[test]
    fn test_create_unknown_provider() {
        let config = ProviderConfig::new("nonexistent", "x", "");
        let result = create_provider(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_provider_config_new_fills_model_info() {
        let config = ProviderConfig::new("openai", "gpt-4o", "sk-test");
        assert_eq!(config.provider_name, "openai");
        assert_eq!(config.model_id, "gpt-4o");
        assert_eq!(config.api_key, "sk-test");
        assert_eq!(config.request_timeout_secs, 600);
        assert!(!config.insecure_skip_verify);
        // Model info should be looked up
        assert!(config.max_tokens.is_some());
    }

    #[test]
    fn test_provider_config_new_unknown_model_no_max_tokens() {
        let config = ProviderConfig::new("openai", "nonexistent", "sk-test");
        assert!(config.max_tokens.is_none());
    }
}
