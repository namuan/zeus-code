use thiserror::Error;

#[derive(Error, Debug)]
pub enum KonError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Provider error: {0}")]
    Provider(String),
    #[error("Provider error (retryable): {0}")]
    ProviderRetryable(String),
    #[error("Tool error ({tool}): {message}")]
    Tool { tool: String, message: String },
    #[error("Session error: {0}")]
    Session(String),
    #[error("Operation cancelled")]
    Cancelled,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("{0}")]
    Other(String),
}

pub type KonResult<T> = Result<T, KonError>;
