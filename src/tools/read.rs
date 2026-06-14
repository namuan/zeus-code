//! File reading tool.
//!
//! Reads file contents with line numbers. Supports text files (with offset/limit
//! for pagination), image files (base64-encoded), and directory listing.

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::ToolResult;
use crate::tools::base::Tool;
use crate::tools::utils::{format_with_line_numbers, shorten_path};

/// Default read limit when the model doesn't specify one (controls context bloat).
const DEFAULT_READ_LIMIT: usize = 500;

pub struct ReadTool;

impl ReadTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read a file from the local filesystem. Supports text files (returns content \
         with line numbers, up to 500 lines by default — use offset/limit to paginate), \
         image files (returns base64), and directory listing."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file or directory to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed, for text files)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to return (for text files)"
                }
            },
            "required": ["file_path"]
        })
    }

    fn is_mutating(&self) -> bool {
        false
    }

    fn icon(&self) -> &str {
        "→"
    }

    fn prompt_guidelines(&self) -> &[&str] {
        &[
            "Use offset/limit for large files instead of reading the entire file",
            "Prefer reading specific ranges over whole files when possible",
        ]
    }

    async fn execute(
        &self,
        params: Value,
        _cancel: watch::Receiver<bool>,
    ) -> KonResult<ToolResult> {
        let file_path = params["file_path"].as_str().ok_or_else(|| KonError::Tool {
            tool: "read".into(),
            message: "missing or invalid 'file_path' parameter".into(),
        })?;

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

        // Directory listing
        if path.is_dir() {
            return self.read_directory(path).await;
        }

        // Check if it's an image file
        if is_image_file(path) {
            return self.read_image(path).await;
        }

        // Default: read as text
        self.read_text(path, &params).await
    }

    fn format_call(&self, params: &Value) -> String {
        let path = params["file_path"].as_str().unwrap_or("?");
        shorten_path(std::path::Path::new(path), 60)
    }

    fn format_preview(&self, _params: &Value) -> Option<String> {
        None
    }
}

impl ReadTool {
    async fn read_text(&self, path: &std::path::Path, params: &Value) -> KonResult<ToolResult> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| KonError::Tool {
                tool: "read".into(),
                message: format!("failed to read {}: {e}", path.display()),
            })?;

        let offset = params["offset"].as_u64().unwrap_or(1).max(1) as usize;
        // Enforce a limit to prevent context-window bloat. If the model didn't
        // specify one, apply the default so a single read can't flood context.
        let limit = params["limit"]
            .as_u64()
            .map(|l| (l as usize).min(DEFAULT_READ_LIMIT))
            .unwrap_or(DEFAULT_READ_LIMIT);

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // Apply offset (1-indexed → 0-indexed)
        let start = (offset - 1).min(total_lines);
        let end = (start + limit).min(total_lines);

        let selected: Vec<&str> = lines[start..end].to_vec();
        let formatted = format_with_line_numbers(&selected.join("\n"), offset);

        let summary = if total_lines == 0 {
            "Empty file".into()
        } else if params["limit"].as_u64().is_some() {
            format!("Lines {}-{} of {}", offset, end, total_lines)
        } else if total_lines > DEFAULT_READ_LIMIT {
            format!(
                "First {} of {} lines (use offset/limit for more)",
                end.saturating_sub(offset).max(0),
                total_lines
            )
        } else {
            format!("Read {} lines", total_lines)
        };

        Ok(ToolResult {
            success: true,
            result: Some(formatted),
            images: vec![],
            ui_summary: Some(summary),
            ui_details: Some(
                serde_json::json!({
                    "path": path.display().to_string(),
                    "total_lines": total_lines,
                    "offset": offset,
                    "limit": limit,
                })
                .to_string(),
            ),
            ui_details_full: None,
            file_changes: None,
        })
    }

    async fn read_directory(&self, path: &std::path::Path) -> KonResult<ToolResult> {
        let mut entries = tokio::fs::read_dir(path)
            .await
            .map_err(|e| KonError::Tool {
                tool: "read".into(),
                message: format!("failed to read directory {}: {e}", path.display()),
            })?;

        let mut listing = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|e| KonError::Tool {
            tool: "read".into(),
            message: format!("error reading directory entry: {e}"),
        })? {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry
                .file_type()
                .await
                .map(|ft| ft.is_dir())
                .unwrap_or(false);
            listing.push(format!("{}{}", name, if is_dir { "/" } else { "" }));
        }

        listing.sort();

        let result = if listing.is_empty() {
            "(empty directory)".into()
        } else {
            listing.join("\n")
        };

        Ok(ToolResult {
            success: true,
            result: Some(result),
            images: vec![],
            ui_summary: Some(format!("{} entries", listing.len())),
            ui_details: None,
            ui_details_full: None,
            file_changes: None,
        })
    }

    async fn read_image(&self, path: &std::path::Path) -> KonResult<ToolResult> {
        let bytes = tokio::fs::read(path).await.map_err(|e| KonError::Tool {
            tool: "read".into(),
            message: format!("failed to read image {}: {e}", path.display()),
        })?;

        let base64_str = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);

        let media_type = mime_type(path);

        Ok(ToolResult {
            success: true,
            result: Some(format!(
                "Image file: {} ({} bytes, {}",
                path.display(),
                bytes.len(),
                media_type
            )),
            images: vec![crate::core::types::ImageContent {
                base64: base64_str,
                media_type: media_type.to_string(),
            }],
            ui_summary: Some(format!(
                "📷 {} ({} KB)",
                path.file_name().unwrap_or_default().to_string_lossy(),
                bytes.len() / 1024
            )),
            ui_details: None,
            ui_details_full: None,
            file_changes: None,
        })
    }
}

fn is_image_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ico")
    )
}

fn mime_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_tool_metadata() {
        let tool = ReadTool::new();
        assert_eq!(tool.name(), "read");
        assert_eq!(tool.icon(), "→");
        assert!(!tool.is_mutating());
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_read_tool_schema_requires_file_path() {
        let tool = ReadTool::new();
        let schema = tool.parameters_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("file_path")));
    }

    #[tokio::test]
    async fn test_read_text_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.txt");
        tokio::fs::write(&file_path, "line 1\nline 2\nline 3")
            .await
            .unwrap();

        let tool = ReadTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({"file_path": file_path.to_str().unwrap()});
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        let text = result.result.unwrap();
        assert!(text.contains("1: line 1"));
        assert!(text.contains("3: line 3"));
    }

    #[tokio::test]
    async fn test_read_with_offset_and_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.txt");
        let content: String = (1..=10).map(|i| format!("line {i}\n")).collect();
        tokio::fs::write(&file_path, &content).await.unwrap();

        let tool = ReadTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "offset": 5,
            "limit": 3
        });
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        let text = result.result.unwrap();
        assert!(text.contains("5: line 5"));
        assert!(text.contains("7: line 7"));
        assert!(!text.contains("line 4"));
        assert!(!text.contains("line 8"));
    }

    #[tokio::test]
    async fn test_read_directory() {
        let tmp = tempfile::tempdir().unwrap();
        tokio::fs::write(tmp.path().join("a.txt"), "")
            .await
            .unwrap();
        tokio::fs::create_dir(tmp.path().join("subdir"))
            .await
            .unwrap();

        let tool = ReadTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({"file_path": tmp.path().to_str().unwrap()});
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        let text = result.result.unwrap();
        assert!(text.contains("a.txt"));
        assert!(text.contains("subdir/"));
    }

    #[tokio::test]
    async fn test_read_missing_file() {
        let tool = ReadTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({"file_path": "/nonexistent/file.txt"});
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(!result.success);
        assert!(result.result.unwrap().contains("not found"));
    }
}
