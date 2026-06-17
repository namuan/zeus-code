use clap::Parser;

/// Zeus — Minimal coding agent harness.
#[derive(Parser)]
#[command(
    name = "zeus-code",
    version,
    about = "Zeus — Minimal coding agent harness"
)]
pub struct Cli {
    /// Model to use (overrides config)
    #[arg(short = 'm', long)]
    pub model: Option<String>,

    /// Provider to use (e.g., openai, openrouter, deepseek)
    #[arg(long)]
    pub provider: Option<String>,

    /// Run a single prompt non-interactively, then exit
    #[arg(short = 'p', long, num_args = 0..=1)]
    pub prompt: Option<Option<String>>,

    /// API key (overrides env vars and config)
    #[arg(short = 'k', long)]
    pub api_key: Option<String>,

    /// Base URL for API (overrides config)
    #[arg(short = 'u', long)]
    pub base_url: Option<String>,

    /// Resume the most recent session
    #[arg(short = 'c', long)]
    pub continue_session: bool,

    /// Resume a specific session by ID prefix
    #[arg(short = 'r', long)]
    pub resume: Option<String>,

    /// Comma-separated extra tools to enable
    #[arg(long, value_delimiter = ',')]
    pub extra_tools: Vec<String>,

    /// Skip TLS verification
    #[arg(long)]
    pub insecure_skip_verify: bool,
}

pub fn parse() -> Cli {
    Cli::parse()
}
