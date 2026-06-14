//! Regex content search tool (via ripgrep).
//!
//! Searches file contents using `rg` (ripgrep). Falls back to a helpful
//! error message if ripgrep is not installed.

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::ToolResult;
use crate::tools::base::Tool;
use crate::tools::utils::truncate_output;

pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search for a regex pattern in files. Uses ripgrep (rg) under the hood. \
         Searches respect .gitignore by default. Results include file paths and \
         line numbers. Use 'include' to filter by file pattern (e.g., '*.rs')."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "include": {
                    "type": "string",
                    "description": "File pattern to filter results (e.g., '*.rs', '*.{ts,tsx}')"
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
        "*"
    }

    fn prompt_guidelines(&self) -> &[&str] {
        &[
            "Use specific patterns to narrow results",
            "Use 'include' to filter by file type",
            "Use 'path' to search within a specific directory",
        ]
    }

    async fn execute(
        &self,
        params: Value,
        _cancel: watch::Receiver<bool>,
    ) -> KonResult<ToolResult> {
        let pattern = params["pattern"].as_str().ok_or_else(|| KonError::Tool {
            tool: "grep".into(),
            message: "missing 'pattern' parameter".into(),
        })?;

        // Check if rg is available
        if which::which("rg").is_err() {
            return Ok(ToolResult {
                success: false,
                result: Some(
                    "ripgrep (rg) is not installed. Install it with:\n\
                     - macOS: brew install ripgrep\n\
                     - Ubuntu: apt install ripgrep\n\
                     - Other: cargo install ripgrep"
                        .into(),
                ),
                images: vec![],
                ui_summary: Some("rg not installed".into()),
                ui_details: None,
                ui_details_full: None,
                file_changes: None,
            });
        }

        let mut cmd = tokio::process::Command::new("rg");
        cmd.arg("--line-number")
            .arg("--no-heading")
            .arg("--color=never")
            .arg("--sort=path")
            .arg(pattern);

        if let Some(include) = params["include"].as_str() {
            cmd.arg("--glob").arg(include);
        }

        if let Some(path) = params["path"].as_str() {
            cmd.arg(path);
        }

        let output = cmd.output().await.map_err(|e| KonError::Tool {
            tool: "grep".into(),
            message: format!("failed to run rg: {e}"),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if stdout.is_empty() {
            let no_match = format!("No matches found for pattern: {pattern}");
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

        let line_count = stdout.lines().count();
        let truncated = truncate_output(&stdout, 500, 20000);

        Ok(ToolResult {
            success: output.status.success() || output.status.code() == Some(1),
            result: Some(truncated.clone()),
            images: vec![],
            ui_summary: Some(format!("{line_count} matches for \"{pattern}\"")),
            ui_details: Some(truncated),
            ui_details_full: Some(stdout),
            file_changes: None,
        })
    }

    fn format_call(&self, params: &Value) -> String {
        let pattern = params["pattern"].as_str().unwrap_or("?");
        if pattern.len() > 40 {
            format!("grep: {}...", &pattern[..37])
        } else {
            format!("grep: {pattern}")
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
    fn test_grep_tool_metadata() {
        let tool = GrepTool::new();
        assert_eq!(tool.name(), "grep");
        assert_eq!(tool.icon(), "*");
        assert!(!tool.is_mutating());
    }

    #[tokio::test]
    async fn test_grep_finds_matches_in_temp_dir() {
        // Skip if rg not installed
        if which::which("rg").is_err() {
            return;
        }

        let tmp = tempfile::tempdir().unwrap();
        tokio::fs::write(tmp.path().join("a.txt"), "hello world\nfoo bar")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("b.txt"), "goodbye world")
            .await
            .unwrap();

        let tool = GrepTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "pattern": "world",
            "path": tmp.path().to_str().unwrap()
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        let text = result.result.unwrap();
        assert!(text.contains("world"));
    }

    #[tokio::test]
    async fn test_grep_no_matches() {
        if which::which("rg").is_err() {
            return;
        }

        let tmp = tempfile::tempdir().unwrap();
        tokio::fs::write(tmp.path().join("test.txt"), "hello world")
            .await
            .unwrap();

        let tool = GrepTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "pattern": "nonexistent_pattern_xyz",
            "path": tmp.path().to_str().unwrap()
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.result.unwrap().contains("No matches"));
    }
}
