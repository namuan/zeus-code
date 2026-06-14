# Zeus

<p align="center">
  <strong>⚡ Zeus ⚡</strong><br>
  <em>Minimal coding agent harness &mdash; in Rust</em>
</p>

Zeus is a minimal coding agent with a tiny core prompt, a small built-in toolset, and project-specific context layered
on top only when you want it. The default system prompt stays **under 270 tokens**, the fixed harness at about **~1,000
tokens**. Six default tools plus two extra web tools give you everything you need.

---

## Quick Start

The fastest way to get started is with [OpenRouter](https://openrouter.ai)'s free models. Get an API key, set it as an
env var, and go:

```bash
export OPENROUTER_API_KEY="sk-or-v1-..."
cargo run -- -p "Hello, what can you do?" --provider openrouter
```

Or install globally:

```bash
git clone https://github.com/0xku/zeus-code.git
cd zeus-code
cargo install --path .
zeus -p "your prompt" --provider openrouter
```

> **Requirements:** Rust 1.85+ (edition 2024). Install via [rustup](https://rustup.rs).

## Why Zeus

- **Minimal by design** &mdash; System prompt under 270 tokens
- **6 core tools** &mdash; `read`, `edit`, `write`, `bash`, `grep`, `find`
- **2 extra web tools** &mdash; `web_search`, `web_fetch`
- **Project context externalized** &mdash; `AGENTS.md`, skills
- **Highly configurable** &mdash; Every default is tunable
- **Built in Rust** &mdash; Fast startup, low memory, zero Python dependency

## Features

- Interactive TUI with streaming responses
- Multi-provider support (OpenRouter, OpenAI, Anthropic, GitHub Copilot, Azure, DeepSeek, ZhiPu, local)
- Append-only session persistence (JSONL)
- Self-compacting context (the LLM summarizes its own history)
- Slash commands (`/model`, `/resume`, `/compact`, `/handoff`, `/export`, &hellip;)
- Shell command integration (`!command`, `!!command`)
- 15 built-in color themes
- Audio notifications
- Non-interactive headless mode (`-p`)

## Providers

| Provider                        | Auth     | Notes                                       |
|---------------------------------|----------|---------------------------------------------|
| **OpenRouter**                  | API key  | Free models available via `openrouter/free` |
| OpenAI                          | API key  | GPT-4o, GPT-5.5, o4-mini                    |
| Anthropic                       | API key  | Claude Sonnet, Haiku, Opus                  |
| GitHub Copilot                  | OAuth    | GPT-5.5, Claude via Copilot                 |
| Azure AI Foundry                | API key  | Anthropic models via Azure                  |
| DeepSeek                        | API key  | V3, V4                                      |
| ZhiPu                           | API key  | GLM-5                                       |
| Local (OpenAI-compatible `/v1`) | Optional | llama.cpp, Ollama, etc.                     |

By default Zeus connects to OpenAI Codex. Use the `--provider` flag or set `default_provider` in config to change this.

## Configuration

Zeus stores its config at `~/.config/zeus/config.toml`. Created automatically on first run. Key options:

```toml
[llm]
default_provider = "openrouter"       # or openai, anthropic, deepseek, etc.
default_model = "openrouter/free"    # auto-routes to best available free model
default_thinking_level = "low"
request_timeout_seconds = 600

[permissions]
mode = "prompt"                       # "prompt" or "auto"

[ui]
theme = "gruvbox-dark"                # 15 themes available
collapse_thinking = true
```

## Development

```bash
git clone https://github.com/0xku/zeus-code.git
cd zeus-code

# Build
cargo build

# Run all checks
cargo fmt && cargo clippy -- -D warnings && cargo test

# Run from source
cargo run -- -p "your prompt" --provider openrouter
```

See [AGENTS.md](AGENTS.md) for project conventions and [docs/](docs/) for architecture and planning documents.

## License

[MIT](LICENSE)
