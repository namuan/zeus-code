//! AGENTS.md / CLAUDE.md discovery and loading.
//!
//! Walks from the current working directory up to the git root
//! (or home directory), loading all matching instruction files.

use std::path::PathBuf;

/// Loaded project instructions from AGENTS.md/CLAUDE.md files.
#[derive(Debug, Default)]
pub struct AgentDocs {
    /// Path and content of each loaded file.
    pub files: Vec<(PathBuf, String)>,
}

/// Discover and load AGENTS.md/CLAUDE.md files from the project tree.
pub fn load_agent_docs() -> AgentDocs {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));

    // Find the boundary: git root or home directory
    let boundary = find_git_root(&cwd).unwrap_or(home);

    let mut docs = AgentDocs::default();
    let mut current = cwd;

    // Walk up from cwd to boundary
    loop {
        for name in &["AGENTS.md", "CLAUDE.md"] {
            let path = current.join(name);
            if path.exists()
                && let Ok(content) = std::fs::read_to_string(&path)
            {
                docs.files.push((path, content));
            }
        }

        if current == boundary {
            break;
        }
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }

    docs
}

/// Format loaded agent docs for the system prompt.
pub fn format_agent_docs(docs: &AgentDocs) -> Option<String> {
    if docs.files.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    parts.push("<agents_md>".to_string());

    for (path, content) in &docs.files {
        let relative = path
            .strip_prefix(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());

        parts.push(format!("## {relative}\n\n{content}"));
    }

    parts.push("</agents_md>".to_string());
    Some(parts.join("\n\n"))
}

fn find_git_root(start: &PathBuf) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(start)
        .output()
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Some(PathBuf::from(path))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_agent_docs_finds_agents_md() {
        // This project has AGENTS.md in the root
        let docs = load_agent_docs();
        assert!(!docs.files.is_empty());

        // At least one should be AGENTS.md
        let has_agents = docs
            .files
            .iter()
            .any(|(p, _)| p.file_name().is_some_and(|n| n == "AGENTS.md"));
        assert!(has_agents);
    }

    #[test]
    fn test_format_agent_docs() {
        let docs = AgentDocs {
            files: vec![(
                PathBuf::from("AGENTS.md"),
                "# Project Rules\n\n- Rule 1\n- Rule 2".into(),
            )],
        };
        let formatted = format_agent_docs(&docs);
        assert!(formatted.is_some());
        let text = formatted.unwrap();
        assert!(text.contains("<agents_md>"));
        assert!(text.contains("Rule 1"));
    }

    #[test]
    fn test_format_empty_docs() {
        let docs = AgentDocs::default();
        assert!(format_agent_docs(&docs).is_none());
    }
}
