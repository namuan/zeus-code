//! Mock provider for testing. Returns canned streaming responses.
//!
//! The mock provider supports different response modes controlled by the
//! model ID suffix:
//! - `"mock"` → simple text response
//! - `"mock:thinking"` → response with thinking block
//! - `"mock:tool"` → response with a tool call
//! - `"mock:multi-tool"` → response with multiple tool calls
//! - `"mock:error"` → returns an error
//! - `"mock:empty"` → empty response (no content)

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use tokio::sync::watch;

use crate::core::errors::{KonError, KonResult};
use crate::core::types::{Message, StopReason, StreamPart, ToolDefinition, Usage};
use crate::llm::base::{LLMStream, Provider, ThinkingLevel};

/// A mock provider that returns canned responses for testing.
pub struct MockProvider {
    model_id: String,
    thinking_level: ThinkingLevel,
}

impl MockProvider {
    /// Create a new mock provider with the given model ID.
    /// The model ID can include a colon-separated mode suffix.
    pub fn new(model_id: String) -> Self {
        Self {
            model_id,
            thinking_level: ThinkingLevel::None,
        }
    }

    /// Extract the response mode from the model ID.
    fn mode(&self) -> &str {
        if let Some((_, mode)) = self.model_id.split_once(':') {
            mode
        } else {
            "default"
        }
    }

    /// Build a canned text response stream.
    fn text_response(
        text: &'static str,
    ) -> Pin<Box<dyn Stream<Item = KonResult<StreamPart>> + Send>> {
        let parts: Vec<KonResult<StreamPart>> = vec![
            Ok(StreamPart::TextDelta {
                text: text.to_string(),
            }),
            Ok(StreamPart::StreamDone {
                stop_reason: StopReason::Stop,
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                },
            }),
        ];
        Box::pin(futures::stream::iter(parts))
    }

    /// Build a thinking + text response stream.
    fn thinking_response() -> Pin<Box<dyn Stream<Item = KonResult<StreamPart>> + Send>> {
        let parts: Vec<KonResult<StreamPart>> = vec![
            Ok(StreamPart::ThinkingDelta {
                thinking: "Let me think about this carefully...".into(),
                level: Some("low".into()),
            }),
            Ok(StreamPart::TextDelta {
                text: "Here is my analysis: the code looks correct.".into(),
            }),
            Ok(StreamPart::StreamDone {
                stop_reason: StopReason::Stop,
                usage: Usage {
                    input_tokens: 20,
                    output_tokens: 15,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                },
            }),
        ];
        Box::pin(futures::stream::iter(parts))
    }

    /// Build a response that includes a tool call.
    fn tool_response() -> Pin<Box<dyn Stream<Item = KonResult<StreamPart>> + Send>> {
        let parts: Vec<KonResult<StreamPart>> = vec![
            Ok(StreamPart::TextDelta {
                text: "I'll read that file for you.".into(),
            }),
            Ok(StreamPart::ToolCallStart {
                id: "mock_call_1".into(),
                name: "read".into(),
            }),
            Ok(StreamPart::ToolCallDelta {
                id: "mock_call_1".into(),
                arguments_delta: r#"{"file_path":"src/main.rs"}"#.into(),
            }),
            Ok(StreamPart::StreamDone {
                stop_reason: StopReason::ToolUse,
                usage: Usage {
                    input_tokens: 30,
                    output_tokens: 25,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                },
            }),
        ];
        Box::pin(futures::stream::iter(parts))
    }

    /// Build a multi-tool response.
    fn multi_tool_response() -> Pin<Box<dyn Stream<Item = KonResult<StreamPart>> + Send>> {
        let parts: Vec<KonResult<StreamPart>> = vec![
            Ok(StreamPart::TextDelta {
                text: "Let me check two things.".into(),
            }),
            Ok(StreamPart::ToolCallStart {
                id: "mock_call_1".into(),
                name: "read".into(),
            }),
            Ok(StreamPart::ToolCallDelta {
                id: "mock_call_1".into(),
                arguments_delta: r#"{"file_path":"src/main.rs"}"#.into(),
            }),
            Ok(StreamPart::ToolCallStart {
                id: "mock_call_2".into(),
                name: "grep".into(),
            }),
            Ok(StreamPart::ToolCallDelta {
                id: "mock_call_2".into(),
                arguments_delta: r#"{"pattern":"fn main"}"#.into(),
            }),
            Ok(StreamPart::StreamDone {
                stop_reason: StopReason::ToolUse,
                usage: Usage {
                    input_tokens: 40,
                    output_tokens: 35,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                },
            }),
        ];
        Box::pin(futures::stream::iter(parts))
    }

    /// Build an empty response.
    fn empty_response() -> Pin<Box<dyn Stream<Item = KonResult<StreamPart>> + Send>> {
        let parts: Vec<KonResult<StreamPart>> = vec![Ok(StreamPart::StreamDone {
            stop_reason: StopReason::Stop,
            usage: Usage {
                input_tokens: 5,
                output_tokens: 0,
                cache_read_tokens: None,
                cache_write_tokens: None,
            },
        })];
        Box::pin(futures::stream::iter(parts))
    }

    /// Build an error response.
    fn error_response() -> Pin<Box<dyn Stream<Item = KonResult<StreamPart>> + Send>> {
        let parts: Vec<KonResult<StreamPart>> =
            vec![Err(KonError::Provider("mock error for testing".into()))];
        Box::pin(futures::stream::iter(parts))
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn stream(
        &self,
        _messages: Vec<Message>,
        _system_prompt: Option<String>,
        _tools: Vec<ToolDefinition>,
        _cancel: watch::Receiver<bool>,
    ) -> KonResult<LLMStream> {
        let inner: Pin<Box<dyn Stream<Item = KonResult<StreamPart>> + Send>> = match self.mode() {
            "thinking" => Self::thinking_response(),
            "tool" => Self::tool_response(),
            "multi-tool" => Self::multi_tool_response(),
            "error" => Self::error_response(),
            "empty" => Self::empty_response(),
            _ => Self::text_response("Mock response: everything looks good!"),
        };

        Ok(LLMStream::new(inner))
    }

    fn should_retry_for_error(&self, _error: &KonError) -> bool {
        false
    }

    fn thinking_levels(&self) -> &[ThinkingLevel] {
        &[ThinkingLevel::None]
    }

    fn set_thinking_level(&mut self, level: ThinkingLevel) {
        self.thinking_level = level;
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn model(&self) -> &str {
        &self.model_id
    }
}

// ── Helper: an adapter that gives Stream impl to futures::stream::Iter ──

// futures::stream::Iter already implements Stream,
// but we need the Unpin bound satisfied for Pin<Box<dyn Stream>>.
// The items are already collected into a Vec, so the iter is Unpin.

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use tokio::sync::watch;

    async fn collect_stream(mut stream: LLMStream) -> Vec<KonResult<StreamPart>> {
        let mut parts = Vec::new();
        while let Some(part) = stream.inner.next().await {
            parts.push(part);
        }
        parts
    }

    #[tokio::test]
    async fn test_mock_default_response() {
        let provider = MockProvider::new("mock".into());
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let stream = provider
            .stream(vec![], None, vec![], cancel_rx)
            .await
            .unwrap();

        let parts = collect_stream(stream).await;
        assert_eq!(parts.len(), 2);

        // First part should be text
        match &parts[0] {
            Ok(StreamPart::TextDelta { text }) => {
                assert!(text.contains("Mock response"));
            }
            other => panic!("expected TextDelta, got {other:?}"),
        }

        // Last part should be StreamDone
        match &parts[1] {
            Ok(StreamPart::StreamDone { stop_reason, usage }) => {
                assert_eq!(*stop_reason, StopReason::Stop);
                assert_eq!(usage.input_tokens, 10);
            }
            other => panic!("expected StreamDone, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_mock_thinking_response() {
        let provider = MockProvider::new("mock:thinking".into());
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let stream = provider
            .stream(vec![], None, vec![], cancel_rx)
            .await
            .unwrap();

        let parts = collect_stream(stream).await;
        assert_eq!(parts.len(), 3);

        // First should be thinking
        assert!(matches!(&parts[0], Ok(StreamPart::ThinkingDelta { .. })));
    }

    #[tokio::test]
    async fn test_mock_tool_response() {
        let provider = MockProvider::new("mock:tool".into());
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let stream = provider
            .stream(vec![], None, vec![], cancel_rx)
            .await
            .unwrap();

        let parts = collect_stream(stream).await;
        assert_eq!(parts.len(), 4);

        // Should have a tool call start
        assert!(matches!(
            &parts[1],
            Ok(StreamPart::ToolCallStart { name, .. }) if name == "read"
        ));

        // Should end with ToolUse stop reason
        match &parts[3] {
            Ok(StreamPart::StreamDone { stop_reason, .. }) => {
                assert_eq!(*stop_reason, StopReason::ToolUse);
            }
            other => panic!("expected StreamDone with ToolUse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_mock_multi_tool_response() {
        let provider = MockProvider::new("mock:multi-tool".into());
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let stream = provider
            .stream(vec![], None, vec![], cancel_rx)
            .await
            .unwrap();

        let parts = collect_stream(stream).await;
        assert_eq!(parts.len(), 6);

        // Check both tool calls are present
        let tool_names: Vec<&str> = parts
            .iter()
            .filter_map(|p| match p {
                Ok(StreamPart::ToolCallStart { name, .. }) => Some(name.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(tool_names, vec!["read", "grep"]);
    }

    #[tokio::test]
    async fn test_mock_error_response() {
        let provider = MockProvider::new("mock:error".into());
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let stream = provider
            .stream(vec![], None, vec![], cancel_rx)
            .await
            .unwrap();

        let parts = collect_stream(stream).await;
        assert_eq!(parts.len(), 1);
        assert!(parts[0].is_err());
    }

    #[tokio::test]
    async fn test_mock_empty_response() {
        let provider = MockProvider::new("mock:empty".into());
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let stream = provider
            .stream(vec![], None, vec![], cancel_rx)
            .await
            .unwrap();

        let parts = collect_stream(stream).await;
        assert_eq!(parts.len(), 1);
        assert!(matches!(&parts[0], Ok(StreamPart::StreamDone { .. })));
    }

    #[test]
    fn test_mock_provider_metadata() {
        let provider = MockProvider::new("mock".into());
        assert_eq!(provider.name(), "mock");
        assert_eq!(provider.model(), "mock");
        assert!(!provider.should_retry_for_error(&KonError::Provider("test".into())));
        assert_eq!(provider.thinking_levels(), &[ThinkingLevel::None]);
    }
}
