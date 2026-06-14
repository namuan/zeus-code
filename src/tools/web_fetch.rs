//! URL content fetching tool.
//!
//! Fetches content from a URL and returns it as markdown or plain text.

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::ToolResult;
use crate::tools::base::Tool;

pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch content from a URL. Returns the content as markdown (default) \
         or plain text. Useful for reading documentation, API references, \
         and other web-based resources."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch content from"
                },
                "format": {
                    "type": "string",
                    "enum": ["markdown", "text", "html"],
                    "description": "Format to return content in (default: markdown)"
                }
            },
            "required": ["url"]
        })
    }

    fn is_mutating(&self) -> bool {
        false
    }

    fn icon(&self) -> &str {
        "%"
    }

    fn prompt_guidelines(&self) -> &[&str] {
        &[
            "Use for reading documentation and API references",
            "Prefer specific URLs over general ones",
            "Fetched content may be summarized if very large",
        ]
    }

    async fn execute(
        &self,
        params: Value,
        _cancel: watch::Receiver<bool>,
    ) -> KonResult<ToolResult> {
        let url = params["url"].as_str().ok_or_else(|| KonError::Tool {
            tool: "web_fetch".into(),
            message: "missing 'url' parameter".into(),
        })?;

        let format = params["format"].as_str().unwrap_or("markdown");

        // Simplified fetch — full implementation in Phase 10
        // would use reqwest + HTML-to-markdown conversion.
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| KonError::Tool {
                tool: "web_fetch".into(),
                message: format!("failed to create HTTP client: {e}"),
            })?;

        let response = client
            .get(url)
            .header("User-Agent", "zeus-code/0.1")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    KonError::Tool {
                        tool: "web_fetch".into(),
                        message: format!("request to {url} timed out"),
                    }
                } else {
                    KonError::Tool {
                        tool: "web_fetch".into(),
                        message: format!("failed to fetch {url}: {e}"),
                    }
                }
            })?;

        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();

        if !status.is_success() {
            return Ok(ToolResult {
                success: false,
                result: Some(format!("HTTP {status}: failed to fetch {url}")),
                images: vec![],
                ui_summary: Some(format!("HTTP {status}")),
                ui_details: None,
                ui_details_full: None,
                file_changes: None,
            });
        }

        let body = response.text().await.map_err(|e| KonError::Tool {
            tool: "web_fetch".into(),
            message: format!("failed to read response body: {e}"),
        })?;

        let result_text = match format {
            "html" => body.clone(),
            _ => {
                // Simple HTML → text conversion (strip tags)
                let text = strip_html(&body);
                if text.len() > 10000 {
                    format!(
                        "{}\n\n[Content truncated: {} chars total. URL: {url}]",
                        &text[..10000],
                        text.len()
                    )
                } else {
                    text
                }
            }
        };

        Ok(ToolResult {
            success: true,
            result: Some(result_text),
            images: vec![],
            ui_summary: Some(format!(
                "Fetched {} ({:.1} KB, {content_type})",
                url,
                body.len() as f64 / 1024.0
            )),
            ui_details: None,
            ui_details_full: None,
            file_changes: None,
        })
    }

    fn format_call(&self, params: &Value) -> String {
        let url = params["url"].as_str().unwrap_or("?");
        if url.len() > 50 {
            format!("fetch: {}...", &url[..47])
        } else {
            format!("fetch: {url}")
        }
    }

    fn format_preview(&self, _params: &Value) -> Option<String> {
        None
    }
}

/// Simple HTML tag stripper (not a full HTML→text converter).
fn strip_html(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut tag_name = String::new();
    let mut skip_content = false;
    let mut prev_was_close = false;

    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            tag_name.clear();
            prev_was_close = false;
            continue;
        }

        if in_tag {
            if c == '>' {
                in_tag = false;
                let lower = tag_name.to_lowercase();
                match lower.as_str() {
                    "script" | "style" => skip_content = true,
                    "/script" | "/style" => skip_content = false,
                    "p" | "/p" | "br" | "/div" | "/tr" | "/li" | "/h1" | "/h2" | "/h3" => {
                        if !prev_was_close {
                            result.push('\n');
                        }
                    }
                    _ => {}
                }
                prev_was_close = true;
                tag_name.clear();
            } else {
                tag_name.push(c);
            }
            continue;
        }

        if !skip_content {
            result.push(c);
        }
    }

    // Collapse whitespace and trim
    result
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_fetch_tool_metadata() {
        let tool = WebFetchTool::new();
        assert_eq!(tool.name(), "web_fetch");
        assert!(!tool.is_mutating());
    }

    #[test]
    fn test_strip_html_basic() {
        let html = "<html><body><p>Hello</p><p>World</p></body></html>";
        let text = strip_html(html);
        assert_eq!(text, "Hello\nWorld");
    }

    #[test]
    fn test_strip_html_removes_scripts() {
        let html = "<html><script>evil()</script><p>safe</p></html>";
        let text = strip_html(html);
        assert!(text.contains("safe"));
        assert!(!text.contains("evil"));
    }

    #[test]
    fn test_strip_html_handles_entities() {
        // HTML entities are left as-is (not decoded) in this simple implementation
        let html = "<p>hello &amp; goodbye</p>";
        let text = strip_html(html);
        assert!(text.contains("&amp;"));
    }
}
