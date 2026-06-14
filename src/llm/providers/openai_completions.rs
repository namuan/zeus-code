//! OpenAI Chat Completions API provider.
//!
//! Handles SSE streaming, reasoning/thinking content extraction,
//! tool call parsing, and retry logic. Also serves as the base for
//! OpenAI-compatible providers (DeepSeek, ZhiPu, local endpoints).

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::core::errors::{KonError, KonResult};
use crate::core::types::{ContentBlock, Message, StopReason, StreamPart, ToolDefinition, Usage};
use crate::llm::base::{LLMStream, Provider, ThinkingLevel};
use crate::llm::providers::ProviderConfig;

// ── OpenAI request/response types ────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingOption>,
}

#[derive(Debug, Serialize)]
struct ThinkingOption {
    #[serde(rename = "type")]
    thinking_type: String,
    level: String,
}

#[derive(Debug, Serialize)]
struct ToolDef {
    #[serde(rename = "type")]
    tool_type: String,
    function: FunctionDef,
}

#[derive(Debug, Serialize)]
struct FunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCallMsg>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ToolCallMsg {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: FunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
struct FunctionCall {
    name: String,
    arguments: String,
}

// SSE chunk types
#[derive(Debug, Deserialize)]
struct ChatChunk {
    choices: Vec<ChoiceDelta>,
    #[serde(default)]
    usage: Option<ChunkUsage>,
}

#[derive(Debug, Deserialize)]
struct ChoiceDelta {
    delta: DeltaContent,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DeltaContent {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    function: Option<FunctionArgDelta>,
}

#[derive(Debug, Deserialize)]
struct FunctionArgDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ChunkUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[serde(default)]
    completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct CompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: u64,
}

// ── Provider implementation ──────────────────────────────────────────────

pub struct OpenAICompletionsProvider {
    client: Client,
    config: ProviderConfig,
    thinking_level: ThinkingLevel,
    base_url: String,
}

impl OpenAICompletionsProvider {
    pub fn new(mut config: ProviderConfig) -> KonResult<Self> {
        // Auto-detect API key from environment variables
        if config.api_key.is_empty() {
            config.api_key = detect_api_key(&config.provider_name);
        }

        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| default_base_url(&config.provider_name));

        let mut client_builder =
            Client::builder().timeout(std::time::Duration::from_secs(config.request_timeout_secs));

        if config.insecure_skip_verify {
            client_builder = client_builder.danger_accept_invalid_certs(true);
        }

        let client = client_builder
            .build()
            .map_err(|e| KonError::Provider(format!("failed to build HTTP client: {e}")))?;

        Ok(Self {
            client,
            config,
            thinking_level: ThinkingLevel::None,
            base_url,
        })
    }

    /// Convert canonical Messages to OpenAI chat format.
    fn build_messages(
        messages: &[Message],
        system_prompt: Option<&str>,
    ) -> KonResult<Vec<ChatMessage>> {
        let mut chat_messages = Vec::new();

        // System prompt goes first if present
        if let Some(sp) = system_prompt
            && !sp.is_empty()
        {
            chat_messages.push(ChatMessage {
                role: "system".into(),
                content: Some(serde_json::Value::String(sp.to_string())),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
        }

        for msg in messages {
            match msg {
                Message::User(user) => {
                    let content = Self::build_content(&user.content)?;
                    chat_messages.push(ChatMessage {
                        role: "user".into(),
                        content: Some(content),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                }
                Message::Assistant(assistant) => {
                    let mut tool_calls: Vec<ToolCallMsg> = Vec::new();
                    let mut text_parts: Vec<serde_json::Value> = Vec::new();

                    for block in &assistant.content {
                        match block {
                            ContentBlock::Text { text } => {
                                text_parts.push(serde_json::json!({
                                    "type": "text",
                                    "text": text
                                }));
                            }
                            ContentBlock::Thinking { thinking, .. } => {
                                // OpenAI expects thinking as content with a type
                                text_parts.push(serde_json::json!({
                                    "type": "thinking",
                                    "thinking": thinking
                                }));
                            }
                            ContentBlock::ToolCall {
                                id,
                                name,
                                arguments,
                            } => {
                                tool_calls.push(ToolCallMsg {
                                    id: id.clone(),
                                    call_type: "function".into(),
                                    function: FunctionCall {
                                        name: name.clone(),
                                        arguments: arguments.clone(),
                                    },
                                });
                            }
                            ContentBlock::Image(_) => {
                                // Images in assistant messages are unusual; skip
                            }
                        }
                    }

                    let content = if text_parts.is_empty() && !tool_calls.is_empty() {
                        // Tool-call-only message: content must be null per OpenAI spec
                        None
                    } else if text_parts.len() == 1
                        && text_parts[0].get("type").is_some_and(|t| t == "text")
                    {
                        // Single text block → send as plain string for compat
                        Some(text_parts[0]["text"].clone())
                    } else {
                        Some(serde_json::Value::Array(text_parts))
                    };

                    let tool_calls_opt = if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    };

                    chat_messages.push(ChatMessage {
                        role: "assistant".into(),
                        content,
                        tool_calls: tool_calls_opt,
                        tool_call_id: None,
                        name: None,
                    });
                }
                Message::ToolResult(tr) => {
                    chat_messages.push(ChatMessage {
                        role: "tool".into(),
                        content: Some(serde_json::Value::String(tr.content.clone())),
                        tool_calls: None,
                        tool_call_id: Some(tr.tool_call_id.clone()),
                        name: Some(tr.tool_name.clone()),
                    });
                }
                Message::System(sys) => {
                    chat_messages.push(ChatMessage {
                        role: "system".into(),
                        content: Some(serde_json::Value::String(sys.content.clone())),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                }
            }
        }

        Ok(chat_messages)
    }

    /// Build content array for user messages (text + images).
    fn build_content(blocks: &[ContentBlock]) -> KonResult<serde_json::Value> {
        if blocks.is_empty() {
            return Ok(serde_json::Value::String(String::new()));
        }

        if blocks.len() == 1
            && let ContentBlock::Text { text } = &blocks[0]
        {
            return Ok(serde_json::Value::String(text.clone()));
        }

        let parts: Vec<serde_json::Value> = blocks
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => {
                    serde_json::json!({"type": "text", "text": text})
                }
                ContentBlock::Image(img) => {
                    serde_json::json!({
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}", img.media_type, img.base64)
                        }
                    })
                }
                _ => serde_json::json!({"type": "text", "text": ""}),
            })
            .collect();

        Ok(serde_json::Value::Array(parts))
    }

    /// Build tool definitions for the request.
    fn build_tools(tools: &[ToolDefinition]) -> Option<Vec<ToolDef>> {
        if tools.is_empty() {
            return None;
        }
        Some(
            tools
                .iter()
                .map(|t| ToolDef {
                    tool_type: "function".into(),
                    function: FunctionDef {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.parameters.clone(),
                    },
                })
                .collect(),
        )
    }

    /// Build the full request payload.
    fn build_request(
        &self,
        messages: &[Message],
        system_prompt: Option<&str>,
        tools: &[ToolDefinition],
    ) -> KonResult<ChatRequest> {
        let chat_messages = Self::build_messages(messages, system_prompt)?;
        let tool_defs = Self::build_tools(tools);
        let has_tools = tool_defs.is_some();

        let thinking = if self.thinking_level != ThinkingLevel::None {
            Some(ThinkingOption {
                thinking_type: "enabled".into(),
                level: level_to_string(self.thinking_level),
            })
        } else {
            None
        };

        Ok(ChatRequest {
            model: self.config.model_id.clone(),
            messages: chat_messages,
            stream: true,
            tools: tool_defs,
            tool_choice: if has_tools { Some("auto".into()) } else { None },
            max_tokens: self.config.max_tokens,
            thinking,
        })
    }

    /// Send the request and return a stream of bytes.
    async fn send_request(&self, request: &ChatRequest) -> KonResult<reqwest::Response> {
        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        let mut req = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(request);

        // OpenRouter headers (optional but recommended)
        if self.config.provider_name == "openrouter" {
            req = req
                .header("HTTP-Referer", "https://github.com/0xku/zeus-code")
                .header("X-Title", "Zeus-Code");
        }

        // OpenAI-compatible providers may use different auth schemes.
        // For endpoints that need a placeholder key, inject a dummy one.
        if self.config.api_key.is_empty() {
            req = req.header("Authorization", "Bearer sk-placeholder");
        }

        let response = req.send().await.map_err(|e| {
            if e.is_timeout() {
                KonError::ProviderRetryable(format!("request timed out: {e}"))
            } else if e.is_connect() {
                KonError::ProviderRetryable(format!("connection failed: {e}"))
            } else {
                KonError::Provider(format!("HTTP request failed: {e}"))
            }
        })?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            let body = response.text().await.unwrap_or_default();
            return Err(KonError::ProviderRetryable(format!(
                "server error {status}: {body}"
            )));
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(KonError::Provider(format!("API error {status}: {body}")));
        }

        Ok(response)
    }
}

fn level_to_string(level: ThinkingLevel) -> String {
    match level {
        ThinkingLevel::None => "none",
        ThinkingLevel::Minimal => "minimal",
        ThinkingLevel::Low => "low",
        ThinkingLevel::Medium => "medium",
        ThinkingLevel::High => "high",
        ThinkingLevel::XHigh => "xhigh",
    }
    .into()
}

/// Auto-detect API key from environment variables.
fn detect_api_key(provider_name: &str) -> String {
    let env_vars: &[&str] = match provider_name {
        "openrouter" => &["OPENROUTER_API_KEY", "OPENAI_API_KEY"],
        "deepseek" => &["DEEPSEEK_API_KEY", "OPENAI_API_KEY"],
        "zhipu" => &["ZHIPU_API_KEY", "OPENAI_API_KEY"],
        "github-copilot" => &["GITHUB_TOKEN", "OPENAI_API_KEY"],
        _ => &["OPENAI_API_KEY"],
    };

    for var in env_vars {
        if let Ok(key) = std::env::var(var)
            && !key.is_empty()
        {
            return key;
        }
    }

    String::new()
}

/// Default base URL per provider.
fn default_base_url(provider_name: &str) -> String {
    match provider_name {
        "openrouter" => "https://openrouter.ai/api".into(),
        "deepseek" => "https://api.deepseek.com".into(),
        "zhipu" => "https://open.bigmodel.cn/api/paas/v4".into(),
        _ => "https://api.openai.com".into(),
    }
}

#[async_trait]
impl Provider for OpenAICompletionsProvider {
    async fn stream(
        &self,
        messages: Vec<Message>,
        system_prompt: Option<String>,
        tools: Vec<ToolDefinition>,
        _cancel: watch::Receiver<bool>,
    ) -> KonResult<LLMStream> {
        let request = self.build_request(&messages, system_prompt.as_deref(), &tools)?;
        tracing::info!(
            "Sending request to {}/v1/chat/completions with model={}",
            self.base_url.trim_end_matches('/'),
            self.config.model_id
        );

        let response = self.send_request(&request).await?;
        tracing::info!("Got response status: {}", response.status());

        // Spawn a task to read the SSE stream and send parts through a channel
        let (tx, rx) = mpsc::channel::<KonResult<StreamPart>>(32);

        tokio::spawn(async move {
            let result = process_sse_stream(response, tx.clone()).await;
            if let Err(e) = result {
                let _ = tx.send(Err(e)).await;
            }
        });

        // Convert the mpsc receiver into a Stream
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        let inner: Pin<Box<dyn Stream<Item = KonResult<StreamPart>> + Send>> = Box::pin(stream);

        Ok(LLMStream::new(inner))
    }

    fn should_retry_for_error(&self, error: &KonError) -> bool {
        matches!(error, KonError::ProviderRetryable(_))
    }

    fn thinking_levels(&self) -> &[ThinkingLevel] {
        &[
            ThinkingLevel::None,
            ThinkingLevel::Minimal,
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
        ]
    }

    fn set_thinking_level(&mut self, level: ThinkingLevel) {
        self.thinking_level = level;
    }

    fn name(&self) -> &str {
        &self.config.provider_name
    }

    fn model(&self) -> &str {
        &self.config.model_id
    }
}

// ── SSE stream processing ────────────────────────────────────────────────

/// Read the SSE response body and emit StreamParts through the sender.
async fn process_sse_stream(
    response: reqwest::Response,
    tx: mpsc::Sender<KonResult<StreamPart>>,
) -> KonResult<()> {
    use futures::StreamExt;

    let mut byte_stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut total_usage = Usage::default();
    let mut tool_buffers: std::collections::HashMap<usize, ToolCallBuffer> =
        std::collections::HashMap::new();
    let mut had_tool_calls = false;

    tracing::info!("SSE stream connected, waiting for chunks...");

    while let Some(chunk) = byte_stream.next().await {
        tracing::debug!(
            "SSE chunk: {} bytes",
            chunk.as_ref().map(|c| c.len()).unwrap_or(0)
        );
        let chunk = chunk.map_err(|e| KonError::Provider(format!("stream read error: {e}")))?;
        let text = String::from_utf8_lossy(&chunk);
        buffer.push_str(&text);

        // Process complete lines
        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            if line == "data: [DONE]" {
                let stop_reason = if had_tool_calls {
                    StopReason::ToolUse
                } else {
                    StopReason::Stop
                };
                let _ = tx
                    .send(Ok(StreamPart::StreamDone {
                        stop_reason,
                        usage: total_usage,
                    }))
                    .await;
                return Ok(());
            }

            if let Some(data) = line.strip_prefix("data: ") {
                match serde_json::from_str::<ChatChunk>(data) {
                    Ok(chunk) => {
                        process_chunk(
                            chunk,
                            &mut total_usage,
                            &mut tool_buffers,
                            &mut had_tool_calls,
                            &tx,
                        )
                        .await?;
                    }
                    Err(e) => {
                        // Some chunks may not parse (e.g., keep-alive pings)
                        tracing::debug!("unparseable SSE chunk: {e}");
                    }
                }
            }
        }
    }

    // Stream ended without [DONE] marker
    let stop_reason = if had_tool_calls {
        StopReason::ToolUse
    } else {
        StopReason::Stop
    };
    let _ = tx
        .send(Ok(StreamPart::StreamDone {
            stop_reason,
            usage: total_usage,
        }))
        .await;

    Ok(())
}

/// State for buffering a tool call across multiple SSE chunks.
#[derive(Default)]
struct ToolCallBuffer {
    id: String,
    name: Option<String>,
    started: bool,
}

/// Process a single SSE chunk into StreamPart events.
async fn process_chunk(
    chunk: ChatChunk,
    total_usage: &mut Usage,
    tool_buffers: &mut std::collections::HashMap<usize, ToolCallBuffer>,
    had_tool_calls: &mut bool,
    tx: &mpsc::Sender<KonResult<StreamPart>>,
) -> KonResult<()> {
    // Accumulate usage if present
    if let Some(ref usage) = chunk.usage {
        total_usage.input_tokens += usage.prompt_tokens;
        total_usage.output_tokens += usage.completion_tokens;
        if let Some(ref details) = usage.completion_tokens_details {
            total_usage.cache_read_tokens =
                Some(total_usage.cache_read_tokens.unwrap_or(0) + details.reasoning_tokens);
        }
    }

    for choice in &chunk.choices {
        let delta = &choice.delta;

        // Reasoning / thinking content
        if let Some(ref reasoning) = delta.reasoning_content
            && !reasoning.is_empty()
        {
            tx.send(Ok(StreamPart::ThinkingDelta {
                thinking: reasoning.clone(),
                level: None,
            }))
            .await
            .map_err(|_| KonError::Provider("event channel closed".into()))?;
        }

        // Text content
        if let Some(ref text) = delta.content
            && !text.is_empty()
        {
            tx.send(Ok(StreamPart::TextDelta { text: text.clone() }))
                .await
                .map_err(|_| KonError::Provider("event channel closed".into()))?;
        }

        // Tool calls
        if let Some(ref tool_calls) = delta.tool_calls {
            for tc in tool_calls {
                let buffer = tool_buffers.entry(tc.index).or_default();

                // New tool call starting
                if let Some(ref id) = tc.id
                    && !buffer.started
                {
                    // Strip special tokens like "<|channel|>json" from name
                    let raw_name = tc
                        .function
                        .as_ref()
                        .and_then(|f| f.name.clone())
                        .unwrap_or_default();
                    let name = sanitize_tool_name(&raw_name);
                    buffer.id = id.clone();
                    buffer.name = Some(name.clone());
                    buffer.started = true;

                    tx.send(Ok(StreamPart::ToolCallStart {
                        id: id.clone(),
                        name,
                    }))
                    .await
                    .map_err(|_| KonError::Provider("event channel closed".into()))?;
                }

                if let Some(ref func) = tc.function {
                    // Use the actual tool call ID for delta matching
                    let id = if buffer.id.is_empty() {
                        buffer.name.clone().unwrap_or_else(|| "unknown".into())
                    } else {
                        buffer.id.clone()
                    };
                    tx.send(Ok(StreamPart::ToolCallDelta {
                        id,
                        arguments_delta: func.arguments.clone(),
                    }))
                    .await
                    .map_err(|_| KonError::Provider("event channel closed".into()))?;
                }
            }
        }

        // Finish reason
        if let Some(ref finish) = choice.finish_reason
            && finish == "tool_calls"
        {
            *had_tool_calls = true;
        }
    }

    Ok(())
}

/// Strip special tokens like `<|channel|>json` that some models emit in tool names.
fn sanitize_tool_name(raw: &str) -> String {
    // Find the first `<` that starts a special token and truncate
    if let Some(pos) = raw.find('<') {
        raw[..pos].to_string()
    } else {
        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_to_string() {
        assert_eq!(level_to_string(ThinkingLevel::None), "none");
        assert_eq!(level_to_string(ThinkingLevel::Low), "low");
        assert_eq!(level_to_string(ThinkingLevel::High), "high");
        assert_eq!(level_to_string(ThinkingLevel::XHigh), "xhigh");
    }

    #[test]
    fn test_build_messages_user_simple() {
        let messages = vec![Message::User(crate::core::types::UserMessage {
            content: vec![ContentBlock::Text {
                text: "hello".into(),
            }],
            skill_name: None,
        })];

        let result = OpenAICompletionsProvider::build_messages(&messages, None).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
    }

    #[test]
    fn test_build_messages_with_system_prompt() {
        let messages = vec![Message::User(crate::core::types::UserMessage {
            content: vec![ContentBlock::Text { text: "hi".into() }],
            skill_name: None,
        })];

        let result =
            OpenAICompletionsProvider::build_messages(&messages, Some("Be helpful")).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, "system");
        assert_eq!(result[1].role, "user");
    }

    #[test]
    fn test_build_tools_empty() {
        let tools: Vec<ToolDefinition> = vec![];
        let result = OpenAICompletionsProvider::build_tools(&tools);
        assert!(result.is_none());
    }

    #[test]
    fn test_build_tools_with_definitions() {
        let tools = vec![ToolDefinition {
            name: "read".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let result = OpenAICompletionsProvider::build_tools(&tools);
        assert!(result.is_some());
        let defs = result.unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].function.name, "read");
    }
}
