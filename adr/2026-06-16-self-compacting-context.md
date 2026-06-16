# ADR-001: Self-Compacting Context — LLM-Powered Conversation Summarization

- **Status:** Accepted
- **Date:** 2026-06-16
- **Plan:** [plans/2026-06-15-self-compacting-context.md](../plans/2026-06-15-self-compacting-context.md)

## Context

Zeus-Code is a long-running coding agent. A single session can accumulate dozens
of user turns, tool calls, and large file contents, eventually exceeding the
LLM's context window. The earlier scaffolding for compaction already had:

- `CompactionConfig` (with `on_overflow: OnOverflow` and `buffer_tokens`)
- `should_compact()` for threshold detection
- `CompactionSummary` data structure
- `SessionEntry::Compaction` variant on the append-only JSONL log
- `AgentEvent::CompactionStart` and `CompactionEnd` events on the channel
- `tokio::sync::watch` cancellation plumbing

What was missing was the actual summarization call. The `generate_summary`
function was a stub that returned `Err("not yet implemented")`, and the agent
loop emitted a fake `CompactionEnd` with `"Compaction summary stub"`. The
scaffolding could detect overflow but could not resolve it.

## Decision

We implement real, LLM-powered self-compaction as a side effect of the agent
loop, with the following shape:

1. **Summarization reuses the existing `Provider::stream` interface.** The
   summarization call is a text-in / text-out request: serialize the
   conversation to a labeled textual form, wrap it in a summarization prompt,
   call `provider.stream(...)` with no tools, and collect the streamed text.
   No new `Provider` method is introduced. `MockProvider` already returns
   canned text and is reused as-is for tests.

2. **The summary is injected as a `Message::System` via `active_messages()`,
   not as a system prompt.** The summary flows through the same
   `messages: Vec<Message>` pipeline that the rest of the conversation uses,
   so it is automatically persisted (via the `Compaction` entry), replayed on
   session resume, and visible to the LLM in the natural turn order. There
   is no separate "synthetic context" channel to keep in sync.

3. **Split point: keep the last user turn, summarize everything before it.**
   `find_compaction_split(entries)` walks the active path (mirroring
   `Session::active_messages()` but also tracking entry IDs), finds the
   *last* `Message::User` in the path, and returns
   `(messages_before, last_user_id)`. The last user turn is preserved intact
   so the LLM has the in-flight conversation context; everything older is
   replaced by the summary. If there is no user message or only one, there
   is nothing to compact and the function returns an empty result.

4. **Per-entry truncation in the summarization prompt.** Each message is
   truncated to `MAX_ENTRY_CHARS = 2000` characters before being embedded
   in the prompt. Without this, the prompt would be roughly the same size
   as the conversation we're trying to shrink — pointless and expensive.
   Thinking blocks are skipped entirely from the prompt (they are internal
   reasoning, not user-visible context).

5. **Cancellation honours `watch::Receiver<bool>`.** The summarization
   stream is consumed under `tokio::select!` against `cancel.changed()`.
   If the cancellation fires mid-stream, the partial summary is discarded
   and `KonError::Cancelled` is returned. The cancellation pattern is
   identical to `TurnRunner::stream_and_consume`.

6. **`OnOverflow::Continue` is the default and keeps the loop alive.**
   After a successful compaction the next iteration of the agent loop runs
   with the compacted context. The `CompactionStart` / `CompactionEnd`
   events bracket the work so the UI can render progress.

7. **`OnOverflow::Pause` breaks the loop.** After the compaction entry is
   persisted, the agent sets `stop_reason = StopReason::EndTurn` and
   `break`s. The user can resume in a new session or with manual
   trimming.

8. **Failed compaction is non-fatal.** If `generate_summary` returns
   `Err(...)` (LLM outage, network error, etc.), the loop logs a warning,
   emits an `AgentEvent::Error`, and *continues* with the uncompacted
   context. The next turn may exceed the window — preferable to killing
   the session — and the threshold check will retry on the following turn.

9. **Cycle protection in `active_messages()`.** A pre-existing latent
   bug: the `Compaction` walk jumped to `first_kept_entry_id`, but if
   that entry's descendants included the `Compaction` itself (a common
   case once the `Compaction` is appended as a child of the user message
   that triggered it), the walk would loop forever. The fix is a
   `HashSet<String>` of visited entry IDs; revisiting an entry stops the
   walk cleanly. This was discovered and fixed during the implementation.

## Walk Layout After Compaction

```
active_messages() walk (with cycle protection):
  header
    └─ user-1
         └─ assistant-1
              └─ tool_result-1
                   └─ user-2     ← the "kept" turn
                        └─ assistant-2
                             └─ tool_result-2
                                  └─ Compaction
                                        └─ user-3 (post-compaction)
```

When the walk hits the `Compaction` entry it:

1. Emits `Message::System(summary)` into the result.
2. Jumps `current_id = first_kept_entry_id` (e.g. `user-2`).
3. Continues walking from there.

The `visited` set prevents the walk from re-entering `user-2` →
`assistant-2` → `tool_result-2` → `Compaction` → `user-2` after the jump.

## API Surface Added

| Symbol | Kind | Purpose |
|---|---|---|
| `generate_summary` | `async fn` in `core::compaction` | The real summarization call |
| `messages_to_summary_prompt_text` | private `fn` | Serialize + truncate messages for the prompt |
| `Session::next_entry_id` | `pub fn` | Public wrapper around the private `short_id()`, for callers that build `SessionEntry` values directly |
| `find_compaction_split` | private `fn` in `loop.rs` | Walk the active path; return `(to_summarize, first_kept_id)` |

The `generate_summary` signature:

```rust
pub async fn generate_summary(
    provider: &dyn Provider,
    messages_to_summarize: &[Message],
    first_kept_entry_id: String,
    cancel: watch::Receiver<bool>,
) -> KonResult<CompactionSummary>
```

The function is a free function (not a method on `Session` or `Agent`) so it
is independently testable with a `MockProvider` and a `watch::channel`.

## Consequences

### Positive

- **Real, end-to-end compaction.** A 100-token context window with the mock
  provider now triggers `CompactionStart` → `generate_summary` →
  `Compaction` entry on disk → `active_messages()` returning the summary
  as a system message, all in a single agent run. The e2e test
  `test_agent_compaction_triggers_and_persists` asserts each of these
  side effects.
- **No new provider surface.** The `Provider` trait is unchanged; any
  provider that implements `stream(...)` works for summarization.
- **Persistent and resumable.** The `Compaction` entry is written to the
  JSONL log, so the summary is replayed on session resume via
  `active_messages()`.
- **Bounded cost.** Per-entry 2000-char truncation and the no-tools call
  keep the summarization prompt small and the response short.
- **Cancellable.** User-initiated cancellation aborts the summary cleanly
  with no partial state written to the session.

### Negative / Trade-offs

- **Pre-compaction messages are still walked in `active_messages()`.** The
  summary is *added* on top of the existing pre-compaction subtree, so the
  LLM sees both the summary and the original messages. A future iteration
  could truncate the walk at the `Compaction` entry and restart from its
  children to fully exclude pre-compaction content. This is intentional
  for now: the cycle protection is correct, and the summary is the
  primary signal even if the originals remain visible.
- **Compaction adds latency.** The summarization call is a full LLM
  round-trip. For short sessions this is wasted work — but the threshold
  check only fires when the context is already too large, so the
  trade-off is acceptable.
- **Failed compaction is silent.** The loop continues without the user
  being able to tell from the event stream alone. The `AgentEvent::Error`
  is emitted so the UI can surface a warning, but no special indicator
  exists today.

## Verification

- `cargo fmt --check` — clean
- `cargo clippy --lib` — no new warnings
- `cargo test` — **243 tests pass** (239 lib + 4 integration)
- New unit tests cover:
  - `generate_summary` with `MockProvider` (returns a summary, token counts non-zero, `first_kept_entry_id` preserved)
  - `generate_summary` empty-input error path
  - `generate_summary` cancellation
  - `messages_to_summary_prompt_text` for user / assistant+tool-call / thinking-skip / long-entry-truncation
  - `find_compaction_split` basic, single-user, no-user edge cases
  - `Session::active_messages_includes_compaction_summary` (cycle-free injection of the system message)
  - `Session::next_entry_id` produces unique 8-char IDs
  - `test_agent_compaction_triggers_and_persists` (e2e: tiny context window → `CompactionStart` event → `CompactionEnd` with non-empty summary → `SessionEntry::Compaction` on disk → `active_messages()` contains the summary as a system message)
- Headless smoke test (`cargo run -- -p "..." -m mock`) runs the agent end-to-end against the mock provider; the real compaction path is exercised under a low `--context-window` override in the e2e test.
