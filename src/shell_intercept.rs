//! Shell command prefix interception.
//!
//! Provides parsing and execution for `!command` and `!!command` prefixes typed
//! in the input prompt. The single-bang form (`!`) executes the command and
//! shows the output in the chat but does not involve the LLM. The double-bang
//! form (`!!`) does the same and then forwards the command + its output to the
//! LLM as context so the assistant can analyze it.
//!
//! Execution is delegated to the existing [`crate::tools::bash::BashTool`] so
//! output truncation, timeout, and error handling stay consistent with the
//! LLM-driven `bash` tool.

use tokio::sync::watch;

use crate::core::errors::KonResult;
use crate::core::types::ToolResult;
use crate::tools::base::Tool;
use crate::tools::bash::BashTool;

/// Default timeout for shell commands intercepted from the prompt.
/// Matches the bash tool's default (120s).
pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// Detect a `!` or `!!` prefix in user input.
///
/// Returns:
/// - `Some((false, command))` for `!command` (execute, do not send to LLM).
/// - `Some((true, command))`  for `!!command` (execute, then send to LLM).
/// - `None` if there is no `!` prefix, or the command would be empty.
///
/// Whitespace immediately after the prefix is stripped; interior whitespace
/// is preserved. A `!!!command` input is treated as `!!command` with `!command`
/// passed to the shell (the leading `!` is part of the command).
pub fn parse_shell_prefix(text: &str) -> Option<(bool, String)> {
    let (send_to_llm, rest) = if let Some(rest) = text.strip_prefix("!!") {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix('!') {
        (false, rest)
    } else {
        return None;
    };

    let command = rest.trim().to_string();
    if command.is_empty() {
        return None;
    }

    Some((send_to_llm, command))
}

/// Execute a shell command using the same `BashTool` the LLM uses, so output
/// formatting, truncation, and timeout behavior are identical.
///
/// The cancel channel is a fresh dummy — there is no agent running to cancel
/// from a `!command`/`!!command` invocation.
pub async fn execute_shell(command: &str, timeout_ms: u64) -> KonResult<ToolResult> {
    let tool = BashTool::new();
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    let params = serde_json::json!({
        "command": command,
        "timeout_ms": timeout_ms,
    });
    tool.execute(params, cancel_rx).await
}

/// Build the user-facing prompt that gets sent to the LLM in `!!command` mode.
///
/// The formatted text includes the command, the captured output (or a failure
/// note), and uses fenced code blocks so the LLM can clearly distinguish the
/// command from its output.
pub fn format_command_output(command: &str, result: &ToolResult) -> String {
    let body = result
        .result
        .as_deref()
        .or(result.ui_details_full.as_deref())
        .unwrap_or("(no output)");

    let status = if result.success { "ok" } else { "failed" };

    format!(
        "I ran the following shell command (status: {status}):\n\
         ```\n\
         {command}\n\
         ```\n\
         \n\
         Output:\n\
         ```\n\
         {body}\n\
         ```"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_shell_prefix ────────────────────────────────────────────

    #[test]
    fn parse_single_bang_returns_execute_only() {
        let (send, cmd) = parse_shell_prefix("!ls").unwrap();
        assert!(!send);
        assert_eq!(cmd, "ls");
    }

    #[test]
    fn parse_double_bang_returns_send_to_llm() {
        let (send, cmd) = parse_shell_prefix("!!git status").unwrap();
        assert!(send);
        assert_eq!(cmd, "git status");
    }

    #[test]
    fn parse_no_prefix_returns_none() {
        assert!(parse_shell_prefix("hello").is_none());
        assert!(parse_shell_prefix("").is_none());
        assert!(parse_shell_prefix(" echo").is_none());
    }

    #[test]
    fn parse_empty_after_prefix_returns_none() {
        assert!(parse_shell_prefix("!").is_none());
        assert!(parse_shell_prefix("!   ").is_none());
        assert!(parse_shell_prefix("!!").is_none());
        assert!(parse_shell_prefix("!!\t").is_none());
    }

    #[test]
    fn parse_trims_leading_and_trailing_whitespace() {
        let (_, cmd) = parse_shell_prefix("!  ls  ").unwrap();
        assert_eq!(cmd, "ls");
        let (_, cmd) = parse_shell_prefix("!!  echo hello  ").unwrap();
        assert_eq!(cmd, "echo hello");
    }

    #[test]
    fn parse_preserves_interior_whitespace() {
        let (_, cmd) = parse_shell_prefix("!ls  -la  /tmp").unwrap();
        assert_eq!(cmd, "ls  -la  /tmp");
    }

    #[test]
    fn parse_triple_bang_is_double_bang_with_bang_command() {
        // !!!foo -> !! (send to LLM) + command "!foo"
        let (send, cmd) = parse_shell_prefix("!!!echo hi").unwrap();
        assert!(send);
        assert_eq!(cmd, "!echo hi");
    }

    #[test]
    fn parse_preserves_newlines_inside_command() {
        // Newlines inside the command are not whitespace-padded in trim,
        // but trim() also strips \n at the edges. Interior \n preserved.
        let (_, cmd) = parse_shell_prefix("!printf 'a\\nb\\n'").unwrap();
        assert_eq!(cmd, "printf 'a\\nb\\n'");
    }

    // ── format_command_output ─────────────────────────────────────────

    fn result(success: bool, body: Option<&str>) -> ToolResult {
        ToolResult {
            success,
            result: body.map(|s| s.to_string()),
            images: vec![],
            ui_summary: Some("summary".into()),
            ui_details: None,
            ui_details_full: Some(body.unwrap_or("").to_string()),
            file_changes: None,
        }
    }

    #[test]
    fn format_output_includes_command_and_body() {
        let r = result(true, Some("hello\nworld"));
        let out = format_command_output("echo hi", &r);
        assert!(out.contains("echo hi"));
        assert!(out.contains("hello\nworld"));
        assert!(out.contains("ok"));
        assert!(out.contains("```"));
    }

    #[test]
    fn format_output_marks_failure_status() {
        let r = result(false, Some("oops"));
        let out = format_command_output("bad-cmd", &r);
        assert!(out.contains("failed"));
        assert!(out.contains("oops"));
    }

    #[test]
    fn format_output_falls_back_to_details_full() {
        let mut r = result(true, None);
        r.ui_details_full = Some("from details".into());
        let out = format_command_output("cmd", &r);
        assert!(out.contains("from details"));
    }

    // ── execute_shell (integration with BashTool) ─────────────────────

    #[tokio::test]
    async fn execute_shell_runs_echo() {
        let r = execute_shell("echo intercepted", DEFAULT_TIMEOUT_MS)
            .await
            .unwrap();
        assert!(r.success);
        assert!(r.result.unwrap().contains("intercepted"));
    }
}
