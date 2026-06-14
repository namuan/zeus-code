//! Shell command execution tool.
//!
//! Executes shell commands via `sh -c`, with configurable timeout and
//! cancellation support. Captures stdout/stderr and truncates long output.

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::ToolResult;
use crate::tools::base::Tool;
use crate::tools::utils::truncate_output;

pub struct BashTool;

impl BashTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the current working directory. \
         Commands run with a timeout (default 120s). Output is truncated \
         after 500 lines. Use for running tests, building, git operations, \
         and other shell tasks. Long-running commands are cancelled if the \
         user interrupts."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 120000)"
                }
            },
            "required": ["command"]
        })
    }

    fn is_mutating(&self) -> bool {
        true
    }

    fn icon(&self) -> &str {
        "$"
    }

    fn prompt_guidelines(&self) -> &[&str] {
        &[
            "Run commands in the project root directory unless specified otherwise",
            "Prefer non-interactive commands (use flags like --yes for npx/npm)",
            "Chain commands with && for sequential execution",
            "Use git commands for version control operations",
        ]
    }

    async fn execute(
        &self,
        params: Value,
        mut cancel: watch::Receiver<bool>,
    ) -> KonResult<ToolResult> {
        let command = params["command"].as_str().ok_or_else(|| KonError::Tool {
            tool: "bash".into(),
            message: "missing 'command' parameter".into(),
        })?;

        let timeout_ms = params["timeout_ms"].as_u64().unwrap_or(120_000);

        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| KonError::Tool {
                tool: "bash".into(),
                message: format!("failed to spawn command: {e}"),
            })?;

        let pid = child.id();

        let output = tokio::select! {
            result = child.wait_with_output() => {
                result.map_err(|e| KonError::Tool {
                    tool: "bash".into(),
                    message: format!("command execution failed: {e}"),
                })?
            }
            _ = cancel.changed() => {
                if let Some(pid) = pid {
                    kill_process(pid);
                }
                return Err(KonError::Cancelled);
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)) => {
                if let Some(pid) = pid {
                    kill_process(pid);
                }
                return Ok(ToolResult {
                    success: false,
                    result: Some(format!(
                        "Command timed out after {timeout_ms}ms."
                    )),
                    images: vec![],
                    ui_summary: Some("Timed out".into()),
                    ui_details: None,
                    ui_details_full: None,
                    file_changes: None,
                });
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let success = output.status.success();
        let exit_code = output.status.code().unwrap_or(-1);

        let mut result_text = String::new();
        if !stdout.is_empty() {
            result_text.push_str(&truncate_output(&stdout, 500, 20000));
        }
        if !stderr.is_empty() {
            if !result_text.is_empty() {
                result_text.push_str("\n\n[stderr]\n");
            }
            result_text.push_str(&truncate_output(&stderr, 200, 10000));
        }
        if result_text.is_empty() {
            result_text = "(no output)".into();
        }

        let summary = if success {
            format!("✓ exit {exit_code} ({} lines)", stdout.lines().count())
        } else {
            format!("✗ exit {exit_code} ({} lines)", stdout.lines().count())
        };

        Ok(ToolResult {
            success,
            result: Some(result_text),
            images: vec![],
            ui_summary: Some(summary),
            ui_details: Some(truncate_output(&stdout, 500, 20000)),
            ui_details_full: Some(stdout),
            file_changes: None,
        })
    }

    fn format_call(&self, params: &Value) -> String {
        let cmd = params["command"].as_str().unwrap_or("?");
        if cmd.len() > 60 {
            format!("{}...", &cmd[..57])
        } else {
            cmd.to_string()
        }
    }

    fn format_preview(&self, params: &Value) -> Option<String> {
        params["command"].as_str().map(|c| c.to_string())
    }
}

/// Kill a process group by PID on Unix systems.
#[cfg(unix)]
fn kill_process(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_process(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bash_tool_metadata() {
        let tool = BashTool::new();
        assert_eq!(tool.name(), "bash");
        assert_eq!(tool.icon(), "$");
        assert!(tool.is_mutating());
    }

    #[tokio::test]
    async fn test_bash_echo() {
        let tool = BashTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({"command": "echo hello world"});
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        let text = result.result.unwrap();
        assert!(text.contains("hello world"));
    }

    #[tokio::test]
    async fn test_bash_failing_command() {
        let tool = BashTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({"command": "exit 1"});
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(!result.success);
        assert!(result.ui_summary.unwrap().contains("exit 1"));
    }

    #[tokio::test]
    async fn test_bash_stderr_captured() {
        let tool = BashTool::new();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let params = serde_json::json!({"command": "echo error >&2"});
        let result = tool.execute(params, cancel_rx).await.unwrap();

        assert!(result.success);
        let text = result.result.unwrap();
        assert!(text.contains("error"));
    }

    #[tokio::test]
    async fn test_bash_cancellation() {
        let tool = BashTool::new();
        let (cancel_tx, cancel_rx) = watch::channel(false);

        // Send cancel before execution starts
        cancel_tx.send(true).unwrap();

        let params = serde_json::json!({"command": "echo should_not_run"});
        let result = tool.execute(params, cancel_rx).await;

        assert!(matches!(result, Err(KonError::Cancelled)));
    }
}
