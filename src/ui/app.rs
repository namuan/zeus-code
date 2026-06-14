//! Main App struct — widget composition, key bindings, input routing.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use tokio::sync::{mpsc, watch};

use crate::config::Config;
use crate::core::errors::KonResult;
use crate::core::types::AgentEvent;
use crate::llm::base::Provider;
use crate::llm::providers::{ProviderConfig, create_provider};
use crate::r#loop::Agent;
use crate::session::Session;
use crate::tools::base::Tool;
use crate::ui::autocomplete::AutocompleteState;
use crate::ui::blocks::{
    render_assistant_message, render_status, render_tool_result, render_user_message,
};
use crate::ui::chat::ChatLog;
use crate::ui::commands::{Command, parse_command};
use crate::ui::input::InputBox;
use crate::ui::styles::Styles;
use crate::ui::widgets;

/// Main application state.
pub struct App {
    #[allow(dead_code)]
    config: Arc<RwLock<Config>>,
    #[allow(dead_code)]
    provider: Box<dyn Provider>,
    #[allow(dead_code)]
    tools: Vec<Box<dyn Tool>>,
    styles: Styles,

    // UI state
    pub chat: ChatLog,
    pub input: InputBox,
    pub autocomplete: AutocompleteState,
    /// Cached list of all project files (populated on first `@`).
    all_files: Vec<String>,

    /// Tool call ID → tool name, used to render a preview when ToolEnd fires.
    pending_tools: std::collections::HashMap<String, String>,
    pub should_quit: bool,
    pub agent_running: bool,
    pub current_turn: u64,
    pub total_tokens: u64,

    /// Line count in chat when the current streaming turn began.
    /// Used to trim streaming text and replace with formatted blocks on TurnEnd.
    streaming_mark: usize,

    /// Line count in chat before the "⏳ Working…" placeholder line.
    /// Used to remove the placeholder when a ToolResult arrives.
    working_mark: Option<usize>,

    /// When the current tool started executing (for elapsed-time display).
    working_start: Option<Instant>,

    /// Accumulated streaming text for the current turn.
    /// Replaced with live markdown rendering on each TextDelta.
    streaming_buffer: String,

    /// Accumulated thinking text for the current turn (shown dimmed).
    streaming_thinking: String,

    /// The current conversation session (persisted across turns).
    /// Wrapped for safe sharing between the TUI thread and agent tasks.
    session: Arc<parking_lot::Mutex<Option<Session>>>,

    // Channels
    event_tx: mpsc::Sender<AgentEvent>,
    event_rx: mpsc::Receiver<AgentEvent>,
    cancel_tx: watch::Sender<bool>,
    cancel_rx: watch::Receiver<bool>,

    // Agent task
    agent_handle: Option<tokio::task::JoinHandle<KonResult<crate::r#loop::AgentEndInfo>>>,

    // Double-tap Ctrl+C for quit
    last_ctrl_c: Option<Instant>,
}

impl App {
    pub fn new(
        config: Arc<RwLock<Config>>,
        provider: Box<dyn Provider>,
        tools: Vec<Box<dyn Tool>>,
        session: Option<Session>,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::channel(128);
        let (cancel_tx, cancel_rx) = watch::channel(false);

        Self {
            config,
            provider,
            tools,
            styles: Styles::default_theme(),
            chat: ChatLog::new(),
            input: InputBox::new(),
            autocomplete: AutocompleteState::inactive(),
            all_files: Vec::new(),
            pending_tools: std::collections::HashMap::new(),
            should_quit: false,
            agent_running: false,
            current_turn: 0,
            total_tokens: 0,
            streaming_mark: 0,
            streaming_buffer: String::new(),
            streaming_thinking: String::new(),
            working_mark: None,
            working_start: None,
            session: Arc::new(parking_lot::Mutex::new(session)),
            event_tx,
            event_rx,
            cancel_tx,
            cancel_rx,
            agent_handle: None,
            last_ctrl_c: None,
        }
    }

    /// Handle a key event from crossterm.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        // ── Autocomplete is active: handle completion keys first ──────
        if self.autocomplete.active {
            // Ctrl+C, Ctrl+D, Ctrl+T — pass through to normal handler
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                self.autocomplete.dismiss();
                self.handle_key(key);
                return;
            }

            match key.code {
                // Tab / Enter: insert highlighted candidate, close popup
                KeyCode::Tab | KeyCode::Enter => {
                    self.apply_autocomplete();
                    return;
                }
                // Esc: dismiss popup, keep text as-is
                KeyCode::Esc => {
                    self.autocomplete.dismiss();
                    return;
                }
                // Up / Down: navigate candidates
                KeyCode::Up => {
                    self.autocomplete.select_prev();
                    return;
                }
                KeyCode::Down => {
                    self.autocomplete.select_next();
                    return;
                }
                // Backspace: delete last char; if filter becomes empty, dismiss
                KeyCode::Backspace => {
                    self.input.backspace();
                    self.after_autocomplete_char();
                    return;
                }
                // Typing a character: insert and re-filter
                KeyCode::Char(c) => {
                    self.input.insert_char(c);
                    self.after_autocomplete_char();
                    return;
                }
                // Left / Right / Home / End still work for cursor positioning
                KeyCode::Left => self.input.cursor_left(),
                KeyCode::Right => self.input.cursor_right(),
                KeyCode::Home => self.input.cursor_home(),
                KeyCode::End => self.input.cursor_end(),
                _ => {
                    // Unknown key — dismiss autocomplete and let normal handler run
                    self.autocomplete.dismiss();
                    self.handle_key(key);
                    return;
                }
            }
            return;
        }

        match key.code {
            // Quit: Ctrl+C (double-tap) or Ctrl+D
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let now = Instant::now();
                if let Some(last) = self.last_ctrl_c
                    && now.duration_since(last) < std::time::Duration::from_secs(1)
                {
                    self.should_quit = true;
                    return;
                }
                self.last_ctrl_c = Some(now);
                self.input.text.clear();
                self.input.cursor = 0;
            }
            KeyCode::Char('d')
                if key.modifiers.contains(KeyModifiers::CONTROL) && self.input.text.is_empty() =>
            {
                self.should_quit = true;
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Cycle thinking level placeholder
            }

            // Cancel: Escape
            KeyCode::Esc => {
                if self.agent_running {
                    let _ = self.cancel_tx.send(true);
                    self.agent_running = false;
                } else {
                    self.input.text.clear();
                    self.input.cursor = 0;
                }
            }

            // Submit: Enter
            KeyCode::Enter => {
                let text = self.input.submit();
                if !text.is_empty() {
                    self.handle_submit(&text);
                }
            }

            // Editing keys
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::Left => self.input.cursor_left(),
            KeyCode::Right => self.input.cursor_right(),
            KeyCode::Home => self.input.cursor_home(),
            // Ctrl+End: scroll chat to bottom
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.chat.scroll_to_bottom();
            }
            KeyCode::End => self.input.cursor_end(),
            KeyCode::Up => self.input.history_prev(),
            KeyCode::Down => self.input.history_next(),
            KeyCode::Char(c) => {
                self.input.insert_char(c);
                // Trigger file autocomplete on @
                if c == '@' {
                    self.activate_autocomplete();
                }
            }

            // Chat scrolling (PageUp / PageDown scroll by screenful)
            KeyCode::PageUp => self.chat.scroll_up(20),
            KeyCode::PageDown => self.chat.scroll_down(20),

            _ => {}
        }
    }

    /// Start the agent running with the given prompt.
    fn run_agent(&mut self, text: &str) {
        if self.agent_running {
            return;
        }

        // Render user message
        let user_block = render_user_message(text, None, &self.styles);
        self.chat.add_line(String::new(), &self.styles);
        self.chat.add_block(user_block);

        self.agent_running = true;
        self.working_start = Some(Instant::now());
        let query = text.to_string();

        // Build new channels for this run
        let event_tx = self.event_tx.clone();
        let cancel_rx = self.cancel_rx.clone();

        // Take the existing session out (or create a new one) for the agent
        let mut session_guard = self.session.lock();
        let mut session = session_guard.take().unwrap_or_else(|| {
            let sp = self.config.read().llm.system_prompt.content.clone();
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            Session::new_sync(cwd, sp, vec![]).expect("failed to create session")
        });
        drop(session_guard);

        let agent = self.build_agent();
        let session_arc = Arc::clone(&self.session);

        // Spawn agent
        let handle = tokio::spawn(async move {
            let result = agent
                .run(&mut session, query, None, event_tx, cancel_rx)
                .await;
            // Put the session back (with all messages appended) for the next turn
            *session_arc.lock() = Some(session);
            result
        });
        self.agent_handle = Some(handle);
    }

    /// Build a new Agent from current state.
    fn build_agent(&self) -> Agent {
        let cfg = self.config.read();
        let mut pc = ProviderConfig::new(&cfg.llm.default_provider, &cfg.llm.default_model, "");
        if !cfg.llm.default_base_url.is_empty() {
            pc.base_url = Some(cfg.llm.default_base_url.clone());
        }
        if cfg.llm.tls.insecure_skip_verify {
            pc.insecure_skip_verify = true;
        }
        let provider = create_provider(&pc)
            .unwrap_or_else(|_| create_provider(&ProviderConfig::new("mock", "mock", "")).unwrap());
        drop(cfg);

        Agent::new(self.config.clone(), provider)
    }

    // ── Session history restore ───────────────────────────────────────

    /// Render the active messages from the current session into the chat log.
    /// Called once at startup when `--continue-session` is used.
    pub fn render_session_history(&mut self) {
        let session_guard = self.session.lock();
        let Some(ref session) = *session_guard else {
            return;
        };

        let messages = session.active_messages();
        for msg in &messages {
            match msg {
                crate::core::types::Message::User(um) => {
                    let text = extract_user_text(&um.content);
                    let block = render_user_message(&text, um.skill_name.as_deref(), &self.styles);
                    self.chat.add_line(String::new(), &self.styles);
                    self.chat.add_block(block);
                }
                crate::core::types::Message::Assistant(am) => {
                    for block in render_assistant_message(&am.content, &self.styles) {
                        self.chat.add_block(block);
                    }
                    self.chat.add_separator(&self.styles);
                }
                crate::core::types::Message::ToolResult(tr) => {
                    let tool_result = crate::core::types::ToolResult {
                        success: true,
                        result: Some(tr.content.clone()),
                        images: tr.images.clone(),
                        ui_summary: Some(tr.tool_name.clone()),
                        ui_details: None,
                        ui_details_full: None,
                        file_changes: tr.file_changes.clone(),
                    };
                    let block = render_tool_result("", &tr.tool_name, &tool_result, &self.styles);
                    self.chat.add_block(block);
                }
                crate::core::types::Message::System(sm) => {
                    let block = render_status(&sm.content, &self.styles, false);
                    self.chat.add_block(block);
                }
            }
        }
        drop(session_guard);
    }

    /// Redraw the streaming portion of the chat (thinking + text).
    fn redraw_streaming(&mut self) {
        self.chat.truncate_to(self.streaming_mark);

        // Render thinking text (if any) as dimmed italic
        if !self.streaming_thinking.is_empty() {
            let thinking_style = self.styles.thinking_text();
            self.chat.add_styled_line("  💭 Thinking…", thinking_style);
            for line in self.streaming_thinking.lines() {
                self.chat
                    .add_styled_line(&format!("  {line}"), thinking_style);
            }
            self.chat.add_styled_line("", self.styles.base());
        }

        // Render streaming text as live markdown
        if !self.streaming_buffer.is_empty() {
            self.chat.replace_streaming_lines(
                self.chat.line_count(),
                &self.streaming_buffer,
                &self.styles,
            );
        }
    }

    // ── Autocomplete helpers ─────────────────────────────────────────

    /// Activate the file autocomplete popup for the `@` trigger.
    fn activate_autocomplete(&mut self) {
        let trigger_pos = self.input.cursor; // cursor is now after '@'
        // Lazy-load the full file list (only on first activation)
        if self.all_files.is_empty() {
            self.all_files = crate::ui::autocomplete::collect_project_files();
        }
        let filter = "";
        self.autocomplete = AutocompleteState::activate(trigger_pos, filter);
    }

    /// After a character is typed while autocomplete is active, update
    /// the filter. Dismiss if the cursor moves past the trigger area.
    fn after_autocomplete_char(&mut self) {
        // Get the text after the trigger position
        let text_after: &str = if self.autocomplete.trigger_pos <= self.input.text.len() {
            &self.input.text[self.autocomplete.trigger_pos..]
        } else {
            self.autocomplete.dismiss();
            return;
        };

        // Dismiss if the trigger character was deleted
        if self.autocomplete.trigger_pos == 0 || self.input.text.is_empty() {
            self.autocomplete.dismiss();
            return;
        }
        let before = self.autocomplete.trigger_pos - 1;
        if !self.input.text.is_char_boundary(before)
            || self.input.text.as_bytes().get(before) != Some(&b'@')
        {
            self.autocomplete.dismiss();
            return;
        }

        self.autocomplete.set_filter(text_after, &self.all_files);
    }

    /// Insert the highlighted candidate, replacing `@filter` in the input.
    /// Appends a trailing space so the user can continue typing immediately.
    fn apply_autocomplete(&mut self) {
        if let Some(candidate) = self.autocomplete.selected_candidate() {
            let mut candidate = candidate.to_string();
            // Append a space so user can type instructions right after the file
            candidate.push(' ');
            // Calculate the byte range to replace: from @ to end of filter
            let start = self.autocomplete.trigger_pos - 1; // position of '@'
            let end = self.input.text.len(); // end of text
            // Replace "@filter" with "candidate "
            self.input.text.replace_range(start..end, &candidate);
            self.input.cursor = start + candidate.len();
        }
        self.autocomplete.dismiss();
    }

    /// Handle submit logic extracted from key handler.
    fn handle_submit(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        // Check for slash commands
        if let Some(cmd) = parse_command(text) {
            match cmd {
                Command::Quit => self.should_quit = true,
                Command::Clear => self.chat.clear(),
                Command::Help => {
                    let help = crate::ui::commands::help_text();
                    self.chat
                        .add_block(render_status(help, &self.styles, false));
                }
                Command::New => {
                    self.chat.clear();
                    self.chat.add_block(render_status(
                        "Starting new conversation…",
                        &self.styles,
                        false,
                    ));
                }
                _ => {
                    self.chat.add_block(render_status(
                        &format!("Command not yet implemented: {text}"),
                        &self.styles,
                        false,
                    ));
                }
            }
            return;
        }

        // Regular input → run agent
        self.chat.scroll_to_bottom();
        self.run_agent(text);
    }

    /// Process agent events from the channel.
    pub fn process_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                AgentEvent::TurnStart { turn } => {
                    self.current_turn = turn;
                    self.streaming_buffer.clear();
                    self.streaming_thinking.clear();
                    // Record current line count so we can trim streaming text later
                    self.streaming_mark = self.chat.line_count();
                    self.chat
                        .add_line(format!("  🤖 Assistant (turn {turn})"), &self.styles);
                    // Show a placeholder so the user knows the assistant is working
                    self.chat.add_styled_line("  …", self.styles.dim_text());
                }
                AgentEvent::ThinkingDelta { text: thinking, .. } => {
                    self.streaming_thinking.push_str(&thinking);
                    self.redraw_streaming();
                }
                AgentEvent::TextDelta { text } => {
                    self.streaming_buffer.push_str(&text);
                    self.redraw_streaming();
                }
                AgentEvent::ToolStart { name, id, .. } => {
                    self.chat.add_line(format!("  🔧 {name}"), &self.styles);
                    // Track this tool for preview rendering
                    self.pending_tools.insert(id, name);
                }
                AgentEvent::ToolArgsDelta { .. } => {
                    // Args accumulate in ToolEnd; we render from the full JSON there
                }
                AgentEvent::ToolEnd { id, arguments, .. } => {
                    if let Some(name) = self.pending_tools.remove(&id)
                        && let Some(preview) = format_tool_preview(&name, &arguments)
                    {
                        self.chat
                            .add_styled_line(&preview, self.styles.thinking_text());
                        // Save mark before adding the working indicator
                        self.working_mark = Some(self.chat.line_count());
                        self.working_start = Some(Instant::now());
                        self.chat
                            .add_line("  | Working (0.0s)…".to_string(), &self.styles);
                    }
                }
                AgentEvent::ToolResult { result, .. } => {
                    // Remove the "Working…" placeholder line
                    if let Some(mark) = self.working_mark.take() {
                        self.chat.truncate_to(mark);
                    }
                    let block = render_tool_result("", "tool", &result, &self.styles);
                    self.chat.add_block(block);
                }
                AgentEvent::TurnEnd { assistant, .. } => {
                    // Trim streaming text back to the mark
                    self.chat.truncate_to(self.streaming_mark);
                    // Render assistant content as formatted markdown blocks
                    for block in render_assistant_message(&assistant.content, &self.styles) {
                        self.chat.add_block(block);
                    }
                    self.chat.add_separator(&self.styles);
                }
                AgentEvent::End {
                    usage, total_turns, ..
                } => {
                    self.total_tokens += usage.input_tokens + usage.output_tokens;
                    self.agent_running = false;
                    self.working_start = None;
                    self.chat.add_line(
                        format!(
                            "  ✓ Done in {total_turns} turns ({} tok)",
                            usage.input_tokens + usage.output_tokens
                        ),
                        &self.styles,
                    );
                }
                AgentEvent::Error { error } => {
                    self.agent_running = false;
                    self.working_start = None;
                    self.chat
                        .add_line(format!("  ✗ Error: {error}"), &self.styles);
                }
                _ => {}
            }
        }

        // Animate the working indicator while a tool is executing
        if let (Some(mark), Some(start)) = (self.working_mark, self.working_start) {
            let elapsed = start.elapsed();
            let spinner = spinner_frame(elapsed);
            let text = format!("  {spinner} Working ({:.1}s)…", elapsed.as_secs_f64());
            self.chat.truncate_to(mark);
            self.chat.add_line(text, &self.styles);
        }
    }

    /// Render the full TUI.
    pub fn render(&self, f: &mut Frame) {
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // chat area
                Constraint::Length(1), // status line
                Constraint::Length(3), // input box
            ])
            .split(f.area());

        // Chat area
        self.chat.render(f, main_layout[0], &self.styles);

        // Status line
        widgets::render_status_line(
            f,
            main_layout[1],
            self.agent_running,
            self.working_start.map(|s| s.elapsed()),
            self.current_turn,
            &self.styles,
        );

        // Input box
        self.input.render(f, main_layout[2], &self.styles);

        // Autocomplete popup (renders on top of everything)
        self.autocomplete.render(f, &self.styles);
    }
}

/// Extract plain text from user message content blocks.
fn extract_user_text(content: &[crate::core::types::ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|cb| {
            if let crate::core::types::ContentBlock::Text { text } = cb {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build a one-line preview of what a tool is about to do.
fn format_tool_preview(name: &str, args_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    match name {
        "edit" => {
            let file = v.get("file_path")?.as_str()?;
            let old = v.get("old_string")?.as_str()?;
            let new = v.get("new_string")?.as_str()?;
            let old_short: String = old.chars().take(60).collect();
            let new_short: String = new.chars().take(60).collect();
            let old_dots = if old.len() > 60 { "…" } else { "" };
            let new_dots = if new.len() > 60 { "…" } else { "" };
            Some(format!(
                "     {}  {old_short}{old_dots} → {new_short}{new_dots}",
                shorten_path_for_display(file),
            ))
        }
        "write" => {
            let file = v.get("file_path")?.as_str()?;
            let content = v.get("content")?.as_str()?;
            let preview: String = content.chars().take(80).collect();
            let dots = if content.len() > 80 { "…" } else { "" };
            Some(format!(
                "     {}  {preview}{dots}",
                shorten_path_for_display(file),
            ))
        }
        "bash" => {
            let cmd = v.get("command")?.as_str()?;
            Some(format!("     $ {cmd}"))
        }
        "read" => {
            let file = v.get("file_path")?.as_str()?;
            Some(format!("     {}", shorten_path_for_display(file)))
        }
        _ => None,
    }
}

fn shorten_path_for_display(path: &str) -> String {
    if path.len() > 50 {
        format!("…{}", &path[path.len().saturating_sub(49)..])
    } else {
        path.to_string()
    }
}

/// Return a spinning character based on elapsed time (cycles every ~200ms).
fn spinner_frame(elapsed: std::time::Duration) -> char {
    const SPINNERS: &[char] = &['|', '/', '-', '\\'];
    let ms = elapsed.as_millis() as usize;
    SPINNERS[(ms / 200) % SPINNERS.len()]
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::ToolResult;
    use crate::ui::autocomplete::AutocompleteState;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// Create a minimal App for testing key handling.
    /// Uses MockProvider and no real tools to keep tests fast.
    fn make_test_app() -> App {
        let config = Arc::new(RwLock::new(Config::load_defaults()));
        let provider = create_provider(&ProviderConfig::new("mock", "mock", "")).unwrap();
        let tools: Vec<Box<dyn Tool>> = vec![];
        let session: Option<Session> = None;
        App::new(config, provider, tools, session)
    }

    /// Press a key on the app.
    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    /// Press a char on the app.
    fn type_char(app: &mut App, c: char) {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }

    // ── @ trigger ────────────────────────────────────────────────────

    #[test]
    fn test_at_sign_activates_autocomplete() {
        let mut app = make_test_app();
        assert!(!app.autocomplete.active);

        type_char(&mut app, '@');

        assert!(app.autocomplete.active);
        assert_eq!(app.input.text, "@");
    }

    // ── Tab inserts file, does NOT submit ─────────────────────────────

    #[tokio::test]
    async fn test_tab_inserts_file_without_submitting() {
        let mut app = make_test_app();

        // Manually put the app into autocomplete mode with a known candidate
        app.input.text = "@mai".to_string();
        app.input.cursor = 4;
        let mut state = AutocompleteState::inactive();
        state.active = true;
        state.trigger_pos = 1; // after '@'
        state.selected = 0;
        // Simulate: candidates field is private, so use set_filter
        app.all_files = vec!["src/main.rs".into(), "src/lib.rs".into()];
        app.autocomplete = state;
        app.autocomplete
            .set_filter("mai", &["src/main.rs".into(), "src/lib.rs".into()]);

        // Press Tab
        press(&mut app, KeyCode::Tab);

        // The highlighted candidate should have replaced "@mai"
        assert!(
            app.input.text.contains("src/main.rs"),
            "expected file insertion, got: {}",
            app.input.text
        );
        // Should end with a space so user can type instructions immediately
        assert!(
            app.input.text.ends_with(' '),
            "expected trailing space after file, got: {:?}",
            app.input.text
        );
        // Autocomplete should be dismissed
        assert!(!app.autocomplete.active);
        // Input should NOT have been submitted (text should still be present)
        assert!(
            !app.input.text.is_empty(),
            "input should still have text after Tab"
        );
    }

    // ── Enter inserts file, does NOT submit ───────────────────────────

    #[tokio::test]
    async fn test_enter_inserts_file_without_submitting() {
        let mut app = make_test_app();

        // Set up autocomplete with a candidate
        app.input.text = "@lib".to_string();
        app.input.cursor = 4;
        let mut state = AutocompleteState::inactive();
        state.active = true;
        state.trigger_pos = 1;
        state.selected = 0;
        app.all_files = vec!["src/main.rs".into(), "src/lib.rs".into()];
        app.autocomplete = state;
        app.autocomplete
            .set_filter("lib", &["src/main.rs".into(), "src/lib.rs".into()]);

        // Press Enter
        press(&mut app, KeyCode::Enter);

        // Enter in autocomplete mode should insert the file, NOT submit
        assert!(
            app.input.text.contains("src/lib.rs"),
            "expected file inserted, got: {}",
            app.input.text
        );
        // Should end with a space so user can type instructions immediately
        assert!(
            app.input.text.ends_with(' '),
            "expected trailing space after file, got: {:?}",
            app.input.text
        );
        assert!(!app.autocomplete.active);
        // Text should still be in the input box (not submitted)
        assert!(
            !app.input.text.is_empty(),
            "Enter in autocomplete should NOT submit the message"
        );
    }

    // ── Esc dismisses autocomplete ────────────────────────────────────

    #[test]
    fn test_esc_dismisses_autocomplete() {
        let mut app = make_test_app();

        // Activate autocomplete via @
        type_char(&mut app, '@');
        assert!(app.autocomplete.active);

        // Press Esc
        press(&mut app, KeyCode::Esc);

        assert!(!app.autocomplete.active);
        // Input text should be preserved
        assert_eq!(app.input.text, "@");
    }

    // ── Esc with agent running cancels the agent ──────────────────────

    #[test]
    fn test_esc_clears_input_when_not_running() {
        let mut app = make_test_app();
        // No autocomplete active; not running
        app.autocomplete.dismiss();
        app.input.text = "hello".to_string();
        app.input.cursor = 5;

        press(&mut app, KeyCode::Esc);

        // Without autocomplete or agent running, Esc clears the input
        assert_eq!(app.input.text, "");
        assert_eq!(app.input.cursor, 0);
    }

    // ── Enter submits when autocomplete is NOT active ─────────────────

    #[tokio::test]
    async fn test_enter_submits_when_autocomplete_inactive() {
        let mut app = make_test_app();

        // Set up input text WITHOUT autocomplete active
        app.input.text = "hello world".to_string();
        app.input.cursor = 11;
        assert!(!app.autocomplete.active);

        // Press Enter
        press(&mut app, KeyCode::Enter);

        // Input should be cleared (submit() was called)
        assert_eq!(app.input.text, "");
        assert_eq!(app.input.cursor, 0);
    }

    // ── Ctrl modifiers dismiss autocomplete before normal handling ────

    #[test]
    fn test_ctrl_c_dismisses_autocomplete() {
        let mut app = make_test_app();

        // Activate autocomplete
        type_char(&mut app, '@');
        assert!(app.autocomplete.active);

        // Press Ctrl+C — should dismiss autocomplete and clear input
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

        assert!(!app.autocomplete.active);
        assert_eq!(app.input.text, "");
    }

    // ── Autocomplete activates multiple times ─────────────────────────

    #[test]
    fn test_autocomplete_reactivates_on_second_at() {
        let mut app = make_test_app();

        // First @
        type_char(&mut app, '@');
        assert!(app.autocomplete.active);

        // Dismiss
        press(&mut app, KeyCode::Esc);
        assert!(!app.autocomplete.active);

        // Type some text
        type_char(&mut app, 'h');
        type_char(&mut app, 'i');
        type_char(&mut app, ' ');

        // Second @
        type_char(&mut app, '@');
        assert!(app.autocomplete.active);
        // After "hi @" the second @ is at byte position 5
        assert_eq!(app.autocomplete.trigger_pos, 5);
    }

    // ── Continue session displays history ─────────────────────────────

    #[tokio::test]
    async fn test_continue_session_displays_user_message() {
        let cwd = PathBuf::from("/tmp/test-continue-session-chat");
        let _ = std::fs::create_dir_all(&cwd);
        let mut session = Session::new(cwd, "sp".into(), vec![]).await.unwrap();
        session
            .append_user_message(crate::core::types::UserMessage {
                content: vec![crate::core::types::ContentBlock::Text {
                    text: "hello world".into(),
                }],
                skill_name: None,
            })
            .await
            .unwrap();

        let config = Arc::new(RwLock::new(Config::load_defaults()));
        let provider = create_provider(&ProviderConfig::new("mock", "mock", "")).unwrap();
        let tools: Vec<Box<dyn Tool>> = vec![];
        let mut app = App::new(config, provider, tools, Some(session));

        // Before restore, chat should be empty
        assert_eq!(app.chat.line_count(), 0);

        // Restore session history into chat
        app.render_session_history();

        // Chat should now contain the rendered user message
        assert!(
            app.chat.line_count() > 0,
            "chat should display session history after restore"
        );
    }

    // ── Tool preview ──────────────────────────────────────────────────

    #[test]
    fn test_format_tool_preview_edit() {
        let args = r#"{"file_path":"src/main.rs","old_string":"hello","new_string":"hello world"}"#;
        let preview = format_tool_preview("edit", args);
        assert!(preview.is_some());
        let p = preview.unwrap();
        assert!(p.contains("src/main.rs"), "should contain file path");
        assert!(p.contains("hello"), "should contain old_string");
        assert!(p.contains("hello world"), "should contain new_string");
        assert!(p.contains("→"), "should contain arrow");
    }

    #[test]
    fn test_format_tool_preview_write() {
        let args =
            r#"{"file_path":"src/lib.rs","content":"fn main() {\n    println!(\"hi\");\n}"}"#;
        let preview = format_tool_preview("write", args);
        assert!(preview.is_some());
        let p = preview.unwrap();
        assert!(p.contains("src/lib.rs"), "should contain file path");
        assert!(p.contains("fn main()"), "should contain content preview");
    }

    #[test]
    fn test_format_tool_preview_bash() {
        let args = r#"{"command":"cargo build --release"}"#;
        let preview = format_tool_preview("bash", args);
        assert!(preview.is_some());
        let p = preview.unwrap();
        assert!(p.contains("cargo build"), "should contain command");
    }

    #[test]
    fn test_format_tool_preview_read() {
        let args = r#"{"file_path":"src/main.rs"}"#;
        let preview = format_tool_preview("read", args);
        assert!(preview.is_some());
        let p = preview.unwrap();
        assert!(p.contains("src/main.rs"), "should contain file path");
    }

    #[test]
    fn test_format_tool_preview_unknown_tool() {
        let args = r#"{"foo":"bar"}"#;
        let preview = format_tool_preview("grep", args);
        assert!(preview.is_none(), "unknown tools should return None");
    }

    // ── Thinking delta streaming ──────────────────────────────────────

    #[test]
    fn test_thinking_delta_accumulates() {
        let mut app = make_test_app();
        // Simulate a TurnStart
        app.streaming_mark = 0;
        app.streaming_thinking.clear();

        // Process a ThinkingDelta
        app.process_event_for_test(AgentEvent::ThinkingDelta {
            text: "Let me think...".into(),
        });

        assert_eq!(app.streaming_thinking, "Let me think...");
    }

    #[test]
    fn test_thinking_delta_cleared_on_turn_start() {
        let mut app = make_test_app();
        app.streaming_thinking = "old thinking".into();

        // TurnStart should clear thinking
        app.process_event_for_test(AgentEvent::TurnStart { turn: 1 });

        assert!(app.streaming_thinking.is_empty());
    }

    // ── Text streaming (live markdown) ───────────────────────────────

    #[test]
    fn test_text_delta_accumulates_in_buffer() {
        let mut app = make_test_app();
        app.process_event_for_test(AgentEvent::TurnStart { turn: 1 });
        app.process_event_for_test(AgentEvent::TextDelta {
            text: "Hello ".into(),
        });
        app.process_event_for_test(AgentEvent::TextDelta {
            text: "world".into(),
        });
        assert_eq!(app.streaming_buffer, "Hello world");
    }

    #[test]
    fn test_text_delta_renders_in_chat() {
        let mut app = make_test_app();
        app.process_event_for_test(AgentEvent::TurnStart { turn: 1 });

        // Before text, chat has "🤖 Assistant" + "…" = 2 lines
        assert!(app.chat.line_count() >= 2);

        // Send text — should replace "…" with markdown
        app.process_event_for_test(AgentEvent::TextDelta {
            text: "Hello **world**".into(),
        });

        // Chat should contain the rendered text
        let all: String = app.chat_lines_as_string();
        assert!(
            all.contains("Hello"),
            "chat should contain streamed text, got: {all}"
        );
        assert!(all.contains("world"));
    }

    #[test]
    fn test_placeholder_replaced_on_first_text() {
        let mut app = make_test_app();
        app.process_event_for_test(AgentEvent::TurnStart { turn: 1 });

        // "…" placeholder should be in chat
        let before: String = app.chat_lines_as_string();
        assert!(
            before.contains("…"),
            "should have placeholder, got: {before}"
        );

        // First text should replace the placeholder
        app.process_event_for_test(AgentEvent::TextDelta {
            text: "analysis".into(),
        });

        let after: String = app.chat_lines_as_string();
        assert!(
            !after.contains('…'),
            "placeholder should be gone, got: {after}"
        );
        assert!(after.contains("analysis"));
    }

    // ── Tool preview in chat ─────────────────────────────────────────

    #[test]
    fn test_tool_preview_appears_in_chat() {
        let mut app = make_test_app();
        app.process_event_for_test(AgentEvent::TurnStart { turn: 1 });

        // Simulate a tool call
        app.process_event_for_test(AgentEvent::ToolStart {
            id: "t1".into(),
            name: "edit".into(),
        });
        app.process_event_for_test(AgentEvent::ToolEnd {
            id: "t1".into(),
            arguments:
                r#"{"file_path":"src/main.rs","old_string":"hello","new_string":"hello world"}"#
                    .into(),
        });

        let all: String = app.chat_lines_as_string();
        assert!(all.contains("🔧"), "should show tool indicator");
        assert!(
            all.contains("src/main.rs"),
            "should show file path in preview"
        );
        assert!(all.contains("→"), "should show edit arrow in preview");
    }

    #[test]
    fn test_working_indicator_appears_on_tool_end() {
        let mut app = make_test_app();
        app.process_event_for_test(AgentEvent::TurnStart { turn: 1 });
        app.process_event_for_test(AgentEvent::ToolStart {
            id: "t2".into(),
            name: "bash".into(),
        });
        app.process_event_for_test(AgentEvent::ToolEnd {
            id: "t2".into(),
            arguments: r#"{"command":"cargo test"}"#.into(),
        });

        let all = app.chat_lines_as_string();
        assert!(all.contains("Working"), "should show working indicator");
        assert!(
            all.contains("cargo test"),
            "preview should still be visible"
        );
    }

    #[test]
    fn test_working_indicator_removed_on_result() {
        let mut app = make_test_app();
        app.process_event_for_test(AgentEvent::TurnStart { turn: 1 });
        app.process_event_for_test(AgentEvent::ToolStart {
            id: "t3".into(),
            name: "bash".into(),
        });
        app.process_event_for_test(AgentEvent::ToolEnd {
            id: "t3".into(),
            arguments: r#"{"command":"cargo build"}"#.into(),
        });
        app.process_event_for_test(AgentEvent::ToolResult {
            id: "t3".into(),
            result: ToolResult {
                success: true,
                result: None,
                images: vec![],
                ui_summary: Some("build succeeded".into()),
                ui_details: None,
                ui_details_full: None,
                file_changes: None,
            },
        });

        let all = app.chat_lines_as_string();
        assert!(!all.contains("Working"), "working indicator should be gone");
        assert!(all.contains("✓"), "should show success checkmark");
        assert!(
            all.contains("build succeeded"),
            "should show result summary"
        );
    }
}

/// Test-only helper to process a single event.
#[cfg(test)]
impl App {
    fn process_event_for_test(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStart { turn } => {
                self.current_turn = turn;
                self.streaming_buffer.clear();
                self.streaming_thinking.clear();
                self.streaming_mark = self.chat.line_count();
                self.chat
                    .add_line(format!("  🤖 Assistant (turn {turn})"), &self.styles);
                self.chat.add_styled_line("  …", self.styles.dim_text());
            }
            AgentEvent::ThinkingDelta { text, .. } => {
                self.streaming_thinking.push_str(&text);
                self.redraw_streaming();
            }
            AgentEvent::TextDelta { text } => {
                self.streaming_buffer.push_str(&text);
                self.redraw_streaming();
            }
            AgentEvent::ToolStart { id, name, .. } => {
                self.chat.add_line(format!("  🔧 {name}"), &self.styles);
                self.pending_tools.insert(id, name);
            }
            AgentEvent::ToolEnd { id, arguments, .. } => {
                if let Some(name) = self.pending_tools.remove(&id)
                    && let Some(preview) = format_tool_preview(&name, &arguments)
                {
                    self.chat
                        .add_styled_line(&preview, self.styles.thinking_text());
                    self.working_mark = Some(self.chat.line_count());
                    self.working_start = Some(Instant::now());
                    self.chat
                        .add_line("  | Working (0.0s)…".to_string(), &self.styles);
                }
            }
            AgentEvent::ToolResult { result, .. } => {
                if let Some(mark) = self.working_mark.take() {
                    self.chat.truncate_to(mark);
                }
                let block = render_tool_result("", "tool", &result, &self.styles);
                self.chat.add_block(block);
            }
            _ => {}
        }
    }

    /// Collect all chat line content into a single string for assertions.
    fn chat_lines_as_string(&self) -> String {
        self.chat.all_text()
    }
}
