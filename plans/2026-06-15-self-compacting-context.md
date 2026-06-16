# Plan: Self-Compacting Context (Real LLM-Powered `generate_summary`)

## Overview

The self-compacting context feature has its scaffolding in place: token estimation,
overflow detection (`should_compact`), `CompactionSummary` data structure, the
`Compaction` session entry type, and the `CompactionStart` / `CompactionEnd` agent
events. What's missing is the actual LLM call — `generate_summary()` in
`src/core/compaction.rs:127` returns `Err(... "not yet implemented")`, and the
agent loop in `src/loop.rs:174-184` has inline stub code that logs
"Context compaction triggered (stub)" and emits a fake `CompactionEnd` event with
hardcoded data.

This plan replaces the stub with a real implementation that:
1. Calls the LLM to produce a real conversation summary.
2. Persists the summary to the session as a `Compaction` entry.
3. Injects the summary into the LLM context as a synthetic system message on
   subsequent turns.
4. Wires the agent loop to perform the full compaction cycle (not just log it).
5. Respects the `OnOverflow::Continue` / `OnOverflow::Pause` config setting.

## Semantics & Edge Cases

- **What to summarize vs. keep**: Keep the most recent user message and all
  subsequent assistant/tool-result messages (the current incomplete turn).
  Summarize everything before that. This preserves immediate conversational
  context while compacting history.
- **`first_kept_entry_id`**: The ID of the oldest kept message entry. This lets
  `active_messages()` skip the summarized range.
- **Summary injection**: The summary is stored in the `SessionEntry::Compaction`
  variant's `summary` field. `active_messages()` is updated to push it as a
  `Message::System` into the returned `messages` vector before skipping to
  `first_kept_entry_id`. This way it flows naturally into the LLM context.
- **`OnOverflow::Continue`**: After compaction, continue the loop (the next
  turn will see the compacted messages).
- **`OnOverflow::Pause`**: After compaction, break the loop. The session is
  persisted with the compaction; the user can continue in a new session or
  resume.
- **Cancellation**: `generate_summary` must respect the existing
  `watch::Receiver<bool>` cancellation channel. If cancelled mid-stream, the
  partial summary is discarded and a `KonError::Cancelled` is returned.
- **Empty messages**: If there are no messages to summarize, return an error
  (compaction shouldn't be triggered with zero messages in practice, but
  guard anyway).
- **Mock provider support**: `MockProvider` already returns a canned text
  response by default — no new mode needed. The summarization call will get
  back the canned text.

## File Changes

### 1. Modify: `src/core/compaction.rs`

Replace the stub `generate_summary()` with a real implementation.

**New signature**:

```rust
pub async fn generate_summary(
    provider: &dyn Provider,
    messages_to_summarize: &[Message],
    first_kept_entry_id: String,
    cancel: watch::Receiver<bool>,
) -> KonResult<CompactionSummary>
```

**Implementation outline**:

1. **Guard empty input** — if `messages_to_summarize` is empty, return
   `KonError::Other("nothing to summarize".into())`.
2. **Serialize messages to a textual form** for the LLM prompt. Walk each
   `Message` variant and produce a labeled text block:
   - `Message::User(m)` → `User: <concatenated text blocks>`
   - `Message::Assistant(m)` → `Assistant: <text blocks>` (skip thinking
     blocks to keep prompt small; include text + tool-call summaries)
   - `Message::ToolResult(m)` → `Tool Result (<tool_name>): <truncated content>`
   - `Message::System(m)` → `System: <content>`
   Truncate each entry to a reasonable size (e.g., 2000 chars) to prevent
   the summarization prompt from being as large as the conversation.
3. **Build the summarization prompt** as a single `Message::User` containing:
   ```
   You are a conversation summarizer. Summarize the conversation below
   so it can be used as compact context for continuing the work.

   Preserve:
   - Key decisions and their rationale
   - File changes (which files, what was done)
   - Unresolved questions or pending tasks
   - Important context (variables, environment, user constraints)

   Be concise but thorough. Output plain text, no markdown headers needed.

   <conversation>
   {serialized messages}
   </conversation>
   ```
4. **Call `provider.stream()`** with a single `Message::User` containing the
   prompt, no tools, and the cancel receiver. System prompt is `None`
   (the summarization instructions are embedded in the user message).
5. **Consume the stream** — collect all `TextDelta` parts into a
   `summary_text: String`. Stop at `StreamDone` (use the existing pattern
   from `TurnRunner::stream_and_consume` for cancellation handling).
6. **Compute token counts**:
   - `tokens_before` = sum of `estimate_message_tokens` for
     `messages_to_summarize`.
   - `tokens_after` = `estimate_tokens(&summary_text)`.
7. **Return `CompactionSummary`** populated with the above.

**New imports** (add at top of file):

```rust
use futures::StreamExt;
use tokio::sync::watch;
use tokio_stream::Stream;

use crate::llm::base::Provider;
```

(Note: `Stream` import may be implicit through `LLMStream`. Only add what
the compiler requires.)

**New constant** for the truncation limit:

```rust
const MAX_ENTRY_CHARS: usize = 2000;
```

**New helper functions** (private to the module):

- `fn messages_to_summary_prompt_text(messages: &[Message]) -> String` —
  serializes messages to a textual form with per-entry truncation.
- `const SUMMARIZATION_INSTRUCTION: &str = ...` — the prompt template.

**Updated `test_generate_summary_stub_returns_error`** → replaced with
positive tests:

- `test_generate_summary_with_mock_provider` — uses `MockProvider::new("mock")`,
  sends a few messages, verifies a non-empty summary is returned and
  `tokens_before > 0`, `tokens_after > 0`, `first_kept_entry_id` is preserved.
- `test_generate_summary_empty_messages_returns_error` — empty input → error.
- `test_messages_to_summary_prompt_text` — unit test the serialization helper
  (user message → `"User: hello"`, assistant with tool call → includes tool
  call, etc.).
- `test_summarization_prompt_contains_conversation` — verifies the full prompt
  includes the conversation inside the `<conversation>` fences.

### 2. Modify: `src/session.rs`

Update `active_messages()` (lines 287-321) to inject the compaction summary
as a system message:

```rust
Some(SessionEntry::Compaction {
    summary,
    first_kept_entry_id,
    ..
}) => {
    // Inject the compaction summary as a synthetic system message so
    // the LLM sees the prior context in compacted form.
    if !summary.is_empty() {
        messages.push(Message::System(SystemMessage {
            content: summary.clone(),
        }));
    }
    // Skip to the first kept entry after compaction.
    current_id = Some(first_kept_entry_id);
    continue;
}
```

**New import** (add to `use crate::core::types::...`):

```rust
SystemMessage,
```

**New test** in `mod tests` (around the existing `test_active_messages`):

- `test_active_messages_includes_compaction_summary` — append a user message,
  append a `Compaction` entry pointing to that user message with a non-empty
  summary, then call `active_messages()` and verify the result is a single
  `Message::System` containing the summary text.

### 3. Modify: `src/loop.rs`

Replace the stub compaction handling (lines 174-184) with real logic.

**Imports** — add to existing imports:

```rust
use crate::core::compaction::{generate_summary, CompactionConfig, should_compact};
use crate::core::types::{Message, SystemMessage};
use crate::session::SessionEntry;
use tokio::sync::watch;
```

(Adjust as needed for what's already imported.)

**New helper method** on `Agent` (or a free function in the same file):

```rust
/// Determine the compaction split point: keep the current turn
/// (the most recent user message and everything after it), summarize
/// everything before. Returns (messages_to_summarize, first_kept_entry_id).
fn find_compaction_split(messages: &[Message], entries: &[SessionEntry]) -> (Vec<Message>, String) {
    // Walk entries to find the last user message. Everything before it
    // (and any earlier user/assistant turns) goes to the summary.
    // The first_kept_entry_id is the entry ID of that last user message.
    //
    // Implementation:
    // 1. Walk the entries in their linear order (entries.iter()).
    // 2. Find the LAST user message in the active path.
    // 3. Collect all messages from index 0 up to (but not including) the
    //    last user message.
    // 4. Return those + the last user message's entry ID.
    //
    // Edge case: if there's only one user message (the very first), there's
    // nothing to summarize. Caller should check for empty result.
    unimplemented!() // filled in during implementation
}
```

**Replace the stub block** (lines 174-184) with:

```rust
// Identify messages to summarize vs. keep.
let (to_summarize, first_kept_id) =
    find_compaction_split(&messages, &session.entries);

if to_summarize.is_empty() {
    // Nothing useful to compact; skip.
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
                id: short_id(),  // need to expose or reimplement
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
    if self.compaction_config.on_overflow
        == crate::core::compaction::OnOverflow::Pause
    {
        stop_reason = StopReason::EndTurn;
        break;
    }
}
```

**Helper for short_id**: `session.rs` has a private `fn short_id()`. Either:
- Make `short_id` public (pub fn) and import it.
- Add a method on `Session` like `pub fn next_entry_id() -> String`.
- Inline the UUID generation in `loop.rs`.

The cleanest is to add `pub fn next_entry_id() -> String` to `Session`.

**New test** in `mod tests` (in `loop.rs`):

- `test_agent_compaction_triggers_and_persists` — uses `MockProvider` with
  a small `default_context_window` and `buffer_tokens` so overflow happens
  on the first turn. Verifies:
  - `CompactionStart` event is emitted.
  - `CompactionEnd` event has a non-empty summary.
  - A `Compaction` entry is appended to the session.
  - `session.active_messages()` returns the summary as a system message.

### 4. Modify: `src/session.rs` — expose entry-id generation

Add a small public helper on `Session`:

```rust
impl Session {
    /// Generate a new short entry ID. Used by callers that append
    /// entries directly via `append_entry`.
    pub fn next_entry_id() -> String {
        short_id()
    }
}
```

This is a one-line wrapper around the existing private `short_id()`.

## Verification

After implementation, run the full check sequence from `AGENTS.md`:

```
cargo fmt
cargo clippy -- -D warnings
cargo test
```

Smoke test the headless mode with a mock provider that has a tiny context
window (to force compaction):

```
cargo run -- -p "explain the codebase" -m mock --context-window 100
```

This will overflow immediately and exercise the real compaction path.

## File Change Summary

| File | Change |
|------|--------|
| `src/core/compaction.rs` | Replace stub `generate_summary` with real async impl + helpers + tests (~150 lines) |
| `src/session.rs` | Inject `Compaction.summary` as `Message::System` in `active_messages`; add `next_entry_id()` helper + test (~20 lines) |
| `src/loop.rs` | Replace stub compaction block with real flow + `find_compaction_split` + test (~80 lines) |
| `defaults/config.toml` | No changes needed (config schema already supports compaction) |

## Decisions Log

- **No new `Provider` method** — reuse `stream()` for summarization. The
  summarization call is a simple text-in / text-out, no tools needed.
- **Summary injected via `active_messages()`** rather than system prompt —
  the summary flows naturally as a system message in the conversation tree
  and is automatically persisted (via the `Compaction` entry) and replayed
  on session resume.
- **Keep last turn, summarize everything before** — preserves the immediate
  context the LLM is mid-producing while compacting the bulk of history.
- **`OnOverflow::Pause` breaks the loop** — gives the user a chance to
  decide whether to continue in a new session or resume with manual
  context trimming.
- **Failed compaction is non-fatal** — logs a warning and emits an `Error`
  event but the loop continues. The conversation may exceed the context
  window on the next turn, but that's preferable to killing the session.
- **Truncate per-entry in summarization prompt** (2000 chars) — prevents
  the prompt from being as large as the conversation, which would be
  pointless and expensive.
- **Skip thinking blocks in summarization** — they're internal reasoning
  that doesn't add value to the summary, and including them would inflate
  the prompt.
