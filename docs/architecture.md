# Zeus-Code: Architecture

Zeus-Code is a minimal coding agent harness — a Rust port of [Kon](https://github.com/0xku/kon). It provides a terminal-based interface for AI-assisted coding, with a focus on being small, understandable, and forkable.

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                          main.rs                                     │
│                    clap parse → dispatch                             │
└───────────┬──────────────────────────────────────┬──────────────────┘
            │                                      │
 ┌──────────▼──────────┐                 ┌─────────▼─────────┐
 │   headless.rs       │                 │  ui/launch.rs     │
 │   (-p mode)         │                 │  (TUI mode)       │
 │   stdin → agent     │                 │  ratatui +        │
 │   → stdout          │                 │  crossterm        │
 └──────────┬──────────┘                 └─────────┬─────────┘
            │                                      │
            │                          ┌───────────┼───────────┐
            │                          │           │           │
 ┌──────────▼──────────┐    ┌──────────▼──┐ ┌──────▼────┐ ┌───▼────────┐
 │   loop.rs           │    │ ui/app.rs   │ │ui/chat.rs │ │ui/widgets  │
 │   Agent::run()      │◄───│             │ │ChatLog    │ │InfoBar     │
 │   Turn loop         │    │ ┌─────────┐ │ │           │ │StatusLine  │
 └──┬───┬───┬──────────┘    │ │InputBox │ │ └───────────┘ │QueueDisplay│
    │   │   │               │ └─────────┘ │               └────────────┘
    │   │   │               │ ┌─────────┐ │
    │   │   │               │ │Commands │ │
    │   │   │               │ └─────────┘ │
    │   │   │               └─────────────┘
    │   │   │
    │   │   │  Communication channels:
    │   │   │    event_tx: mpsc::Sender<AgentEvent>  (agent → UI)
    │   │   │    cancel_rx: watch::Receiver<bool>    (UI → agent)
    │   │   │    approval_tx: oneshot::Sender         (UI → agent)
    │   │   │
┌───▼──────┐ │ ┌──────────▼──────┐
│turn.rs   │ │ │ session.rs      │
│Open      │ │ │ JSONL append    │
│stream    │ │ │ Tree navigation │
│Consume   │ │ │ Compaction      │
│Execute   │ │ └─────────────────┘
│tools     │ │
└───┬──────┘ │
    │        │
┌───▼────────▼──┐  ┌──────────────┐
│ llm/           │  │ core/         │
│ base.rs        │  │ types.rs      │
│ models.rs      │  │ compaction.rs │
│ providers/     │  │ handoff.rs    │
│   openai_*.rs  │  │ errors.rs     │
│   anthropic.rs │  └──────────────┘
│   copilot.rs   │
│   azure_*.rs   │  ┌──────────────┐
│   mock.rs      │  │ context/      │
└────────────────┘  │ agent_mds.rs  │
                    │ skills.rs     │
┌────────────────┐  │ git.rs        │
│ tools/          │  │ loader.rs     │
│ base.rs         │  └──────────────┘
│ read/edit/bash  │
│ grep/find/write │  ┌──────────────┐
│ web_*.rs        │  │ config.rs     │
└────────────────┘  │ themes.rs     │
                    │ permissions.rs│
                    │ notify.rs     │
                    └──────────────┘
```

## Event-Driven Design

The entire system is built around an event-driven architecture. The agent loop and the UI communicate via Tokio channels:

### Channels

| Channel | Type | Direction | Purpose |
|---|---|---|---|
| `event_tx` | `mpsc::Sender<AgentEvent>` | Agent → UI | Streams turn events (thinking, text, tool calls, results) |
| `event_rx` | `mpsc::Receiver<AgentEvent>` | Agent → UI | Receives events in the UI event loop |
| `cancel_tx` | `watch::Sender<bool>` | UI → Agent | Signals the agent to stop (user pressed Escape) |
| `cancel_rx` | `watch::Receiver<bool>` | UI → Agent | Agent checks this before/after each operation |
| `approval_tx` | `oneshot::Sender<ToolApproval>` | UI → Agent | One-shot response for tool approval prompts |
| `approval_rx` | `oneshot::Receiver<ToolApproval>` | UI → Agent | Agent awaits this before executing mutating tools |

### Agent Events

```rust
pub enum AgentEvent {
    Start,                                        // Agent begins processing
    TurnStart { turn: u64 },                     // New turn starts
    ThinkingStart { level: Option<String> },     // LLM begins thinking
    ThinkingDelta { text: String },              // Streaming thinking content
    ThinkingEnd,                                  // Thinking block complete
    TextStart,                                    // LLM begins text response
    TextDelta { text: String },                  // Streaming text content
    TextEnd,                                      // Text block complete
    ToolStart { id: String, name: String },      // LLM requests a tool
    ToolArgsDelta { id: String, delta: String }, // Streaming tool arguments
    ToolEnd { id: String, arguments: String },   // Tool arguments complete
    ToolApproval { ... },                        // Waiting for user approval
    ToolResult { id: String, result: ToolResult }, // Tool execution result
    TurnEnd { ... },                             // Turn complete
    CompactionStart,                              // Compaction begins
    CompactionEnd { summary: String, ... },      // Compaction complete
    End { ... },                                  // Agent finished
    Error { error: String },                     // Error occurred
}
```

## Turn Lifecycle

A single turn (one LLM request → response cycle) follows this sequence:

```
User submits input
       │
       ▼
┌─────────────────┐
│ Phase 1:         │
│ open_stream()    │  POST /v1/chat/completions with SSE
│                  │  Retry on transient errors (429, 5xx)
│                  │  Exponential backoff
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Phase 2:         │
│ consume_stream() │  Read SSE chunks in a loop
│                  │  Race against cancel signal
│                  │  Buffer thinking → emit ThinkingDelta
│                  │  Buffer text → emit TextDelta
│                  │  Collect tool calls → emit ToolStart/ArgsDelta
│                  │  On StreamDone → capture usage + stop_reason
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Phase 3:         │
│ run_pending_     │  For each tool call collected:
│ tools()          │   1. Find tool in registry
│                  │   2. Parse + validate arguments
│                  │   3. Permission check
│                  │   4. If approval needed → wait for user
│                  │   5. Execute tool with cancel signal
│                  │   6. Emit ToolResult event
│                  │  If cancelled → skip remaining tools
└────────┬────────┘
         │
         ▼
   Append messages to session
   Check compaction
   If stop_reason == ToolUse → next turn
   Else → agent done
```

## Module Responsibilities

### `core/` — Foundation Types

| File | Responsibility |
|---|---|
| `types.rs` | All data types: Message, ContentBlock, StreamPart, ToolDefinition, ToolResult, Usage, StopReason, AgentEvent |
| `errors.rs` | Error enum using `thiserror`; covers all error categories |
| `compaction.rs` | Context overflow detection; LLM-based conversation summarization |
| `handoff.rs` | Handoff prompt generation for focused child sessions |

### `config.rs` — Configuration

- Loads/creates `~/.config/zeus-code/config.toml`
- Deep-merges user config over embedded defaults
- Schema migrations for backward compatibility
- Runtime overrides from CLI flags
- Atomic writes for config setters
- Exposed as `Arc<RwLock<Config>>`

### `llm/` — LLM Providers

| File | Responsibility |
|---|---|
| `base.rs` | `Provider` trait, `LLMStream` wrapper, `ThinkingLevel` enum |
| `models.rs` | Static model catalog (capabilities, context windows, thinking support) |
| `providers/mod.rs` | Provider registry (name → implementation factory) |
| `providers/openai_completions.rs` | OpenAI Chat Completions API (main provider) |
| `providers/openai_responses.rs` | OpenAI Responses API |
| `providers/openai_codex_responses.rs` | ChatGPT backend API (OAuth) |
| `providers/anthropic.rs` | Anthropic Messages API |
| `providers/copilot.rs` | GitHub Copilot Chat Completions |
| `providers/azure_ai_foundry.rs` | Azure AI Foundry (Anthropic via Azure) |
| `providers/mock.rs` | Mock provider for testing |

### `tools/` — Tool System

| File | Responsibility |
|---|---|
| `base.rs` | `Tool` trait (name, description, schema, execute, format, etc.) |
| `mod.rs` | Tool registry, default tools, extra tools, lookup |
| `read.rs` | File reading with line numbers, image support, directory listing |
| `edit.rs` | Surgical text replacement with diff generation |
| `write.rs` | File creation and overwriting |
| `bash.rs` | Shell command execution with timeout and cancellation |
| `grep.rs` | Regex content search via ripgrep |
| `find.rs` | Glob-based file discovery via fd |
| `web_search.rs` | DuckDuckGo web search |
| `web_fetch.rs` | URL content fetching and extraction |
| `utils.rs` | Shared helpers (path shortening, output truncation, etc.) |

### `session.rs` — Persistence

- Append-only JSONL files in `~/.config/zeus-code/sessions/`
- Tree structure via parent_id + LeafEntry pointers
- Session creation, loading, resuming
- Compaction entries
- Session discovery (most recent, by prefix)

### `loop.rs` + `turn.rs` — Agent Loop

- `Agent::run()` orchestrates the full multi-turn conversation
- `TurnRunner::run()` handles a single LLM request/response cycle
- System prompt building (base + AGENTS.md + skills + git + tools)
- Retry logic with exponential backoff
- Cancellation at every phase
- Self-compaction when approaching context limits

### `ui/` — Terminal UI

| File | Responsibility |
|---|---|
| `launch.rs` | TUI entrypoint: terminal setup, app creation, event loop |
| `app.rs` | Main `App` struct; widget composition; key bindings; input routing |
| `chat.rs` | Chat log widget: scrolling message display with streaming updates |
| `input.rs` | Input box with autocomplete, history, paste detection, skill triggers |
| `blocks.rs` | Widgets for each message type: UserBlock, ThinkingBlock, ContentBlock, ToolBlock |
| `widgets.rs` | Status widgets: InfoBar, StatusLine, QueueDisplay, FileChangesModal |
| `commands.rs` | Slash command handler: /new, /resume, /model, /compact, /export, etc. |
| `autocomplete.rs` | Autocomplete providers: commands, files, paths, PRs, skills |
| `formatting.rs` | Markdown → ratatui Text conversion |
| `styles.rs` | Theme colors → ratatui Style objects |
| `export.rs` | HTML export of session transcripts |
| `welcome.rs` | Welcome screen with shortcuts |

### `context/` — Project Context

| File | Responsibility |
|---|---|
| `agent_mds.rs` | Discovery and loading of AGENTS.md/CLAUDE.md files |
| `skills.rs` | Skills discovery, parsing, validation, rendering |
| `git.rs` | Git context formatting (branch, status, commits) |
| `loader.rs` | Unified context loader (combines agent_mds + skills + git) |

### `permissions.rs` — Security

- Two modes: Prompt (ask before mutating) and Auto (skip prompts)
- Safe command whitelist for auto-approval in Prompt mode
- Git subcommand whitelist (status, log, diff, branch, etc.)
- Web tools treated as read-only

### `themes.rs` — Visual Themes

- 24 built-in color themes
- Each defines: background, foreground, accent, dim, success, warning, error, surface, etc.
- Thinking level background shading per level
- Switchable at runtime via `/themes`

### `notify.rs` — Audio Feedback

- Platform detection (macOS, Linux, Windows)
- Plays completion, error, and permission-prompt sounds
- Embedded WAV files
- Configurable volume and enabled/disabled

## Key Design Decisions

1. **Channels over callbacks:** All agent ↔ UI communication uses Tokio channels (mpsc, watch, oneshot). No callback hell.

2. **Cancel everywhere:** Every long-running operation (HTTP requests, subprocesses, stream reading) races against a `watch::Receiver<bool>`. User can interrupt at any time.

3. **Provider abstraction:** All LLM APIs are behind a single `Provider` trait. Adding a new provider means implementing `stream()`, `should_retry_for_error()`, and `thinking_levels()`.

4. **Tool trait:** All tools implement the `Tool` trait. The tool registry makes them discoverable by name. The LLM receives JSON Schema definitions automatically.

5. **Append-only session storage:** Sessions are JSONL files that only grow. Never modified in place. Tree structure allows branching. Easy to inspect, archive, and move.

6. **Self-compacting context:** When the conversation approaches the context window, Zeus asks the LLM to summarize earlier messages. The summary replaces the original messages in the active context.

7. **Externalized project context:** Project instructions live in AGENTS.md/CLAUDE.md files, not in the system prompt. Skills are reusable instruction packs. This keeps the core system prompt tiny (~270 tokens).

8. **Minimal by default, configurable when needed:** 6 core tools, 2 extra tools. Everything can be extended through skills and configuration.
