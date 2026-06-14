//! File editing tool.
//!
//! Performs surgical text replacement on files. Supports:
//! - `old_string` / `new_string`: exact match-and-replace
//! - `replace_all`: replace every occurrence of old_string
//! - Generates diffs via the `similar` crate

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::{FileChanges, ToolResult};
use crate::tools::base::Tool;
use crate::tools::utils::shorten_path;

pub struct EditTool;

impl EditTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EditTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Perform exact string replacements in an existing file. \
         Use 'old_string' to specify the text to replace and 'new_string' \
         for the replacement. Set 'replace_all' to true to replace every \
         occurrence. The edit will FAIL if old_string is not unique in the file \
         (unless 'replace_all' is true)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_string": {
                    "type": "string",
                    "description": "The text to replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement text"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences instead of just the first one (default: false)"
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    fn is_mutating(&self) -> bool {
        true
    }

    fn icon(&self) -> &str {
        "←"
    }

    fn prompt_guidelines(&self) -> &[&str] {
        &[
            "Include enough context in old_string to make it unique in the file",
            "Preserve exact indentation (tabs/spaces) when constructing old_string",
            "Use replace_all for renaming or consistent replacements across the file",
        ]
    }

    async fn execute(
        &self,
        params: Value,
        _cancel: watch::Receiver<bool>,
    ) -> KonResult<ToolResult> {
        let file_path = params["file_path"].as_str().ok_or_else(|| KonError::Tool {
            tool: "edit".into(),
            message: "missing 'file_path' parameter".into(),
        })?;

        let old_string = params["old_string"]
            .as_str()
            .ok_or_else(|| KonError::Tool {
                tool: "edit".into(),
                message: "missing 'old_string' parameter".into(),
            })?;

        let new_string = params["new_string"]
            .as_str()
            .ok_or_else(|| KonError::Tool {
                tool: "edit".into(),
                message: "missing 'new_string' parameter".into(),
            })?;

        let replace_all = params["replace_all"].as_bool().unwrap_or(false);

        let path = std::path::Path::new(file_path);

        if !path.exists() {
            return Ok(ToolResult {
                success: false,
                result: Some(format!("File not found: {file_path}")),
                images: vec![],
                ui_summary: Some("File not found".into()),
                ui_details: None,
                ui_details_full: None,
                file_changes: None,
            });
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| KonError::Tool {
                tool: "edit".into(),
                message: format!("failed to read {}: {e}", path.display()),
            })?;

        if old_string.is_empty() {
            return Ok(ToolResult {
                success: false,
                result: Some("old_string must not be empty".into()),
                images: vec![],
                ui_summary: Some("Empty old_string".into()),
                ui_details: None,
                ui_details_full: None,
                file_changes: None,
            });
        }

        // Count occurrences
        let occurrences = content.matches(old_string).count();

        if occurrences == 0 {
            return Ok(ToolResult {
                success: false,
                result: Some(format!(
                    "old_string not found in {file_path}. The text to replace was not found in the file."
                )),
                images: vec![],
                ui_summary: Some("No match found".into()),
                ui_details: None,
                ui_details_full: None,
                file_changes: None,
            });
        }

        if occurrences > 1 && !replace_all {
            return Ok(ToolResult {
                success: false,
                result: Some(format!(
                    "Found {occurrences} matches for old_string in {file_path}. \
                     To replace all occurrences, set 'replace_all' to true. \
                     To replace only one, include more surrounding context in old_string to make it unique."
                )),
                images: vec![],
                ui_summary: Some(format!("{occurrences} matches — need unique context")),
                ui_details: None,
                ui_details_full: None,
                file_changes: None,
            });
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        // Generate diff
        let diff = similar::TextDiff::from_lines(&content, &new_content);
        let diff_str = diff.unified_diff().context_radius(3).to_string();

        // Write the modified content
        tokio::fs::write(path, &new_content)
            .await
            .map_err(|e| KonError::Tool {
                tool: "edit".into(),
                message: format!("failed to write {}: {e}", path.display()),
            })?;

        // Count actual line changes
        let old_lines = content.lines().count();
        let new_lines = new_content.lines().count();
        let lines_added = new_lines.saturating_sub(old_lines) as u64;
        let lines_removed = old_lines.saturating_sub(new_lines) as u64;

        let summary = if replace_all {
            format!(
                "Applied {} replacements in {} (+{} / -{})",
                occurrences,
                shorten_path(path, 40),
                lines_added,
                lines_removed
            )
        } else {
            format!(
                "Applied edit to {} (+{} / -{})",
                shorten_path(path, 40),
                lines_added,
                lines_removed
            )
        };

        Ok(ToolResult {
            success: true,
            result: Some(format!(
                "Successfully edited {}. {} replacement(s) made.\n\nDiff:\n{diff_str}",
                path.display(),
                if replace_all { occurrences } else { 1 }
            )),
            images: vec![],
            ui_summary: Some(summary),
            ui_details: Some(diff_str.clone()),
            ui_details_full: Some(diff_str.clone()),
            file_changes: Some(FileChanges {
                path: path.display().to_string(),
                lines_added,
                lines_removed,
                diff: Some(diff_str),
            }),
        })
    }

    fn format_call(&self, params: &Value) -> String {
        let path = params["file_path"].as_str().unwrap_or("?");
        shorten_path(std::path::Path::new(path), 50)
    }

    fn format_preview(&self, params: &Value) -> Option<String> {
        let old = params["old_string"].as_str().unwrap_or("");
        let new = params["new_string"].as_str().unwrap_or("");
        if old.len() + new.len() > 200 {
            Some(format!(
                "{} → {}",
                &old[..old.len().min(100)],
                &new[..new.len().min(100)]
            ))
        } else {
            Some(format!("{old} → {new}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_tool_metadata() {
        let tool = EditTool::new();
        assert_eq!(tool.name(), "edit");
        assert_eq!(tool.icon(), "←");
        assert!(tool.is_mutating());
    }

    #[tokio::test]
    async fn test_edit_single_replacement() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.rs");
        tokio::fs::write(&file_path, "fn hello() {\n    println!(\"hi\");\n}")
            .await
            .unwrap();

        let tool = EditTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "hello",
            "new_string": "greet"
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert!(content.contains("fn greet()"));
    }

    #[tokio::test]
    async fn test_edit_multiple_matches_without_replace_all() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.txt");
        tokio::fs::write(&file_path, "foo bar foo baz foo")
            .await
            .unwrap();

        let tool = EditTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "foo",
            "new_string": "qux"
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        // Should fail: 3 matches, not unique
        assert!(!result.success);
        assert!(result.result.unwrap().contains("3 matches"));
    }

    #[tokio::test]
    async fn test_edit_replace_all() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.txt");
        tokio::fs::write(&file_path, "foo bar foo baz foo")
            .await
            .unwrap();

        let tool = EditTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "foo",
            "new_string": "qux",
            "replace_all": true
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "qux bar qux baz qux");
    }

    #[tokio::test]
    async fn test_edit_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.txt");
        tokio::fs::write(&file_path, "hello world").await.unwrap();

        let tool = EditTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "nonexistent",
            "new_string": "replacement"
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(!result.success);
        assert!(result.result.unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_edit_generates_diff() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.txt");
        tokio::fs::write(&file_path, "line1\nold line\nline3")
            .await
            .unwrap();

        let tool = EditTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "old line",
            "new_string": "new line"
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        let text = result.result.unwrap();
        assert!(text.contains("old line"));
        assert!(text.contains("new line"));
        // Diff should be in the file_changes
        let fc = result.file_changes.unwrap();
        assert!(fc.diff.is_some());
    }
}
