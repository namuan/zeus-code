//! TUI entrypoint — terminal setup, app creation, event loop.

use std::io;
use std::sync::Arc;

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use parking_lot::RwLock;
use ratatui::prelude::*;

use crate::cli::Cli;
use crate::config::Config;
use crate::llm::providers::{ProviderConfig, create_provider};
use crate::tools as tools_mod;
use crate::ui::app::App;

/// Launch the TUI.
pub async fn run_tui(cli: Cli) -> anyhow::Result<()> {
    // Load config
    let mut config = Config::load_or_create();
    config.merge_cli_overrides(&cli);
    let config = Arc::new(RwLock::new(config));

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Build the app
    let mut app = build_app(config.clone()).await?;

    // Main event loop
    let result = run_event_loop(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result?;
    Ok(())
}

async fn build_app(config: Arc<RwLock<Config>>) -> anyhow::Result<App> {
    let cfg = config.read();

    // Create provider (fall back to mock)
    let provider_config =
        ProviderConfig::new(&cfg.llm.default_provider, &cfg.llm.default_model, "");
    let provider = create_provider(&provider_config).unwrap_or_else(|e| {
        tracing::warn!("{e} — using mock provider");
        create_provider(&ProviderConfig::new("mock", "mock", "")).unwrap()
    });

    let tools = tools_mod::core_tools();

    drop(cfg);

    Ok(App::new(config, provider, tools))
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    loop {
        // Process agent events
        app.process_events();

        // Draw
        terminal.draw(|f| app.render(f))?;

        if app.should_quit {
            break;
        }

        // Poll for input with a short timeout (non-blocking event loop)
        if event::poll(std::time::Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    app.handle_key(key);
                }
                Event::Resize(_, _) => {
                    // Terminal was resized — will be handled on next draw
                }
                _ => {}
            }
        }
    }

    Ok(())
}
