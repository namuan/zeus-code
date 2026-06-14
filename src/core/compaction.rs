use serde::{Deserialize, Serialize};
use serde_json;

use crate::core::errors::KonResult;
use crate::core::types::{Message, ToolDefinition};

/// Token estimation: rough heuristic (characters ÷ 4).
/// This is a fast approximation for English text; a real tokenizer would be more accurate,
/// but this is sufficient for compaction threshold calculations.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64) / 4
}

/// Estimate tokens for a single message by serializing to JSON and counting characters.
pub fn estimate_message_tokens(message: &Message) -> u64 {
    match serde_json::to_string(message) {
        Ok(json) => estimate_tokens(&json),
        Err(_) => {
            // Fallback: sum up visible text content
            match message {
                Message::User(m) => estimate_content_blocks_tokens(&m.content),
                Message::Assistant(m) => estimate_content_blocks_tokens(&m.content),
                Message::ToolResult(m) => estimate_tokens(&m.content),
                Message::System(m) => estimate_tokens(&m.content),
            }
        }
    }
}

/// Estimate tokens for a tool definition by serializing to JSON.
pub fn estimate_tool_def_tokens(def: &ToolDefinition) -> u64 {
    match serde_json::to_string(def) {
        Ok(json) => estimate_tokens(&json),
        Err(_) => 0,
    }
}

/// Sum tokens across all content blocks in a message.
fn estimate_content_blocks_tokens(blocks: &[crate::core::types::ContentBlock]) -> u64 {
    blocks
        .iter()
        .map(|block| match block {
            crate::core::types::ContentBlock::Text { text } => estimate_tokens(text),
            crate::core::types::ContentBlock::Thinking {
                thinking,
                signature,
                ..
            } => {
                let sig_len = signature.as_ref().map(|s| s.len()).unwrap_or(0);
                estimate_tokens(thinking) + (sig_len as u64 / 4)
            }
            crate::core::types::ContentBlock::Image(img) => {
                // Images are heavy — rough estimate based on base64 length
                estimate_tokens(&img.base64)
            }
            crate::core::types::ContentBlock::ToolCall {
                name, arguments, ..
            } => estimate_tokens(name) + estimate_tokens(arguments),
        })
        .sum()
}

/// Check if the conversation is approaching the context window
/// and needs to be compacted.
pub fn should_compact(
    messages: &[Message],
    system_prompt: &str,
    context_window: u64,
    buffer_tokens: u64,
    tool_definitions: &[ToolDefinition],
) -> bool {
    let total = estimate_tokens(system_prompt)
        + messages.iter().map(estimate_message_tokens).sum::<u64>()
        + tool_definitions
            .iter()
            .map(estimate_tool_def_tokens)
            .sum::<u64>();

    total > context_window.saturating_sub(buffer_tokens)
}

/// Compaction configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// What to do after compaction: continue the turn or pause.
    pub on_overflow: OnOverflow,
    /// Safety margin reserved before the model context window is treated as full.
    pub buffer_tokens: u64,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            on_overflow: OnOverflow::Continue,
            buffer_tokens: 20000,
        }
    }
}

/// Behavior after compaction triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnOverflow {
    /// Keep going with a synthetic continue prompt after compaction.
    Continue,
    /// Stop after compaction and wait for user input.
    Pause,
}

/// Summary produced by compacting earlier messages in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionSummary {
    /// The LLM-generated summary text.
    pub summary: String,
    /// Token count before compaction.
    pub tokens_before: u64,
    /// Token count after compaction (estimated).
    pub tokens_after: u64,
    /// ID of the first entry kept after compaction.
    pub first_kept_entry_id: String,
}

/// Generate a summary of the conversation using the LLM.
///
/// This is a stub — the full implementation requires a provider reference,
/// which will be wired up in Phase 7 (Agent Loop).
pub fn generate_summary(_messages: &[Message], _max_tokens: u64) -> KonResult<CompactionSummary> {
    // Stub — will be implemented in Phase 7.
    // In practice, this sends a summarization prompt to the LLM:
    //   "Summarize the conversation so far, preserving key decisions,
    //    file changes, and unresolved questions..."
    Err(crate::core::errors::KonError::Other(
        "compaction summary generation not yet implemented".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{
        AssistantMessage, ContentBlock, StopReason, SystemMessage, UserMessage,
    };

    #[test]
    fn test_estimate_tokens_basic() {
        // 4 chars ≈ 1 token
        assert_eq!(estimate_tokens("hello"), 1);
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("1234"), 1);
        // 19 chars / 4 = 4 (integer division)
        assert_eq!(estimate_tokens("The quick brown fox"), 4);
    }

    #[test]
    fn test_estimate_message_tokens_user() {
        let msg = Message::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "Fix the failing test".into(),
            }],
            skill_name: None,
        });
        let tokens = estimate_message_tokens(&msg);
        // Should be non-zero (exact value depends on JSON serialization)
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_message_tokens_system() {
        let msg = Message::System(SystemMessage {
            content: "You are a helpful assistant".into(),
        });
        let tokens = estimate_message_tokens(&msg);
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_message_tokens_assistant_with_thinking() {
        let msg = Message::Assistant(AssistantMessage {
            content: vec![
                ContentBlock::Thinking {
                    thinking: "Let me think about this...".into(),
                    signature: None,
                    level: Some("low".into()),
                },
                ContentBlock::Text {
                    text: "Here is the fix".into(),
                },
            ],
            usage: None,
            stop_reason: Some(StopReason::Stop),
        });
        let tokens = estimate_message_tokens(&msg);
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_tool_def_tokens() {
        let def = ToolDefinition {
            name: "read".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        };
        let tokens = estimate_tool_def_tokens(&def);
        assert!(tokens > 0);
    }

    #[test]
    fn test_should_compact_below_threshold() {
        let messages = vec![Message::User(UserMessage {
            content: vec![ContentBlock::Text { text: "hi".into() }],
            skill_name: None,
        })];
        let system_prompt = "You are helpful.";
        let tools = vec![];

        // Small messages, huge context window → no compaction
        assert!(!should_compact(
            &messages,
            system_prompt,
            200000,
            20000,
            &tools
        ));
    }

    #[test]
    fn test_should_compact_above_threshold() {
        // Create a very large message that exceeds a tiny context window
        let big_text = "x".repeat(10000);
        let messages = vec![Message::User(UserMessage {
            content: vec![ContentBlock::Text { text: big_text }],
            skill_name: None,
        })];
        let system_prompt = "";
        let tools = vec![];

        // context_window = 500, buffer = 100 → threshold = 400
        // 10000 chars / 4 = 2500 tokens > 400
        assert!(should_compact(&messages, system_prompt, 500, 100, &tools));
    }

    #[test]
    fn test_compaction_config_defaults() {
        let config = CompactionConfig::default();
        assert_eq!(config.buffer_tokens, 20000);
        assert_eq!(config.on_overflow, OnOverflow::Continue);
    }

    #[test]
    fn test_generate_summary_stub_returns_error() {
        let result = generate_summary(&[], 1000);
        assert!(result.is_err());
    }
}
