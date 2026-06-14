//! File writing tool.
//!
//! Creates new files or overwrites existing ones. Always a mutating operation.

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::{FileChanges, ToolResult};
use crate::tools::base::Tool;
use crate::tools::utils::shorten_path;

pub struct WriteTool;

impl WriteTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WriteTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write a file to the local filesystem. Creates a new file or overwrites \
         an existing one. Use the 'edit' tool for surgical changes to existing files."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    fn is_mutating(&self) -> bool {
        true
    }

    fn icon(&self) -> &str {
        "+"
    }

    fn prompt_guidelines(&self) -> &[&str] {
        &[
            "Use 'edit' for targeted changes to existing files",
            "Use 'write' only for new files or complete rewrites",
        ]
    }

    async fn execute(
        &self,
        params: Value,
        _cancel: watch::Receiver<bool>,
    ) -> KonResult<ToolResult> {
        let file_path = params["file_path"].as_str().ok_or_else(|| KonError::Tool {
            tool: "write".into(),
            message: "missing 'file_path' parameter".into(),
        })?;

        let content = params["content"].as_str().ok_or_else(|| KonError::Tool {
            tool: "write".into(),
            message: "missing 'content' parameter".into(),
        })?;

        let path = std::path::Path::new(file_path);

        // Ensure parent directory exists
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| KonError::Tool {
                    tool: "write".into(),
                    message: format!("failed to create directory {}: {e}", parent.display()),
                })?;
        }

        let existed = path.exists();
        let old_content = if existed {
            tokio::fs::read_to_string(path).await.ok()
        } else {
            None
        };

        tokio::fs::write(path, content)
            .await
            .map_err(|e| KonError::Tool {
                tool: "write".into(),
                message: format!("failed to write {}: {e}", path.display()),
            })?;

        let line_count = content.lines().count();
        let old_lines = old_content.as_ref().map(|c| c.lines().count()).unwrap_or(0);
        let lines_added = line_count.saturating_sub(old_lines);
        let lines_removed = old_lines.saturating_sub(line_count);

        let action = if existed { "Overwrote" } else { "Created" };
        let summary = format!("{action} {} ({} lines)", shorten_path(path, 40), line_count);

        Ok(ToolResult {
            success: true,
            result: Some(format!(
                "{action} file: {}\n{} lines written.",
                path.display(),
                line_count
            )),
            images: vec![],
            ui_summary: Some(summary),
            ui_details: None,
            ui_details_full: None,
            file_changes: Some(FileChanges {
                path: path.display().to_string(),
                lines_added: lines_added as u64,
                lines_removed: lines_removed as u64,
                diff: None,
            }),
        })
    }

    fn format_call(&self, params: &Value) -> String {
        let path = params["file_path"].as_str().unwrap_or("?");
        shorten_path(std::path::Path::new(path), 50)
    }

    fn format_preview(&self, params: &Value) -> Option<String> {
        params["content"]
            .as_str()
            .map(|c| c.lines().take(5).collect::<Vec<_>>().join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_tool_metadata() {
        let tool = WriteTool::new();
        assert_eq!(tool.name(), "write");
        assert_eq!(tool.icon(), "+");
        assert!(tool.is_mutating());
    }

    #[tokio::test]
    async fn test_write_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("new_file.txt");

        let tool = WriteTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "content": "hello\nworld"
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        assert!(result.ui_summary.unwrap().contains("Created"));

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "hello\nworld");
    }

    #[tokio::test]
    async fn test_write_overwrites_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.txt");
        tokio::fs::write(&file_path, "old content").await.unwrap();

        let tool = WriteTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "content": "new content"
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        assert!(result.ui_summary.unwrap().contains("Overwrote"));

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn test_write_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("deep/nested/file.txt");

        let tool = WriteTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "content": "nested"
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        assert!(file_path.exists());
    }

    #[test]
    fn test_format_preview_shows_first_lines() {
        let tool = WriteTool::new();
        let params = serde_json::json!({
            "file_path": "test.txt",
            "content": "line1\nline2\nline3\nline4\nline5\nline6"
        });
        let preview = tool.format_preview(&params).unwrap();
        assert_eq!(preview.lines().count(), 5);
    }
}
