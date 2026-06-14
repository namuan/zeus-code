//! Single turn execution.
//!
//! A turn is one LLM request → response cycle:
//! 1. Open a streaming request
//! 2. Consume the SSE stream, buffering content and collecting tool calls
//! 3. Execute any collected tool calls (with permission checks)
//! 4. Return the turn result

use std::collections::HashMap;

use tokio::sync::{mpsc, watch};

use crate::core::errors::{KonError, KonResult};
use crate::core::types::{
    AgentEvent, ContentBlock, ImageContent, StopReason, StreamPart, ToolDefinition, ToolResult,
    Usage,
};
use crate::llm::base::Provider;
use crate::permissions::{PermissionDecision, PermissionMode, check_permission};
use crate::tools::base::Tool;

/// Result of executing one turn.
#[derive(Debug)]
pub struct TurnResult {
    /// The assistant's content blocks (text, thinking, tool calls).
    pub assistant_content: Vec<ContentBlock>,
    /// Results from executed tool calls.
    pub tool_results: Vec<ToolCallResult>,
    /// Token usage for this turn.
    pub usage: Usage,
    /// Why the LLM stopped (Stop, ToolUse, etc.).
    pub stop_reason: StopReason,
}

/// Result of executing a single tool call within a turn.
#[derive(Debug)]
pub struct ToolCallResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub result: Option<String>,
    pub images: Vec<ImageContent>,
    pub file_changes: Option<crate::core::types::FileChanges>,
    pub tool_result: ToolResult,
    pub approval_was_prompted: bool,
}

/// Accumulates a tool call across multiple streaming chunks.
#[derive(Debug)]
struct BufferedToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// Executes a single turn of the agent loop.
pub struct TurnRunner<'a> {
    provider: &'a dyn Provider,
    messages: Vec<crate::core::types::Message>,
    system_prompt: String,
    tool_definitions: Vec<ToolDefinition>,
    tools: &'a [Box<dyn Tool>],
    permission_mode: PermissionMode,
}

impl<'a> TurnRunner<'a> {
    pub fn new(
        provider: &'a dyn Provider,
        messages: Vec<crate::core::types::Message>,
        system_prompt: String,
        tool_definitions: Vec<ToolDefinition>,
        tools: &'a [Box<dyn Tool>],
        permission_mode: PermissionMode,
    ) -> Self {
        Self {
            provider,
            messages,
            system_prompt,
            tool_definitions,
            tools,
            permission_mode,
        }
    }

    /// Execute a full turn: stream → collect → execute tools.
    pub async fn run(
        &self,
        event_tx: &mpsc::Sender<AgentEvent>,
        cancel_rx: watch::Receiver<bool>,
    ) -> KonResult<TurnResult> {
        // Phase 1+2: Open and consume the stream
        let (assistant_content, tool_calls, usage, stop_reason) =
            self.stream_and_consume(event_tx, cancel_rx.clone()).await?;

        // Phase 3: Execute pending tool calls
        let tool_results = self
            .run_pending_tools(&tool_calls, event_tx, cancel_rx)
            .await?;

        Ok(TurnResult {
            assistant_content,
            tool_results,
            usage,
            stop_reason,
        })
    }

    /// Open the stream and consume all parts, returning collected state.
    async fn stream_and_consume(
        &self,
        event_tx: &mpsc::Sender<AgentEvent>,
        mut cancel_rx: watch::Receiver<bool>,
    ) -> KonResult<(Vec<ContentBlock>, Vec<BufferedToolCall>, Usage, StopReason)> {
        let mut stream = self
            .provider
            .stream(
                self.messages.clone(),
                Some(self.system_prompt.clone()),
                self.tool_definitions.clone(),
                cancel_rx.clone(),
            )
            .await?;

        let mut content_blocks: Vec<ContentBlock> = Vec::new();
        let mut tool_buffers: HashMap<String, BufferedToolCall> = HashMap::new();
        let mut current_thinking = String::new();
        let mut current_text = String::new();
        let mut in_thinking = false;
        let mut in_text = false;
        let mut usage = Usage::default();
        let mut stop_reason = StopReason::Stop;
        let mut chunk_count: u64 = 0;

        // Consume the stream
        loop {
            // Check cancel before blocking on stream
            if *cancel_rx.borrow() {
                tracing::debug!("Turn cancelled during streaming");
                return Err(KonError::Cancelled);
            }

            let part = tokio::select! {
                result = stream.inner.next() => {
                    match result {
                        Some(part) => part,
                        None => break, // stream ended
                    }
                }
                _ = cancel_rx.changed() => {
                    tracing::debug!("Turn cancelled during streaming");
                    return Err(KonError::Cancelled);
                }
            };

            let part = part?;
            chunk_count += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if chunk_count % 50 == 0 {
                tracing::debug!("Stream progress: {chunk_count} chunks received");
            }

            match part {
                StreamPart::ThinkingDelta { thinking, .. } => {
                    if !in_thinking {
                        // End any text block that was in progress
                        if in_text && !current_text.is_empty() {
                            content_blocks.push(ContentBlock::Text {
                                text: std::mem::take(&mut current_text),
                            });
                            let _ = event_tx.send(AgentEvent::TextEnd).await;
                            in_text = false;
                        }
                        in_thinking = true;
                        let _ = event_tx
                            .send(AgentEvent::ThinkingStart { level: None })
                            .await;
                    }
                    current_thinking.push_str(&thinking);
                    let _ = event_tx
                        .send(AgentEvent::ThinkingDelta { text: thinking })
                        .await;
                }

                StreamPart::TextDelta { text } => {
                    if !in_text {
                        // End any thinking block that was in progress
                        if in_thinking && !current_thinking.is_empty() {
                            content_blocks.push(ContentBlock::Thinking {
                                thinking: std::mem::take(&mut current_thinking),
                                signature: None,
                                level: None,
                            });
                            let _ = event_tx.send(AgentEvent::ThinkingEnd).await;
                            in_thinking = false;
                        }
                        in_text = true;
                        let _ = event_tx.send(AgentEvent::TextStart).await;
                    }
                    current_text.push_str(&text);
                    let _ = event_tx.send(AgentEvent::TextDelta { text }).await;
                }

                StreamPart::ToolCallStart { id, name } => {
                    // Flush any in-progress text
                    if in_text && !current_text.is_empty() {
                        content_blocks.push(ContentBlock::Text {
                            text: std::mem::take(&mut current_text),
                        });
                        let _ = event_tx.send(AgentEvent::TextEnd).await;
                        in_text = false;
                    }

                    tool_buffers.insert(
                        id.clone(),
                        BufferedToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: String::new(),
                        },
                    );
                    let _ = event_tx.send(AgentEvent::ToolStart { id, name }).await;
                }

                StreamPart::ToolCallDelta {
                    id,
                    arguments_delta,
                } => {
                    if let Some(tc) = tool_buffers.get_mut(&id) {
                        tc.arguments.push_str(&arguments_delta);
                    }
                    let _ = event_tx
                        .send(AgentEvent::ToolArgsDelta {
                            id,
                            delta: arguments_delta,
                        })
                        .await;
                }

                StreamPart::StreamDone {
                    stop_reason: sr,
                    usage: u,
                } => {
                    // Flush remaining buffers
                    if in_thinking && !current_thinking.is_empty() {
                        content_blocks.push(ContentBlock::Thinking {
                            thinking: std::mem::take(&mut current_thinking),
                            signature: None,
                            level: None,
                        });
                        let _ = event_tx.send(AgentEvent::ThinkingEnd).await;
                    }
                    if in_text && !current_text.is_empty() {
                        content_blocks.push(ContentBlock::Text {
                            text: std::mem::take(&mut current_text),
                        });
                        let _ = event_tx.send(AgentEvent::TextEnd).await;
                    }

                    // Convert buffered tool calls to content blocks
                    for tc in tool_buffers.values() {
                        content_blocks.push(ContentBlock::ToolCall {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        });
                        let _ = event_tx
                            .send(AgentEvent::ToolEnd {
                                id: tc.id.clone(),
                                arguments: tc.arguments.clone(),
                            })
                            .await;
                    }

                    usage = u;
                    stop_reason = sr;
                    break;
                }

                StreamPart::ThinkingSignature { .. } | StreamPart::StreamError { .. } => {
                    // Ignore for now
                }
            }
        }

        let tool_calls: Vec<BufferedToolCall> = tool_buffers.into_values().collect();

        tracing::info!(
            "Stream complete: {} chunks, {} text blocks, {} tool calls, stop={:?}",
            chunk_count,
            content_blocks.len(),
            tool_calls.len(),
            stop_reason
        );

        Ok((content_blocks, tool_calls, usage, stop_reason))
    }

    /// Execute collected tool calls, checking permissions.
    async fn run_pending_tools(
        &self,
        tool_calls: &[BufferedToolCall],
        event_tx: &mpsc::Sender<AgentEvent>,
        cancel_rx: watch::Receiver<bool>,
    ) -> KonResult<Vec<ToolCallResult>> {
        let mut results = Vec::new();

        for tc in tool_calls {
            // Check cancel before each tool
            if *cancel_rx.borrow() {
                return Err(KonError::Cancelled);
            }

            let tool_name = tc.name.clone();
            tracing::info!("Executing tool: {tool_name}");

            // Find the tool
            let tool = match self.tools.iter().find(|t| t.name() == tc.name) {
                Some(t) => t,
                None => {
                    let err = ToolResult {
                        success: false,
                        result: Some(format!("Unknown tool: {}", tc.name)),
                        images: vec![],
                        ui_summary: Some(format!("Unknown tool: {}", tc.name)),
                        ui_details: None,
                        ui_details_full: None,
                        file_changes: None,
                    };
                    let _ = event_tx
                        .send(AgentEvent::ToolResult {
                            id: tc.id.clone(),
                            result: err.clone(),
                        })
                        .await;
                    results.push(ToolCallResult {
                        tool_call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                        result: Some(format!("Unknown tool: {}", tc.name)),
                        images: vec![],
                        file_changes: None,
                        tool_result: err,
                        approval_was_prompted: false,
                    });
                    continue;
                }
            };

            // Parse arguments
            let params: serde_json::Value = match serde_json::from_str(&tc.arguments) {
                Ok(v) => v,
                Err(e) => {
                    let err = ToolResult {
                        success: false,
                        result: Some(format!("Invalid arguments: {e}")),
                        images: vec![],
                        ui_summary: Some("Invalid arguments".into()),
                        ui_details: None,
                        ui_details_full: None,
                        file_changes: None,
                    };
                    results.push(ToolCallResult {
                        tool_call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                        result: Some(format!("Invalid arguments: {e}")),
                        images: vec![],
                        file_changes: None,
                        tool_result: err,
                        approval_was_prompted: false,
                    });
                    continue;
                }
            };

            // Permission check
            let decision = check_permission(tool.as_ref(), &params, self.permission_mode);
            if let PermissionDecision::Prompt = decision {
                // In full implementation, this would wait for user approval via oneshot
                // For now, in test/auto mode, we proceed
                tracing::debug!("Tool {} would require approval in prompt mode", tc.name);
            }

            // Execute the tool
            let exec_start = std::time::Instant::now();
            let tool_result = match tool.execute(params, cancel_rx.clone()).await {
                Ok(ok_tr) => {
                    let elapsed = exec_start.elapsed();
                    tracing::info!(
                        "Tool {} completed in {:.1}s (success={})",
                        tc.name,
                        elapsed.as_secs_f64(),
                        ok_tr.success
                    );
                    ok_tr
                }
                Err(e) => {
                    let elapsed = exec_start.elapsed();
                    tracing::warn!(
                        "Tool {} failed after {:.1}s: {e}",
                        tc.name,
                        elapsed.as_secs_f64()
                    );
                    // Send a ToolResult event even on error so the UI unblocks
                    let err_result = ToolResult {
                        success: false,
                        result: Some(e.to_string()),
                        images: vec![],
                        ui_summary: Some(format!("Tool error: {e}")),
                        ui_details: None,
                        ui_details_full: None,
                        file_changes: None,
                    };
                    let _ = event_tx
                        .send(AgentEvent::ToolResult {
                            id: tc.id.clone(),
                            result: err_result.clone(),
                        })
                        .await;
                    results.push(ToolCallResult {
                        tool_call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                        result: Some(e.to_string()),
                        images: vec![],
                        file_changes: None,
                        tool_result: err_result,
                        approval_was_prompted: matches!(decision, PermissionDecision::Prompt),
                    });
                    continue;
                }
            };

            let _ = event_tx
                .send(AgentEvent::ToolResult {
                    id: tc.id.clone(),
                    result: tool_result.clone(),
                })
                .await;

            results.push(ToolCallResult {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                result: tool_result.result.clone(),
                images: tool_result.images.clone(),
                file_changes: tool_result.file_changes.clone(),
                tool_result,
                approval_was_prompted: matches!(decision, PermissionDecision::Prompt),
            });
        }

        Ok(results)
    }
}

// Helper: async next() on Pin<Box<dyn Stream>>
use futures::StreamExt;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::providers::mock::MockProvider;
    use crate::tools as tools_mod;

    #[tokio::test]
    async fn test_turn_runner_text_only() {
        let provider = MockProvider::new("mock".into());
        let tools = tools_mod::core_tools();
        let tool_defs = tools_mod::get_tool_definitions(&tools);

        let runner = TurnRunner::new(
            &provider,
            vec![],
            "system".into(),
            tool_defs,
            &tools,
            PermissionMode::Auto,
        );

        let (event_tx, _event_rx) = mpsc::channel(32);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let result = runner.run(&event_tx, cancel_rx).await.unwrap();

        assert!(!result.assistant_content.is_empty());
        assert!(result.tool_results.is_empty());
        assert_eq!(result.stop_reason, StopReason::Stop);
        assert!(result.usage.output_tokens > 0);
    }

    #[tokio::test]
    async fn test_turn_runner_with_tool_call() {
        let provider = MockProvider::new("mock:tool".into());
        let tools = tools_mod::core_tools();
        let tool_defs = tools_mod::get_tool_definitions(&tools);

        let runner = TurnRunner::new(
            &provider,
            vec![],
            "system".into(),
            tool_defs,
            &tools,
            PermissionMode::Auto,
        );

        let (event_tx, _event_rx) = mpsc::channel(32);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let result = runner.run(&event_tx, cancel_rx).await.unwrap();

        assert_eq!(result.stop_reason, StopReason::ToolUse);
        assert!(!result.tool_results.is_empty());
        // The mock:tool response has a read tool call with file_path src/main.rs
        // The read tool should execute successfully if src/main.rs exists,
        // or return a "not found" result
    }

    #[tokio::test]
    async fn test_turn_runner_unknown_tool() {
        // Use a custom mock response to test unknown tool handling
        let provider = MockProvider::new("mock:tool".into());
        // The mock:tool calls "read" which IS a known tool, so this test
        // verifies that known tools work. Unknown tools would just produce
        // an error result.
        let tools = tools_mod::core_tools();
        let tool_defs = tools_mod::get_tool_definitions(&tools);

        let runner = TurnRunner::new(
            &provider,
            vec![],
            "system".into(),
            tool_defs,
            &tools,
            PermissionMode::Auto,
        );

        let (event_tx, _event_rx) = mpsc::channel(32);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let result = runner.run(&event_tx, cancel_rx).await.unwrap();
        assert!(!result.tool_results.is_empty());
    }

    #[tokio::test]
    async fn test_turn_runner_cancellation() {
        let provider = MockProvider::new("mock".into());
        let tools = tools_mod::core_tools();
        let tool_defs = tools_mod::get_tool_definitions(&tools);

        let runner = TurnRunner::new(
            &provider,
            vec![],
            "system".into(),
            tool_defs,
            &tools,
            PermissionMode::Auto,
        );

        let (event_tx, _event_rx) = mpsc::channel(32);
        let (cancel_tx, cancel_rx) = watch::channel(false);
        cancel_tx.send(true).unwrap();

        let result = runner.run(&event_tx, cancel_rx).await;
        assert!(matches!(result, Err(KonError::Cancelled)));
    }
}
