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
    let api_key = cli.api_key.unwrap_or_default();
    let mut app = build_app(config.clone(), cli.continue_session, api_key).await?;

    // Main event loop
    let result = run_event_loop(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result?;
    Ok(())
}

async fn build_app(
    config: Arc<RwLock<Config>>,
    continue_session: bool,
    cli_api_key: String,
) -> anyhow::Result<App> {
    let (provider, tools) = {
        let cfg = config.read();
        let provider_config = cfg.llm.to_provider_config(&cli_api_key);
        let provider = create_provider(&provider_config).unwrap_or_else(|e| {
            tracing::warn!("{e} — using mock provider");
            create_provider(&ProviderConfig::mock()).unwrap()
        });
        let tools = tools_mod::core_tools();
        (provider, tools)
    };

    let session = if continue_session {
        match crate::session::most_recent_session() {
            Some(path) => {
                tracing::info!("Resuming session: {}", path.display());
                Some(crate::session::Session::load(path).await?)
            }
            None => {
                tracing::info!("No previous session found — starting fresh");
                None
            }
        }
    } else {
        None
    };

    let mut app = App::new(config, provider, tools, session, cli_api_key);
    // If a session was restored, render its history into the chat
    app.render_session_history();
    Ok(app)
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    let mut iteration: u64 = 0;
    let mut last_log = std::time::Instant::now();
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

        iteration += 1;
        if last_log.elapsed().as_millis() > 1000 {
            tracing::info!(
                "TUI loop: {} iters/sec, agent_running={}",
                iteration,
                app.agent_running
            );
            last_log = std::time::Instant::now();
            iteration = 0;
        }
    }

    Ok(())
}
