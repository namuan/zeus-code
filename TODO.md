# TODO

Features listed in the README that are not yet implemented or are only partially working.

## Not Implemented

- [/] **Shell command integration** (`!command`, `!!command`) — no code exists. Prefix handling, command execution, and output routing are all missing. Currently all text goes to the LLM.

- [ ] **Anthropic provider** (`src/llm/providers/anthropic.rs`) — 2-line stub. Needs full API implementation with streaming, tool calls, and thinking support.

- [ ] **Azure AI Foundry provider** (`src/llm/providers/azure_ai_foundry.rs`) — 2-line stub.

- [ ] **OpenAI Codex Responses provider** (`src/llm/providers/openai_codex_responses.rs`) — 2-line stub.

- [ ] **OpenAI Responses API provider** (`src/llm/providers/openai_responses.rs`) — 2-line stub.

## Partially Implemented

- [/] **Self-compacting context** — token estimation and overflow detection work, but `generate_summary()` returns a hardcoded stub. The LLM is never called to produce a real summary (`src/core/compaction.rs` line 127).

- [ ] **Slash commands** — only 4 of 12 work (`/help`, `/quit`, `/clear`, `/new`). Still stubbed:
  - `/model` — change provider/model at runtime
  - `/resume` — load a previous session
  - `/compact` — trigger context compaction
  - `/themes` — cycle through color themes
  - `/thinking` — toggle thinking level
  - `/permissions` — switch permission mode
  - `/notifications` — toggle audio notifications
  - `/export` — `src/ui/export.rs` is a 2-line stub
  - `/handoff` — not even parsed

- [ ] **Audio notifications** — only plays the terminal bell (`\x07`). All three sound types (Complete, Error, Prompt) produce the same beep. No distinct audio feedback.

- [ ] **Multi-provider support** — 5 of 9 providers work (Mock + OpenAI-compat covering OpenRouter/OpenAI/DeepSeek/ZhiPu/Copilot). The 4 stubs are listed above under Not Implemented.
