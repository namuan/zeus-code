//! Agent loop orchestration.
//!
//! The Agent struct holds all state for a coding session and drives
//! the turn-by-turn interaction between the LLM, tools, and session
//! persistence.

use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::{mpsc, watch};

use crate::config::Config;
use crate::core::compaction::{CompactionConfig, should_compact};
use crate::core::errors::KonResult;
use crate::core::types::{AgentEvent, StopReason, Usage, UserMessage};
use crate::llm::base::Provider;
use crate::permissions::PermissionMode;
use crate::session::Session;
use crate::tools::base::Tool;
use crate::tools::{self as tools_mod};
use crate::turn::TurnRunner;

/// Orchestrates the multi-turn conversation loop.
pub struct Agent {
    config: Arc<RwLock<Config>>,
    provider: Box<dyn Provider>,
    tools: Vec<Box<dyn Tool>>,
    max_turns: u64,
    context_window: u64,
    compaction_config: CompactionConfig,
    permission_mode: PermissionMode,
}

/// Result returned when the agent finishes.
#[derive(Debug)]
pub struct AgentEndInfo {
    pub stop_reason: StopReason,
    pub total_turns: u64,
    pub usage: Usage,
}

impl Agent {
    pub fn new(config: Arc<RwLock<Config>>, provider: Box<dyn Provider>) -> Self {
        let cfg = config.read();
        let tools = tools_mod::core_tools();

        Self {
            config: config.clone(),
            provider,
            tools,
            max_turns: cfg.agent.max_turns,
            context_window: cfg.agent.default_context_window,
            compaction_config: cfg.compaction.clone(),
            permission_mode: match cfg.permissions.mode.as_str() {
                "auto" => PermissionMode::Auto,
                _ => PermissionMode::Prompt,
            },
        }
    }

    /// Run the agent on a user query, emitting events to the channel.
    /// The session is mutated in-place (messages appended) but remains owned by the caller.
    pub async fn run(
        &self,
        session: &mut Session,
        query: String,
        skill_name: Option<String>,
        event_tx: mpsc::Sender<AgentEvent>,
        cancel_rx: watch::Receiver<bool>,
    ) -> KonResult<AgentEndInfo> {
        let _ = event_tx.send(AgentEvent::Start).await;

        // Append user message to session
        let user_msg = UserMessage {
            content: vec![crate::core::types::ContentBlock::Text { text: query }],
            skill_name,
        };
        session.append_user_message(user_msg).await?;

        // Build system prompt once
        let system_prompt = build_system_prompt(&self.config, &self.tools);

        // Build tool definitions
        let tool_definitions = tools_mod::get_tool_definitions(&self.tools);

        let mut total_usage = Usage::default();
        let mut total_turns: u64 = 0;
        let mut stop_reason;

        loop {
            // Check for cancellation before each turn
            if *cancel_rx.borrow() {
                return Err(crate::core::errors::KonError::Cancelled);
            }

            // Check max turns
            if total_turns >= self.max_turns {
                stop_reason = StopReason::MaxTurns;
                break;
            }

            total_turns += 1;
            let _ = event_tx
                .send(AgentEvent::TurnStart { turn: total_turns })
                .await;

            // Get messages from session
            let messages = session.active_messages();

            // Run one turn
            let turn_runner = TurnRunner::new(
                self.provider.as_ref(),
                messages,
                system_prompt.clone(),
                tool_definitions.clone(),
                &self.tools,
                self.permission_mode,
            );

            let turn_result = turn_runner.run(&event_tx, cancel_rx.clone()).await?;

            // Append assistant message to session
            let assistant_content = turn_result.assistant_content.clone();
            session
                .append_assistant_message(
                    assistant_content,
                    Some(turn_result.usage.clone()),
                    Some(turn_result.stop_reason.clone()),
                )
                .await?;

            // Append tool results
            for tr in &turn_result.tool_results {
                let msg = crate::core::types::ToolResultMessage {
                    tool_call_id: tr.tool_call_id.clone(),
                    tool_name: tr.tool_name.clone(),
                    content: tr.result.clone().unwrap_or_default(),
                    images: tr.images.clone(),
                    file_changes: tr.file_changes.clone(),
                };
                session.append_tool_result(msg).await?;
            }

            // Accumulate usage
            total_usage.input_tokens += turn_result.usage.input_tokens;
            total_usage.output_tokens += turn_result.usage.output_tokens;

            // Emit TurnEnd
            let _ = event_tx
                .send(AgentEvent::TurnEnd {
                    stop_reason: turn_result.stop_reason.clone(),
                    assistant: crate::core::types::AssistantMessage {
                        content: turn_result.assistant_content.clone(),
                        usage: Some(turn_result.usage.clone()),
                        stop_reason: Some(turn_result.stop_reason.clone()),
                    },
                    tool_results: turn_result
                        .tool_results
                        .iter()
                        .map(|tr| tr.tool_result.clone())
                        .collect(),
                })
                .await;

            // Check compaction
            let messages = session.active_messages();
            if should_compact(
                &messages,
                &system_prompt,
                self.context_window,
                self.compaction_config.buffer_tokens,
                &tool_definitions,
            ) {
                let _ = event_tx.send(AgentEvent::CompactionStart).await;
                // Compaction summary will be implemented in Phase 10
                // For now, just log that compaction would occur
                tracing::info!("Context compaction triggered (stub)");
                let _ = event_tx
                    .send(AgentEvent::CompactionEnd {
                        summary: "Compaction summary stub".into(),
                        tokens_before: 0,
                    })
                    .await;
            }

            // Check stop reason
            stop_reason = turn_result.stop_reason;
            if stop_reason != StopReason::ToolUse {
                break;
            }
        }

        let _ = event_tx
            .send(AgentEvent::End {
                stop_reason: stop_reason.clone(),
                total_turns,
                usage: total_usage.clone(),
            })
            .await;

        Ok(AgentEndInfo {
            stop_reason,
            total_turns,
            usage: total_usage,
        })
    }
}

// ── System prompt builder ────────────────────────────────────────────────

/// Build the combined system prompt from config + context + tool guidelines + date.
fn build_system_prompt(config: &Arc<RwLock<Config>>, tools: &[Box<dyn Tool>]) -> String {
    let cfg = config.read();
    let mut parts = Vec::new();

    // Base system prompt from config
    let base = &cfg.llm.system_prompt.content;
    if !base.is_empty() {
        parts.push(base.clone());
    }

    // Project context (AGENTS.md, git status)
    if cfg.llm.system_prompt.git_context {
        let ctx = crate::context::loader::load_context();
        let enriched = crate::context::loader::append_context("", &ctx);
        if !enriched.is_empty() {
            parts.push(enriched);
        }
    }

    // Tool guidelines
    let mut seen = std::collections::HashSet::new();
    let mut tool_lines = Vec::new();
    for tool in tools {
        for guideline in tool.prompt_guidelines() {
            if seen.insert(*guideline) {
                tool_lines.push(format!("- {guideline}"));
            }
        }
    }
    if !tool_lines.is_empty() {
        parts.push(format!(
            "# Tool Usage Guidelines\n{}",
            tool_lines.join("\n")
        ));
    }

    // Current date
    let now = chrono::Utc::now();
    parts.push(format!("Today's date: {}", now.format("%a %b %d %Y")));

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::core::errors::KonError;
    use crate::llm::providers::mock::MockProvider;
    use crate::tools as tools_mod;
    use std::path::PathBuf;

    async fn test_agent() -> (Agent, Session) {
        let config = Arc::new(RwLock::new(Config::load_defaults()));
        let provider = Box::new(MockProvider::new("mock".into()));
        let cwd = PathBuf::from("/tmp/test");
        let session = Session::new(cwd, "test sp".into(), vec![]).await.unwrap();

        (Agent::new(config, provider), session)
    }

    #[tokio::test]
    async fn test_agent_simple_run() {
        let (agent, mut session) = test_agent().await;
        let (event_tx, mut event_rx) = mpsc::channel(32);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(async move {
            agent
                .run(&mut session, "hello".into(), None, event_tx, cancel_rx)
                .await
        });

        // Collect events
        let mut events = Vec::new();
        while let Some(event) = event_rx.recv().await {
            let is_end = matches!(event, AgentEvent::End { .. });
            events.push(event);
            if is_end {
                break;
            }
        }

        let result = handle.await.unwrap().unwrap();
        assert!(result.total_turns >= 1);
        assert!(matches!(result.stop_reason, StopReason::Stop));

        // Should have Start, TurnStart, TextDelta, StreamDone (via TurnEnd), End
        assert!(events.iter().any(|e| matches!(e, AgentEvent::Start)));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AgentEvent::TurnStart { .. }))
        );
        assert!(events.iter().any(|e| matches!(e, AgentEvent::End { .. })));
    }

    #[tokio::test]
    async fn test_agent_tool_use_loop() {
        let mut config = Config::load_defaults();
        config.agent.max_turns = 2; // Prevent infinite loop from mock:tool
        let config = Arc::new(RwLock::new(config));
        let provider = Box::new(MockProvider::new("mock:tool".into()));
        let cwd = PathBuf::from("/tmp/test");
        let mut session = Session::new(cwd, "sp".into(), vec![]).await.unwrap();
        let agent = Agent::new(config, provider);

        let (event_tx, mut event_rx) = mpsc::channel(64);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(async move {
            agent
                .run(
                    &mut session,
                    "do something".into(),
                    None,
                    event_tx,
                    cancel_rx,
                )
                .await
        });

        let mut events = Vec::new();
        let timeout_dur = std::time::Duration::from_secs(5);
        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Some(e) => {
                            let is_end = matches!(e, AgentEvent::End { .. });
                            events.push(e);
                            if is_end {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = tokio::time::sleep(timeout_dur) => {
                    break;
                }
            }
        }

        let _ = handle.await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AgentEvent::ToolResult { .. }))
        );
    }

    #[tokio::test]
    async fn test_agent_cancellation() {
        let (agent, mut session) = test_agent().await;
        let (event_tx, _event_rx) = mpsc::channel(32);
        let (cancel_tx, cancel_rx) = watch::channel(false);

        // Cancel before starting
        cancel_tx.send(true).unwrap();

        let result = agent
            .run(&mut session, "hello".into(), None, event_tx, cancel_rx)
            .await;

        assert!(matches!(result, Err(KonError::Cancelled)));
    }

    #[test]
    fn test_build_system_prompt() {
        let config = Arc::new(RwLock::new(Config::load_defaults()));
        let tools = tools_mod::core_tools();
        let prompt = build_system_prompt(&config, &tools);

        assert!(prompt.contains("Zeus"));
        assert!(prompt.contains("Tool Usage Guidelines"));
        assert!(!prompt.is_empty());
    }
}
