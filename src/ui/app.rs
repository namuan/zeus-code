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
    pub should_quit: bool,
    pub agent_running: bool,
    pub current_turn: u64,
    pub total_tokens: u64,

    /// Line count in chat when the current streaming turn began.
    /// Used to trim streaming text and replace with formatted blocks on TurnEnd.
    streaming_mark: usize,

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
            should_quit: false,
            agent_running: false,
            current_turn: 0,
            total_tokens: 0,
            streaming_mark: 0,
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
                if text.is_empty() {
                    return;
                }

                // Check for slash commands
                if let Some(cmd) = parse_command(&text) {
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
                self.run_agent(&text);
            }

            // Editing keys
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::Left => self.input.cursor_left(),
            KeyCode::Right => self.input.cursor_right(),
            KeyCode::Home => self.input.cursor_home(),
            KeyCode::End => self.input.cursor_end(),
            KeyCode::Up => self.input.history_prev(),
            KeyCode::Down => self.input.history_next(),
            KeyCode::Char(c) => self.input.insert_char(c),

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
        let query = text.to_string();

        // Build new channels for this run
        let event_tx = self.event_tx.clone();
        let cancel_rx = self.cancel_rx.clone();

        // Clone what the agent task needs
        let mut agent = self.build_agent();

        // Spawn agent
        let handle = tokio::spawn(async move { agent.run(query, None, event_tx, cancel_rx).await });
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
        let sp = cfg.llm.system_prompt.content.clone();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        drop(cfg);

        let session = Session::new_sync(cwd, sp, vec![])
            .unwrap_or_else(|e| panic!("failed to create session: {e}"));

        Agent::new(self.config.clone(), provider, session)
    }

    /// Process agent events from the channel.
    pub fn process_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                AgentEvent::TurnStart { turn } => {
                    self.current_turn = turn;
                    // Record current line count so we can trim streaming text later
                    self.streaming_mark = self.chat.line_count();
                    self.chat
                        .add_line(format!("  🤖 Assistant (turn {turn})"), &self.styles);
                }
                AgentEvent::ThinkingDelta { .. } => {}
                AgentEvent::TextDelta { text } => {
                    self.chat.append_text(&text, &self.styles);
                }
                AgentEvent::ToolStart { name, .. } => {
                    self.chat.add_line(format!("  🔧 {name}"), &self.styles);
                }
                AgentEvent::ToolResult { result, .. } => {
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
                    self.chat
                        .add_line(format!("  ✗ Error: {error}"), &self.styles);
                }
                _ => {}
            }
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
            self.current_turn,
            &self.styles,
        );

        // Input box
        self.input.render(f, main_layout[2], &self.styles);
    }
}
