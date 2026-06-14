# Python → Rust Crate Mapping

This document maps every Python library used by Kon to its Rust equivalent for the Zeus-Code port.

## Runtime & Async

| Python | Rust | Notes |
|---|---|---|
| `asyncio` (built-in) | `tokio` | Full async runtime. Features: `full` |
| `asyncio.Event` | `tokio::sync::watch` | Single-producer, multi-consumer value broadcast |
| `asyncio.Future` (one-shot) | `tokio::sync::oneshot` | One-shot channel for approval responses |
| `asyncio.Queue` | `tokio::sync::mpsc` | Multi-producer, single-consumer channel |
| `asyncio.wait(FIRST_COMPLETED)` | `tokio::select!` | Race multiple futures |
| `asyncio.create_subprocess_exec` | `tokio::process::Command` | Async subprocess spawning |
| `asyncio.to_thread` | `tokio::task::spawn_blocking` | Run blocking code on thread pool |
| `asyncio.sleep` | `tokio::time::sleep` | Async sleep |
| `asyncio.timeout` | `tokio::time::timeout` | Timeout wrapper |
| `asyncio.Task` | `tokio::task::JoinHandle` | Spawned task handle |
| `asyncio.gather` | `futures::future::join_all` | Run multiple futures concurrently |
| `asyncio.as_completed` | `futures::stream::FuturesUnordered` | Process futures as they complete |
| `contextvars.ContextVar` | `Arc<RwLock<T>>` or `tokio::task_local!` | Per-task/per-execution context |

## Data Validation & Serialization

| Python | Rust | Notes |
|---|---|---|
| `pydantic.BaseModel` | `serde::Serialize` + `serde::Deserialize` | Data classes with validation |
| `.model_json_schema()` | `schemars::JsonSchema` | Generate JSON Schema from types |
| `@dataclass` | `#[derive(Debug, Clone)]` struct | Plain data containers |
| `json` (stdlib) | `serde_json` | JSON serialization/deserialization |
| `tomllib` (stdlib) | `toml` | TOML parsing |
| `yaml` (skills frontmatter) | `serde_yaml` | YAML frontmatter parsing |
| `TypedDict` | Struct with `serde` | Typed dictionary |
| `Enum` | `strum` + serde tagged enum | Enums with string conversion |
| `base64` (stdlib) | `base64` | Base64 encoding (images) |
| `uuid` (py → uuid module) | `uuid` | UUID generation (v4) |

## HTTP & Networking

| Python | Rust | Notes |
|---|---|---|
| `httpx` | `reqwest` | HTTP client with streaming, JSON, TLS |
| `aiohttp` | `reqwest` | Async HTTP (same crate) |
| `openai` SDK | Raw `reqwest` or `async-openai` crate | OpenAI API client |
| `anthropic` SDK | Raw `reqwest` | Anthropic API client |
| `ddgs` (DuckDuckGo) | `reqwest` + manual API | Web search |
| `curl-cffi` | N/A (reqwest handles TLS) | TLS fingerprinting (not needed with reqwest) |

## TUI & Terminal

| Python | Rust | Notes |
|---|---|---|
| `textual` (framework) | `ratatui` + manual event loop | TUI framework |
| `textual.App` | Custom `App` struct using ratatui | Application root |
| `textual.Widget` | `ratatui::widgets::Widget` trait | Widget rendering |
| `textual.VerticalScroll` | ratatui `Paragraph` in scrollable area | Scrolling content |
| `textual.TextArea` | Custom `TextArea` or `tui-textarea` crate | Multi-line text input |
| `textual.run_worker()` | `tokio::spawn` | Background tasks |
| `textual.CSS` styling | ratatui `Style` objects | Component styling |
| `textual.Binding` | Manual key event matching | Key bindings |
| `rich` (terminal rendering) | ratatui (built-in) | Rich text rendering |
| `rich.markdown` | `comrak` + `pulldown-cmark` | Markdown → styled text |
| `rich.Syntax` | `syntect` | Syntax highlighting |
| `rich.Text` / `rich.Span` | ratatui `Text` / `Span` | Styled text spans |
| `rich.console.Console` | `crossterm::terminal` | Terminal control |
| `rich.panel.Panel` | ratatui `Block` with borders | Bordered panels |
| `rich.table.Table` | ratatui `Table` widget | Tables |
| `rich.progress.Progress` | ratatui `Gauge` or `Spinner` | Progress indicators |

## File System & I/O

| Python | Rust | Notes |
|---|---|---|
| `aiofiles` | `tokio::fs` | Async file I/O |
| `pathlib.Path` | `std::path::PathBuf` | File paths |
| `shutil.which` | `which` crate | Binary discovery (fd, rg) |
| `os` module | `std::fs`, `std::env` | File system, environment |
| `tempfile` module | `tempfile` crate | Temporary files |
| `glob` module | `glob` crate (or manual) | File pattern matching |

## Process Management

| Python | Rust | Notes |
|---|---|---|
| `subprocess.run` | `std::process::Command` (sync) | Spawn external commands |
| `asyncio.create_subprocess_exec` | `tokio::process::Command` (async) | Async subprocess |
| `os.killpg` | `nix::sys::signal::killpg` (Unix) | Process group kill |
| `signal` module | `tokio::signal` | Signal handling |
| `os.getpid` | `std::process::id` | Current process ID |

## Text Processing & Diff

| Python | Rust | Notes |
|---|---|---|
| `re` (regex) | `regex` | Regular expressions |
| `difflib` | `similar` | Diff generation |
| `readability-lxml` | `readability` crate or manual | HTML content extraction |
| `html-to-markdown` | `htmd` crate or manual | HTML → Markdown |
| `lxml-html-clean` | `ammonia` crate | HTML sanitization |

## Image Processing

| Python | Rust | Notes |
|---|---|---|
| `Pillow` (PIL) | `image` | Image loading, resize, format conversion |
| `Pillow.Image.resize` | `image::imageops::resize` | Image resizing |
| `Pillow.Image.save` | `image::ImageBuffer::save` | Image encoding |

## Utilities

| Python | Rust | Notes |
|---|---|---|
| `datetime` | `chrono` | Date/time handling, ISO 8601 |
| `logging` | `tracing` + `tracing-subscriber` | Structured logging |
| `platform.system()` | `std::env::consts::OS` | Platform detection |
| `functools.lru_cache` | `once_cell::sync::Lazy` or manual | Function memoization |
| `threading.Lock` | `parking_lot::Mutex` | Synchronization |
| `threading.RLock` | `parking_lot::ReentrantMutex` | Reentrant lock |
| `collections.defaultdict` | `HashMap` with `.entry().or_default()` | Default-value map |
| `collections.OrderedDict` | `indexmap::IndexMap` | Ordered map |
| `hashlib` (stdlib) | `sha2`, `md5` crates | Hashing |
| `argparse` | `clap` with derive | CLI argument parsing |
| `dataclasses.asdict()` | `serde::Serialize` | Convert to dict/JSON |
| `textwrap` (stdlib) | `textwrap` crate | Text wrapping |
| `unicodedata` (stdlib) | `unicode-width` crate | Unicode character width |
| `inspect` module | N/A (no runtime reflection needed) | Introspection |
| `importlib.resources` | `include_bytes!` / `include_str!` | Embedded resources |

## Development & Testing

| Python | Rust | Notes |
|---|---|---|
| `pytest` | `#[test]` + `rstest` crate | Test framework |
| `pytest-asyncio` | `tokio::test` | Async test support |
| `pyright` | `cargo check` / `cargo clippy` | Type checking / linting |
| `ruff` formatter | `cargo fmt` | Code formatting |
| `ruff` linter | `cargo clippy` | Code linting |
| `uv` (package manager) | `cargo` (built-in) | Package management |
| `hatchling` (build system) | `cargo build` (built-in) | Build system |

## Not Used / Not Needed

Some Python dependencies have no direct Rust equivalent or aren't needed:

| Python Library | Reason Not Needed |
|---|---|
| `curl-cffi` | reqwest handles TLS natively; no fingerprint spoofing needed |
| `rich` (core rendering) | ratatui replaces rich for all terminal rendering |
| `textual` (framework) | ratatui replaces textual for the entire TUI |
| `pydantic` settings | serde handles all configuration parsing |
