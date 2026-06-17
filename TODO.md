# TODO

Features planned but not yet fully implemented.

## Not Implemented

- [ ] **Anthropic provider** (`src/llm/providers/anthropic.rs`) — 2-line stub. Needs full API implementation with streaming, tool calls, and thinking support.
- [ ] **Azure AI Foundry provider** (`src/llm/providers/azure_ai_foundry.rs`) — 2-line stub.
- [ ] **OpenAI Codex Responses provider** (`src/llm/providers/openai_codex_responses.rs`) — 2-line stub.
- [ ] **OpenAI Responses API provider** (`src/llm/providers/openai_responses.rs`) — 2-line stub.

## Partially Implemented

- [ ] **Slash commands** — 6 of 12 work (`/help`, `/quit`, `/clear`, `/new`, `/compact`, `/model`). Still stubbed:
  - `/resume` — load a previous session
  - `/themes` — cycle through color themes
  - `/thinking` — toggle thinking level
  - `/permissions` — switch permission mode
  - `/notifications` — toggle audio notifications
  - `/export` — `src/ui/export.rs` is a 2-line stub
  - `/handoff` — not even parsed

- [ ] **Audio notifications** — only plays the terminal bell (`\x07`). All three sound types (Complete, Error, Prompt) produce the same beep. No distinct audio feedback.

- [ ] **GitHub Copilot provider** (`src/llm/providers/copilot.rs`) — 2-line stub. (Uses OpenAI-compatible fallback for now.)
