use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::core::errors::KonResult;
use crate::core::types::ToolResult;

/// The Tool trait — every built-in and custom tool implements this.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool name (e.g., "read", "edit", "bash").
    fn name(&self) -> &str;

    /// Description shown to the LLM in the system prompt.
    fn description(&self) -> &str;

    /// JSON Schema for the tool's parameters (sent to LLM as function definition).
    fn parameters_schema(&self) -> Value;

    /// Whether this tool modifies files or system state.
    fn is_mutating(&self) -> bool;

    /// Icon shown in the UI (e.g., "→", "$", "←", "+").
    fn icon(&self) -> &str;

    /// Guidelines injected into the system prompt for this tool.
    fn prompt_guidelines(&self) -> &[&str];

    /// Execute the tool with raw JSON parameters.
    async fn execute(&self, params: Value, cancel: watch::Receiver<bool>) -> KonResult<ToolResult>;

    /// Short human-readable call text for the UI header.
    fn format_call(&self, params: &Value) -> String;

    /// Preview text shown at approval time (None = no preview).
    fn format_preview(&self, params: &Value) -> Option<String>;
}
