# ADR-003: `/model` — Runtime Provider/Model Switching with Interactive Popup

- **Status:** Accepted
- **Date:** 2026-06-16
- **Plan:** [plans/2026-06-16-slash-command-model.md](../plans/2026-06-16-slash-command-model.md)

## Context

Zeus supports 9 providers and 26+ models in a compile-time catalog
(`src/llm/models.rs`), but once the TUI started the user was locked into
whatever provider and model they selected on the command line. The `App`
held a single `Box<dyn Provider>` created at launch, and every turn reused
it. The `/model` command was parsed (`Command::Model(arg)`) but fell
through to a `"Command not yet implemented"` status message.

The underlying infrastructure already supported dynamic model switching:

- `App::build_agent()` read `cfg.llm.default_provider` and
  `cfg.llm.default_model` from the shared `Arc<RwLock<Config>>` on every
  call, so updating the config would make the **next** turn pick up the
  new model automatically.
- `ProviderConfig::new()` looked up model metadata (context window,
  thinking levels, vision support) from the catalog and auto-configured
  the provider.
- `create_provider()` dispatched to the right `Provider` implementation
  based on the provider name string.

What was missing was a way for the user to change the running
provider/model at runtime — without quitting and restarting Zeus.

## Decision

We implement `/model` as a synchronous slash command (no LLM call, no
I/O) with two paths: a **direct switch** for explicit arguments like
`/model openai/gpt-4o`, and an **interactive popup** for the no-argument
`/model` case.

### 1. Resolution logic in a pure, testable module

The new `src/ui/model_switch.rs` contains pure functions — no async, no
state, no TUI dependencies. `resolve_model_switch(arg)` returns an enum:

| Return variant | When |
|---|---|
| `Switch { provider, model_id, model_info }` | Successfully resolved |
| `ShowCatalog` | Empty argument — activate the popup |
| `Ambiguous { name, candidates }` | Model ID exists in multiple providers |
| `NotFound { arg, reason }` | Provider or model doesn't exist in catalog |
| `ProviderNotImplemented { provider }` | Provider known but stubbed |

Resolution order:
1. Empty / whitespace → `ShowCatalog`
2. Contains `/` → split into `provider/model`, validate both
3. Matches a provider name → use provider's default model
4. Matches a model ID in exactly one provider → switch to it
5. Matches a model ID in multiple providers → `Ambiguous`
6. Nothing matches → `NotFound`

### 2. Synchronous handler — no spawned task

Unlike `/compact` (which calls the LLM), `/model` is a pure catalog
lookup + config mutation. It runs synchronously in `handle_submit()`.
This means feedback is instantaneous and the implementation is simpler.

**Allowed while agent is running.** Each turn calls `build_agent()` fresh,
which reads the updated config. The in-flight turn's `Agent` holds its
own `Box<dyn Provider>` — mutating the App's fields has zero effect on a
running turn.

### 3. Extracted `build_provider()` helper

The provider-construction incantation (base_url,
insecure_skip_verify, fallback to mock) was duplicated in `build_agent()`.
We extracted it into `App::build_provider()` so both the `/model` handler
and `run_agent` use the same code path:

```rust
fn build_provider(&self) -> Box<dyn Provider> { ... }
fn build_agent(&self) -> Agent {
    let provider = self.build_provider();
    Agent::new(self.config.clone(), provider)
}
```

`apply_model_switch(provider, model_id)` updates the config, rebuilds
the provider, and renders a confirmation — called by both the direct
path and the popup.

### 4. Cross-provider model lookup

The catalog had `find_model(provider, id)` for single-provider lookup.
We added `find_models_by_id(id)` that returns all matches across
providers. This powers the unique-match detection (step 4 above) and the
ambiguous-match detection (step 5).

### 5. Interactive popup for `/model` with no argument

The initial implementation dumped the full catalog as a static text
block in the chat. User feedback requested a navigable popup overlay.
We implemented an interactive selector:

- **Rendering:** A ratatui overlay using `Clear` + bordered `Block` with
  `List`. Provider headers shown in accent color, models in normal text,
  non-implemented providers dimmed. Selected row highlighted. Footer
  shows scroll position and key hints. A summary line at the bottom
  displays the selected model's metadata.
- **Navigation:** Arrow keys, Page Up/Down, Enter to select, Esc to
  dismiss. Selection wraps around. Auto-scrolling keeps the selected
  item visible.
- **Architecture:** Self-contained `ModelPopup` struct in
  `src/ui/model_popup.rs`, mirroring the existing `AutocompleteState`
  pattern. Key handling is routed at the top of `App::handle_key` with
  an early-return guard (similar to the autocomplete guard).

### 6. Fixed stale defaults in `default_model_for_provider`

The existing `default_model_for_provider()` had stale model IDs for
`deepseek` (`"deepseek/deepseek-chat"` — an OpenRouter-style name not in
the deepseek provider's catalog), and lacked entries for `zhipu` and
`mock`. We corrected all entries to reference real catalog model IDs.
This bug existed before this feature and would have caused panics on
`cargo run --provider deepseek`.

## API Surface Added

| Symbol | Kind | Purpose |
|---|---|---|
| `resolve_model_switch` | `pub fn` in `ui::model_switch` | Resolve a `/model <arg>` string to a `ModelSwitch` |
| `format_switch_result` | `pub fn` in `ui::model_switch` | Format the resolution outcome for chat rendering |
| `is_provider_implemented` | `pub(crate) fn` in `ui::model_switch` | Check whether a provider is not a stub |
| `ModelPopup` | `pub struct` in `ui::model_popup` | Interactive model-selector overlay state |
| `App::build_provider` | `fn` on `App` | Build a provider from current config (extracted helper) |
| `App::apply_model_switch` | `fn` on `App` | Update config + rebuild provider + render confirmation |
| `App::handle_model_command` | `fn` on `App` | Orchestrate the `/model` command |
| `find_models_by_id` | `pub fn` in `llm::models` | Cross-provider model lookup by ID |
| `default_model_for_provider` | `pub(crate) fn` in `config` | Default model for a provider (visibility upgraded + fixed) |
| `AgentEvent::CompactionResult` | enum variant in `core::types` | (from ADR-002; reused pattern for async results) |

## Consequences

### Positive

- **Zero-latency model switching.** No API call, no spawn, no I/O. The
  user types `/model gpt-4o` and sees the result instantly. This works
  even while the agent is mid-turn.
- **Discoverable catalog.** The interactive popup shows all 26+ models
  grouped by provider with scrollable navigation, making it easy to
  explore what's available without reading source code or quitting the
  TUI.
- **Forward-compatible.** When a new provider graduates from stub to
  real implementation, adding it to `is_provider_implemented` makes it
  immediately selectable via `/model`. No other code changes needed.
- **Independently testable.** The resolution logic in `model_switch.rs`
  has 18 unit tests covering every edge case (empty, whitespace,
  provider/model, provider-only, unique-model, ambiguous-model,
  not-implemented, not-found). The popup's entry-building logic is
  deterministic and can be tested by inspecting the entry list.
- **Fixed latent bug.** `default_model_for_provider` had wrong model IDs
  for `deepseek`, `zhipu`, and `mock` — the `/model` implementation
  uncovered and fixed these.
- **Piggybacked cleanup.** Extracting `build_provider()` eliminated
  duplication between `build_agent()` and the new switch code, and
  derived `PartialEq` + `Eq` on `Model` (needed for test assertions).

### Negative / Trade-offs

- **Compile-time catalog.** Adding a new model requires editing
  `src/llm/models.rs` and recompiling. A future iteration could load
  models from config or an API, but the current approach is simple and
  sufficient for the supported provider set.
- **Popup scroll miscalculation on tiny terminals.** The popup clamps to
  40 columns minimum and uses a constant 20-row scroll window. On
  extremely small terminals (e.g. 24 rows), the popup's visual height
  exceeds the available rendering area, and ratatui clips it. A future
  iteration could make the popup fully responsive to terminal size at
  the scroll-offset level (not just visual rendering).
- **Ambiguous model names require manual specificity.** `gpt-5.5` exists
  in 3 providers. The resolution returns `Ambiguous` rather than
  silently picking one. The user must type `openai/gpt-5.5` or use the
  popup to select. This is correct UX but adds a step for the most
  common ambiguous case.
- **`format_catalog` is still defined but unused by the TUI.** The
  popup replaced the static catalog render, but `format_catalog()` is
  kept in `model_switch.rs` for backward compatibility (it's tested and
  could be useful for headless mode or future features).

## Verification

- `cargo fmt` — clean
- `cargo clippy --all-targets -- -D warnings` — clean
- `cargo test` — **264 tests pass** (260 lib + 4 integration)
- New tests:
  - `model_switch.rs`: 18 tests (resolve empty, whitespace, provider/model, provider-only, unique-model, ambiguous, not-implemented, not-found, catalog format, switch-result format, token formatting)
  - `models.rs`: `find_models_by_id` implicitly tested by the `model_switch` resolution tests
  - `config.rs`: `default_model_for_provider` implicitly tested by the `model_switch::test_resolve_provider_only` and `test_resolve_mock` tests
  - No regressions: all 243 pre-existing tests continue to pass
