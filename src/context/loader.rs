//! Unified context loader.
//!
//! Combines agent docs, git context, and skills into a single
//! context block for the system prompt.

use crate::context::agent_mds::{format_agent_docs, load_agent_docs};
use crate::context::git::{format_git_context, gather_git_context};

/// Full project context for the system prompt.
#[derive(Debug, Default)]
pub struct ProjectContext {
    pub agent_docs: Option<String>,
    pub git_context: Option<String>,
}

/// Load all available project context.
pub fn load_context() -> ProjectContext {
    let agent_docs = format_agent_docs(&load_agent_docs());
    let git_ctx = gather_git_context();
    let git_context = git_ctx.as_ref().map(format_git_context);

    ProjectContext {
        agent_docs,
        git_context,
    }
}

/// Append loaded context to the system prompt.
pub fn append_context(system_prompt: &str, ctx: &ProjectContext) -> String {
    let mut parts = vec![system_prompt.to_string()];

    if let Some(ref docs) = ctx.agent_docs {
        parts.push(docs.clone());
    }

    if let Some(ref git) = ctx.git_context {
        parts.push(git.clone());
    }

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_context() {
        let ctx = load_context();
        // In this repo, agent_docs should be Some (we have AGENTS.md)
        assert!(ctx.agent_docs.is_some());
        // git_context should be Some (we're in a git repo)
        assert!(ctx.git_context.is_some());
    }

    #[test]
    fn test_append_context() {
        let ctx = ProjectContext {
            agent_docs: Some("<agents_md>\n# Rules\n</agents_md>".into()),
            git_context: Some("<git_status>\nbranch: main\n</git_status>".into()),
        };
        let result = append_context("You are Zeus.", &ctx);
        assert!(result.contains("You are Zeus."));
        assert!(result.contains("# Rules"));
        assert!(result.contains("branch: main"));
    }

    #[test]
    fn test_append_context_empty() {
        let ctx = ProjectContext::default();
        let result = append_context("base prompt", &ctx);
        assert_eq!(result, "base prompt");
    }
}
