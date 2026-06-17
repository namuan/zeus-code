# Zeus-Code: Implementation Plan

Zeus-Code is a Rust port of [Kon](https://github.com/0xku/kon), a minimal coding agent harness. The original is written in Python (~7000+ lines). This document outlines every step required to rebuild it in Rust, phase by phase.

---

## Table of Contents

1. [Phase 1: Project Scaffolding](#phase-1-project-scaffolding)
2. [Phase 2: Core Types and Data Structures](#phase-2-core-types-and-data-structures)
3. [Phase 3: Configuration System](#phase-3-configuration-system)
4. [Phase 4: LLM Provider Abstraction](#phase-4-llm-provider-abstraction)
5. [Phase 5: Tool System](#phase-5-tool-system)
6. [Phase 6: Session Persistence](#phase-6-session-persistence)
7. [Phase 7: Agent Loop](#phase-7-agent-loop)
8. [Phase 8: CLI Entry Point](#phase-8-cli-entry-point)
9. [Phase 9: TUI](#phase-9-tui)
10. [Phase 10: Polish](#phase-10-polish)
11. [Phase 11: Testing and Documentation](#phase-11-testing-and-documentation)
12. [Architecture Overview](#architecture-overview)
13. [Python → Rust Mapping](#python--rust-mapping)

---

## Phase 1: Project Scaffolding

### Cargo Setup

```bash
cargo init zeus-code --name zeus
```

### Directory Structure

```
zeus-code/
├── Cargo.toml
├── build.rs                      # Embed defaults/config.toml
├── src/
│   ├── main.rs                   # Entry point
│   ├── lib.rs                    # Re-exports
│   ├── cli.rs                    # clap argument parsing + dispatch
│   ├── config.rs                 # TOML config loading, validation, migration
│   ├── loop.rs                   # Agent loop / turn orchestration
│   ├── turn.rs                   # Single turn execution
│   ├── headless.rs               # Non-interactive -p mode
│   ├── session.rs                # Append-only JSONL persistence + tree
│   ├── permissions.rs            # Tool permission decisions
│   ├── notify.rs                 # Audio notifications
│   ├── diff_display.rs           # Diff rendering helpers
│   ├── themes.rs                 # Color theme definitions
│   ├── context/
│   │   ├── mod.rs
│   │   ├── agent_mds.rs          # AGENTS.md/CLAUDE.md discovery + loading
│   │   ├── git.rs                # Git context formatting
│   │   ├── skills.rs             # Skills discovery, parsing, rendering
│   │   └── loader.rs             # Context dataclass: loads agent_mds + skills
│   ├── core/
│   │   ├── mod.rs
│   │   ├── types.rs              # Message, StreamPart, ToolDefinition, ToolResult
│   │   ├── compaction.rs         # Context overflow + summary generation
│   │   ├── handoff.rs            # Handoff prompt generation
│   │   └── errors.rs             # Error types
│   ├── llm/
│   │   ├── mod.rs                # Provider registry
│   │   ├── base.rs               # Provider trait, LLMStream
│   │   ├── models.rs             # Model catalog
│   │   └── providers/
│   │       ├── mod.rs
│   │       ├── openai_completions.rs
│   │       ├── openai_responses.rs
│   │       ├── openai_codex_responses.rs
│   │       ├── anthropic.rs
│   │       ├── copilot.rs
│   │       ├── azure_ai_foundry.rs
│   │       └── mock.rs
│   ├── tools/
│   │   ├── mod.rs                # Registry + definitions
│   │   ├── base.rs               # Tool trait
│   │   ├── read.rs
│   │   ├── edit.rs
│   │   ├── write.rs
│   │   ├── bash.rs
│   │   ├── grep.rs
│   │   ├── find.rs
│   │   ├── web_search.rs
│   │   ├── web_fetch.rs
│   │   └── utils.rs
│   └── ui/
│       ├── mod.rs
│       ├── app.rs                # Main App struct + event loop
│       ├── chat.rs               # Chat log widget
│       ├── input.rs              # Input box widget
│       ├── blocks.rs             # Message block widgets
│       ├── widgets.rs            # Status bar, queue display, etc.
│       ├── commands.rs           # Slash command handling
│       ├── autocomplete.rs       # File/path/command completion
│       ├── formatting.rs         # Markdown rendering
│       ├── styles.rs             # Dynamic theme → ratatui styles
│       ├── export.rs             # HTML export
│       ├── launch.rs             # TUI entrypoint
│       └── welcome.rs            # Welcome screen
├── defaults/
│   └── config.toml               # Embedded default config
├── tests/
│   ├── config_tests.rs
│   ├── session_tests.rs
│   ├── tool_tests.rs
│   ├── agent_tests.rs
│   └── integration_tests.rs
├── docs/
│   ├── implementation-plan.md    # This file
│   ├── architecture.md
│   └── rust-crate-mapping.md
├── .gitignore
├── README.md
└── AGENTS.md
```

### Key Dependencies (Cargo.toml)

```toml
[package]
name = "zeus-code"
version = "0.1.0"
edition = "2024"
description = "Minimal coding agent harness — Rust port of Kon"
license = "MIT"

[dependencies]
# CLI
clap = { version = "4", features = ["derive"] }

# TUI
ratatui = "0.29"
crossterm = "0.28"

# Async runtime
tokio = { version = "1", features = ["full"] }
tokio-stream = "0.1"
tokio-util = "0.7"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# HTTP
reqwest = { version = "0.12", features = ["stream", "json", "rustls-tls"] }

# Utilities
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
regex = "1"
dirs = "6"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
syntect = "5"
comrak = "0.32"
base64 = "0.22"
which = "7"
similar = "2"
image = "0.25"
strum = { version = "0.26", features = ["derive"] }
futures = "0.3"
async-trait = "0.1"
anyhow = "1"
parking_lot = "0.12"
once_cell = "1"
dashmap = "6"

[dev-dependencies]
tempfile = "3"
pretty_assertions = "1"
tokio-test = "0.4"
```

---

## Phase 2: Core Types and Data Structures

### File: `src/core/types.rs`

Define all data structures that flow through the system — these are the "nouns" of the application.

```rust
/// A chat message role
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "user")]
    User(UserMessage),
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultMessage),
    #[serde(rename = "system")]
    System(SystemMessage),
}
```

**Message variants:**
- `UserMessage` — `content: Vec<ContentBlock>`, `skill_name: Option<String>`
- `AssistantMessage` — `content: Vec<ContentBlock>`, `usage: Option<Usage>`, `stop_reason: Option<StopReason>`
- `ToolResultMessage` — `tool_call_id: String`, `tool_name: String`, `content: String`, `images: Vec<ImageContent>`, `file_changes: Option<FileChanges>`
- `SystemMessage` — `content: String`

**Content blocks:**
```rust
pub enum ContentBlock {
    Text { text: String },
    Thinking {
        thinking: String,
        signature: Option<String>,
        level: Option<String>,
    },
    Image(ImageContent),
    ToolCall {
        id: String,
        name: String,
        arguments: String, // JSON string
    },
}
```

**Streaming parts (from LLM providers):**
```rust
pub enum StreamPart {
    TextDelta { text: String },
    ThinkingDelta { thinking: String, level: Option<String> },
    ThinkingSignature { signature: String },
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, arguments_delta: String },
    StreamDone { stop_reason: StopReason, usage: Usage },
    StreamError { error: String, retryable: bool },
}
```

**Tool definition (sent to LLM):**
```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON Schema
}
```

**Tool result:**
```rust
pub struct ToolResult {
    pub success: bool,
    pub result: Option<String>,            // Raw text sent to LLM
    pub images: Vec<ImageContent>,
    pub ui_summary: Option<String>,        // One-line result
    pub ui_details: Option<String>,        // Collapsed multiline
    pub ui_details_full: Option<String>,   // Expanded multiline
    pub file_changes: Option<FileChanges>,
}
```

**Usage:**
```rust
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}
```

**Stop reason:**
```rust
pub enum StopReason {
    Stop,
    ToolUse,
    MaxTurns,
    EndTurn,
    Refusal,
    Error,
}
```

**Agent events (emitted to UI):**
```rust
pub enum AgentEvent {
    Start,
    TurnStart { turn: u64 },
    ThinkingStart { level: Option<String> },
    ThinkingDelta { text: String },
    ThinkingEnd,
    TextStart,
    TextDelta { text: String },
    TextEnd,
    ToolStart { id: String, name: String },
    ToolArgsDelta { id: String, delta: String },
    ToolEnd { id: String, arguments: String },
    ToolApproval { tool_name: String, call_text: String, preview: Option<String>, response_tx: oneshot::Sender<ToolApproval> },
    ToolResult { id: String, result: ToolResult },
    TurnEnd { stop_reason: StopReason, assistant: AssistantMessage, tool_results: Vec<ToolResult> },
    CompactionStart,
    CompactionEnd { summary: String, tokens_before: u64 },
    End { stop_reason: StopReason, total_turns: u64, usage: Usage },
    Error { error: String },
}
```

### File: `src/core/errors.rs`

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum KonError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Provider error: {0}")]
    Provider(String),
    #[error("Provider error (retryable): {0}")]
    ProviderRetryable(String),
    #[error("Tool error ({tool}): {message}")]
    Tool { tool: String, message: String },
    #[error("Session error: {0}")]
    Session(String),
    #[error("Operation cancelled")]
    Cancelled,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("{0}")]
    Other(String),
}
```

---

## Phase 3: Configuration System

### File: `src/config.rs`

Port the TOML-based config system. The original stores config at `~/.config/kon/config.toml` — we use `~/.config/zeus-code/config.toml`.

### Config Structure

```toml
[meta]
config_version = 6

[llm]
default_provider = "openai-codex"
default_model = "gpt-5.5"
default_base_url = ""
default_thinking_level = "low"
tool_call_idle_timeout_seconds = 180
request_timeout_seconds = 600

[llm.auth]
openai_compat = "auto"         # "auto" | "required" | "none"
anthropic_compat = "auto"

[llm.tls]
insecure_skip_verify = false

[llm.system_prompt]
git_context = true
content = """You are an expert coding assistant called Zeus..."""

[compaction]
on_overflow = "continue"       # "continue" | "pause"
buffer_tokens = 20000

[agent]
max_turns = 500
default_context_window = 200000

[tools]
extra = ["web_search", "web_fetch"]

[ui]
theme = "gruvbox-dark"
collapse_thinking = true
thinking_lines = "1"
colored_tool_badge = true
show_welcome_shortcuts = true
hidden_models = []

[permissions]
mode = "prompt"                # "prompt" | "auto"

[notifications]
enabled = true
volume = 0.5
```

### Implementation Steps

1. **Embed default config:** Use `include_str!("../defaults/config.toml")` to bundle the default
2. **Define serde structs:** One struct per TOML section, derive `Deserialize`/`Serialize`
3. **Config loading:**
   ```
   get_config() → load from disk OR create from default → validate → cache
   ```
4. **Deep merge:** Merge user config over defaults (field-by-field, not whole-section replace)
5. **Schema migration:** Versioned migration functions (`migrate_v0_to_v1` … `migrate_v5_to_v6`) that transform old config formats
6. **Runtime overrides:** CLI flags (`--model`, `--provider`, `--api-key`, `--base-url`) layer on top of config
7. **Atomic writes:** Config setters (theme, permissions, etc.) write back to TOML atomically
8. **Runtime access:** Store as `Arc<RwLock<Config>>` in a lazy static

### Key Components

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub meta: MetaConfig,
    pub llm: LLMConfig,
    pub compaction: CompactionConfig,
    pub agent: AgentConfig,
    pub tools: ToolsConfig,
    pub ui: UIConfig,
    pub permissions: PermissionsConfig,
    pub notifications: NotificationsConfig,
}

impl Config {
    pub fn load() -> KonResult<Arc<RwLock<Config>>> { /* ... */ }
    pub fn load_or_create() -> KonResult<Arc<RwLock<Config>>> { /* ... */ }
    pub fn save(&self) -> KonResult<()> { /* ... */ }
    pub fn merge_cli_overrides(&mut self, cli: &Cli) { /* ... */ }
}
```

---

## Phase 4: LLM Provider Abstraction

### File: `src/llm/base.rs`

The provider abstraction is the most critical interface — every LLM call goes through it.

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stream a chat completion request, yielding typed stream parts
    async fn stream(
        &self,
        messages: Vec<Message>,
        system_prompt: Option<String>,
        tools: Vec<ToolDefinition>,
    ) -> KonResult<LLMStream>;

    /// Whether to retry the request on this error
    fn should_retry_for_error(&self, error: &KonError) -> bool;

    /// Valid thinking levels for this provider
    fn thinking_levels(&self) -> &[ThinkingLevel];

    /// Set the thinking level
    fn set_thinking_level(&mut self, level: ThinkingLevel);

    /// Provider name string
    fn name(&self) -> &str;

    /// Model ID
    fn model(&self) -> &str;
}

/// Wraps a stream of parts, captures final usage + id after completion
pub struct LLMStream {
    inner: Pin<Box<dyn tokio_stream::Stream<Item = KonResult<StreamPart>> + Send>>,
    pub usage: Option<Usage>,
    pub stream_id: Option<String>,
    finished: bool,
}

impl LLMStream {
    pub async fn next_part(&mut self) -> KonResult<Option<StreamPart>> { /* ... */ }
}

/// Thinking levels supported across providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinkingLevel {
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}
```

### File: `src/llm/models.rs`

Static model catalog mapping provider names and model IDs to capability info:

```rust
pub struct Model {
    pub id: String,
    pub provider: String,
    pub display_name: String,
    pub context_window: u64,
    pub supports_vision: bool,
    pub supports_thinking: bool,
    pub thinking_levels: Vec<ThinkingLevel>,
    pub max_output_tokens: u64,
}

pub fn find_model(provider: &str, model_id: &str) -> Option<Model> { /* ... */ }
pub fn models_for_provider(provider: &str) -> Vec<&Model> { /* ... */ }
pub fn all_models() -> Vec<&Model> { /* ... */ }
```

### File: `src/llm/providers/mod.rs`

Provider registry that maps provider names to implementations:

```rust
pub enum ProviderType {
    OpenAIChatCompletions,
    OpenAIResponses,
    OpenAICodexResponses,
    AnthropicMessages,
    AnthropicCopilot,
    GitHubCopilot,
    AzureAIFoundry,
    Mock,
}

pub fn create_provider(config: &ProviderConfig, provider_type: ProviderType) -> KonResult<Box<dyn Provider>> {
    match provider_type {
        ProviderType::OpenAIChatCompletions => Ok(Box::new(OpenAICompletionsProvider::new(config)?)),
        // ... etc
    }
}
```

### Provider Implementations

Each provider implementation must:

1. Accept a `ProviderConfig` (api_key, base_url, model, max_tokens, thinking_level, etc.)
2. Convert canonical `Message` types to provider-specific request format
3. Send HTTP request with streaming enabled (SSE or JSON-lines)
4. Parse the response stream into `StreamPart` events
5. Handle thinking content (reasoning_content, thinking blocks, etc.)
6. Handle tool calls (function calling, tool_use blocks, etc.)
7. Handle errors with retry logic (429 rate limits, 5xx server errors, connection resets)
8. Capture final usage stats after stream completion

| Provider | API Endpoint | Auth Method | Key Complexity |
|---|---|---|---|
| `OpenAICompletions` | `/v1/chat/completions` | API key (Bearer) | SSE parsing; reasoning_content extraction; per-provider compat (ZhiPu, DeepSeek) |
| `OpenAIResponses` | `/v1/responses` | API key | Different request/response format than completions |
| `OpenAICodexResponses` | `chatgpt.com/backend-api` | OAuth token | Manual HTTP client; OAuth token management |
| `AnthropicMessages` | `/v1/messages` | API key (`x-api-key`) | Thinking blocks; tool_use blocks; different streaming format |
| `GitHubCopilot` | Copilot endpoints | OAuth token | Copilot-specific auth headers; OpenAI-compatible format |
| `AzureAIFoundry` | Azure endpoints | API key | Anthropic models via Azure; custom headers |
| `Mock` | None | None | Returns canned responses for testing |

### Streaming Protocol

Each provider converts its native streaming format into the unified `StreamPart` sequence:

```
// Example: OpenAI Chat Completions SSE
data: {"choices":[{"delta":{"content":"Hello"}}]}

// → StreamPart::TextDelta { text: "Hello" }

data: {"choices":[{"delta":{"reasoning_content":"Let me think..."}}]}

// → StreamPart::ThinkingDelta { thinking: "Let me think...", level: None }

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":"{\"fi"}}]}}]}

// → StreamPart::ToolCallStart { id: "call_1", name: "read" }
// → StreamPart::ToolCallDelta { id: "call_1", arguments_delta: "{\"fi" }

data: [DONE]

// → StreamPart::StreamDone { stop_reason, usage }
```

---

## Phase 5: Tool System

### File: `src/tools/base.rs`

```rust
/// The Tool trait — every built-in and custom tool implements this
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool name (e.g., "read", "edit", "bash")
    fn name(&self) -> &str;

    /// Description shown to the LLM in the system prompt
    fn description(&self) -> &str;

    /// JSON Schema for the tool's parameters (sent to LLM as function definition)
    fn parameters_schema(&self) -> serde_json::Value;

    /// Whether this tool modifies files or system state
    fn is_mutating(&self) -> bool;

    /// Icon shown in the UI (e.g., "→", "$", "←", "+")
    fn icon(&self) -> &str;

    /// Guidelines injected into the system prompt for this tool
    fn prompt_guidelines(&self) -> &[&str];

    /// Execute the tool with raw JSON parameters
    async fn execute(
        &self,
        params: serde_json::Value,
        cancel: watch::Receiver<bool>,
    ) -> KonResult<ToolResult>;

    /// Short human-readable call text for the UI header
    fn format_call(&self, params: &serde_json::Value) -> String;

    /// Preview text shown at approval time (None = no preview)
    fn format_preview(&self, params: &serde_json::Value) -> Option<String>;
}
```

### File: `src/tools/mod.rs`

Tool registry with built-in tools:

```rust
/// 6 core tools enabled by default
pub fn core_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(read::ReadTool::new()),
        Box::new(edit::EditTool::new()),
        Box::new(write::WriteTool::new()),
        Box::new(bash::BashTool::new()),
        Box::new(grep::GrepTool::new()),
        Box::new(find::FindTool::new()),
    ]
}

/// 2 extra tools configurable via `[tools] extra = [...]`
pub fn extra_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(web_search::WebSearchTool::new()),
        Box::new(web_fetch::WebFetchTool::new()),
    ]
}

/// Lookup a tool by name
pub fn get_tool(name: &str, active_tools: &[Box<dyn Tool>]) -> Option<&Box<dyn Tool>> {
    active_tools.iter().find(|t| t.name() == name)
}

/// Generate tool definitions for the LLM request
pub fn get_tool_definitions(tools: &[Box<dyn Tool>]) -> Vec<ToolDefinition> {
    tools.iter().map(|t| ToolDefinition {
        name: t.name().to_string(),
        description: t.description().to_string(),
        parameters: t.parameters_schema(),
    }).collect()
}
```

### Built-in Tool Details

| Tool | Icon | Mutating | Backend | Details |
|---|---|---|---|---|
| `read` | → | No | `tokio::fs` + `fd` binary | Reads file contents with line numbers (format: `<line>: <content>`). Supports image files (base64 encode via `image` crate). Can also list directories via `fd`. Pagination support with offset/limit parameters. |
| `edit` | ← | Yes | `tokio::fs` + `similar` crate | Surgical text replacement via `old_string`/`new_string` matching. Uses the `similar` crate to generate and display diffs with context lines. Fails if `old_string` not found or found multiple times (requires exact match). |
| `write` | + | Yes | `tokio::fs` | Creates new files or overwrites existing ones. Takes `file_path` and `content` parameters. |
| `bash` | $ | Yes | `tokio::process::Command` | Executes shell commands with a configurable timeout. Spawns process via `Command::new("sh").arg("-c").arg(command)`. Captures stdout/stderr. Kills process tree on timeout or cancellation. Truncates long output with a note about temp file. |
| `grep` | * | No | `rg` binary (ripgrep) | Fast regex content search using ripgrep. Sorted by modification time. Respects `.gitignore`. Line number prefix in output. Line truncation for long lines. |
| `find` | ⌕ | No | `fd` binary | Fast file discovery using fd. Supports glob patterns. Respects `.gitignore`. Returns relative file paths sorted by modification time. |
| `web_search` | % | No | `reqwest` + DuckDuckGo | Searches the web via DuckDuckGo. Returns title, URL, and snippet for each result. |
| `web_fetch` | % | No | `reqwest` | Fetches URL content. Extracts readable content (text/markdown). Handles HTML → markdown conversion. |

### Permission Model

File: `src/permissions.rs`

```rust
pub enum PermissionMode {
    /// Ask before mutating tool calls
    Prompt,
    /// Skip all approval prompts
    Auto,
}

pub enum PermissionDecision {
    Allow,
    Prompt,
}

pub fn check_permission(tool: &dyn Tool, params: &serde_json::Value, mode: PermissionMode) -> PermissionDecision {
    match mode {
        PermissionMode::Auto => PermissionDecision::Allow,
        PermissionMode::Prompt => {
            if !tool.is_mutating() {
                return PermissionDecision::Allow;
            }
            // For bash, check if the command is on the safe list
            if tool.name() == "bash" {
                if is_safe_command(params) {
                    return PermissionDecision::Allow;
                }
            }
            PermissionDecision::Prompt
        }
    }
}
```

---

## Phase 6: Session Persistence

### File: `src/session.rs`

Sessions are stored as append-only JSONL files at:
```
~/.config/zeus-code/sessions/<sanitized-cwd>/<timestamp>_<uuid>.jsonl
```

### Entry Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionEntry {
    #[serde(rename = "header")]
    Header {
        version: u8,
        id: String,           // UUID
        timestamp: String,    // ISO 8601
        cwd: String,
        system_prompt: String,
        tools: Vec<String>,
        initial_thinking_level: String,
    },

    #[serde(rename = "message")]
    Message {
        id: String,           // 8-char short ID
        parent_id: String,    // previous entry's ID
        timestamp: String,
        message: Message,     // UserMessage, AssistantMessage, or ToolResultMessage
    },

    #[serde(rename = "compaction")]
    Compaction {
        id: String,
        parent_id: String,
        timestamp: String,
        summary: String,
        first_kept_entry_id: String,
        tokens_before: u64,
    },

    #[serde(rename = "model_change")]
    ModelChange {
        id: String,
        parent_id: String,
        timestamp: String,
        provider: String,
        model_id: String,
        base_url: Option<String>,
    },

    #[serde(rename = "thinking_level_change")]
    ThinkingLevelChange {
        id: String,
        parent_id: String,
        timestamp: String,
        thinking_level: String,
    },

    #[serde(rename = "leaf")]
    Leaf {
        id: String,
        parent_id: String,
        timestamp: String,
        target_id: String,    // Points to another entry in the tree
    },
}
```

### Session Struct

```rust
pub struct Session {
    pub id: Uuid,
    pub file_path: PathBuf,
    pub entries: Vec<SessionEntry>,
    pub cwd: PathBuf,
    pub tree: ConversationTree,
    pub system_prompt: String,
    pub active_tools: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl Session {
    /// Create a new session
    pub async fn new(cwd: PathBuf, system_prompt: String, tools: Vec<String>) -> KonResult<Self>;

    /// Load a session from a JSONL file
    pub async fn load(path: PathBuf) -> KonResult<Self>;

    /// Append an entry to the session (writes one line to JSONL)
    pub async fn append_entry(&mut self, entry: SessionEntry) -> KonResult<()>;

    /// Get active (uncompacted) messages for the LLM context
    pub fn active_messages(&self) -> Vec<Message>;

    /// Resolve a session by ID prefix
    pub fn resolve_by_prefix(prefix: &str) -> KonResult<PathBuf>;

    /// Find the most recent session file
    pub fn most_recent() -> Option<PathBuf>;

    /// Compact the session (keep last N messages, summarize earlier ones)
    pub async fn compact(&mut self, provider: &dyn Provider) -> KonResult<CompactionSummary>;
}
```

### Conversation Tree

Each session forms a tree structure via `parent_id` pointers on each entry. `LeafEntry` allows branching to different points in the conversation.

```rust
pub struct ConversationTree {
    nodes: HashMap<String, TreeNode>,
}

pub struct TreeNode {
    pub entry: SessionEntry,
    pub children: Vec<String>,     // Child entry IDs
    pub parent: Option<String>,
    pub leaves: Vec<String>,       // Leaf entry IDs pointing to this node
}
```

### Session Discovery

- Sessions are organized by working directory (sanitized path → subdirectory)
- Each session is a single `.jsonl` file
- `resolve_by_prefix(prefix)` scans session directories for matching UUID prefix
- `most_recent()` finds the newest JSONL file across all session directories
- Session picker in TUI shows recent sessions with metadata (timestamp, message count, cwd)

---

## Phase 7: Agent Loop

### File: `src/loop.rs`

The agent loop is the heart of the system — it orchestrates the turn-by-turn interaction between the LLM, tools, and session.

```rust
pub struct Agent {
    provider: Box<dyn Provider>,
    tools: Vec<Box<dyn Tool>>,
    session: Session,
    max_turns: u64,
    context_window: u64,
    compaction_config: CompactionConfig,
    permission_mode: PermissionMode,
}

impl Agent {
    /// Run the agent on a user query, emitting events to the channel
    pub async fn run(
        &mut self,
        query: String,
        skill_name: Option<String>,
        event_tx: mpsc::Sender<AgentEvent>,
        cancel_rx: watch::Receiver<bool>,
    ) -> KonResult<AgentEndInfo>;
}
```

### Turn Loop (pseudocode)

```
1. Append UserMessage to session
2. Emit AgentEvent::Start
3. Build system prompt:
   - Base system prompt from config
   - AGENTS.md/CLAUDE.md context (loaded from project)
   - Skills context (loaded from ~/.agents/skills/ and .agents/skills/)
   - Git context (branch, status, recent commits)
   - Tool guidelines (deduplicated from all active tools)
   - Current date/time

4. For turn in 1..max_turns:
   a. Emit AgentEvent::TurnStart { turn }
   b. Call provider.stream(messages, system_prompt, tool_definitions)
      - Retry with exponential backoff on transient errors
      - Check cancel_rx before each retry

   c. Consume the stream:
      - Loop: select! between stream.next() and cancel_rx.changed()
      - Buffer thinking blocks → emit ThinkingDelta events
      - Buffer text → emit TextDelta events
      - Collect tool calls → emit ToolStart/ToolArgsDelta events
      - On StreamDone: capture stop_reason, usage
      - On timeout: emit warning, allow continuation

   d. Execute pending tool calls:
      For each tool_call:
        - Find tool by name in tool registry
        - Parse arguments from JSON string
        - Validation: check params against schema
        - Permission check:
          - If ALLOW → proceed
          - If PROMPT → emit ToolApproval event, wait for oneshot response
        - Execute tool with cancel signal
        - Emit ToolResult event
        - If cancelled during execution → break (skip remaining tools)
        - Collect result for session appending

   e. Append AssistantMessage (with content, thinking, tool_calls, usage, stop_reason)
      to session

   f. Append ToolResultMessages to session

   g. Check compaction:
      - Count tokens in all messages + system prompt
      - If approaching context window (window - buffer_tokens):
        - Compact: summarize earlier messages via LLM
        - Inject synthetic Q&A: "Summarize what we did so far" → summary
        - Update session with CompactionEntry
        - If compaction_config.on_overflow == "pause" → break

   h. If stop_reason != ToolUse → break (LLM is done)

5. Emit AgentEvent::End { stop_reason, total_turns, usage }
6. Return AgentEndInfo
```

### File: `src/turn.rs`

Single turn execution details:

```rust
pub struct TurnRunner<'a> {
    provider: &'a dyn Provider,
    messages: Vec<Message>,
    system_prompt: String,
    tools: Vec<ToolDefinition>,
    cancel_rx: watch::Receiver<bool>,
    event_tx: mpsc::Sender<AgentEvent>,
    permission_mode: PermissionMode,
    tool_registry: &'a [Box<dyn Tool>],
    request_timeout: Duration,
    idle_timeout: Duration,
}

impl TurnRunner<'_> {
    /// Execute one full turn: stream → collect → execute tools
    pub async fn run(&mut self) -> KonResult<TurnResult>;

    /// Phase 1: Open the stream with retries
    async fn open_stream(&mut self) -> KonResult<LLMStream>;

    /// Phase 2: Consume stream chunks, buffer state, emit events
    async fn consume_stream(&mut self, stream: LLMStream) -> KonResult<StreamConsumption>;

    /// Phase 3: Execute collected tool calls (permission → approval → execute)
    async fn run_pending_tools(&mut self, tool_calls: Vec<BufferedToolCall>) -> KonResult<Vec<ToolResult>>;
}
```

### Cancellation

Every long-running operation races against the cancel signal:

```rust
tokio::select! {
    result = operation => { /* handle result */ },
    _ = cancel_rx.changed() => { return Err(KonError::Cancelled); },
}

// For multiple futures:
tokio::select! {
    Some(part) = stream.next() => { /* process part */ },
    _ = cancel_rx.changed() => { stream.abort(); return Err(KonError::Cancelled); },
    _ = tokio::time::sleep(idle_timeout) => { /* handle idle timeout */ },
}
```

### Compaction

File: `src/core/compaction.rs`

```rust
/// Check if the conversation is approaching the context window
pub fn should_compact(
    messages: &[Message],
    system_prompt: &str,
    context_window: u64,
    buffer_tokens: u64,
    tool_definitions: &[ToolDefinition],
) -> bool {
    let total = estimate_tokens(system_prompt)
        + messages.iter().map(estimate_message_tokens).sum::<u64>()
        + tool_definitions.iter().map(estimate_tool_def_tokens).sum::<u64>();
    total > (context_window - buffer_tokens)
}

/// Generate a summary of the conversation so far using the LLM
pub async fn generate_summary(
    messages: &[Message],
    provider: &dyn Provider,
) -> KonResult<String> {
    // Build a summarization prompt with all messages
    // Ask the LLM to produce a concise summary
    // Return the summary text
}

/// Rough token estimation (characters ÷ 4 for English text)
pub fn estimate_tokens(text: &str) -> u64 {
    text.len() as u64 / 4
}
```

---

## Phase 8: CLI Entry Point

### File: `src/cli.rs`

```rust
use clap::{Parser, ValueEnum};

#[derive(Parser)]
#[command(
    name = "zeus-code",
    about = "ZeusCode — Minimal coding agent harness",
    version,
    long_about = None
)]
pub struct Cli {
    /// Model to use (overrides config)
    #[arg(short = 'm', long)]
    pub model: Option<String>,

    /// Provider to use (overrides config)
    #[arg(long, value_enum)]
    pub provider: Option<ProviderArg>,

    /// Run a single prompt non-interactively, then exit
    /// Omit the value or pipe stdin to read the prompt from stdin
    #[arg(short = 'p', long, num_args = 0..=1)]
    pub prompt: Option<Option<String>>,

    /// API key (overrides env vars and config)
    #[arg(short = 'k', long)]
    pub api_key: Option<String>,

    /// Base URL for API (overrides config)
    #[arg(short = 'u', long)]
    pub base_url: Option<String>,

    /// Auth mode for OpenAI-compatible endpoints
    #[arg(long, value_enum)]
    pub openai_compat_auth: Option<AuthMode>,

    /// Auth mode for Anthropic-compatible endpoints
    #[arg(long, value_enum)]
    pub anthropic_compat_auth: Option<AuthMode>,

    /// Skip TLS verification (e.g., self-signed certs on local providers)
    #[arg(long)]
    pub insecure_skip_verify: bool,

    /// Resume the most recent session
    #[arg(short = 'c', long)]
    pub continue_session: bool,

    /// Resume a specific session by ID (full or unique prefix)
    #[arg(short = 'r', long)]
    pub resume: Option<String>,

    /// Comma-separated extra tools to enable (e.g., web_search,web_fetch)
    #[arg(long, value_delimiter = ',')]
    pub extra_tools: Vec<String>,

    /// Show version and exit
    #[arg(long)]
    pub version: bool,
}

#[derive(Clone, ValueEnum)]
pub enum ProviderArg {
    AzureAiFoundry,
    Deepseek,
    GitHubCopilot,
    OpenAI,
    OpenAICodex,
    OpenAIResponses,
    Zhipu,
}
```

### File: `src/main.rs`

```rust
mod cli;
mod config;
mod loop_;
mod turn;
mod headless;
mod session;
mod permissions;
mod notify;
mod themes;
mod context;
mod core;
mod llm;
mod tools;
mod ui;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing/logging
    tracing_subscriber::fmt::init();

    let cli = cli::Cli::parse();

    match cli.prompt {
        Some(prompt_value) => {
            // Headless mode: run a single prompt and exit
            let exit_code = headless::run_headless(cli, prompt_value).await?;
            std::process::exit(exit_code);
        }
        None => {
            // TUI mode: interactive terminal UI
            ui::launch::run_tui(cli).await?;
        }
    }

    Ok(())
}
```

### File: `src/headless.rs`

```rust
/// Run a single prompt non-interactively and exit
///
/// Exit codes:
///   0 - completed successfully
///   1 - error during execution
///   2 - startup error (empty prompt, provider init failure)
///   3 - hit the max-turn limit
pub async fn run_headless(cli: Cli, prompt_arg: Option<String>) -> KonResult<i32> {
    // 1. Read prompt from arg or stdin
    // 2. Load config, merge CLI overrides
    // 3. Create provider
    // 4. Create session (ephemeral, not persisted to disk)
    // 5. Create agent with auto-approved permissions
    // 6. Run agent until completion
    // 7. Print final response to stdout
    // 8. Return exit code
}
```

---

## Phase 9: TUI

This is the largest and most complex phase. The original uses Python's Textual framework — we rebuild it with ratatui + crossterm.

### File: `src/ui/app.rs`

```rust
pub struct App {
    // State
    config: Arc<RwLock<Config>>,
    provider: Box<dyn Provider>,
    tools: Vec<Box<dyn Tool>>,
    session: Session,
    permission_mode: PermissionMode,
    thinking_level: ThinkingLevel,

    // UI state
    chat_log: ChatLogState,
    input: InputState,
    status_line: StatusLineState,
    info_bar: InfoBarState,
    queue: Vec<QueuedMessage>,

    // Channels for agent communication
    event_tx: mpsc::Sender<AgentEvent>,
    event_rx: mpsc::Receiver<AgentEvent>,
    cancel_tx: watch::Sender<bool>,
    cancel_rx: watch::Receiver<bool>,

    // Agent
    agent_task: Option<JoinHandle<KonResult<AgentEndInfo>>>,
    agent_running: bool,

    // UI flags
    should_quit: bool,
    last_cancel_press: Option<Instant>,  // Double Ctrl+C to quit
    completion_open: bool,
    thinking_collapsed: bool,
    tool_expanded: HashSet<String>,
}

impl App {
    pub fn new(config: Arc<RwLock<Config>>, provider: Box<dyn Provider>, tools: Vec<Box<dyn Tool>>, session: Session) -> Self;
    pub async fn run(&mut self, terminal: &mut Terminal<CrosstermBackend>) -> KonResult<()>;
}
```

### Widget Layout

```
┌──────────────────────────────────────────────────┐
│                                                  │
│  ChatLog (scrollable)                           │  ← All conversation: user messages,
│  ┌──────────────────────────────────────────┐   │     thinking blocks, assistant text,
│  │ 👤 User: fix the failing test            │   │     tool calls + results
│  │                                          │   │
│  │ 💭 Thinking (collapsed) [Ctrl+Shift+T]   │   │
│  │                                          │   │
│  │ 🤖 Assistant: I'll look at the test...   │   │
│  │                                          │   │
│  │ → read tests/test_config.py              │   │
│  │   ✓ Read 42 lines                        │   │
│  │   [─────────────────] collapsed          │   │  Ctrl+O to expand
│  │                                          │   │
│  │ ← edit tests/test_config.py              │   │
│  │   ✓ Applied edit (+3 / -1)               │   │
│  │   ┌ diff ─────────────────────────┐     │   │
│  │   │ - old_line                      │     │   │
│  │   │ + new_line                      │     │   │
│  │   └────────────────────────────────┘     │   │
│  │                                          │   │
│  └──────────────────────────────────────────┘   │
│                                                  │
├──────────────────────────────────────────────────┤
│  Queue: [1] "also check the other tests"         │  ← Hidden when empty
│  Steer: "wait, use pytest instead"               │
├──────────────────────────────────────────────────┤
│                                                  │
│  > fix the failing test                          │  ← InputBox with autocomplete
│                                                  │
├──────────────────────────────────────────────────┤
│  /Users/nnn/project  main  12.3k tokens prompt   │  ← InfoBar (cwd, branch, tokens, mode)
│  Working... ⠋  turn 3  [Esc to cancel]           │  ← StatusLine (spinner, turn count)
└──────────────────────────────────────────────────┘
```

### Key Bindings

| Key | Action |
|---|---|
| `Enter` | Submit input |
| `Ctrl+C` | First press: clear input. Second press within 2s: exit |
| `Escape` | Interrupt running agent / close completion / close picker |
| `Shift+Enter` | Insert newline in input (multi-line messages) |
| `Alt+Enter` | Steer — queue a message to the running agent |
| `Ctrl+O` | Toggle tool output expansion (collapsed ↔ expanded) |
| `Ctrl+T` | Cycle thinking level (none → minimal → low → medium → high → xhigh) |
| `Ctrl+Shift+T` | Toggle thinking block visibility |
| `Shift+Tab` | Cycle permission mode (prompt ↔ auto) |
| `Tab` | Trigger autocomplete (path, file, command) |
| `Up` / `Down` | Navigate prompt history / scroll chat |
| `PageUp` / `PageDown` | Scroll chat by page |
| `y` / `n` | Approve / deny tool execution (when approval prompt shown) |

### Slash Commands

File: `src/ui/commands.rs`

| Command | Description |
|---|---|
| `/new` | Start a new conversation; reload project context |
| `/resume` | Open interactive session picker to restore a saved session |
| `/model` | Switch model via interactive picker |
| `/session` | Show current session info (ID, message count, token stats) |
| `/compact` | Compact the conversation immediately (truncate + summarize) |
| `/handoff <query>` | Create a focused handoff session from current context |
| `/themes` | Open theme picker to switch color theme |
| `/permissions` | Toggle permission mode (prompt ↔ auto) |
| `/thinking` | Switch thinking level via picker |
| `/notifications` | Toggle notification sounds |
| `/export` | Export current session as standalone HTML file |
| `/copy` | Copy last assistant response to system clipboard |
| `/clear` | Clear the current conversation (start fresh within same session) |
| `/help` | Show help text with all keybindings and commands |
| `/login` | Authenticate with a supported OAuth provider |
| `/logout` | Remove stored provider credentials |
| `/quit` | Exit Zeus |

### Input Processing

File: `src/ui/input.rs`

```rust
pub struct InputState {
    pub text: String,
    pub cursor_position: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub completion_state: Option<CompletionState>,
    pub paste_detected: bool,
}

impl InputState {
    fn handle_key(&mut self, key: KeyEvent) -> InputAction {
        match key {
            // Enter: submit if not completion open
            KeyCode::Enter if !self.completion_open => InputAction::Submit(self.text.clone()),
            // Tab: trigger autocomplete
            KeyCode::Tab => InputAction::Autocomplete,
            // Escape: cancel completion, or clear input
            KeyCode::Esc => { /* ... */ }
            // Ctrl+C: clear or quit
            KeyCode::Char('c') if ctrl => { /* ... */ }
            // Shift+Enter: newline
            KeyCode::Enter if shift => { /* insert newline at cursor */ }
            // Alt+Enter: steer
            KeyCode::Enter if alt => InputAction::Steer(self.text.clone()),
            // Up/Down: history navigation
            KeyCode::Up => { /* previous in history */ }
            KeyCode::Down => { /* next in history */ }
            // Regular text input
            _ => { /* insert character, move cursor */ }
        }
    }
}
```

### Chat Log

File: `src/ui/chat.rs`

The chat log is a scrollable list of message blocks. Each block type is rendered differently:

- **UserBlock:** Right-aligned or colored differently; skill badge if applicable
- **ThinkingBlock:** Italic/dim text; collapsible to a summary line (`Ctrl+Shift+T`); per-level background shading
- **ContentBlock:** Markdown rendered via `comrak` → ratatui `Text` (styled spans with colors, bold, italic, code blocks with syntax highlighting)
- **ToolBlock:** Header with tool icon, name, call text; result summary; expandable details with `Ctrl+O`; approval buttons (y/n/←/→/Enter); diff display with `+`/`-` coloring

### Status Widgets

File: `src/ui/widgets.rs`

**InfoBar (top row of footer):**
```
/Users/nnn/project  main  ↑3↓2  12.3k tokens  prompt  [GPT-5.5]  🧠low
```
- cwd (truncated)
- Git branch + status (↑ahead ↓behind)
- Token count
- Permission mode
- Model name
- Thinking level

**StatusLine (bottom row of footer):**
- Working state: spinner + "Agent working... turn 3 [Esc to cancel]"
- Completed state: "Done in 2.3s · 3 tool calls · 450 tokens [Ctrl+C to quit]"

**QueueDisplay:**
```
Queue: [1] "fix the tests" [2] "also check types"
Steer: "wait, use the mock client instead"
```
Hidden when empty. Shows up to 5 normal queued prompts and 5 steer messages.

**FileChangesModal:**
Shows summary of file changes after an agent run:
```
Files changed (3):
  + src/config.rs     (+42 / -3)
  ~ src/loop.rs       (+15 / -8)
  - tests/old_test.rs (deleted)
```

---

## Phase 10: Polish

### Themes: `src/themes.rs`

24 built-in themes, each with:
```rust
pub struct Theme {
    pub name: &'static str,
    pub background: Color,
    pub foreground: Color,
    pub accent: Color,
    pub dim: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub surface: Color,
    pub surface_bright: Color,
    pub border: Color,
    pub thinking_levels: HashMap<ThinkingLevel, Color>,
}
```

Themes include: gruvbox-dark, gruvbox-light, catppuccin (4 variants), dracula, nord, tokyo-night, tokyo-day, solarized (2 variants), monokai, one-dark, one-light, rosepine, everforest, kanagawa, nightowl, palenight, flexoki, github-dark, github-light, ayu.

### Notifications: `src/notify.rs`

```rust
pub fn play_notification(sound: NotificationSound, volume: f32) {
    // macOS: afplay
    // Linux: paplay (PulseAudio) or aplay (ALSA)
    // Windows: powershell -c (New-Object Media.SoundPlayer)
    // Fallback: terminal bell (\x07)
}
```

Sound files embedded via `include_bytes!()` for completion, error, and permission-prompt events.

### Context Loading: `src/context/`

**AGENTS.md / CLAUDE.md discovery:**
- Walk from current working directory up to git root (or home directory)
- Load all matching `AGENTS.md` and `CLAUDE.md` files
- Global override: `~/.config/zeus-code/AGENTS.md` is loaded first
- Format each file's contents into the system prompt under an `<agents_md>` section

**Skills:**
- Discover skills from `~/.agents/skills/` and `.agents/skills/`
- Each skill is a directory with a `SKILL.md` file
- Parse YAML frontmatter for name, description, register_cmd, cmd_info
- Validate skill names (lowercase, hyphens, max 64 chars)
- Skills with `register_cmd: true` appear as slash commands
- Skills are described to the model in the system prompt (unless `register_cmd: "only"`)

**Git context:**
- Current branch name
- Working tree status (modified, staged, untracked counts)
- Recent commit messages (last 5)
- Formatted into system prompt under `<git_status>` section

### Handoff: `src/core/handoff.rs`

```
/handoff "implement the database layer"
```
1. Gather current conversation messages
2. Create a handoff prompt asking the LLM to synthesize context
3. Send the synthesis prompt to the LLM
4. Create a new session with the synthesized handoff context
5. Create navigation links between old and new sessions (LeafEntry)
6. Switch the UI to the new session

### Shell Commands

Special input prefixes handled before LLM submission:

- `!command` — Run shell command in working directory, show output in chat as a system message
- `!!command` — Run shell command, show output, then send the output to the LLM as context for a follow-up response

### Export: `src/ui/export.rs`

`/export` generates a standalone HTML file with embedded CSS:
- Full conversation transcript
- Message styling matching the current theme
- Collapsible thinking blocks
- Tool call/result blocks with expand/collapse
- Diff display
- Written to current working directory with timestamped filename

---

## Phase 11: Testing and Documentation

### Unit Tests

| Module | What to Test |
|---|---|
| `config` | TOML parsing, deep merge, migrations, CLI overrides |
| `session` | JSONL round-trip, tree structure, compaction entries |
| `tools` | Each tool's execute, format_call, format_preview; permission checks |
| `core/types` | Serde round-trip for all types |
| `llm/models` | Model lookup, provider resolution |
| `permissions` | Safe command detection, mode switching |
| `context` | AGENTS.md loading, skills parsing, git formatting |

### Integration Tests

- Agent loop with `MockProvider`: test turn orchestration, compaction, cancellation
- Headless mode: test `-p` flag with mock provider
- Session lifecycle: create, append, compact, resume

### TUI Tests

Using `ratatui::backend::TestBackend`:
- Widget rendering snapshot tests
- Key binding behavior
- Slash command routing
- Input processing (history, completion, paste detection)

### E2E Tests

- Full headless run with mock provider
- Config creation on first run
- Session resume by prefix
- Exit code verification (0, 1, 2, 3)

### Documentation

- `README.md` — Quick start, features, configuration reference
- `AGENTS.md` — Project conventions for AI coding agents working on Zeus itself
- `docs/implementation-plan.md` — This document
- `docs/architecture.md` — Architecture overview with diagrams
- `docs/rust-crate-mapping.md` — Python to Rust dependency mapping
- `cargo doc` — API documentation for the codebase

---

## Architecture Overview

```
                    ┌─────────────────────────────────────┐
                    │            main.rs                   │
                    │   clap parse → dispatch              │
                    └──────────┬──────────────────────────┘
                               │
              ┌────────────────┼────────────────┐
              │                                 │
     ┌────────▼────────┐              ┌────────▼────────┐
     │   headless.rs   │              │   ui/launch.rs  │
     │   -p mode       │              │   TUI mode      │
     └────────┬────────┘              └────────┬────────┘
              │                                 │
              │              ┌──────────────────┼──────────────────┐
              │              │                  │                  │
     ┌────────▼────────┐   ┌▼─────────┐   ┌────▼────┐   ┌───────▼──────┐
     │   loop.rs       │   │ ui/app   │   │ ui/chat │   │ ui/widgets   │
     │   Agent::run()  │◄──│          │   │ ChatLog │   │ InfoBar      │
     │   Turn loop     │   │ ┌──────┐ │   │         │   │ StatusLine   │
     └──┬───┬───┬──────┘   │ │Input │ │   └─────────┘   │ QueueDisplay │
        │   │   │          │ └──────┘ │                  └──────────────┘
        │   │   │          │ ┌──────┐ │
        │   │   │          │ │Cmds  │ │   event_tx (mpsc channel)
        │   │   │          │ └──────┘ │   cancel_rx (watch channel)
        │   │   │          └──────────┘
        │   │   │
   ┌────▼───┐ │ ┌─────────▼────┐
   │turn.rs │ │ │ session.rs   │
   │Single  │ │ │ JSONL I/O    │
   │turn    │ │ │ Tree nav     │
   └───┬────┘ │ └──────────────┘
       │      │
  ┌────▼────┐ │ ┌──────────────┐
  │ llm/    │ │ │ core/        │
  │Providers│ │ │ compaction   │
  │Stream   │ │ │ handoff      │
  └─────────┘ │ │ errors       │
              │ └──────────────┘
         ┌────▼────┐
         │ tools/  │
         │ read    │
         │ edit    │
         │ write   │
         │ bash    │
         │ grep    │
         │ find    │
         │ web     │
         └─────────┘
```

### Data Flow (One Turn)

```
User types "fix the failing test" and presses Enter
  │
  ▼
ui/app.rs: on_input_submitted()
  │ Creates Agent, spawns agent_task
  │
  ▼
loop.rs: Agent::run(query)
  │ Appends UserMessage to session
  │ Builds system prompt
  │
  ├─► turn.rs: TurnRunner::run()
  │     │
  │     ├─► Phase 1: open_stream()
  │     │     llm/providers/*.rs: POST /v1/chat/completions (SSE)
  │     │     Retries on transient errors
  │     │
  │     ├─► Phase 2: consume_stream()
  │     │     Reads SSE chunks → StreamPart events
  │     │     event_tx.send(ThinkingDelta) → ui/chat.rs updates thinking block
  │     │     event_tx.send(TextDelta) → ui/chat.rs appends text
  │     │     Collects tool calls → ToolCall { name: "read", args: {...} }
  │     │
  │     ├─► Phase 3: run_pending_tools()
  │     │     For each tool call:
  │     │       1. permissions.rs: check_permission() → Allow/Prompt
  │     │       2. If Prompt: event_tx.send(ToolApproval) → ui displays y/n
  │     │          Wait for oneshot response
  │     │       3. tools/read.rs: ReadTool::execute(params, cancel)
  │     │          Reads file, returns ToolResult
  │     │       4. event_tx.send(ToolResult)
  │     │
  │     └─► Returns TurnResult { stop_reason, assistant_message, tool_results }
  │
  ├─► Appends AssistantMessage + ToolResultMessages to session
  ├─► Checks compaction: if near context window → compact via LLM
  ├─► If stop_reason == ToolUse → loop back for next turn
  │
  └─► event_tx.send(AgentEnd)
```

---

## Python → Rust Mapping

| Python | Rust |
|---|---|
| `asyncio` (built-in runtime) | `tokio` |
| Async generators (`async yield`) | `tokio::sync::mpsc::channel` |
| `asyncio.Event` (cancellation signal) | `tokio::sync::watch` |
| `asyncio.Future` (one-shot approval) | `tokio::sync::oneshot` |
| `asyncio.wait(FIRST_COMPLETED)` (race) | `tokio::select!` |
| `asyncio.create_subprocess_exec` | `tokio::process::Command` |
| `asyncio.to_thread` (blocking → async) | `tokio::task::spawn_blocking` |
| Pydantic `BaseModel` | `serde::Deserialize` + `serde::Serialize` derive |
| `model_json_schema()` | `schemars::JsonSchema` derive |
| `@dataclass` | `#[derive(Debug, Clone)]` struct |
| `@abstractmethod` + ABC | `#[async_trait]` trait |
| `contextvars.ContextVar` | `Arc<RwLock<T>>` or `tokio::task_local!` |
| `httpx` / `aiohttp` HTTP client | `reqwest` |
| `openai` SDK | Raw `reqwest` with SSE parsing (or `async-openai` crate) |
| `anthropic` SDK | Raw `reqwest` |
| `rich.markdown` | `comrak` + `syntect` (or `pulldown-cmark`) |
| `rich.Syntax` (syntax highlighting) | `syntect` |
| `rich.Text` (styled spans) | ratatui `Text` + `Span` |
| `textual.App` | ratatui `App` + manual event loop |
| `textual.Widget` | ratatui `Widget` trait |
| `textual.VerticalScroll` | ratatui `Paragraph` in scrollable area |
| `textual.TextArea` | ratatui `TextArea` widget (or custom) |
| `textual.run_worker()` | `tokio::spawn` |
| `tomllib` (stdlib) | `toml` crate |
| `json` (stdlib) | `serde_json` |
| `pathlib.Path` | `std::path::PathBuf` |
| `aiofiles` (async file I/O) | `tokio::fs` |
| `Pillow` (image processing) | `image` crate |
| `difflib` (diff generation) | `similar` crate |
| `subprocess` (binary spawning) | `std::process::Command` |
| `shutil.which` (binary detection) | `which` crate |
| `argparse` (CLI) | `clap` with derive |
| `uuid.uuid4()` | `uuid::Uuid::new_v4()` |
| `base64` | `base64` crate |
| `datetime` | `chrono` |
| `re` (regex) | `regex` crate |
| `platform.system()` | `std::env::consts::OS` |
| `logging` | `tracing` |
| `dataclasses.asdict()` | `serde::Serialize` |
| `Enum` | `strum` + `serde` tagged enum |
| `TypedDict` | Struct with `serde` |
| `functools.lru_cache` | `once_cell::sync::Lazy` or manually cached |
| `threading.Lock` | `parking_lot::Mutex` |
| `multiprocessing` (?) | N/A (single async runtime) |
