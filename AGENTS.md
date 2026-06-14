# Agent Guidelines for Zeus-Code

## Code Style

- Run `cargo fmt` after editing or creating Rust files.
- Run `cargo clippy` for linting; fix all warnings before committing.
- Use `thiserror` for error types; avoid bare `anyhow` in library code.
- Prefer `&str` over `String` for function parameters when ownership isn't needed.
- Group imports: std first, then external crates, then crate-local.

## Testing

- Run `cargo test` for all tests (currently 157+).
- For specific modules: `cargo test --lib <module_name>`.
- Async tests use `#[tokio::test]`.
- Use `MockProvider` for tests that need LLM responses.
- Integration tests live in `tests/`.

## Committing Code

- Group logical changes into separate commits.
- Follow conventional commit prefixes: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`.
- Run `cargo fmt && cargo clippy -- -D warnings && cargo test` before committing.

## Pushing

- Run `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` before pushing.
- Only push if all checks pass.

## Project Structure

| Directory            | Purpose                                                                          |
|----------------------|----------------------------------------------------------------------------------|
| `src/core/`          | Foundation types (`types.rs`, `errors.rs`), compaction, handoff                  |
| `src/llm/`           | Provider trait + model catalog + 7 providers (mock, openai-completions, + stubs) |
| `src/tools/`         | 8 tools: read, edit, write, bash, grep, find, web_search, web_fetch              |
| `src/ui/`            | ratatui TUI: app, chat, input, blocks, widgets, commands, styles, launch         |
| `src/context/`       | Project context: AGENTS.md loader, git status, unified loader                    |
| `src/config.rs`      | TOML config with deep merge, migrations, atomic save, lazy_static singleton      |
| `src/loop.rs`        | Agent loop orchestration — multi-turn conversation driver                        |
| `src/turn.rs`        | Single turn: stream → consume → execute tools                                    |
| `src/session.rs`     | Append-only JSONL persistence with conversation tree                             |
| `src/permissions.rs` | Prompt/Auto mode with safe bash command whitelist                                |
| `src/themes.rs`      | 15 color themes (gruvbox, dracula, nord, tokyo-night, etc.)                      |
| `src/notify.rs`      | Terminal bell audio feedback                                                     |
| `src/cli.rs`         | clap CLI (--model, -p, -k, -u, -c, -r, --extra-tools)                            |
| `src/main.rs`        | Entry point: dispatch to headless (-p) or TUI mode                               |
| `src/headless.rs`    | Non-interactive mode with exit codes (0/1/2/3)                                   |
| `build.rs`           | Embed `defaults/config.toml` at compile time                                     |
| `docs/`              | Architecture, implementation plan, crate mapping                                 |
| `tests/`             | Integration tests (E2E agent loop, session lifecycle, config)                    |

## Architecture Notes

- The agent and UI communicate via tokio channels (mpsc for events, watch for cancellation).
- All long-running operations race against a `watch::Receiver<bool>` for immediate cancellation.
- Sessions are append-only JSONL files — never modified in place.
- The `Provider` trait abstracts all LLM API differences behind a unified streaming interface.
- Tools implement the `Tool` trait with JSON Schema parameter definitions.
- Config is loaded via TOML deep merge (user overrides embedded defaults) and accessible globally via
  `config::get_config()`.

## Implementation Status

| Phase                                | Status                                        |
|--------------------------------------|-----------------------------------------------|
| 1. Project Scaffolding               | ✅                                             |
| 2. Core Types & Data Structures      | ✅                                             |
| 3. Configuration System              | ✅                                             |
| 4. LLM Provider Abstraction          | ✅ (Mock + OpenAI Completions; others stubbed) |
| 5. Tool System                       | ✅ (8 tools fully implemented)                 |
| 6. Session Persistence               | ✅                                             |
| 7. Agent Loop                        | ✅                                             |
| 8. CLI / Headless Mode               | ✅                                             |
| 9. Terminal UI                       | ✅                                             |
| 10. Polish (themes, context, notify) | ✅                                             |
| 11. Testing & Documentation          | ✅                                             |
