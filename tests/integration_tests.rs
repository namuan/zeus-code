//! Integration tests — end-to-end workflows.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::{mpsc, watch};

use zeus_code::config::Config;
use zeus_code::core::types::{AgentEvent, StopReason};
use zeus_code::llm::providers::{ProviderConfig, create_provider};
use zeus_code::r#loop::Agent;
use zeus_code::session::Session;

/// Full agent run: user message → assistant response → events emitted.
#[tokio::test]
async fn test_full_agent_run_text_response() {
    let config = Arc::new(RwLock::new(Config::load_defaults()));
    let provider = create_provider(&ProviderConfig::new("mock", "mock", "")).unwrap();
    let cwd = PathBuf::from("/tmp/test-integration");
    let mut session = Session::new(cwd, "test sp".into(), vec![]).await.unwrap();

    let agent = Agent::new(config, provider);

    let (event_tx, mut event_rx) = mpsc::channel(128);
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    let handle = tokio::spawn(async move {
        agent
            .run(&mut session, "hello".into(), None, event_tx, cancel_rx)
            .await
    });

    let mut got_start = false;
    let mut got_turn = false;
    let mut got_text = false;
    let mut got_end = false;

    while let Some(event) = event_rx.recv().await {
        match event {
            AgentEvent::Start => got_start = true,
            AgentEvent::TurnStart { .. } => got_turn = true,
            AgentEvent::TextDelta { .. } => got_text = true,
            AgentEvent::End { .. } => {
                got_end = true;
                break;
            }
            _ => {}
        }
    }

    let result = handle.await.unwrap();
    assert!(result.is_ok());

    assert!(got_start);
    assert!(got_turn);
    assert!(got_text);
    assert!(got_end);
}

/// Verify that the agent stops after max_turns.
#[tokio::test]
async fn test_agent_respects_max_turns() {
    let mut config = Config::load_defaults();
    config.agent.max_turns = 1; // Only allow one turn
    let config = Arc::new(RwLock::new(config));

    // Use mock:tool which returns ToolUse → agent would loop if max_turns not enforced
    let provider = create_provider(&ProviderConfig::new("mock", "mock:tool", "")).unwrap();
    let cwd = PathBuf::from("/tmp/test-max-turns");
    let mut session = Session::new(cwd, "sp".into(), vec![]).await.unwrap();

    let agent = Agent::new(config, provider);

    let (event_tx, mut event_rx) = mpsc::channel(128);
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    let handle = tokio::spawn(async move {
        agent
            .run(&mut session, "test".into(), None, event_tx, cancel_rx)
            .await
    });

    let mut end_reason = None;
    let mut total_turns = 0;

    while let Some(event) = event_rx.recv().await {
        if let AgentEvent::End {
            stop_reason,
            total_turns: turns,
            ..
        } = event
        {
            end_reason = Some(stop_reason);
            total_turns = turns;
            break;
        }
    }

    let result = handle.await.unwrap();
    assert!(result.is_ok());
    assert_eq!(total_turns, 1);
    assert_eq!(end_reason, Some(StopReason::MaxTurns));
}

/// Session: create → append → load → verify.
#[tokio::test]
async fn test_session_create_append_load() {
    let cwd = PathBuf::from("/tmp/test-session-lifecycle");
    let mut session = Session::new(cwd.clone(), "system prompt".into(), vec!["read".into()])
        .await
        .unwrap();

    // Append a user message
    session
        .append_user_message(zeus_code::core::types::UserMessage {
            content: vec![zeus_code::core::types::ContentBlock::Text {
                text: "hello world".into(),
            }],
            skill_name: None,
        })
        .await
        .unwrap();

    assert_eq!(session.entries.len(), 2);

    // Get active messages
    let messages = session.active_messages();
    assert_eq!(messages.len(), 1);
    assert!(matches!(
        &messages[0],
        zeus_code::core::types::Message::User(_)
    ));

    // Reload from disk
    let path = session.file_path.clone();
    drop(session);

    let loaded = Session::load(path).await.unwrap();
    assert_eq!(loaded.entries.len(), 2);
    assert_eq!(loaded.system_prompt, "system prompt");
    assert_eq!(loaded.active_tools, vec!["read"]);
}

/// Config load → override → save → reload.
#[test]
fn test_config_roundtrip() {
    let mut config = Config::load_defaults();
    config.llm.default_model = "custom-model".into();
    config.ui.theme = "dracula".into();

    let toml_str = toml::to_string_pretty(&config).unwrap();
    let loaded: Config = toml::from_str(&toml_str).unwrap();

    assert_eq!(loaded.llm.default_model, "custom-model");
    assert_eq!(loaded.ui.theme, "dracula");
    // Other fields should be preserved
    assert_eq!(loaded.agent.max_turns, 500);
}
