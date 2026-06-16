use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json;
use tokio::sync::watch;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::{Message, ToolDefinition};
use crate::llm::base::Provider;

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

/// Per-message character limit when serializing for the summarization prompt.
/// Prevents the prompt from being as large as the conversation itself.
const MAX_ENTRY_CHARS: usize = 2000;

/// Instruction sent to the LLM when asking it to summarize the conversation.
const SUMMARIZATION_INSTRUCTION: &str = "You are a conversation summarizer. Summarize the \
conversation below so it can be used as compact context for continuing the work.\n\n\
Preserve:\n\
- Key decisions and their rationale\n\
- File changes (which files, what was done)\n\
- Unresolved questions or pending tasks\n\
- Important context (variables, environment, user constraints)\n\n\
Be concise but thorough. Output plain text, no markdown headers needed.";

/// Generate a summary of the conversation using the LLM.
///
/// Serializes the messages to summarize into a textual form, sends a
/// summarization prompt to the provider, collects the streamed text response,
/// and returns it together with token counts before/after compaction.
///
/// `first_kept_entry_id` is stored in the returned summary so that
/// `Session::active_messages()` can skip the summarized range on subsequent
/// turns.
///
/// Honours the provided `watch::Receiver<bool>` cancellation channel: if the
/// stream is cancelled mid-flight, a `KonError::Cancelled` is returned and any
/// partial summary is discarded.
pub async fn generate_summary(
    provider: &dyn Provider,
    messages_to_summarize: &[Message],
    first_kept_entry_id: String,
    mut cancel: watch::Receiver<bool>,
) -> KonResult<CompactionSummary> {
    // Guard: nothing to summarize.
    if messages_to_summarize.is_empty() {
        return Err(KonError::Other("nothing to summarize".into()));
    }

    // Build the prompt.
    let serialized = messages_to_summary_prompt_text(messages_to_summarize);
    let prompt_body =
        format!("{SUMMARIZATION_INSTRUCTION}\n\n<conversation>\n{serialized}\n</conversation>");
    let prompt_message = Message::User(crate::core::types::UserMessage {
        content: vec![crate::core::types::ContentBlock::Text { text: prompt_body }],
        skill_name: None,
    });

    // Open the stream.
    let mut stream = provider
        .stream(vec![prompt_message], None, Vec::new(), cancel.clone())
        .await?;

    // Collect the streamed text.
    let mut summary_text = String::new();
    loop {
        // Check cancellation before blocking on the stream.
        if *cancel.borrow() {
            return Err(KonError::Cancelled);
        }

        let part = tokio::select! {
            result = stream.inner.next() => {
                match result {
                    Some(part) => part,
                    None => break, // stream ended without StreamDone
                }
            }
            _ = cancel.changed() => {
                return Err(KonError::Cancelled);
            }
        };

        let part = part?;
        match part {
            crate::core::types::StreamPart::TextDelta { text } => {
                summary_text.push_str(&text);
            }
            crate::core::types::StreamPart::StreamDone { .. } => {
                break;
            }
            // Ignore thinking, tool calls (we requested no tools), signatures,
            // and error parts here — a stream-level error is handled below by
            // `?` on the next iteration's part.
            _ => {}
        }
    }

    // Compute token counts.
    let tokens_before: u64 = messages_to_summarize
        .iter()
        .map(estimate_message_tokens)
        .sum();
    let tokens_after = estimate_tokens(&summary_text);

    Ok(CompactionSummary {
        summary: summary_text,
        tokens_before,
        tokens_after,
        first_kept_entry_id,
    })
}

/// Serialize a slice of messages to a labeled textual form, truncating each
/// entry to `MAX_ENTRY_CHARS` characters. Thinking blocks are skipped to keep
/// the summarization prompt focused on user-visible content.
fn messages_to_summary_prompt_text(messages: &[Message]) -> String {
    let mut out = String::new();
    for (idx, msg) in messages.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        match msg {
            Message::User(m) => {
                let text = join_text_blocks(&m.content);
                out.push_str("User: ");
                out.push_str(&truncate(&text, MAX_ENTRY_CHARS));
            }
            Message::Assistant(m) => {
                let mut pieces: Vec<String> = Vec::new();
                for block in &m.content {
                    match block {
                        crate::core::types::ContentBlock::Text { text } => {
                            pieces.push(text.clone());
                        }
                        crate::core::types::ContentBlock::ToolCall {
                            name, arguments, ..
                        } => {
                            pieces.push(format!("[tool_call: {name}({arguments})]"));
                        }
                        // Skip thinking and image blocks from the summary prompt.
                        _ => {}
                    }
                }
                out.push_str("Assistant: ");
                out.push_str(&truncate(&pieces.join(" "), MAX_ENTRY_CHARS));
            }
            Message::ToolResult(m) => {
                out.push_str(&format!("Tool Result ({}): ", m.tool_name));
                out.push_str(&truncate(&m.content, MAX_ENTRY_CHARS));
            }
            Message::System(m) => {
                out.push_str("System: ");
                out.push_str(&truncate(&m.content, MAX_ENTRY_CHARS));
            }
        }
    }
    out
}

/// Concatenate the text portions of a slice of content blocks.
fn join_text_blocks(blocks: &[crate::core::types::ContentBlock]) -> String {
    let mut out = String::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if let crate::core::types::ContentBlock::Text { text } = block {
            out.push_str(text);
        }
    }
    out
}

/// Truncate `s` to at most `max_chars`, appending a marker when truncated.
fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}…[truncated]")
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
    fn test_generate_summary_empty_messages_returns_error() {
        // Build a dummy provider; the empty-input guard short-circuits
        // before the provider is invoked.
        let provider = crate::llm::providers::mock::MockProvider::new("mock".into());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let result = runtime.block_on(generate_summary(&provider, &[], "kept-1".into(), cancel_rx));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_generate_summary_with_mock_provider() {
        use crate::llm::providers::mock::MockProvider;

        let provider = MockProvider::new("mock".into());
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let messages = vec![
            Message::User(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "Please refactor the config loader.".into(),
                }],
                skill_name: None,
            }),
            Message::Assistant(AssistantMessage {
                content: vec![ContentBlock::Text {
                    text: "I'll read the file first.".into(),
                }],
                usage: None,
                stop_reason: Some(StopReason::Stop),
            }),
            Message::User(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "Sounds good.".into(),
                }],
                skill_name: None,
            }),
        ];

        let summary = generate_summary(&provider, &messages, "kept-1".into(), cancel_rx)
            .await
            .expect("summarization should succeed with the mock provider");

        // The mock provider returns canned text; it should appear in the summary.
        assert!(!summary.summary.is_empty());
        assert!(
            summary.tokens_before > 0,
            "tokens_before should reflect the input messages"
        );
        assert!(
            summary.tokens_after > 0,
            "tokens_after should reflect the summary text"
        );
        assert_eq!(summary.first_kept_entry_id, "kept-1");
    }

    #[tokio::test]
    async fn test_generate_summary_cancellation() {
        use crate::llm::providers::mock::MockProvider;

        let provider = MockProvider::new("mock".into());
        let (cancel_tx, cancel_rx) = watch::channel(false);
        cancel_tx.send(true).unwrap();

        let messages = vec![Message::User(UserMessage {
            content: vec![ContentBlock::Text { text: "hi".into() }],
            skill_name: None,
        })];

        let result = generate_summary(&provider, &messages, "kept-1".into(), cancel_rx).await;
        assert!(matches!(result, Err(KonError::Cancelled)));
    }

    #[test]
    fn test_messages_to_summary_prompt_text_user() {
        let msgs = vec![Message::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
            skill_name: None,
        })];
        let out = messages_to_summary_prompt_text(&msgs);
        assert!(out.contains("User: hello"));
    }

    #[test]
    fn test_messages_to_summary_prompt_text_assistant_tool_call() {
        let msgs = vec![Message::Assistant(AssistantMessage {
            content: vec![
                ContentBlock::Text {
                    text: "Reading".into(),
                },
                ContentBlock::ToolCall {
                    id: "c1".into(),
                    name: "read".into(),
                    arguments: r#"{"file_path":"x.rs"}"#.into(),
                },
            ],
            usage: None,
            stop_reason: Some(StopReason::ToolUse),
        })];
        let out = messages_to_summary_prompt_text(&msgs);
        assert!(out.contains("Assistant:"));
        assert!(out.contains("Reading"));
        assert!(out.contains("[tool_call: read("));
    }

    #[test]
    fn test_messages_to_summary_prompt_text_skips_thinking() {
        let msgs = vec![Message::Assistant(AssistantMessage {
            content: vec![
                ContentBlock::Thinking {
                    thinking: "internal reasoning".into(),
                    signature: None,
                    level: Some("low".into()),
                },
                ContentBlock::Text {
                    text: "external answer".into(),
                },
            ],
            usage: None,
            stop_reason: Some(StopReason::Stop),
        })];
        let out = messages_to_summary_prompt_text(&msgs);
        assert!(!out.contains("internal reasoning"));
        assert!(out.contains("external answer"));
    }

    #[test]
    fn test_messages_to_summary_prompt_text_truncates_long_entries() {
        let long = "x".repeat(MAX_ENTRY_CHARS + 100);
        let msgs = vec![Message::User(UserMessage {
            content: vec![ContentBlock::Text { text: long }],
            skill_name: None,
        })];
        let out = messages_to_summary_prompt_text(&msgs);
        assert!(out.contains("…[truncated]"));
        // Should not contain the full original text length.
        assert!(out.len() < MAX_ENTRY_CHARS + 100 + 20);
    }

    #[tokio::test]
    async fn test_summarization_prompt_contains_conversation() {
        use crate::llm::providers::mock::MockProvider;

        let provider = MockProvider::new("mock".into());
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let messages = vec![Message::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "FIX-MARKER-ABC".into(),
            }],
            skill_name: None,
        })];

        // The mock provider ignores the prompt content for its canned response,
        // but we can verify our serialization helper produces the expected fence.
        let serialized = messages_to_summary_prompt_text(&messages);
        assert!(serialized.contains("FIX-MARKER-ABC"));

        // Exercise the full path so we know the prompt is wired through
        // provider.stream() without error.
        let summary = generate_summary(&provider, &messages, "kept-x".into(), cancel_rx)
            .await
            .unwrap();
        assert!(!summary.summary.is_empty());
    }
}
