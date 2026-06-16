# ADR-002: `/compact` — User-Initiated Compaction via Slash Command

- **Status:** Accepted
- **Date:** 2026-06-16
- **Depends on:** [ADR-001: Self-Compacting Context](2026-06-16-self-compacting-context.md)

## Context

ADR-001 delivered a fully working, LLM-powered self-compacting context system:
`generate_summary()` calls the LLM, the agent loop detects overflow via
`should_compact()`, splits the active path at the last user message,
summarises earlier messages, and persists the result as a
`SessionEntry::Compaction`. Four unit tests and one end-to-end test verified
every side effect.

However, the user had no way to trigger compaction on demand. The `Command::Compact`
variant was parsed in `src/ui/commands.rs` but fell through to the catch-all arm
in `handle_submit`, showing `"Command not yet implemented"`. The agent loop's
compaction path was tightly coupled to the `Agent::run` method — it ran inline
during the loop iteration, shared session ownership with the turn runner, and was
guarded by a token-threshold check. This made it impossible to call from outside
the loop without duplicating ~60 lines of compaction logic.

## Decision

We implement the `/compact` slash command by extracting the compaction logic
into a reusable public method and wiring it into the TUI event pipeline.

1. **Extract `Agent::compact_now` as a reusable public method.** The 60-line
   compaction block inside `Agent::run` (split-identify, summarise, persist,
   emit events) is moved into `pub async fn compact_now(&self, session, event_tx,
   cancel_rx) -> KonResult<Option<CompactionSummary>>`. The `run` method now
   calls `self.compact_now(...)` for the automatic path, reducing the in-loop
   compaction site to 15 lines. Both the automatic and manual paths share
   identical logic for event emission, persistence, and cancellation.

2. **Skip the `should_compact` threshold check on manual trigger.** The
   automatic path only fires when `should_compact()` returns `true` (token
   count > `context_window - buffer_tokens`). The `/compact` command skips
   this guard — the user explicitly asked for compaction and it should proceed
   regardless of current token count. The `compact_now` method has no
   threshold parameter.

3. **Return `Ok(None)` when there is nothing to compact.** `find_compaction_split`
   returns an empty vector when the active path has fewer than two user messages.
   In the automatic path this was handled with a `tracing::debug!` and a no-op
   continuation. In the manual path `compact_now` returns `Ok(None)` so the TUI
   can render an appropriate message ("Nothing to compact — need at least 2 user
   turns").

4. **New `AgentEvent::CompactionResult` for TUI feedback.** The existing
   `CompactionStart` and `CompactionEnd` events flow through the same channel
   as every other agent event but are currently not rendered by `process_events`
   (they fall through to `_ => {}`). Rather than requiring the TUI to correlate
   start/end pairs, a single `CompactionResult` event carries the complete
   outcome — success/no-op/failure flags, token counts, summary length, and
   optional error string — so the TUI needs only one match arm to render all
   three states.

5. **Guard against concurrent agent execution.** `/compact` is rejected while
   `self.agent_running` is `true`, with an error message shown in the chat.
   Running compaction while the agent loop is mid-turn would race on session
   state (the agent loop appends assistant + tool result entries during its
   compaction window). This guard mirrors the existing `!!command` guard in
   `handle_submit`.

6. **Session ownership follows the `run_agent` pattern.** The session is held
   in an `Arc<parking_lot::Mutex<Option<Session>>>` on the `App` struct. When
   the user types `/compact`, `handle_compact_command` takes the session out
   of the mutex, spawns a tokio task that calls `agent.compact_now(...)`, and
   on completion (success or failure) puts the session back. If no session
   exists yet, the command is a no-op with a status message. This is identical
   to how `run_agent` manages session ownership.

7. **Error propagation differs between the two callers.** The automatic path
   inside `Agent::run` catches errors from `compact_now` and continues the
   loop (per ADR-001's "failed compaction is non-fatal" decision). The manual
   path surfaces the error to the user via the `CompactionResult` event,
   rendered as `"✗ Compaction failed: {error}"`. Both paths receive the
   `AgentEvent::Error` emitted by `compact_now` itself, but only the UI
   handler currently renders it (the loop logs a warning and continues).

8. **Reuse of existing event types.** `compact_now` emits `CompactionStart`
   and `CompactionEnd` for both the automatic and manual paths. These event
   types exist on the channel and are available for any future UI work (e.g.
   a progress bar, a status-line indicator). Today they are silently dropped
   by `process_events` (`_ => {}`), but they remain in the stream for
   forward-compatibility.

## API Surface Added

| Symbol | Kind | Purpose |
|---|---|---|
| `Agent::compact_now` | `pub async fn` on `Agent` | Extracted compaction logic shared by auto + manual paths |
| `AgentEvent::CompactionResult` | enum variant | Carries success/no-op/error outcome from spawned compaction task to TUI |
| `App::handle_compact_command` | `fn` on `App` | Orchestrates the `/compact` slash command: guards, task spawn, session management |

The `compact_now` signature:

```rust
pub async fn compact_now(
    &self,
    session: &mut Session,
    event_tx: mpsc::Sender<AgentEvent>,
    cancel_rx: watch::Receiver<bool>,
) -> KonResult<Option<CompactionSummary>>
```

The `CompactionResult` event:

```rust
AgentEvent::CompactionResult {
    success: bool,          // true for success or no-op
    no_op: bool,            // true when < 2 user turns
    summary_length: usize,  // chars; 0 on no-op/error
    tokens_before: u64,     // tokens summarised; 0 on no-op/error
    error: Option<String>,  // human-readable; None on success
}
```

## Data Flow

```
User types /compact
  │
  └─ parse_command("/compact") → Command::Compact
      │
      └─ handle_compact_command()
          ├─ if agent_running → error, return
          ├─ if no session → "Nothing to compact", return
          ├─ show "⏳ Compacting…" placeholder
          └─ tokio::spawn:
              ├─ agent.compact_now(session, event_tx, cancel_rx)
              │   ├─ find_compaction_split() → (msgs_to_summarise, first_kept_id)
              │   ├─ if msgs empty → return Ok(None)
              │   ├─ emit CompactionStart
              │   ├─ generate_summary(provider, msgs, id, cancel)
              │   │   └─ LLM call via Provider::stream (no tools)
              │   ├─ append SessionEntry::Compaction to JSONL
              │   ├─ emit CompactionEnd
              │   └─ return Ok(Some(summary))
              ├─ *session_arc.lock() = Some(session)
              └─ emit CompactionResult { success, no_op, ... }
                  │
                  └─ process_events()
                      ├─ remove "Compacting…" placeholder
                      ├─ no_op  → "ℹ Nothing to compact"
                      ├─ success → "✓ Compacted (N tokens → N chars)"
                      └─ error  → "✗ Compaction failed: {msg}"
```

## Consequences

### Positive

- **User control over context size.** Long-running sessions where the LLM starts
  losing context can be compacted on demand without waiting for the automatic
  threshold. This is a standard feature in agentic CLIs (Claude Code's
  `/compact`, Aider's `/drop`).
- **Code deduplication.** The 60-line compaction block now lives in exactly one
  place. The automatic and manual paths are semantically identical, differing
  only in how errors are surfaced (loop-continue vs. TUI-render).
- **Testable in isolation.** `compact_now` can be called directly in unit tests
  with a `MockProvider` and a pre-populated `Session`, without standing up the
  full agent loop or TUI. Four new tests cover success, empty session,
  single-user no-op, and cancellation.
- **No new provider surface.** Same as ADR-001 — `Provider::stream` is the
  only API needed.
- **Non-disruptive.** The automatic compaction path is unchanged in behaviour
  (all 5 assertions in `test_agent_compaction_triggers_and_persists` still
  pass). The refactor is a pure extraction.

### Negative / Trade-offs

- **CompactionStart/End events are still dropped by the TUI.** The manual
  `/compact` path renders its result via the new `CompactionResult` event,
  but the intermediate `CompactionStart`/`CompactionEnd` events pass through
  `process_events` unhandled. A future PR could add a status-line progress
  indicator for these events, but today they are invisible to the user.
- **Blocking the LLM call in a spawned task.** The `handle_compact_command`
  method captures `cancel_rx` from `self.cancel_rx.clone()`. If the user
  presses Escape, `cancel_tx.send(true)` fires, but the cancellation
  happens inside the spawned task, not the UI thread. The TUI remains
  responsive but the user sees no cancellation UX for compaction
  specifically (the "Compacting…" placeholder remains until the task
  returns, which may be instant on cancellation).
- **No progress feedback during summarisation.** The LLM streams its
  response, but `compact_now` collects the full text before emitting
  `CompactionEnd`. The user sees "Compacting…" throughout the entire
  LLM round-trip with no incremental updates. This is acceptable for
  the short summaries (a few hundred characters) that `MAX_ENTRY_CHARS`
  truncation produces.

## Verification

- `cargo fmt` — clean
- `cargo clippy --all-targets -- -D warnings` — clean
- `cargo test` — **247 tests pass** (243 lib + 4 integration)
- New unit tests for the manual path:
  - `test_compact_now_succeeds_on_two_user_session` — two user turns, mock provider,
    asserts `CompactionStart` + `CompactionEnd` events emitted, `SessionEntry::Compaction`
    persisted, summary present in `active_messages()`
  - `test_compact_now_empty_session_returns_none` — no user messages, asserts
    `Ok(None)` and zero events on the channel
  - `test_compact_now_single_user_session_returns_none` — one user message,
    asserts `Ok(None)`
  - `test_compact_now_cancellation_propagates` — pre-cancelled channel, asserts
    `Err(Cancelled)` and no `Compaction` entry persisted
- Existing compaction test still passes:
  - `test_agent_compaction_triggers_and_persists` — the automatic path continues
    to work identically after the refactor
- Also fixed 3 pre-existing clippy errors (unrelated to this ADR) to keep the
  codebase warning-clean
