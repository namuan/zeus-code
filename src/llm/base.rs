use std::pin::Pin;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio_stream::Stream;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::{Message, StreamPart, ToolDefinition, Usage};

/// Thinking levels supported across providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThinkingLevel {
    #[default]
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

/// The Provider trait abstracts all LLM API differences behind a unified streaming interface.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stream a chat completion request, yielding typed stream parts.
    async fn stream(
        &self,
        messages: Vec<Message>,
        system_prompt: Option<String>,
        tools: Vec<ToolDefinition>,
        cancel: watch::Receiver<bool>,
    ) -> KonResult<LLMStream>;

    /// Whether to retry the request on this error.
    fn should_retry_for_error(&self, error: &KonError) -> bool;

    /// Valid thinking levels for this provider.
    fn thinking_levels(&self) -> &[ThinkingLevel];

    /// Set the thinking level.
    fn set_thinking_level(&mut self, level: ThinkingLevel);

    /// Provider name string.
    fn name(&self) -> &str;

    /// Model ID.
    fn model(&self) -> &str;
}

/// Wraps a stream of parts, captures final usage + id after completion.
pub struct LLMStream {
    pub inner: Pin<Box<dyn Stream<Item = KonResult<StreamPart>> + Send>>,
    pub usage: Option<Usage>,
    pub stream_id: Option<String>,
    #[allow(dead_code)]
    finished: bool,
}

impl LLMStream {
    pub fn new(stream: Pin<Box<dyn Stream<Item = KonResult<StreamPart>> + Send>>) -> Self {
        Self {
            inner: stream,
            usage: None,
            stream_id: None,
            finished: false,
        }
    }
}
