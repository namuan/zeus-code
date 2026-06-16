# Plan: `/model` — Runtime Provider/Model Switching

## Overview

Implement the `/model` slash command to change the LLM provider and/or model at
runtime from within the TUI, without restarting Zeus. The user can switch
providers (e.g., `openai` → `deepseek`), models (e.g., `gpt-4o` → `gpt-5.5`),
or both at once (e.g., `/model openai/gpt-5.5`). The change takes effect on the
next turn — each turn rebuilds the agent from config, so no in-flight state is
affected.

## Motivation

Zeus supports 9 providers and 26+ models in its catalog, but once the TUI is
running the user is stuck with whatever they selected on the command line. If
the user starts with a cheap model for exploration but wants to switch to a
reasoning model for a complex refactor, or if they hit rate limits and want to
fall back to a different provider, they must quit and restart. `/model` makes
this seamless.

## Current State

- `parse_command("/model <arg>")` → `Command::Model(arg)` — already parsed, but
  falls through to the catch-all `"Command not yet implemented"` in
  `handle_submit()` (src/ui/app.rs:482).
- `App::build_agent()` (src/ui/app.rs:312-327) reads `cfg.llm.default_provider`
  and `cfg.llm.default_model` from the shared `Arc<RwLock<Config>>` on every
  call, then calls `create_provider(&pc)`. So updating the config guarantees the
  **next** `run_agent` call picks up the new model.
- `App` holds its own `provider: Box<dyn Provider>` (line 35) which was set at
  construction time (src/ui/launch.rs:49-60). This field is `#[allow(dead_code)]`
  and appears unused by the TUI — it's the `Agent` that drives the LLM, not the
  `App`. Nevertheless, for consistency and for any future direct use (e.g.
  metadata queries), it should be updated when the model changes.
- The `Agent` constructor (src/loop.rs:44-59) reads `config.agent.max_turns`,
  `config.agent.default_context_window`, `config.compaction.*`, and
  `config.permissions.mode` at construction time. The model **is not** stored on
  the `Agent` — it's the `Provider` that encapsulates it. So `Agent::new(config,
  provider)` gets the right provider, and the config fields (max_turns,
  context_window, etc.) are read once — but since `build_agent()` is called
  fresh on every `run_agent()`, stale config fields are not a concern.

## Semantics & Edge Cases

| Input | Behaviour |
|---|---|
| `/model` (no arg) | Show current model info and a list of available models for all providers |
| `/model openai/gpt-5.5` | Switch to `openai` provider, `gpt-5.5` model |
| `/model gpt-4o` (model only) | Search all providers for `gpt-4o`. Since it exists only in `openai`, switch to `openai/gpt-4o` |
| `/model deepseek` (provider only) | Switch to `deepseek` provider, use `deepseek/deepseek-chat` as the model |
| `/model gpt-5.5` (ambiguous) | `gpt-5.5` exists for `openai`, `openai-codex`, and `github-copilot`. Show the three options and ask the user to specify. |
| `/model nonexistent` | Error: "Unknown model 'nonexistent'. Use `/model` to see available models." |
| `/model anthropic/claude-sonnet-4-5-20250929` | Error: "Provider 'anthropic' is not yet implemented." The model exists in the catalog but `create_provider` returns `Err`. |
| `/model openai/gpt-5.5` while agent is running | **Allowed.** The change takes effect on the next turn; no in-flight request is affected. This is unlike `/compact` which manipulates the session. |
| `/model openai/gpt-5.5` at startup before any turns | Works — updates the provider before the first `run_agent()` call. |
| Trailing whitespace, extra args: `/model  openai/gpt-5.5  ` | Tolerated — args are trimmed before resolution. |
| Multiple providers, same model id | Ambiguous-match warning; user must be explicit. |

### Why `/model` can run while agent is running

Unlike `/compact` (which mutates the session concurrently with the agent loop),
`/model` only mutates the config and replaces a provider reference. The agent
loop holds its own `Agent` struct with its own `Box<dyn Provider>` — mutating
the App's fields has zero effect on the in-flight turn. The next turn calls
`build_agent()` fresh, which reads the updated config. This is safe and
consistent with how CLI tools like Claude Code allow mid-conversation model
switching.

## File Changes

### 1. New: `src/ui/model_switch.rs` (~80 lines)

Pure function module — no I/O, no async, no state. Contains the resolution logic
shared by the TUI handler. Separating this from `app.rs` keeps the TUI handler
thin and makes the resolution logic independently testable.

```rust
use crate::llm::models::{self, Model, all_models, all_providers};

/// Outcome of parsing a `/model` argument.
pub enum ModelSwitch {
    /// Successful resolution.
    Switch {
        provider: String,
        model_id: String,
        model_info: Model,
    },
    /// No argument — user wants to see the catalog.
    ShowCatalog,
    /// Model name matched multiple providers.
    Ambiguous {
        name: String,
        candidates: Vec<Model>,
    },
    /// Provider or model not found in the catalog.
    NotFound {
        arg: String,
        reason: String,
    },
    /// Provider is known but not yet implemented (e.g. anthropic).
    ProviderNotImplemented {
        provider: String,
    },
}

/// Resolve a `/model <arg>` string into a [`ModelSwitch`].
///
/// Resolution order:
/// 1. Empty/whitespace-only arg → `ShowCatalog`.
/// 2. Arg contains `/` → split into `provider/model`. Validate provider
///    exists, validate model exists for that provider, check implementation.
/// 3. Arg matches a provider name → switch provider, use
///    `default_model_for_provider(provider)`.
/// 4. Arg matches a model ID in exactly one provider → switch to that
///    provider+model.
/// 5. Arg matches a model ID in multiple providers → `Ambiguous`.
/// 6. Nothing matches → `NotFound`.
pub fn resolve_model_switch(arg: &str) -> ModelSwitch { ... }

/// Format a catalog summary for display in the chat.
pub fn format_catalog() -> String { ... }

/// Format the outcome of a model switch for display in the chat.
pub fn format_switch_result(result: &ModelSwitch) -> String { ... }
```

**Tests** (all in the same file, `#[cfg(test)] mod tests`):

- `test_resolve_empty_returns_show_catalog`
- `test_resolve_whitespace_returns_show_catalog`
- `test_resolve_provider_slash_model` — `"openai/gpt-4o"` → `Switch` with correct provider/model
- `test_resolve_provider_slash_model_not_found` — `"openai/nonexistent"` → `NotFound`
- `test_resolve_provider_only` — `"deepseek"` → `Switch` to `deepseek/deepseek-chat`
- `test_resolve_unknown_provider` — `"nonexistent"` → `NotFound`
- `test_resolve_model_only_unique` — `"gpt-4o"` → `Switch` to `openai/gpt-4o`
- `test_resolve_model_only_ambiguous` — `"gpt-5.5"` → `Ambiguous` with 3 candidates
- `test_resolve_provider_not_implemented` — `"anthropic/claude-sonnet-4-5"` → `ProviderNotImplemented`
- `test_format_catalog_contains_providers`
- `test_format_switch_result_contains_model_info`

### 2. Modify: `src/lib.rs` (+1 line)

Add module declaration:

```rust
pub mod shell_intercept;
pub mod ui {
    ...
    pub mod model_switch;  // ← new
}
```

### 3. Modify: `src/ui/app.rs` (~50 lines)

#### 3a. `handle_submit()` — add `Command::Model` arm

Replace the fall-through to `"Command not yet implemented"` for `Command::Model`
with a real handler:

```rust
Command::Model(ref arg) => {
    use crate::ui::model_switch::{
        ModelSwitch, format_catalog, format_switch_result, resolve_model_switch,
    };

    let result = resolve_model_switch(arg);

    match &result {
        ModelSwitch::Switch { provider, model_id, .. } => {
            // Update the shared config so the next turn picks up the new model.
            let mut cfg = self.config.write();
            cfg.llm.default_provider = provider.clone();
            cfg.llm.default_model = model_id.clone();
            drop(cfg);

            // Rebuild the app-level provider for consistency.
            self.provider = self.build_provider();

            self.chat.add_block(render_status(
                &format_switch_result(&result),
                &self.styles,
                false,
            ));
        }
        ModelSwitch::ShowCatalog => {
            self.chat.add_block(render_status(
                &format!("Current: {}/{}\n\n{}",
                    self.provider.name(),
                    self.provider.model(),
                    format_catalog(),
                ),
                &self.styles,
                false,
            ));
        }
        _ => {
            self.chat.add_block(render_status(
                &format_switch_result(&result),
                &self.styles,
                true, // is_error = true for NotFound/Ambiguous/ProviderNotImplemented
            ));
        }
    }
}
```

#### 3b. Add `build_provider()` helper on `App`

Extract the provider construction from `build_agent()` into its own method so
it can be reused by the `/model` handler:

```rust
/// Build a provider from the current config values (without constructing a
/// full Agent). Used by `/model` to update self.provider in-place.
fn build_provider(&self) -> Box<dyn Provider> {
    let cfg = self.config.read();
    let mut pc = ProviderConfig::new(&cfg.llm.default_provider, &cfg.llm.default_model, "");
    if !cfg.llm.default_base_url.is_empty() {
        pc.base_url = Some(cfg.llm.default_base_url.clone());
    }
    if cfg.llm.tls.insecure_skip_verify {
        pc.insecure_skip_verify = true;
    }
    create_provider(&pc)
        .unwrap_or_else(|_| create_provider(&ProviderConfig::new("mock", "mock", "")).unwrap())
}
```

Then refactor `build_agent()` to call `build_provider()`:

```rust
fn build_agent(&self) -> Agent {
    let provider = self.build_provider();
    Agent::new(self.config.clone(), provider)
}
```

This reduces duplication and ensures `/model` and `run_agent` use the same
provider-construction logic.

### 4. Modify: `src/config.rs` — make `default_model_for_provider` pub(crate)

Currently it's a private free function at line 338. It needs to be accessible
from `src/ui/model_switch.rs`:

```rust
pub(crate) fn default_model_for_provider(provider: &str) -> String { ... }
```

Alternatively, the resolution function in `model_switch.rs` can duplicate the
five-line match — but extracting it is cleaner.

### 5. Modify: `src/llm/models.rs` — add a `find_model_by_id` helper

Currently the catalog only has `find_model(provider, id)` which requires a
provider. The `/model` resolution needs to search across all providers for a
model ID. Add:

```rust
/// Find all models matching a given model ID across all providers.
pub fn find_models_by_id(model_id: &str) -> Vec<Model> {
    MODELS
        .iter()
        .filter(|m| m.id == model_id)
        .cloned()
        .collect()
}
```

### 6. Modify: `src/core/types.rs` — extend `ContentBlock` for large status blocks

The `/model` catalog listing can be quite long (26+ models). The current
`render_status()` uses `Message::System` content, which is fine for short
messages but may be truncated or hard to read for a multi-line catalog. 

**Decision: keep it simple.** Use `render_status()` as-is for now. The catalog
text can be reformatted in a future iteration (e.g. with `add_block` /
`add_line` for fancier rendering). The key deliverable is model switching, not
catalog aesthetics.

## Verification

```
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

Smoke tests in the TUI (using mock provider):

```
cargo run -- -m mock
# Type:
/model                          → shows catalog and current model
/model gpt-4o                   → switches to openai/gpt-4o
/model openai/gpt-5.5           → switches to openai/gpt-5.5
/model deepseek                 → switches to deepseek/deepseek-chat
/model nonexistent              → error
/model gpt-5.5                  → ambiguous (3 providers)
/model anthropic/claude-sonnet  → provider not implemented
```

## File Change Summary

| File | Change |
|---|---|
| `src/ui/model_switch.rs` | **New file** (~80 lines): `resolve_model_switch`, `format_catalog`, `format_switch_result`, + 11 unit tests |
| `src/lib.rs` | +1 line: `pub mod model_switch;` |
| `src/ui/app.rs` | +50 lines: `Command::Model` arm in `handle_submit()`, + `build_provider()` extract, refactor `build_agent()` |
| `src/config.rs` | ~1 char: make `default_model_for_provider` `pub(crate)` |
| `src/llm/models.rs` | +7 lines: add `find_models_by_id()` helper |

## Decisions Log

- **Synchronous handler** — no spawned task. Model resolution is pure catalog
  lookup + config mutation; there is no I/O or LLM call. This keeps the
  implementation simple and the feedback instantaneous.
- **Allowed while agent is running** — unlike `/compact`, model switching does
  not race on session state. Each turn builds a fresh `Agent` from config, so
  the in-flight turn is unaffected and the next turn picks up the new model.
- **New `model_switch.rs` module** rather than inlining into `app.rs` — the
  resolution logic is pure functions that are independently testable without
  standing up an App, TUI, or session. This mirrors the pattern used for
  `shell_intercept.rs`.
- **Extract `build_provider()` from `build_agent()`** — avoids duplicating the
  provider-construction incantation (base_url, insecure_skip_verify, fallback to
  mock). The `/model` handler and `run_agent` use the same code path.
- **`find_models_by_id` rather than inlining search** — the catalog already has
  a `find_model(provider, id)` for single-provider lookup; a cross-provider
  variant belongs in the same module.
- **No catalog pagination yet** — 26 models fit in a single status block. If
  the catalog grows significantly, a future iteration could add scrolling or
  filtering.
- **Ambiguous model names require user specificity** — `gpt-5.5` exists in
  three providers. Rather than silently picking one (which would be surprising),
  the command shows all three and asks the user to use the `provider/model`
  syntax. This is the safe UX choice.
