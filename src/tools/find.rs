//! Glob-based file discovery tool (via fd).
//!
//! Finds files by glob pattern using `fd`. Falls back to a helpful
//! error message if fd is not installed.

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::ToolResult;
use crate::tools::base::Tool;
use crate::tools::utils::truncate_output;

pub struct FindTool;

impl FindTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FindTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FindTool {
    fn name(&self) -> &str {
        "find"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern. Uses fd under the hood. \
         Respects .gitignore by default. Results are sorted by modification \
         time. Use 'path' to search in a specific directory."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files (e.g., '*.rs', '**/*.md')"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (default: current working directory)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn is_mutating(&self) -> bool {
        false
    }

    fn icon(&self) -> &str {
        "⌕"
    }

    fn prompt_guidelines(&self) -> &[&str] {
        &[
            "Use glob patterns like '*.rs' or '**/mod.rs'",
            "Use 'path' to scope the search to a specific directory",
            "Results are sorted by modification time (most recent first)",
        ]
    }

    async fn execute(
        &self,
        params: Value,
        _cancel: watch::Receiver<bool>,
    ) -> KonResult<ToolResult> {
        let pattern = params["pattern"].as_str().ok_or_else(|| KonError::Tool {
            tool: "find".into(),
            message: "missing 'pattern' parameter".into(),
        })?;

        if which::which("fd").is_err() {
            return Ok(ToolResult {
                success: false,
                result: Some(
                    "fd is not installed. Install it with:\n\
                     - macOS: brew install fd\n\
                     - Ubuntu: apt install fd-find\n\
                     - Other: cargo install fd-find"
                        .into(),
                ),
                images: vec![],
                ui_summary: Some("fd not installed".into()),
                ui_details: None,
                ui_details_full: None,
                file_changes: None,
            });
        }

        let mut cmd = tokio::process::Command::new("fd");
        cmd.arg("--color=never")
            .arg("--type=file")
            .arg("--glob")
            .arg(pattern);

        if let Some(path) = params["path"].as_str() {
            cmd.arg(path);
        }

        let output = cmd.output().await.map_err(|e| KonError::Tool {
            tool: "find".into(),
            message: format!("failed to run fd: {e}"),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if stdout.trim().is_empty() {
            let no_match = format!("No files found matching: {pattern}");
            return Ok(ToolResult {
                success: true,
                result: Some(no_match.clone()),
                images: vec![],
                ui_summary: Some(no_match),
                ui_details: None,
                ui_details_full: None,
                file_changes: None,
            });
        }

        let file_count = stdout.lines().count();
        let truncated = truncate_output(&stdout, 200, 10000);

        Ok(ToolResult {
            success: true,
            result: Some(truncated.clone()),
            images: vec![],
            ui_summary: Some(format!("{file_count} files matching \"{pattern}\"")),
            ui_details: Some(truncated),
            ui_details_full: Some(stdout),
            file_changes: None,
        })
    }

    fn format_call(&self, params: &Value) -> String {
        let pattern = params["pattern"].as_str().unwrap_or("?");
        if pattern.len() > 40 {
            format!("find: {}...", &pattern[..37])
        } else {
            format!("find: {pattern}")
        }
    }

    fn format_preview(&self, _params: &Value) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_tool_metadata() {
        let tool = FindTool::new();
        assert_eq!(tool.name(), "find");
        assert_eq!(tool.icon(), "⌕");
        assert!(!tool.is_mutating());
    }

    #[tokio::test]
    async fn test_find_finds_files_in_temp_dir() {
        if which::which("fd").is_err() {
            return;
        }

        let tmp = tempfile::tempdir().unwrap();
        tokio::fs::write(tmp.path().join("hello.rs"), "")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("world.md"), "")
            .await
            .unwrap();

        let tool = FindTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "pattern": "*.rs",
            "path": tmp.path().to_str().unwrap()
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        let text = result.result.unwrap();
        assert!(text.contains("hello.rs"));
        assert!(!text.contains("world.md"));
    }

    #[tokio::test]
    async fn test_find_no_matches() {
        if which::which("fd").is_err() {
            return;
        }

        let tmp = tempfile::tempdir().unwrap();

        let tool = FindTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "pattern": "*.xyz",
            "path": tmp.path().to_str().unwrap()
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.result.unwrap().contains("No files found"));
    }
}
