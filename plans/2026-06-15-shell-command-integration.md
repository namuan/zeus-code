# Plan: Shell Command Integration (`!command`, `!!command`)

## Overview

Add support for executing shell commands directly from the input prompt, bypassing
the LLM. Two prefix levels:

| Prefix | Behavior |
|--------|----------|
| `!command` | Execute shell command, display output in chat. No LLM involved. |
| `!!command` | Execute shell command, display output, then send the command + its output to the LLM as context. |

This unifies the CLI and TUI experience with shell out ergonomics familiar to
tools like Claude Code, while keeping the LLM loop untouched for regular prompts.

## Semantics & Edge Cases

- `!ls` → execute `ls`, show output, stop.
- `!!git diff` → execute `git diff`, show output, then LLM analyzes the diff.
- `!` alone → no-op (empty command, returns to prompt).
- `!!` alone → no-op (empty command, returns to prompt).
- `!  ls  -la  ` → trims whitespace; command is `ls  -la`.
- `!!!something` → interpreted as `!!` + `!something` (triple-`!` is double-`!` mode with
  the rest as the command). This is consistent with how the bash tool treats `!` in shell.
- `!command` while agent is running → allowed. Shell commands are side-effect-free
  from the agent's perspective; the user can peek at files.
- `!!command` while agent is running → blocked; show a status message and ignore.

## File Changes

### 1. New: `src/shell_intercept.rs`

Core logic shared by both TUI and headless modes.

**`pub fn parse_shell_prefix(text: &str) -> Option<(bool, String)>`**

- Returns `None` if `text` does not start with `!`.
- Returns `Some((false, cmd))` for `!cmd` (execute only).
- Returns `Some((true, cmd))` for `!!cmd` (execute + send to LLM).
- Strips the prefix and leading/trailing whitespace from the command.
- Empty command (after trimming) returns `None`.
- `!!!cmd` returns `Some((true, format!("!{cmd}")))` so the `!` is preserved as part
  of the command for the shell.

**`pub async fn execute_shell(command: &str, timeout_ms: u64) -> Result<ToolResult>`**

- Reuses `BashTool` from `src/tools/bash.rs` — instantiates `BashTool::new()`, calls
  `execute()` with `serde_json::json!({"command": command, "timeout_ms": timeout_ms})`.
- Uses a dummy `watch::channel(false)` for cancellation (never fires).
- This guarantees consistent execution, output truncation, and error handling with
  the LLM's bash tool.

**`pub fn format_command_output(command: &str, result: &ToolResult) -> String`**

Builds the text sent to the LLM in `!!command` mode:

```
I ran the following shell command:
```
<command>
```

Output:
```
<result.result>
```

If the result failed, the word "failed" is prepended; if `result.result` is `None`,
the `ui_details_full` is used instead.

**Tests** — all in the same file:

- `parse_shell_prefix("!ls")` → `Some((false, "ls"))`
- `parse_shell_prefix("!!git status")` → `Some((true, "git status"))`
- `parse_shell_prefix("hello")` → `None`
- `parse_shell_prefix("!")` → `None`
- `parse_shell_prefix("!!")` → `None`
- `parse_shell_prefix("!   ls  -la  ")` → `Some((false, "ls  -la"))` (trims only edges)
- `parse_shell_prefix("!!  echo hello  ")` → `Some((true, "echo hello"))`
- `parse_shell_prefix("!!!something")` → `Some((true, "!something"))`
- `format_command_output` contains the command, output, and a fenced code block.

### 2. Modify: `src/lib.rs`

Add module declaration:

```rust
pub mod shell_intercept;
```

### 3. Modify: `src/core/types.rs`

Add one variant to `AgentEvent`:

```rust
/// Result of a `!command` / `!!command` shell execution. Routes from the
/// async spawn back to the TUI for rendering and optional LLM forwarding.
ShellResult {
    command: String,
    result: ToolResult,
    send_to_llm: bool,
},
```

The variant is channel-only (`AgentEvent` is not serialized) so no serde impact.

Add a unit test in the existing `mod tests`:

```rust
#[test]
fn test_agent_event_shell_result_construction() {
    let ev = AgentEvent::ShellResult {
        command: "ls".into(),
        result: ToolResult { success: true, result: Some("file.txt".into()), ..Default::default() },
        send_to_llm: false,
    };
    match ev {
        AgentEvent::ShellResult { command, send_to_llm, .. } => {
            assert_eq!(command, "ls");
            assert!(!send_to_llm);
        }
        _ => panic!("wrong variant"),
    }
}
```

(This requires `ToolResult` to implement `Default` — see step 4.)

### 4. Modify: `src/core/types.rs` — add `Default` for `ToolResult`

`ToolResult` currently lacks `Default`. Add a `#[derive(Default)]` after the
existing `#[derive(...)]` attributes. All fields have natural defaults:
`success: false`, `result: None`, `images: vec![]`, etc.

### 5. Modify: `src/ui/app.rs`

#### 5a. `handle_submit()` — intercept `!` prefix

Insert shell prefix detection between the existing slash-command check (line 465)
and the `self.run_agent(text)` call (line 495). Block `!!command` if `agent_running`
is true.

```rust
// Check for shell commands (! / !!)
if let Some((send_to_llm, command)) = crate::shell_intercept::parse_shell_prefix(text) {
    if send_to_llm && self.agent_running {
        self.chat.add_block(render_status(
            "Cannot run !!command while agent is running",
            &self.styles,
            true,
        ));
        return;
    }

    let command = command.to_string();

    // Display the command in chat as a "You" message
    let user_block = render_user_message(text, None, &self.styles);
    self.chat.add_line(String::new(), &self.styles);
    self.chat.add_block(user_block);

    // Add a "Running…" placeholder line
    self.working_mark = Some(self.chat.len());
    self.working_start = Some(Instant::now());
    self.chat.add_line("  … Running shell command…", &self.styles);

    // Spawn async execution
    let event_tx = self.event_tx.clone();
    tokio::spawn(async move {
        let timeout_ms = 120_000;
        let result = crate::shell_intercept::execute_shell(&command, timeout_ms)
            .await
            .unwrap_or_else(|e| ToolResult {
                success: false,
                result: Some(format!("{e}")),
                images: vec![],
                ui_summary: Some(format!("Shell error: {e}")),
                ui_details: None,
                ui_details_full: None,
                file_changes: None,
            });
        let _ = event_tx.send(AgentEvent::ShellResult {
            command,
            result,
            send_to_llm,
        }).await;
    });

    return; // Don't fall through to run_agent()
}
```

#### 5b. `process_events()` — handle `ShellResult` event

Add a new match arm in the event processing loop (near the existing `ToolResult`
handler around line 570):

```rust
AgentEvent::ShellResult { command, result, send_to_llm } => {
    // Remove the "Running shell command…" placeholder
    if let Some(mark) = self.working_mark.take() {
        self.chat.truncate_to(mark);
    }
    self.working_start = None;

    // Render output
    let block = render_tool_result("", "shell", &result, &self.styles);
    self.chat.add_block(block);

    // For !!command: forward to LLM as context
    if send_to_llm {
        let augmented = crate::shell_intercept::format_command_output(&command, &result);
        self.chat.scroll_to_bottom();
        self.run_agent(&augmented);
    }
}
```

### 6. Modify: `src/headless.rs`

Intercept the prompt before `agent.run()` (around lines 38-53 / 121-126):

```rust
// After the prompt is read, check for shell prefix
let prompt = match crate::shell_intercept::parse_shell_prefix(&prompt) {
    Some((false, command)) => {
        // !command: pure shell, exit with the command's status
        let result = crate::shell_intercept::execute_shell(&command, 120_000).await?;
        if let Some(ref output) = result.result {
            print!("{output}");
        }
        if let Some(ref details) = result.ui_details_full {
            eprint!("{details}");
        }
        return Ok(if result.success { 0 } else { 1 });
    }
    Some((true, command)) => {
        // !!command: execute, capture output, then send to LLM
        let result = crate::shell_intercept::execute_shell(&command, 120_000).await?;
        if let Some(ref output) = result.result {
            println!("{output}");
        }
        if let Some(ref details) = result.ui_details_full {
            eprintln!("{details}");
        }
        crate::shell_intercept::format_command_output(&command, &result)
    }
    None => prompt, // Not a shell command, normal flow
};
```

The rest of `run_headless_inner()` (creating config, session, agent) is unchanged;
`prompt` is now either the original prompt or the augmented shell-context string.

### 7. Tests

Unit tests in `src/shell_intercept.rs` (described in step 1).
Unit test in `src/core/types.rs` (described in step 3).

Integration test in `tests/shell_intercept_integration.rs` (optional):

- Spawn a process that runs `zeus -p '!echo hello'` (or a small helper binary) and
  verify exit code 0 and stdout contains `hello`. Skip this if the test environment
  is too fragile; the unit tests cover the core logic.

## Verification

After implementation, run the full check sequence from `AGENTS.md`:

```
cargo fmt
cargo clippy -- -D warnings
cargo test
```

Smoke test the TUI:

```
cargo run -- -m mock        # then type !echo hello, !!git status in the TUI
```

Smoke test the headless modes:

```
cargo run -- -p '!echo hello world'
cargo run -- -p '!!echo hello world'   # goes through mock provider
```

## File Change Summary

| File | Change |
|------|--------|
| `src/lib.rs` | +1 line: `pub mod shell_intercept;` |
| `src/shell_intercept.rs` | **New file** (~100 lines): parse, execute, format, plus unit tests |
| `src/core/types.rs` | +8 lines: `ShellResult` variant + unit test; `Default` derive on `ToolResult` |
| `src/ui/app.rs` | +50 lines in `handle_submit()`, +12 in `process_events()` |
| `src/headless.rs` | +25 lines before `agent.run()` call |
| `tests/shell_intercept_integration.rs` | Optional integration test |

## Decisions Log

- **Reuse `BashTool`** rather than building a new executor — guarantees consistent
  output truncation, timeout, and cancellation semantics with the LLM's bash tool.
- **`AgentEvent::ShellResult` variant** rather than a new channel — minimal
  plumbing, fits the existing event-driven model.
- **No new permissions** for `!command` — user-initiated, not LLM-initiated.
- **Ephemeral output** for `!command` (not in session JSONL); `!!command` output is
  in the LLM conversation and therefore persisted naturally.
- **Block `!!command` while agent running** — avoids double-LLM-message conflicts;
  `!command` remains allowed because it's pure local execution.
