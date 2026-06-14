//! Git context formatting for the system prompt.
//!
//! Gathers current branch, working tree status, and recent commits
//! to inject into the LLM's system prompt.

use std::process::Command;

/// Git context to inject into the system prompt.
#[derive(Debug, Default)]
pub struct GitContext {
    pub branch: Option<String>,
    pub status_summary: Option<String>,
    pub recent_commits: Vec<String>,
}

/// Gather git context from the current working directory.
pub fn gather_git_context() -> Option<GitContext> {
    // Quick check: are we in a git repo?
    let output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let mut ctx = GitContext::default();

    // Current branch
    if let Ok(branch) = git_branch() {
        ctx.branch = Some(branch);
    }

    // Working tree status
    if let Ok(status) = git_status() {
        ctx.status_summary = Some(status);
    }

    // Recent commits
    if let Ok(commits) = git_recent_commits(5) {
        ctx.recent_commits = commits;
    }

    Some(ctx)
}

/// Format git context for the system prompt.
pub fn format_git_context(ctx: &GitContext) -> String {
    let mut lines = Vec::new();
    lines.push("<git_status>".to_string());

    if let Some(ref branch) = ctx.branch {
        lines.push(format!("Current branch: {branch}"));
    }

    if let Some(ref status) = ctx.status_summary {
        lines.push(status.clone());
    }

    if !ctx.recent_commits.is_empty() {
        lines.push("Recent commits:".to_string());
        for commit in &ctx.recent_commits {
            lines.push(format!("  {commit}"));
        }
    }

    lines.push("</git_status>".to_string());
    lines.join("\n")
}

fn git_branch() -> Result<String, std::io::Error> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_status() -> Result<String, std::io::Error> {
    let output = Command::new("git")
        .args(["status", "--short", "-b"])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_recent_commits(count: usize) -> Result<Vec<String>, std::io::Error> {
    let output = Command::new("git")
        .args(["log", &format!("-{count}"), "--oneline", "--no-decorate"])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gather_git_context_in_repo() {
        // This test repo should have git context
        let ctx = gather_git_context();
        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert!(ctx.branch.is_some());
    }

    #[test]
    fn test_format_git_context() {
        let ctx = GitContext {
            branch: Some("main".into()),
            status_summary: Some("M src/main.rs".into()),
            recent_commits: vec!["abc1234 feat: add thing".into()],
        };
        let formatted = format_git_context(&ctx);
        assert!(formatted.contains("<git_status>"));
        assert!(formatted.contains("main"));
        assert!(formatted.contains("feat: add thing"));
        assert!(formatted.contains("</git_status>"));
    }
}
