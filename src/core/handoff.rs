//! Handoff prompt generation for focused child sessions.
//!
//! A handoff creates a new focused session that inherits context from the
//! current session. The LLM synthesizes relevant context into a handoff prompt
//! that the child session uses as its starting point.
//!
//! Full implementation in Phase 7 (Agent Loop).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Configuration for handoff behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffConfig {
    /// Maximum number of recent messages to include in the context synthesis.
    pub max_context_messages: usize,
    /// Maximum tokens for the synthesized handoff prompt.
    pub max_handoff_tokens: u64,
}

impl Default for HandoffConfig {
    fn default() -> Self {
        Self {
            max_context_messages: 50,
            max_handoff_tokens: 2000,
        }
    }
}

/// The result of generating a handoff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffResult {
    /// Unique ID for the handoff.
    pub id: Uuid,
    /// The synthesized handoff prompt that the child session starts with.
    pub prompt: String,
    /// The parent session ID this handoff was created from.
    pub parent_session_id: Uuid,
    /// Brief description of what the handoff is for.
    pub description: String,
}

/// Build a prompt that asks the LLM to synthesize a handoff context.
///
/// This produces a pair of messages: the system-level instruction for the
/// synthesizing LLM, and the user prompt containing the conversation context.
pub fn build_handoff_prompt(description: &str, _parent_session_id: &Uuid) -> (String, String) {
    let system = format!(
        "You are synthesizing context for a focused coding session. \
         The user wants to hand off work on: \"{description}\". \
         Produce a concise summary of relevant context from the conversation below. \
         Include: key decisions made, files being worked on, current state, \
         and any unresolved issues. Be thorough but concise."
    );

    // The conversation messages would be passed separately in the full
    // implementation. For now, return the system instruction.
    let user_prompt = format!(
        "Handoff description: {description}\n\n\
         [Conversation context would be injected here in Phase 7]"
    );

    (system, user_prompt)
}

/// Create a handoff by synthesizing a context prompt from a conversation.
///
/// Stub — full implementation requires a provider reference and will be
/// completed in Phase 7.
pub fn create_handoff(
    _description: &str,
    _parent_session_id: &Uuid,
) -> crate::core::errors::KonResult<HandoffResult> {
    Err(crate::core::errors::KonError::Other(
        "handoff creation not yet implemented".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handoff_config_defaults() {
        let config = HandoffConfig::default();
        assert_eq!(config.max_context_messages, 50);
        assert_eq!(config.max_handoff_tokens, 2000);
    }

    #[test]
    fn test_build_handoff_prompt() {
        let parent_id = Uuid::new_v4();
        let (system, user) = build_handoff_prompt("fix the database layer", &parent_id);

        assert!(system.contains("fix the database layer"));
        assert!(system.contains("synthesizing context"));
        assert!(user.contains("fix the database layer"));
    }

    #[test]
    fn test_create_handoff_stub_returns_error() {
        let parent_id = Uuid::new_v4();
        let result = create_handoff("test", &parent_id);
        assert!(result.is_err());
    }
}
