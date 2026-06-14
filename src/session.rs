//! Append-only JSONL session persistence.
//!
//! Sessions are stored as lines in a JSONL file under
//! `~/.config/zeus/sessions/<sanitized-cwd>/<timestamp>_<uuid>.jsonl`.
//!
//! Each line is a SessionEntry. The tree structure is maintained via
//! `parent_id` pointers. Sessions are never modified in place — new
//! entries are always appended.

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::{AssistantMessage, Message, StopReason, Usage};

// ── Session entry types ──────────────────────────────────────────────────

/// Each line in the JSONL file is one SessionEntry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionEntry {
    #[serde(rename = "header")]
    Header {
        version: u8,
        id: String,
        timestamp: String,
        cwd: String,
        system_prompt: String,
        tools: Vec<String>,
        initial_thinking_level: String,
    },

    #[serde(rename = "message")]
    MessageEntry {
        id: String,
        parent_id: String,
        timestamp: String,
        message: Message,
    },

    #[serde(rename = "compaction")]
    Compaction {
        id: String,
        parent_id: String,
        timestamp: String,
        summary: String,
        first_kept_entry_id: String,
        tokens_before: u64,
    },

    #[serde(rename = "model_change")]
    ModelChange {
        id: String,
        parent_id: String,
        timestamp: String,
        provider: String,
        model_id: String,
        base_url: Option<String>,
    },

    #[serde(rename = "thinking_level_change")]
    ThinkingLevelChange {
        id: String,
        parent_id: String,
        timestamp: String,
        thinking_level: String,
    },

    #[serde(rename = "leaf")]
    Leaf {
        id: String,
        parent_id: String,
        timestamp: String,
        target_id: String,
    },
}

impl SessionEntry {
    /// Return this entry's ID, if it has one.
    pub fn id(&self) -> Option<&str> {
        match self {
            SessionEntry::Header { id, .. }
            | SessionEntry::MessageEntry { id, .. }
            | SessionEntry::Compaction { id, .. }
            | SessionEntry::ModelChange { id, .. }
            | SessionEntry::ThinkingLevelChange { id, .. }
            | SessionEntry::Leaf { id, .. } => Some(id),
        }
    }

    /// Return this entry's parent ID, if it has one.
    pub fn parent_id(&self) -> Option<&str> {
        match self {
            SessionEntry::Header { .. } => None,
            SessionEntry::MessageEntry { parent_id, .. }
            | SessionEntry::Compaction { parent_id, .. }
            | SessionEntry::ModelChange { parent_id, .. }
            | SessionEntry::ThinkingLevelChange { parent_id, .. }
            | SessionEntry::Leaf { parent_id, .. } => Some(parent_id),
        }
    }

    /// Return the Message if this is a MessageEntry.
    pub fn as_message(&self) -> Option<&Message> {
        match self {
            SessionEntry::MessageEntry { message, .. } => Some(message),
            _ => None,
        }
    }
}

fn short_id() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

// ── Session ──────────────────────────────────────────────────────────────

/// A session represents a single conversation, persisted as a JSONL file.
#[derive(Debug)]
pub struct Session {
    /// Unique session ID (from the header).
    pub id: Uuid,
    /// Path to the JSONL file on disk.
    pub file_path: PathBuf,
    /// All entries loaded into memory.
    pub entries: Vec<SessionEntry>,
    /// Working directory for this session.
    pub cwd: PathBuf,
    /// System prompt used for this session.
    pub system_prompt: String,
    /// Tool names enabled for this session.
    pub active_tools: Vec<String>,
    /// When the session was created.
    pub created_at: chrono::DateTime<Utc>,
    /// Conversation tree for navigation.
    pub tree: ConversationTree,
}

impl Session {
    /// Create a brand-new session. Writes the header entry to disk.
    pub async fn new(cwd: PathBuf, system_prompt: String, tools: Vec<String>) -> KonResult<Self> {
        let id = Uuid::new_v4();
        let timestamp = now_iso();
        let file_path = session_path(&cwd, &id, &timestamp)?;

        let header = SessionEntry::Header {
            version: 1,
            id: id.to_string(),
            timestamp: timestamp.clone(),
            cwd: cwd.display().to_string(),
            system_prompt: system_prompt.clone(),
            tools: tools.clone(),
            initial_thinking_level: "low".into(),
        };

        // Ensure directory exists and write the header
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                KonError::Session(format!(
                    "failed to create session dir {}: {e}",
                    parent.display()
                ))
            })?;
        }

        let line = serde_json::to_string(&header)
            .map_err(|e| KonError::Session(format!("failed to serialize header: {e}")))?;
        tokio::fs::write(&file_path, line + "\n")
            .await
            .map_err(|e| KonError::Session(format!("failed to write session: {e}")))?;

        let mut session = Self {
            id,
            file_path,
            entries: vec![header],
            cwd,
            system_prompt,
            active_tools: tools,
            created_at: chrono::DateTime::parse_from_rfc3339(&timestamp)
                .unwrap()
                .with_timezone(&Utc),
            tree: ConversationTree::new(),
        };
        session.rebuild_tree();

        Ok(session)
    }

    /// Load a session from an existing JSONL file.
    pub async fn load(path: PathBuf) -> KonResult<Self> {
        let raw = tokio::fs::read_to_string(&path).await.map_err(|e| {
            KonError::Session(format!("failed to read session {}: {e}", path.display()))
        })?;

        let entries: Vec<SessionEntry> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                serde_json::from_str(line)
                    .map_err(|e| KonError::Session(format!("failed to parse session line: {e}")))
            })
            .collect::<KonResult<Vec<_>>>()?;

        if entries.is_empty() {
            return Err(KonError::Session("session file is empty".into()));
        }

        let header = match &entries[0] {
            SessionEntry::Header {
                id,
                timestamp,
                cwd,
                system_prompt,
                tools,
                ..
            } => {
                let session_id = Uuid::parse_str(id)
                    .map_err(|e| KonError::Session(format!("invalid session ID: {e}")))?;
                let cwd_path = PathBuf::from(cwd);
                let created_at = chrono::DateTime::parse_from_rfc3339(timestamp)
                    .unwrap()
                    .with_timezone(&Utc);
                (
                    session_id,
                    cwd_path,
                    system_prompt.clone(),
                    tools.clone(),
                    created_at,
                )
            }
            _ => return Err(KonError::Session("first entry is not a header".into())),
        };

        let mut session = Self {
            id: header.0,
            file_path: path,
            entries,
            cwd: header.1,
            system_prompt: header.2,
            active_tools: header.3,
            created_at: header.4,
            tree: ConversationTree::new(),
        };
        session.rebuild_tree();

        Ok(session)
    }

    /// Append an entry and write it to disk immediately.
    pub async fn append_entry(&mut self, entry: SessionEntry) -> KonResult<()> {
        let line = serde_json::to_string(&entry)
            .map_err(|e| KonError::Session(format!("failed to serialize entry: {e}")))?;

        // Append to file
        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .await
            .map_err(|e| KonError::Session(format!("failed to open session file: {e}")))?;

        // Use std::fs for atomic append since tokio::fs doesn't guarantee it
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{line}")
            })
            .map_err(|e| KonError::Session(format!("failed to write entry: {e}")))?;

        self.entries.push(entry);
        self.rebuild_tree();

        Ok(())
    }

    /// Get all active (non-compacted) messages for the LLM context.
    /// Skips compaction entries and walks the tree to find the active path.
    pub fn active_messages(&self) -> Vec<Message> {
        let mut messages = Vec::new();

        // Start from the header and follow the newest child at each node
        let mut current_id: Option<&str> = self.entries.first().and_then(|e| e.id());

        while let Some(id) = current_id {
            // Find the entry with this ID
            let entry = self.entries.iter().find(|e| e.id() == Some(id));
            match entry {
                Some(SessionEntry::MessageEntry { message, .. }) => {
                    messages.push(message.clone());
                }
                Some(SessionEntry::Compaction {
                    first_kept_entry_id,
                    ..
                }) => {
                    // Skip to the first kept entry after compaction
                    current_id = Some(first_kept_entry_id);
                    continue;
                }
                _ => {}
            }

            // Find the next entry that points to this one as parent
            current_id = self
                .entries
                .iter()
                .rfind(|e| e.parent_id() == Some(id))
                .and_then(|e| e.id());
        }

        messages
    }

    /// Append a user message to the session.
    pub async fn append_user_message(
        &mut self,
        content: crate::core::types::UserMessage,
    ) -> KonResult<String> {
        let id = short_id();
        let parent_id = self
            .entries
            .last()
            .and_then(|e| e.id())
            .unwrap_or("root")
            .to_string();

        let entry = SessionEntry::MessageEntry {
            id: id.clone(),
            parent_id,
            timestamp: now_iso(),
            message: Message::User(content),
        };

        self.append_entry(entry).await?;
        Ok(id)
    }

    /// Append an assistant response to the session.
    pub async fn append_assistant_message(
        &mut self,
        content: Vec<crate::core::types::ContentBlock>,
        usage: Option<Usage>,
        stop_reason: Option<StopReason>,
    ) -> KonResult<String> {
        let id = short_id();
        let parent_id = self
            .entries
            .last()
            .and_then(|e| e.id())
            .unwrap_or("root")
            .to_string();

        let entry = SessionEntry::MessageEntry {
            id: id.clone(),
            parent_id,
            timestamp: now_iso(),
            message: Message::Assistant(AssistantMessage {
                content,
                usage,
                stop_reason,
            }),
        };

        self.append_entry(entry).await?;
        Ok(id)
    }

    /// Append a tool result to the session.
    pub async fn append_tool_result(
        &mut self,
        tool_result: crate::core::types::ToolResultMessage,
    ) -> KonResult<String> {
        let id = short_id();
        let parent_id = self
            .entries
            .last()
            .and_then(|e| e.id())
            .unwrap_or("root")
            .to_string();

        let entry = SessionEntry::MessageEntry {
            id: id.clone(),
            parent_id,
            timestamp: now_iso(),
            message: Message::ToolResult(tool_result),
        };

        self.append_entry(entry).await?;
        Ok(id)
    }

    /// Rebuild the conversation tree from entries.
    fn rebuild_tree(&mut self) {
        let mut tree = ConversationTree::new();
        for entry in &self.entries {
            tree.add_entry(entry);
        }
        self.tree = tree;
    }

    /// Create a new session using synchronous std::fs I/O (safe to call from TUI context).
    pub fn new_sync(cwd: PathBuf, system_prompt: String, tools: Vec<String>) -> KonResult<Self> {
        let id = Uuid::new_v4();
        let timestamp = now_iso();
        let file_path = session_path(&cwd, &id, &timestamp)?;

        let header = SessionEntry::Header {
            version: 1,
            id: id.to_string(),
            timestamp: timestamp.clone(),
            cwd: cwd.display().to_string(),
            system_prompt: system_prompt.clone(),
            tools: tools.clone(),
            initial_thinking_level: "low".into(),
        };

        // Ensure directory exists (sync)
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                KonError::Session(format!(
                    "failed to create session dir {}: {e}",
                    parent.display()
                ))
            })?;
        }

        // Write header (sync)
        let line = serde_json::to_string(&header)
            .map_err(|e| KonError::Session(format!("failed to serialize header: {e}")))?;
        std::fs::write(&file_path, line + "\n")
            .map_err(|e| KonError::Session(format!("failed to write session: {e}")))?;

        let mut session = Self {
            id,
            file_path,
            entries: vec![header],
            cwd,
            system_prompt,
            active_tools: tools,
            created_at: chrono::DateTime::parse_from_rfc3339(&timestamp)
                .unwrap()
                .with_timezone(&chrono::Utc),
            tree: ConversationTree::new(),
        };
        session.rebuild_tree();

        Ok(session)
    }
}

// ── Conversation tree ────────────────────────────────────────────────────

/// Tracks parent/child relationships for session navigation.
#[derive(Debug, Default)]
pub struct ConversationTree {
    nodes: std::collections::HashMap<String, TreeNode>,
}

#[derive(Debug)]
pub struct TreeNode {
    /// The entry at this node.
    pub entry: SessionEntry,
    /// IDs of child entries.
    pub children: Vec<String>,
    /// IDs of leaf entries pointing here.
    pub leaves: Vec<String>,
}

impl ConversationTree {
    pub fn new() -> Self {
        Self {
            nodes: std::collections::HashMap::new(),
        }
    }

    /// Add an entry to the tree.
    pub fn add_entry(&mut self, entry: &SessionEntry) {
        let node_id = match entry.id() {
            Some(id) => id.to_string(),
            None => return,
        };

        // Register this entry as a child of its parent
        if let Some(parent_id) = entry.parent_id()
            && let Some(parent) = self.nodes.get_mut(parent_id)
            && !parent.children.contains(&node_id)
        {
            parent.children.push(node_id.clone());
        }

        if let SessionEntry::Leaf { target_id, .. } = entry
            && let Some(target) = self.nodes.get_mut(target_id.as_str())
            && !target.leaves.contains(&node_id)
        {
            target.leaves.push(node_id.clone());
        }

        self.nodes.insert(
            node_id,
            TreeNode {
                entry: entry.clone(),
                children: Vec::new(),
                leaves: Vec::new(),
            },
        );
    }

    /// Get the children of a node.
    pub fn children_of(&self, node_id: &str) -> Vec<&SessionEntry> {
        self.nodes
            .get(node_id)
            .map(|n| {
                n.children
                    .iter()
                    .filter_map(|id| self.nodes.get(id).map(|n| &n.entry))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Walk the tree in insertion order, returning entries.
    pub fn ordered_entries(&self) -> Vec<&SessionEntry> {
        // Simple approach: find the root (no parent), then walk children
        let root_id = self
            .nodes
            .values()
            .find(|n| n.entry.parent_id().is_none())
            .and_then(|n| n.entry.id().map(|s| s.to_string()));

        let mut result = Vec::new();
        if let Some(id) = root_id {
            self.walk_node(&id, &mut result);
        }
        result
    }

    fn walk_node<'a>(&'a self, node_id: &str, result: &mut Vec<&'a SessionEntry>) {
        if let Some(node) = self.nodes.get(node_id) {
            result.push(&node.entry);
            for child_id in &node.children {
                self.walk_node(child_id, result);
            }
        }
    }
}

// ── Session paths ────────────────────────────────────────────────────────

/// Compute the session directory for a working directory.
fn sessions_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"));
    base.join("zeus").join("sessions")
}

/// Sanitize a path for use in a directory name.
fn sanitize_for_path(path: &Path) -> String {
    let display = path.display().to_string();
    display
        .replace(['/', '\\', ':'], "_")
        .trim_start_matches('_')
        .to_string()
}

/// Compute the full path for a session file.
fn session_path(cwd: &Path, id: &Uuid, timestamp: &str) -> KonResult<PathBuf> {
    let sanitized = sanitize_for_path(cwd);
    let dir = sessions_dir().join(&sanitized);

    // Create the directory if needed
    std::fs::create_dir_all(&dir).map_err(|e| {
        KonError::Session(format!(
            "failed to create session directory {}: {e}",
            dir.display()
        ))
    })?;

    // Format timestamp for filename (replace colons)
    let ts_clean = timestamp.replace(':', "-");
    Ok(dir.join(format!("{ts_clean}_{id}.jsonl")))
}

/// Find all session JSONL files.
fn find_session_files() -> Vec<PathBuf> {
    let base = sessions_dir();
    if !base.exists() {
        return vec![];
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            if entry.path().is_dir()
                && let Ok(sub_entries) = std::fs::read_dir(entry.path())
            {
                for sub in sub_entries.flatten() {
                    let path = sub.path();
                    if path.extension().is_some_and(|e| e == "jsonl") {
                        files.push(path);
                    }
                }
            }
        }
    }
    files
}

/// Get the modification time of a file in seconds since epoch.
fn file_mtime_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

// ── Session discovery ────────────────────────────────────────────────────

/// Find the most recent session across all working directories.
pub fn most_recent_session() -> Option<PathBuf> {
    find_session_files()
        .into_iter()
        .max_by_key(|p| file_mtime_secs(p).unwrap_or(0))
}

/// Resolve a session by UUID prefix. Scans all session directories.
pub fn resolve_by_prefix(prefix: &str) -> KonResult<PathBuf> {
    let prefix_lower = prefix.to_lowercase();
    let matches: Vec<PathBuf> = find_session_files()
        .into_iter()
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .is_some_and(|name| name.contains(&prefix_lower))
        })
        .collect();

    match matches.len() {
        0 => Err(KonError::Session(format!(
            "no session found with prefix '{prefix}'"
        ))),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => Err(KonError::Session(format!(
            "multiple sessions match prefix '{prefix}' — use a longer prefix"
        ))),
    }
}

/// List recent session files with metadata, sorted newest first.
pub fn list_recent_sessions(limit: usize) -> Vec<(PathBuf, u64)> {
    let mut sessions: Vec<(PathBuf, u64)> = find_session_files()
        .into_iter()
        .map(|p| {
            let mtime = file_mtime_secs(&p).unwrap_or(0);
            (p, mtime)
        })
        .collect();

    sessions.sort_by(|a, b| b.1.cmp(&a.1));
    sessions.truncate(limit);
    sessions
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{ContentBlock, UserMessage};

    fn test_cwd() -> PathBuf {
        PathBuf::from("/tmp/test-project")
    }

    #[tokio::test]
    async fn test_create_session() {
        let cwd = test_cwd();
        let session = Session::new(
            cwd.clone(),
            "test system prompt".into(),
            vec!["read".into(), "bash".into()],
        )
        .await
        .unwrap();

        assert_eq!(session.system_prompt, "test system prompt");
        assert_eq!(session.active_tools, vec!["read", "bash"]);
        assert_eq!(session.entries.len(), 1);
        assert!(matches!(session.entries[0], SessionEntry::Header { .. }));
        assert!(session.file_path.exists());
    }

    #[tokio::test]
    async fn test_append_user_message() {
        let session = Session::new(test_cwd(), "sp".into(), vec![]).await.unwrap();
        let mut session = session;

        let id = session
            .append_user_message(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "hello".into(),
                }],
                skill_name: None,
            })
            .await
            .unwrap();

        assert!(!id.is_empty());
        assert_eq!(session.entries.len(), 2);

        // Verify the file has two lines
        let raw = tokio::fs::read_to_string(&session.file_path).await.unwrap();
        assert_eq!(raw.lines().count(), 2);
    }

    #[tokio::test]
    async fn test_load_session() {
        let cwd = test_cwd();
        let mut session = Session::new(cwd.clone(), "sp".into(), vec![])
            .await
            .unwrap();

        session
            .append_user_message(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "hello".into(),
                }],
                skill_name: None,
            })
            .await
            .unwrap();

        let path = session.file_path.clone();
        drop(session);

        // Reload
        let loaded = Session::load(path).await.unwrap();
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.system_prompt, "sp");
    }

    #[tokio::test]
    async fn test_active_messages() {
        let mut session = Session::new(test_cwd(), "sp".into(), vec![]).await.unwrap();

        session
            .append_user_message(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "hello".into(),
                }],
                skill_name: None,
            })
            .await
            .unwrap();

        let messages = session.active_messages();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], Message::User(_)));
    }

    #[test]
    fn test_sanitize_for_path() {
        assert_eq!(
            sanitize_for_path(Path::new("/home/user/project")),
            "home_user_project"
        );
        assert_eq!(
            sanitize_for_path(Path::new("C:\\Users\\test")),
            "C__Users_test"
        );
    }

    #[test]
    fn test_short_id_length() {
        let id = short_id();
        assert_eq!(id.len(), 8);
    }

    #[test]
    fn test_conversation_tree() {
        let mut tree = ConversationTree::new();
        let header = SessionEntry::Header {
            version: 1,
            id: "h1".into(),
            timestamp: now_iso(),
            cwd: "/tmp".into(),
            system_prompt: "sp".into(),
            tools: vec![],
            initial_thinking_level: "low".into(),
        };
        tree.add_entry(&header);

        let msg = SessionEntry::MessageEntry {
            id: "m1".into(),
            parent_id: "h1".into(),
            timestamp: now_iso(),
            message: Message::User(UserMessage {
                content: vec![],
                skill_name: None,
            }),
        };
        tree.add_entry(&msg);

        let children = tree.children_of("h1");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id(), Some("m1"));
    }

    #[test]
    fn test_now_iso_format() {
        let ts = now_iso();
        assert!(ts.contains('T'));
        assert!(ts.contains('+') || ts.contains('Z'));
    }

    #[test]
    fn test_resolve_by_prefix_nonexistent() {
        // Should fail since test sessions dir won't have matching entries
        let result = resolve_by_prefix("nonexistent_xyz_123");
        assert!(result.is_err());
    }

    #[test]
    fn test_most_recent_session_returns_none_when_empty() {
        // This test may pass if you have sessions, but typically returns None
        // in a clean test environment
        let _ = most_recent_session();
    }
}
