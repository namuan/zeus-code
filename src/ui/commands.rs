//! Slash command handler.

/// Result of parsing a slash command from user input.
#[derive(Debug)]
pub enum Command {
    New,
    Resume,
    Model(String),
    Compact,
    Help,
    Quit,
    Themes,
    Thinking(String),
    Export,
    Clear,
    Permissions,
    Notifications,
}

/// Parse a slash command from user input. Returns None if not a command.
pub fn parse_command(input: &str) -> Option<Command> {
    let input = input.trim();
    if !input.starts_with('/') {
        return None;
    }

    let parts: Vec<&str> = input.splitn(2, ' ').collect();
    let cmd = parts[0];
    let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");

    match cmd {
        "/new" => Some(Command::New),
        "/resume" => Some(Command::Resume),
        "/model" => Some(Command::Model(arg.to_string())),
        "/compact" => Some(Command::Compact),
        "/help" => Some(Command::Help),
        "/quit" | "/exit" => Some(Command::Quit),
        "/themes" => Some(Command::Themes),
        "/thinking" => Some(Command::Thinking(arg.to_string())),
        "/export" => Some(Command::Export),
        "/clear" => Some(Command::Clear),
        "/permissions" => Some(Command::Permissions),
        "/notifications" => Some(Command::Notifications),
        _ => None,
    }
}

/// Help text for available commands.
pub fn help_text() -> &'static str {
    "\
Available commands:
  /new          Start a new conversation
  /resume       Resume a previous session
  /model <id>   Switch model
  /compact      Compact conversation context
  /themes       Switch color theme
  /thinking     Change thinking level
  /permissions  Toggle permission mode
  /export       Export session to HTML
  /clear        Clear the screen
  /help         Show this help
  /quit         Exit Zeus

Key bindings:
  Enter         Submit input
  Esc           Cancel / close
  Ctrl+C        Quit
  Ctrl+T        Cycle thinking level
  Ctrl+O        Toggle tool output
  Tab           Autocomplete
"
}
