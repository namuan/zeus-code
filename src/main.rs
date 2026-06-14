use zeus_code::cli;
use zeus_code::headless;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = cli::parse();

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
