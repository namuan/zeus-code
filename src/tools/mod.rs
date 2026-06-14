pub mod base;
pub mod bash;
pub mod edit;
pub mod find;
pub mod grep;
pub mod read;
pub mod utils;
pub mod web_fetch;
pub mod web_search;
pub mod write;

use crate::core::types::ToolDefinition;
use crate::tools::base::Tool;

// ── Tool registry ────────────────────────────────────────────────────────

/// Return the 6 core tools enabled by default.
pub fn core_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(read::ReadTool::new()),
        Box::new(edit::EditTool::new()),
        Box::new(write::WriteTool::new()),
        Box::new(bash::BashTool::new()),
        Box::new(grep::GrepTool::new()),
        Box::new(find::FindTool::new()),
    ]
}

/// Return the 2 extra tools (configurable via `[tools] extra = [...]`).
pub fn extra_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(web_search::WebSearchTool::new()),
        Box::new(web_fetch::WebFetchTool::new()),
    ]
}

/// Look up a tool by name from a list of active tools.
pub fn get_tool<'a>(name: &str, active_tools: &'a [Box<dyn Tool>]) -> Option<&'a dyn Tool> {
    active_tools
        .iter()
        .find(|t| t.name() == name)
        .map(|t| t.as_ref())
}

/// Build the tool definitions sent to the LLM.
pub fn get_tool_definitions(tools: &[Box<dyn Tool>]) -> Vec<ToolDefinition> {
    tools
        .iter()
        .map(|t| ToolDefinition {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters_schema(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_tools_count() {
        let tools = core_tools();
        assert_eq!(tools.len(), 6);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"read"));
        assert!(names.contains(&"edit"));
        assert!(names.contains(&"write"));
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"grep"));
        assert!(names.contains(&"find"));
    }

    #[test]
    fn test_extra_tools_count() {
        let tools = extra_tools();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_get_tool_found() {
        let tools = core_tools();
        let tool = get_tool("read", &tools);
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name(), "read");
    }

    #[test]
    fn test_get_tool_not_found() {
        let tools = core_tools();
        assert!(get_tool("nonexistent", &tools).is_none());
    }

    #[test]
    fn test_get_tool_definitions() {
        let tools = core_tools();
        let defs = get_tool_definitions(&tools);
        assert_eq!(defs.len(), 6);
        for def in &defs {
            assert!(!def.name.is_empty());
            assert!(!def.description.is_empty());
        }
    }
}
