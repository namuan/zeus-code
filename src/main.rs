use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use zeus_code::cli;
use zeus_code::headless;
use zeus_code::logging::RollingWriter;

#[tokio::main]
async fn main() {
    // Parse CLI first — we need to know TUI vs headless
    let cli = cli::parse();
    let is_tui = cli.prompt.is_none();

    // Build the env filter
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    // File layer with size-rolling (always active)
    let file_writer = RollingWriter::new().expect("failed to open log file");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_writer);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(non_blocking);

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer);

    // Console layer only in headless mode (TUI owns the terminal)
    if is_tui {
        registry.init();
    } else {
        let console_layer = tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_writer(std::io::stderr);
        registry.with(console_layer).init();
    }

    // Keep the non-blocking writer alive
    std::mem::forget(_guard);

    match cli.prompt.clone() {
        Some(prompt_value) => {
            let exit_code = headless::run_headless(cli, prompt_value).await;
            std::process::exit(exit_code);
        }
        None => {
            // TUI mode
            if let Err(e) = zeus_code::ui::launch::run_tui(cli).await {
                eprintln!("zeus: {e}");
                std::process::exit(1);
            }
        }
    }
}
