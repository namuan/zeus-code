//! Agent loop orchestration.
//!
//! The Agent struct holds all state for a coding session and drives
//! the turn-by-turn interaction between the LLM, tools, and session
//! persistence.

use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::{mpsc, watch};

use crate::config::Config;
use crate::core::compaction::{CompactionConfig, OnOverflow, generate_summary, should_compact};
use crate::core::errors::KonResult;
use crate::core::types::{AgentEvent, Message, StopReason, Usage, UserMessage};
use crate::llm::base::Provider;
use crate::permissions::PermissionMode;
use crate::session::{Session, SessionEntry};
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
                // Identify messages to summarize vs. keep.
                let (to_summarize, first_kept_id) = find_compaction_split(&session.entries);

                if to_summarize.is_empty() {
                    tracing::debug!("Compaction triggered but no messages to summarize");
                } else {
                    let _ = event_tx.send(AgentEvent::CompactionStart).await;

                    match generate_summary(
                        self.provider.as_ref(),
                        &to_summarize,
                        first_kept_id,
                        cancel_rx.clone(),
                    )
                    .await
                    {
                        Ok(summary) => {
                            // Persist the compaction to the session.
                            let entry = SessionEntry::Compaction {
                                id: Session::next_entry_id(),
                                parent_id: session
                                    .entries
                                    .last()
                                    .and_then(|e| e.id())
                                    .unwrap_or("root")
                                    .to_string(),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                summary: summary.summary.clone(),
                                first_kept_entry_id: summary.first_kept_entry_id.clone(),
                                tokens_before: summary.tokens_before,
                            };
                            session.append_entry(entry).await?;

                            // Emit real CompactionEnd.
                            let _ = event_tx
                                .send(AgentEvent::CompactionEnd {
                                    summary: summary.summary,
                                    tokens_before: summary.tokens_before,
                                })
                                .await;
                        }
                        Err(e) => {
                            tracing::warn!("Compaction failed: {e}");
                            let _ = event_tx
                                .send(AgentEvent::Error {
                                    error: format!("Compaction failed: {e}"),
                                })
                                .await;
                        }
                    }

                    // Honor OnOverflow::Pause.
                    if self.compaction_config.on_overflow == OnOverflow::Pause {
                        stop_reason = StopReason::EndTurn;
                        break;
                    }
                }
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

// ── Compaction split helper ──────────────────────────────────────────────

/// Walk the active path through `entries` and split it into the messages to
/// summarize and the ID of the first kept message.
///
/// The split point is the **last** user message in the active path:
/// everything before it (including earlier user/assistant turns) goes to the
/// summary, and the last user message onward is kept intact. This preserves
/// the in-flight turn's context while compacting history.
///
/// Returns `(to_summarize, first_kept_entry_id)`. If there is nothing useful
/// to summarize (no user messages, or only a single user message at the start
/// of the path), the returned vector is empty and the ID is the empty string;
/// the caller is expected to skip compaction in that case.
fn find_compaction_split(entries: &[SessionEntry]) -> (Vec<Message>, String) {
    // Walk the active path. Each element is (entry_id, message).
    let mut path: Vec<(&str, &Message)> = Vec::new();
    let mut current_id: Option<&str> = entries.first().and_then(|e| e.id());

    while let Some(id) = current_id {
        let entry = entries.iter().find(|e| e.id() == Some(id));
        match entry {
            Some(SessionEntry::MessageEntry { message, .. }) => {
                path.push((id, message));
            }
            Some(SessionEntry::Compaction {
                first_kept_entry_id,
                ..
            }) => {
                // Mirror `Session::active_messages`: jump to the first kept
                // entry after a compaction.
                current_id = Some(first_kept_entry_id);
                continue;
            }
            _ => {}
        }
        current_id = entries
            .iter()
            .rfind(|e| e.parent_id() == Some(id))
            .and_then(|e| e.id());
    }

    // Find the last user message in the active path.
    let last_user_idx = path
        .iter()
        .rposition(|(_, m)| matches!(m, Message::User(_)));

    match last_user_idx {
        Some(0) => (Vec::new(), String::new()),
        Some(idx) => {
            let first_kept_id = path[idx].0.to_string();
            let to_summarize: Vec<Message> =
                path[..idx].iter().map(|(_, m)| (*m).clone()).collect();
            (to_summarize, first_kept_id)
        }
        None => (Vec::new(), String::new()),
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

    // ── Compaction tests ─────────────────────────────────────────────

    /// Build a session whose active path is:
    ///   header → user-1 → assistant-1 → user-2 → assistant-2
    async fn build_split_test_session() -> Session {
        use crate::core::types::{ContentBlock, UserMessage};

        let mut session = Session::new(PathBuf::from("/tmp/test"), "sp".into(), vec![])
            .await
            .unwrap();

        let _ = session
            .append_user_message(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "first user".into(),
                }],
                skill_name: None,
            })
            .await
            .unwrap();

        let _ = session
            .append_assistant_message(
                vec![ContentBlock::Text {
                    text: "first assistant".into(),
                }],
                None,
                Some(StopReason::Stop),
            )
            .await
            .unwrap();

        let _ = session
            .append_user_message(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "second user".into(),
                }],
                skill_name: None,
            })
            .await
            .unwrap();

        let _ = session
            .append_assistant_message(
                vec![ContentBlock::Text {
                    text: "second assistant".into(),
                }],
                None,
                Some(StopReason::Stop),
            )
            .await
            .unwrap();

        session
    }

    #[tokio::test]
    async fn test_find_compaction_split_basic() {
        let session = build_split_test_session().await;
        let (to_summarize, first_kept_id) = find_compaction_split(&session.entries);

        // The last user message is "second user" — the path before it is
        // [user-1, assistant-1].
        assert_eq!(to_summarize.len(), 2);
        assert!(matches!(to_summarize[0], Message::User(_)));
        assert!(matches!(to_summarize[1], Message::Assistant(_)));
        assert!(!first_kept_id.is_empty());
    }

    #[tokio::test]
    async fn test_find_compaction_split_single_user_returns_empty() {
        use crate::core::types::{ContentBlock, UserMessage};

        let mut session = Session::new(PathBuf::from("/tmp/test"), "sp".into(), vec![])
            .await
            .unwrap();
        let _ = session
            .append_user_message(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "only message".into(),
                }],
                skill_name: None,
            })
            .await
            .unwrap();

        let (to_summarize, first_kept_id) = find_compaction_split(&session.entries);
        assert!(to_summarize.is_empty());
        assert!(first_kept_id.is_empty());
    }

    #[tokio::test]
    async fn test_find_compaction_split_no_user_returns_empty() {
        use crate::core::types::ContentBlock;

        let mut session = Session::new(PathBuf::from("/tmp/test"), "sp".into(), vec![])
            .await
            .unwrap();
        let _ = session
            .append_assistant_message(
                vec![ContentBlock::Text {
                    text: "no user message in this session".into(),
                }],
                None,
                Some(StopReason::Stop),
            )
            .await
            .unwrap();

        let (to_summarize, first_kept_id) = find_compaction_split(&session.entries);
        assert!(to_summarize.is_empty());
        assert!(first_kept_id.is_empty());
    }

    #[tokio::test]
    async fn test_agent_compaction_triggers_and_persists() {
        use crate::core::compaction::OnOverflow;
        use crate::core::types::{ContentBlock, UserMessage};

        // Build a config with a tiny context window so compaction triggers
        // on the first turn.
        let mut config = Config::load_defaults();
        config.agent.default_context_window = 100;
        config.compaction.buffer_tokens = 0;
        config.compaction.on_overflow = OnOverflow::Continue;
        let config = Arc::new(RwLock::new(config));
        let provider = Box::new(MockProvider::new("mock".into()));
        let agent = Agent::new(config, provider);
        let mut session = Session::new(PathBuf::from("/tmp/test"), "sp".into(), vec![])
            .await
            .unwrap();

        // Pre-populate the session with a prior user message and assistant
        // turn. The new "hello" user message will be the second user message,
        // giving `find_compaction_split` something to summarize.
        let _ = session
            .append_user_message(UserMessage {
                content: vec![ContentBlock::Text {
                    text: "earlier question".into(),
                }],
                skill_name: None,
            })
            .await
            .unwrap();
        let _ = session
            .append_assistant_message(
                vec![ContentBlock::Text {
                    text: "earlier response".into(),
                }],
                None,
                Some(StopReason::Stop),
            )
            .await
            .unwrap();

        let (event_tx, mut event_rx) = mpsc::channel(64);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        // The session is moved into the spawn, so collect the events and the
        // final session state through the channel + a follow-up helper task.
        let (session_tx, mut session_rx) = mpsc::channel::<Session>(1);
        let handle = tokio::spawn(async move {
            let result = agent
                .run(&mut session, "hello".into(), None, event_tx, cancel_rx)
                .await;
            // Send the session back for post-run assertions.
            let _ = session_tx.send(session).await;
            result
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

        let result = handle.await.unwrap().unwrap();
        let session = session_rx.recv().await.expect("session was sent");
        assert!(matches!(result.stop_reason, StopReason::Stop));

        // CompactionStart was emitted.
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AgentEvent::CompactionStart)),
            "expected CompactionStart event"
        );

        // CompactionEnd was emitted with a non-empty summary.
        let compact_end = events
            .iter()
            .find_map(|e| match e {
                AgentEvent::CompactionEnd { summary, .. } => Some(summary.clone()),
                _ => None,
            })
            .expect("expected CompactionEnd event");
        assert!(!compact_end.is_empty());

        // A Compaction entry was appended to the session.
        let compactions: Vec<&SessionEntry> = session
            .entries
            .iter()
            .filter(|e| matches!(e, SessionEntry::Compaction { .. }))
            .collect();
        assert_eq!(
            compactions.len(),
            1,
            "expected exactly one Compaction entry"
        );

        // active_messages() returns the summary as a system message.
        let active = session.active_messages();
        let has_system_summary = active
            .iter()
            .any(|m| matches!(m, Message::System(s) if s.content == compact_end));
        assert!(
            has_system_summary,
            "expected active_messages to contain the compaction summary"
        );
    }
}
