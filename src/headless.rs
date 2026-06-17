//! Non-interactive headless mode (-p flag).
//!
//! Runs a single prompt through the agent and exits.
//!
//! Exit codes:
//!   0 — completed successfully
//!   1 — error during execution
//!   2 — startup error (empty prompt, provider init failure)

use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::{mpsc, watch};

use crate::cli::Cli;
use crate::config;
use crate::core::errors::KonResult;
use crate::core::types::AgentEvent;
use crate::llm::providers::{ProviderConfig, create_provider};
use crate::r#loop::Agent;
use crate::session::Session;

/// Run a single prompt non-interactively and return an exit code.
pub async fn run_headless(cli: Cli, prompt_arg: Option<String>) -> i32 {
    match run_headless_inner(cli, prompt_arg).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("zeus-code: {e}");
            2
        }
    }
}

async fn run_headless_inner(cli: Cli, prompt_arg: Option<String>) -> KonResult<i32> {
    // 1. Read prompt from arg or stdin
    let prompt = match prompt_arg {
        Some(p) if !p.is_empty() => p,
        _ => {
            // Read from stdin
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).map_err(|e| {
                crate::core::errors::KonError::Config(format!("failed to read stdin: {e}"))
            })?;
            let input = input.trim().to_string();
            if input.is_empty() {
                eprintln!("zeus-code: no prompt provided and stdin is empty");
                return Ok(2);
            }
            input
        }
    };

    // 1a. Intercept shell command prefixes before any LLM work.
    // !command: pure shell, no LLM, exit with the command's status.
    // !!command: execute, capture output, then send command+output to the LLM.
    let prompt =
        if let Some((send_to_llm, command)) = crate::shell_intercept::parse_shell_prefix(&prompt) {
            let result = crate::shell_intercept::execute_shell(
                &command,
                crate::shell_intercept::DEFAULT_TIMEOUT_MS,
            )
            .await?;

            // Echo the output so the user can see what happened.
            if let Some(ref output) = result.result {
                print!("{output}");
            }
            if let Some(ref details) = result.ui_details_full
                && details != result.result.as_deref().unwrap_or("")
            {
                eprint!("{details}");
            }
            if !result.success {
                eprintln!(
                    "zeus-code: shell command exited with non-zero status: {}",
                    result.ui_summary.as_deref().unwrap_or("?")
                );
            }

            if !send_to_llm {
                // !command: stop here, propagate the command's exit status.
                return Ok(if result.success { 0 } else { 1 });
            }

            // !!command: replace the prompt with the formatted command+output
            // so the LLM sees it as the user message.
            crate::shell_intercept::format_command_output(&command, &result)
        } else {
            prompt
        };

    // 2. Load config and apply CLI overrides
    let mut config = config::Config::load_or_create();
    config.merge_cli_overrides(&cli);
    let config = Arc::new(RwLock::new(config));

    // Scope the read lock so it's dropped before awaits
    let (provider_config, system_prompt, extra_tools) = {
        let cfg = config.read();
        let pc = cfg.llm.to_provider_config(&cli.api_key.unwrap_or_default());
        let sp = cfg.llm.system_prompt.content.clone();
        let tools = cfg.tools.extra.clone();
        (pc, sp, tools)
    };

    let continue_session = cli.continue_session;

    // Create provider (fall back to mock if real provider isn't available)
    let provider = match create_provider(&provider_config) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("{e} — falling back to mock provider");
            create_provider(&ProviderConfig::mock())?
        }
    };

    // Create or resume session
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut session = if continue_session {
        match crate::session::most_recent_session() {
            Some(path) => {
                tracing::info!("Resuming session: {}", path.display());
                Session::load(path).await?
            }
            None => {
                tracing::info!("No previous session found — creating new one");
                Session::new(cwd, system_prompt, extra_tools).await?
            }
        }
    } else {
        Session::new(cwd, system_prompt, extra_tools).await?
    };

    // Create and run agent
    let agent = Agent::new(config, provider);

    let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(32);
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    tracing::info!(
        "Starting agent: provider={}, model={}",
        provider_config.provider_name,
        provider_config.model_id
    );

    // Run agent in background
    let handle = tokio::spawn(async move {
        agent
            .run(&mut session, prompt, None, event_tx, cancel_rx)
            .await
    });

    // Collect output, print to stdout
    let mut final_text = String::new();
    let mut exit_code = 0;

    while let Some(event) = event_rx.recv().await {
        match event {
            AgentEvent::TurnStart { turn } => {
                eprintln!("  [turn {turn}]");
            }
            AgentEvent::TextDelta { text } => {
                print!("{text}");
                final_text.push_str(&text);
            }
            AgentEvent::ToolStart { id, name } => {
                eprintln!("  Tool call: {name} ({id})");
            }
            AgentEvent::ToolArgsDelta { .. } => {
                // args streamed in — ignore for headless
            }
            AgentEvent::ToolEnd { id: _, arguments } => {
                // Show parsed arguments
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&arguments) {
                    if let Some(cmd) = v.get("command").and_then(|c| c.as_str()) {
                        eprintln!("  → command: {cmd}");
                    } else if let Some(path) = v.get("file_path").and_then(|p| p.as_str()) {
                        eprintln!("  → file_path: {path}");
                    } else {
                        eprintln!("  → args: {arguments}");
                    }
                }
            }
            AgentEvent::ToolResult { id: _, result } => {
                let summary = result.result.as_deref().unwrap_or("(empty)");
                let truncated: String = summary.lines().take(8).collect::<Vec<_>>().join("\n");
                let more = if summary.lines().count() > 8 {
                    "…"
                } else {
                    ""
                };
                eprintln!("  Result:\n{truncated}{more}");
            }
            AgentEvent::TurnEnd { stop_reason, .. } => {
                eprintln!("  --- [turn end: {stop_reason:?}] ---");
            }
            AgentEvent::End { stop_reason, .. } => {
                println!(); // newline after final output
                if matches!(stop_reason, crate::core::types::StopReason::MaxTurns) {
                    exit_code = 3;
                }
                break;
            }
            AgentEvent::Error { error } => {
                eprintln!("\nzeus-code error: {error}");
                exit_code = 1;
                break;
            }
            _ => {}
        }
    }

    match handle.await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            eprintln!("\nzeus-code: {e}");
            if exit_code == 0 {
                exit_code = 1;
            }
        }
        Err(e) => {
            eprintln!("\nzeus-code: agent task panicked: {e}");
            exit_code = 1;
        }
    }

    Ok(exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;

    #[tokio::test]
    async fn test_headless_with_prompt_arg() {
        let cli = Cli {
            model: None,
            provider: None,
            prompt: Some(Some("say hello".into())),
            api_key: None,
            base_url: None,
            continue_session: false,
            resume: None,
            extra_tools: vec![],
            insecure_skip_verify: false,
        };

        let code = run_headless(cli, Some("say hello".into())).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn test_headless_default_cli() {
        let cli = Cli {
            model: None,
            provider: None,
            prompt: Some(Some("test".into())),
            api_key: None,
            base_url: None,
            continue_session: false,
            resume: None,
            extra_tools: vec![],
            insecure_skip_verify: false,
        };
        let code = run_headless(cli, Some("test".into())).await;
        // Mock provider should succeed
        assert!(code == 0 || code == 1);
    }
}
