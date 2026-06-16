# TODO

Features listed in the README that are not yet implemented or are only partially working.

## Not Implemented

- [x] **Shell command integration** (`!command`, `!!command`) — fully implemented. Prefix parsing in `src/shell_intercept.rs`, async execution with timeout, output rendering in TUI via `AgentEvent::ShellResult`, and optional LLM forwarding with `!!command`.

- [ ] **Anthropic provider** (`src/llm/providers/anthropic.rs`) — 2-line stub. Needs full API implementation with streaming, tool calls, and thinking support.

- [ ] **Azure AI Foundry provider** (`src/llm/providers/azure_ai_foundry.rs`) — 2-line stub.

- [ ] **OpenAI Codex Responses provider** (`src/llm/providers/openai_codex_responses.rs`) — 2-line stub.

- [ ] **OpenAI Responses API provider** (`src/llm/providers/openai_responses.rs`) — 2-line stub.

## Partially Implemented

- [x] **Self-compacting context** — fully implemented. `generate_summary()` makes real LLM calls via `Provider::stream`. The agent loop detects overflow via `should_compact()`, splits at the last user turn, summarizes earlier messages, and persists `SessionEntry::Compaction`. `/compact` slash command triggers manual compaction. Summaries injected as system messages via `active_messages()`.

- [ ] **Slash commands** — 5 of 12 work (`/help`, `/quit`, `/clear`, `/new`, `/compact`). Still stubbed:
  - `/model` — change provider/model at runtime
  - `/resume` — load a previous session
  - `/themes` — cycle through color themes
  - `/thinking` — toggle thinking level
  - `/permissions` — switch permission mode
  - `/notifications` — toggle audio notifications
  - `/export` — `src/ui/export.rs` is a 2-line stub
  - `/handoff` — not even parsed

- [ ] **Audio notifications** — only plays the terminal bell (`\x07`). All three sound types (Complete, Error, Prompt) produce the same beep. No distinct audio feedback.

- [ ] **Multi-provider support** — 5 of 9 providers work (Mock + OpenAI-compat covering OpenRouter/OpenAI/DeepSeek/ZhiPu/Copilot). The 4 stubs are listed above under Not Implemented.
