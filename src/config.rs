use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::cli::Cli;
use crate::core::compaction::CompactionConfig;
use crate::core::errors::{KonError, KonResult};

// ── Config path ──────────────────────────────────────────────────────────

fn config_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    base.join("zeus")
}

fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

fn sessions_dir() -> PathBuf {
    config_dir().join("sessions")
}

// ── Config structs ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaConfig {
    pub config_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub openai_compat: String,
    pub anthropic_compat: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TLSConfig {
    pub insecure_skip_verify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPromptConfig {
    pub git_context: bool,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    pub default_provider: String,
    pub default_model: String,
    #[serde(default)]
    pub default_base_url: String,
    pub default_thinking_level: String,
    pub tool_call_idle_timeout_seconds: u64,
    pub request_timeout_seconds: u64,
    pub auth: AuthConfig,
    pub tls: TLSConfig,
    pub system_prompt: SystemPromptConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub max_turns: u64,
    pub default_context_window: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    pub extra: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIConfig {
    pub theme: String,
    pub collapse_thinking: bool,
    pub thinking_lines: String,
    pub colored_tool_badge: bool,
    pub show_welcome_shortcuts: bool,
    #[serde(default)]
    pub hidden_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsConfig {
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationsConfig {
    pub enabled: bool,
    pub volume: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub meta: MetaConfig,
    pub llm: LLMConfig,
    pub compaction: CompactionConfig,
    pub agent: AgentConfig,
    pub tools: ToolsConfig,
    pub ui: UIConfig,
    pub permissions: PermissionsConfig,
    pub notifications: NotificationsConfig,
}

// ── Global config singleton ──────────────────────────────────────────────

static CONFIG: Lazy<Arc<RwLock<Config>>> =
    Lazy::new(|| Arc::new(RwLock::new(Config::load_or_create())));

/// Get a reference to the global configuration (thread-safe, read/write).
pub fn get_config() -> Arc<RwLock<Config>> {
    CONFIG.clone()
}

// ── Loading ──────────────────────────────────────────────────────────────

impl Config {
    /// Load config from disk, or create a fresh one from the embedded defaults.
    pub fn load_or_create() -> Self {
        match Self::load_from_disk() {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!("Could not load config, using defaults: {e}");
                Self::load_defaults()
            }
        }
    }

    /// Load config from disk. Returns an error if the file cannot be read or parsed.
    fn load_from_disk() -> KonResult<Self> {
        let path = config_path();

        if !path.exists() {
            let defaults = Self::load_defaults();
            defaults.save_to_disk()?;
            return Ok(defaults);
        }

        let raw = fs::read_to_string(&path).map_err(|e| {
            KonError::Config(format!("failed to read config at {}: {e}", path.display()))
        })?;

        let user_value: toml::Value = toml::from_str(&raw)
            .map_err(|e| KonError::Config(format!("failed to parse config: {e}")))?;

        // Migrate older config formats if needed
        let user_value = Self::migrate_if_needed(user_value);

        // Load the embedded defaults as a TOML value, then deep-merge user over it
        let default_toml = include_str!("../defaults/config.toml");
        let default_value: toml::Value =
            toml::from_str(default_toml).expect("embedded default config.toml is invalid");

        let merged_value = merge_toml_values(default_value, user_value);

        let config: Config = toml::from_str(&toml::to_string(&merged_value).unwrap())
            .map_err(|e| KonError::Config(format!("failed to deserialize config: {e}")))?;

        Ok(config)
    }

    /// Load the embedded default configuration.
    pub fn load_defaults() -> Self {
        let default_toml = include_str!("../defaults/config.toml");
        toml::from_str(default_toml).unwrap_or_else(|e| {
            panic!("embedded default config.toml is invalid: {e}");
        })
    }

    // ── Saving ──────────────────────────────────────────────────────────

    /// Save the current config to disk atomically (write to temp + rename).
    pub fn save_to_disk(&self) -> KonResult<()> {
        let path = config_path();
        let dir = path.parent().unwrap();

        fs::create_dir_all(dir).map_err(|e| {
            KonError::Config(format!(
                "failed to create config dir {}: {e}",
                dir.display()
            ))
        })?;

        let toml_str = toml::to_string_pretty(self)
            .map_err(|e| KonError::Config(format!("failed to serialize config: {e}")))?;

        // Atomic write: write to a temp file, then rename
        let tmp_path = path.with_extension("toml.tmp");
        {
            let mut f = fs::File::create(&tmp_path).map_err(|e| {
                KonError::Config(format!(
                    "failed to create temp config at {}: {e}",
                    tmp_path.display()
                ))
            })?;
            f.write_all(toml_str.as_bytes())
                .map_err(|e| KonError::Config(format!("failed to write config: {e}")))?;
            f.flush()
                .map_err(|e| KonError::Config(format!("failed to flush config: {e}")))?;
        }

        fs::rename(&tmp_path, &path)
            .map_err(|e| KonError::Config(format!("failed to rename config: {e}")))?;

        Ok(())
    }

    // ── CLI overrides ───────────────────────────────────────────────────

    /// Apply command-line overrides on top of the loaded config.
    pub fn merge_cli_overrides(&mut self, cli: &Cli) {
        if let Some(ref model) = cli.model {
            self.llm.default_model = model.clone();
        }
        if let Some(ref provider) = cli.provider {
            self.llm.default_provider = provider.clone();
            // If the user switches provider without specifying a model,
            // use a sensible default for that provider.
            if cli.model.is_none() {
                self.llm.default_model = default_model_for_provider(provider);
            }
        }
        if let Some(ref base_url) = cli.base_url {
            self.llm.default_base_url = base_url.clone();
        }
        if cli.insecure_skip_verify {
            self.llm.tls.insecure_skip_verify = true;
        }
        if !cli.extra_tools.is_empty() {
            self.tools.extra = cli.extra_tools.clone();
        }
    }

    // ── Migration ──────────────────────────────────────────────────────

    /// Migrate an older config format to the current version if needed.
    fn migrate_if_needed(mut value: toml::Value) -> toml::Value {
        let version = value
            .get("meta")
            .and_then(|m| m.get("config_version"))
            .and_then(|v| v.as_integer())
            .unwrap_or(0) as u32;

        const CURRENT_VERSION: u32 = 3;

        if version < CURRENT_VERSION {
            for v in version..CURRENT_VERSION {
                value = Self::apply_migration(value, v);
            }
            if let Some(meta) = value.get_mut("meta")
                && let Some(table) = meta.as_table_mut()
            {
                table.insert(
                    "config_version".into(),
                    toml::Value::Integer(CURRENT_VERSION as i64),
                );
            }
        }

        value
    }

    fn apply_migration(value: toml::Value, from_version: u32) -> toml::Value {
        match from_version {
            0 => Self::migrate_v0_to_v1(value),
            1 => Self::migrate_v1_to_v2(value),
            2 => value,
            _ => value,
        }
    }

    /// v0 → v1: Ensure meta.config_version exists.
    fn migrate_v0_to_v1(mut value: toml::Value) -> toml::Value {
        if let Some(table) = value.as_table_mut()
            && !table.contains_key("meta")
        {
            let mut meta = toml::Table::new();
            meta.insert("config_version".into(), toml::Value::Integer(1));
            table.insert("meta".into(), toml::Value::Table(meta));
        }
        value
    }

    /// v1 → v2: Ensure auth and tls subsections exist under [llm].
    fn migrate_v1_to_v2(mut value: toml::Value) -> toml::Value {
        if let Some(table) = value.as_table_mut()
            && let Some(llm) = table.get_mut("llm")
            && let Some(llm_table) = llm.as_table_mut()
        {
            if !llm_table.contains_key("auth") {
                let mut auth = toml::Table::new();
                auth.insert("openai_compat".into(), toml::Value::String("auto".into()));
                auth.insert(
                    "anthropic_compat".into(),
                    toml::Value::String("auto".into()),
                );
                llm_table.insert("auth".into(), toml::Value::Table(auth));
            }
            if !llm_table.contains_key("tls") {
                let mut tls = toml::Table::new();
                tls.insert("insecure_skip_verify".into(), toml::Value::Boolean(false));
                llm_table.insert("tls".into(), toml::Value::Table(tls));
            }
        }
        value
    }

    // ── Convenience accessors ───────────────────────────────────────────

    /// Path to the sessions directory (does not create it).
    pub fn sessions_dir_path(&self) -> PathBuf {
        sessions_dir()
    }

    /// Ensure the sessions directory exists, returning its path.
    pub fn ensure_sessions_dir(&self) -> KonResult<PathBuf> {
        let dir = sessions_dir();
        fs::create_dir_all(&dir).map_err(|e| {
            KonError::Config(format!(
                "failed to create sessions dir {}: {e}",
                dir.display()
            ))
        })?;
        Ok(dir)
    }
}

/// Sensible default model when a user switches providers via --provider
/// without also specifying --model.
///
/// Must return a model ID that exists in `src/llm/models.rs` for the given
/// provider, otherwise callers that look up the returned ID via
/// `find_model(provider, id)` will panic.
pub(crate) fn default_model_for_provider(provider: &str) -> String {
    match provider {
        "openai" => "gpt-4o".into(),
        "openrouter" => "openrouter/free".into(),
        "deepseek" => "deepseek-v4-pro".into(),
        "zhipu" => "glm-5-plus".into(),
        "anthropic" => "claude-sonnet-4-5-20250929".into(),
        "github-copilot" => "gpt-5.5".into(),
        "mock" => "mock".into(),
        _ => "gpt-4o".into(),
    }
}

// ── TOML value deep merge ────────────────────────────────────────────────

/// Recursively merge two TOML values: `user` keys override `default` keys.
fn merge_toml_values(default: toml::Value, user: toml::Value) -> toml::Value {
    match (default, user) {
        (toml::Value::Table(mut default_table), toml::Value::Table(user_table)) => {
            for (key, user_val) in user_table {
                match default_table.get_mut(&key) {
                    Some(default_val) => {
                        if default_val.is_table() && user_val.is_table() {
                            let merged = merge_toml_values(default_val.clone(), user_val);
                            default_table.insert(key, merged);
                        } else {
                            default_table.insert(key, user_val);
                        }
                    }
                    None => {
                        default_table.insert(key, user_val);
                    }
                }
            }
            toml::Value::Table(default_table)
        }
        (_default, user) => user,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Default config tests ───────────────────────────────────────────

    #[test]
    fn test_load_defaults() {
        let config = Config::load_defaults();

        assert_eq!(config.llm.default_provider, "openai-codex");
        assert_eq!(config.llm.default_model, "gpt-5.5");
        assert_eq!(config.llm.default_thinking_level, "low");
        assert_eq!(config.llm.tool_call_idle_timeout_seconds, 60);
        assert_eq!(config.llm.request_timeout_seconds, 120);
        assert_eq!(config.llm.auth.openai_compat, "auto");
        assert_eq!(config.llm.auth.anthropic_compat, "auto");
        assert!(!config.llm.tls.insecure_skip_verify);
        assert!(config.llm.system_prompt.git_context);

        assert_eq!(config.compaction.buffer_tokens, 20000);
        assert_eq!(
            config.compaction.on_overflow,
            crate::core::compaction::OnOverflow::Continue
        );

        assert_eq!(config.agent.max_turns, 500);
        assert_eq!(config.agent.default_context_window, 200000);

        assert_eq!(config.tools.extra, vec!["web_search", "web_fetch"]);

        assert_eq!(config.ui.theme, "gruvbox-dark");
        assert!(config.ui.collapse_thinking);
        assert_eq!(config.ui.thinking_lines, "1");
        assert!(config.ui.colored_tool_badge);
        assert!(config.ui.show_welcome_shortcuts);
        assert!(config.ui.hidden_models.is_empty());

        assert_eq!(config.permissions.mode, "prompt");

        assert!(!config.notifications.enabled);
        assert!((config.notifications.volume - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_default_system_prompt_not_empty() {
        let config = Config::load_defaults();
        assert!(!config.llm.system_prompt.content.is_empty());
        assert!(config.llm.system_prompt.content.contains("Zeus"));
    }

    // ── TOML merge: user overrides partial sections ───────────────────

    #[test]
    fn test_merge_partial_user_config() {
        // User only sets model and theme — everything else from defaults
        let user_toml = r#"
            [meta]
            config_version = 3

            [llm]
            default_model = "custom-model"

            [ui]
            theme = "dracula"
        "#;

        let default_config = Config::load_defaults();
        let default_toml = toml::to_string(&default_config).unwrap();
        let default_value: toml::Value = toml::from_str(&default_toml).unwrap();
        let user_value: toml::Value = toml::from_str(user_toml).unwrap();

        let merged_value = merge_toml_values(default_value, user_value);
        let merged: Config = toml::from_str(&toml::to_string(&merged_value).unwrap()).unwrap();

        // User overrides apply
        assert_eq!(merged.llm.default_model, "custom-model");
        assert_eq!(merged.ui.theme, "dracula");
        // Defaults are preserved for untouched fields
        assert_eq!(merged.llm.default_provider, "openai-codex");
        assert_eq!(merged.agent.max_turns, 500);
    }

    #[test]
    fn test_merge_nested_partial_section() {
        // User sets one auth field, the other should come from defaults
        let user_toml = r#"
            [meta]
            config_version = 3

            [llm.auth]
            openai_compat = "required"
        "#;

        let default_config = Config::load_defaults();
        let default_toml = toml::to_string(&default_config).unwrap();
        let default_value: toml::Value = toml::from_str(&default_toml).unwrap();
        let user_value: toml::Value = toml::from_str(user_toml).unwrap();

        let merged_value = merge_toml_values(default_value, user_value);
        let merged: Config = toml::from_str(&toml::to_string(&merged_value).unwrap()).unwrap();

        assert_eq!(merged.llm.auth.openai_compat, "required");
        assert_eq!(merged.llm.auth.anthropic_compat, "auto");
    }

    // ── CLI override tests ─────────────────────────────────────────────

    fn test_cli() -> Cli {
        Cli {
            model: None,
            provider: None,
            prompt: None,
            api_key: None,
            base_url: None,
            continue_session: false,
            resume: None,
            extra_tools: vec![],
            insecure_skip_verify: false,
        }
    }

    #[test]
    fn test_merge_cli_overrides_model() {
        let mut config = Config::load_defaults();
        let cli = Cli {
            model: Some("gpt-4o".into()),
            ..test_cli()
        };
        config.merge_cli_overrides(&cli);
        assert_eq!(config.llm.default_model, "gpt-4o");
    }

    #[test]
    fn test_merge_cli_overrides_base_url() {
        let mut config = Config::load_defaults();
        let cli = Cli {
            base_url: Some("http://localhost:8080/v1".into()),
            ..test_cli()
        };
        config.merge_cli_overrides(&cli);
        assert_eq!(config.llm.default_base_url, "http://localhost:8080/v1");
    }

    #[test]
    fn test_merge_cli_overrides_insecure_tls() {
        let mut config = Config::load_defaults();
        let cli = Cli {
            insecure_skip_verify: true,
            ..test_cli()
        };
        config.merge_cli_overrides(&cli);
        assert!(config.llm.tls.insecure_skip_verify);
    }

    #[test]
    fn test_merge_cli_overrides_extra_tools() {
        let mut config = Config::load_defaults();
        let cli = Cli {
            extra_tools: vec!["web_search".into()],
            ..test_cli()
        };
        config.merge_cli_overrides(&cli);
        assert_eq!(config.tools.extra, vec!["web_search"]);
    }

    #[test]
    fn test_merge_cli_overrides_no_op() {
        let mut config = Config::load_defaults();
        let original_model = config.llm.default_model.clone();
        let cli = test_cli();
        config.merge_cli_overrides(&cli);
        assert_eq!(config.llm.default_model, original_model);
    }

    // ── Save / load round-trip ─────────────────────────────────────────

    #[test]
    fn test_save_and_load_roundtrip() {
        let config = Config::load_defaults();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(loaded.llm.default_provider, config.llm.default_provider);
        assert_eq!(loaded.llm.default_model, config.llm.default_model);
        assert_eq!(
            loaded.compaction.buffer_tokens,
            config.compaction.buffer_tokens
        );
        assert_eq!(loaded.agent.max_turns, config.agent.max_turns);
        assert_eq!(loaded.permissions.mode, config.permissions.mode);
        assert_eq!(loaded.ui.theme, config.ui.theme);
        assert_eq!(loaded.tools.extra, config.tools.extra);
    }

    #[test]
    fn test_save_load_preserves_multiline() {
        let mut config = Config::load_defaults();
        config.llm.system_prompt.content = "Line 1\nLine 2\nLine 3".into();

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(loaded.llm.system_prompt.content, "Line 1\nLine 2\nLine 3");
    }

    // ── Migration tests ────────────────────────────────────────────────

    #[test]
    fn test_migrate_v0_to_v1_adds_meta() {
        let raw = r#"
            [llm]
            default_provider = "openai-codex"
        "#;
        let value: toml::Value = toml::from_str(raw).unwrap();
        let migrated = Config::migrate_v0_to_v1(value);

        let meta = migrated.get("meta").unwrap();
        assert_eq!(meta.get("config_version").unwrap().as_integer(), Some(1));
    }

    #[test]
    fn test_migrate_v1_to_v2_adds_auth_and_tls() {
        let raw = r#"
            [meta]
            config_version = 1
            [llm]
            default_provider = "openai-codex"
        "#;
        let value: toml::Value = toml::from_str(raw).unwrap();
        let migrated = Config::migrate_v1_to_v2(value);

        let llm = migrated.get("llm").unwrap();
        assert!(llm.get("auth").is_some());
        assert!(llm.get("tls").is_some());

        let auth = llm.get("auth").unwrap();
        assert_eq!(auth.get("openai_compat").unwrap().as_str(), Some("auto"));
    }

    #[test]
    fn test_migrate_if_needed_noop_for_current() {
        let raw = r#"
            [meta]
            config_version = 3
            [llm]
            default_provider = "openai-codex"
            default_model = "custom-model"
            [llm.auth]
            openai_compat = "required"
            anthropic_compat = "auto"
            [llm.tls]
            insecure_skip_verify = false
        "#;
        let value: toml::Value = toml::from_str(raw).unwrap();
        let migrated = Config::migrate_if_needed(value);

        let llm = migrated.get("llm").unwrap();
        assert_eq!(
            llm.get("default_model").unwrap().as_str(),
            Some("custom-model")
        );
    }

    // ── Merge TOML values unit tests ───────────────────────────────────

    #[test]
    fn test_merge_toml_values_scalar_user_wins() {
        let default: toml::Value = toml::from_str("key = \"default\"").unwrap();
        let user: toml::Value = toml::from_str("key = \"user\"").unwrap();
        let merged = merge_toml_values(default, user);
        assert_eq!(merged.get("key").unwrap().as_str(), Some("user"));
    }

    #[test]
    fn test_merge_toml_values_nested_preserves_default() {
        let default: toml::Value =
            toml::from_str("[section]\nkey1 = \"d1\"\nkey2 = \"d2\"").unwrap();
        let user: toml::Value = toml::from_str("[section]\nkey1 = \"u1\"").unwrap();
        let merged = merge_toml_values(default, user);

        let section = merged.get("section").unwrap();
        assert_eq!(section.get("key1").unwrap().as_str(), Some("u1"));
        assert_eq!(section.get("key2").unwrap().as_str(), Some("d2"));
    }

    #[test]
    fn test_merge_toml_values_user_adds_new_key() {
        let default: toml::Value = toml::from_str("existing = 1").unwrap();
        let user: toml::Value = toml::from_str("new_key = 42").unwrap();
        let merged = merge_toml_values(default, user);

        assert_eq!(merged.get("existing").unwrap().as_integer(), Some(1));
        assert_eq!(merged.get("new_key").unwrap().as_integer(), Some(42));
    }

    // ── Global config singleton ────────────────────────────────────────

    #[test]
    fn test_get_config_returns_config() {
        let config = get_config();
        let guard = config.read();
        assert_eq!(guard.llm.default_provider, "openai-codex");
    }

    #[test]
    fn test_get_config_is_singleton() {
        let c1 = get_config();
        let c2 = get_config();
        let g1 = c1.read();
        let g2 = c2.read();
        assert_eq!(g1.llm.default_model, g2.llm.default_model);
    }
}
