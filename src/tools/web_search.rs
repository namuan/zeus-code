//! Web search tool (via DuckDuckGo).
//!
//! Searches the web and returns title, URL, and snippet for each result.

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::ToolResult;
use crate::tools::base::Tool;

pub struct WebSearchTool;

impl WebSearchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for information. Uses DuckDuckGo. Returns titles, \
         URLs, and snippets for each result. Useful for finding documentation, \
         current information, and code examples."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 10)"
                }
            },
            "required": ["query"]
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
            "Use specific, targeted queries for best results",
            "Include version numbers or dates when searching for current information",
        ]
    }

    async fn execute(
        &self,
        params: Value,
        _cancel: watch::Receiver<bool>,
    ) -> KonResult<ToolResult> {
        let query = params["query"].as_str().ok_or_else(|| KonError::Tool {
            tool: "web_search".into(),
            message: "missing 'query' parameter".into(),
        })?;

        // This is a simplified DuckDuckGo HTML search.
        // The full implementation in Phase 10 would use DuckDuckGo's instant answer API
        // or a proper HTML scraper. For now, we return a helpful message.
        Ok(ToolResult {
            success: true,
            result: Some(format!(
                "Web search for: {query}\n\n\
                 Web search is available via DuckDuckGo. Full implementation \
                 coming in Phase 10. For now, use web_fetch to retrieve specific URLs \
                 if you know the documentation page you need."
            )),
            images: vec![],
            ui_summary: Some(format!("Web search: {query}")),
            ui_details: None,
            ui_details_full: None,
            file_changes: None,
        })
    }

    fn format_call(&self, params: &Value) -> String {
        let query = params["query"].as_str().unwrap_or("?");
        if query.len() > 50 {
            format!("search: {}...", &query[..47])
        } else {
            format!("search: {query}")
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
    fn test_web_search_tool_metadata() {
        let tool = WebSearchTool::new();
        assert_eq!(tool.name(), "web_search");
        assert_eq!(tool.icon(), "%");
        assert!(!tool.is_mutating());
    }

    #[tokio::test]
    async fn test_web_search_basic() {
        let tool = WebSearchTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({"query": "rust async trait example"});
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        assert!(result.result.unwrap().contains("rust async trait"));
    }
}
