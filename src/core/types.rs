use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

// ── Messages ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserMessage {
    pub content: Vec<ContentBlock>,
    pub skill_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantMessage {
    pub content: Vec<ContentBlock>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<StopReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolResultMessage {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub images: Vec<ImageContent>,
    pub file_changes: Option<FileChanges>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemMessage {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "user")]
    User(UserMessage),
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultMessage),
    #[serde(rename = "system")]
    System(SystemMessage),
}

// ── Content Blocks ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageContent {
    pub base64: String,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        signature: Option<String>,
        level: Option<String>,
    },
    Image(ImageContent),
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
}

// ── Streaming Parts ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum StreamPart {
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
        level: Option<String>,
    },
    ThinkingSignature {
        signature: String,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        arguments_delta: String,
    },
    StreamDone {
        stop_reason: StopReason,
        usage: Usage,
    },
    StreamError {
        error: String,
        retryable: bool,
    },
}

// ── Tools ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileChanges {
    pub path: String,
    pub lines_added: u64,
    pub lines_removed: u64,
    pub diff: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolResult {
    pub success: bool,
    pub result: Option<String>,
    pub images: Vec<ImageContent>,
    pub ui_summary: Option<String>,
    pub ui_details: Option<String>,
    pub ui_details_full: Option<String>,
    pub file_changes: Option<FileChanges>,
}

// ── Usage / Stop ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StopReason {
    Stop,
    ToolUse,
    MaxTurns,
    EndTurn,
    Refusal,
    Error,
}

// ── Agent Events (agent → UI channel) ───────────────────────────────────

#[derive(Debug, Clone)]
pub enum ToolApproval {
    Allow,
    Deny,
    AllowAll,
}

#[derive(Debug)]
pub enum AgentEvent {
    Start,
    TurnStart {
        turn: u64,
    },
    ThinkingStart {
        level: Option<String>,
    },
    ThinkingDelta {
        text: String,
    },
    ThinkingEnd,
    TextStart,
    TextDelta {
        text: String,
    },
    TextEnd,
    ToolStart {
        id: String,
        name: String,
    },
    ToolArgsDelta {
        id: String,
        delta: String,
    },
    ToolEnd {
        id: String,
        arguments: String,
    },
    ToolApprovalRequest {
        tool_name: String,
        call_text: String,
        preview: Option<String>,
        response_tx: oneshot::Sender<ToolApproval>,
    },
    ToolResult {
        id: String,
        result: ToolResult,
    },
    TurnEnd {
        stop_reason: StopReason,
        assistant: AssistantMessage,
        tool_results: Vec<ToolResult>,
    },
    CompactionStart,
    CompactionEnd {
        summary: String,
        tokens_before: u64,
    },
    End {
        stop_reason: StopReason,
        total_turns: u64,
        usage: Usage,
    },
    Error {
        error: String,
    },
    /// Result of a `!command` / `!!command` shell execution. Routes from the
    /// async spawn back to the TUI for rendering and optional LLM forwarding.
    ShellResult {
        command: String,
        result: ToolResult,
        send_to_llm: bool,
    },
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    // ── Message serde round-trip ──────────────────────────────────────

    #[test]
    fn test_user_message_serde_roundtrip() {
        let msg = Message::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "fix the bug".into(),
            }],
            skill_name: Some("rust-debugger".into()),
        });

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(msg, parsed);

        // Verify the role tag is present
        assert!(json.contains("\"role\":\"user\""));
    }

    #[test]
    fn test_assistant_message_serde_roundtrip() {
        let msg = Message::Assistant(AssistantMessage {
            content: vec![
                ContentBlock::Thinking {
                    thinking: "Let me trace the issue...".into(),
                    signature: None,
                    level: Some("low".into()),
                },
                ContentBlock::Text {
                    text: "Found the bug in config.rs".into(),
                },
                ContentBlock::ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    arguments: r#"{"file_path":"src/config.rs"}"#.into(),
                },
            ],
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: Some(20),
                cache_write_tokens: None,
            }),
            stop_reason: Some(StopReason::ToolUse),
        });

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(msg, parsed);
        assert!(json.contains("\"role\":\"assistant\""));
    }

    #[test]
    fn test_tool_result_message_serde_roundtrip() {
        let msg = Message::ToolResult(ToolResultMessage {
            tool_call_id: "call_1".into(),
            tool_name: "read".into(),
            content: "1: fn main() {\n2:     println!(\"hello\");\n3: }".into(),
            images: vec![],
            file_changes: None,
        });

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(msg, parsed);
        assert!(json.contains("\"role\":\"tool_result\""));
    }

    #[test]
    fn test_system_message_serde_roundtrip() {
        let msg = Message::System(SystemMessage {
            content: "You are a helpful assistant.".into(),
        });

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(msg, parsed);
        assert!(json.contains("\"role\":\"system\""));
    }

    #[test]
    fn test_message_discriminated_deserialization() {
        // Each JSON variant should parse to the correct enum variant
        let user_json = r#"{"role":"user","content":[{"Text":{"text":"hi"}}],"skill_name":null}"#;
        let msg: Message = serde_json::from_str(user_json).unwrap();
        assert!(matches!(msg, Message::User(_)));

        let assistant_json = r#"{"role":"assistant","content":[],"usage":null,"stop_reason":null}"#;
        let msg: Message = serde_json::from_str(assistant_json).unwrap();
        assert!(matches!(msg, Message::Assistant(_)));

        let tool_json = r#"{"role":"tool_result","tool_call_id":"x","tool_name":"read","content":"ok","images":[],"file_changes":null}"#;
        let msg: Message = serde_json::from_str(tool_json).unwrap();
        assert!(matches!(msg, Message::ToolResult(_)));

        let system_json = r#"{"role":"system","content":"be helpful"}"#;
        let msg: Message = serde_json::from_str(system_json).unwrap();
        assert!(matches!(msg, Message::System(_)));
    }

    // ── ContentBlock serde ────────────────────────────────────────────

    #[test]
    fn test_content_block_text_serde() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, parsed);
    }

    #[test]
    fn test_content_block_thinking_serde() {
        let block = ContentBlock::Thinking {
            thinking: "analysis...".into(),
            signature: Some("sig1".into()),
            level: Some("high".into()),
        };
        let json = serde_json::to_string(&block).unwrap();
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, parsed);
    }

    #[test]
    fn test_content_block_tool_call_serde() {
        let block = ContentBlock::ToolCall {
            id: "call_1".into(),
            name: "bash".into(),
            arguments: r#"{"command":"ls"}"#.into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, parsed);
    }

    #[test]
    fn test_content_block_image_serde() {
        let block = ContentBlock::Image(ImageContent {
            base64: "aGVsbG8=".into(),
            media_type: "image/png".into(),
        });
        let json = serde_json::to_string(&block).unwrap();
        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, parsed);
    }

    // ── StopReason serde ──────────────────────────────────────────────

    #[test]
    fn test_stop_reason_serde_all_variants() {
        let variants = [
            (StopReason::Stop, "\"Stop\""),
            (StopReason::ToolUse, "\"ToolUse\""),
            (StopReason::MaxTurns, "\"MaxTurns\""),
            (StopReason::EndTurn, "\"EndTurn\""),
            (StopReason::Refusal, "\"Refusal\""),
            (StopReason::Error, "\"Error\""),
        ];

        for (variant, expected_json) in variants {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_json, "failed for variant {:?}", variant);

            let parsed: StopReason = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    // ── Usage ─────────────────────────────────────────────────────────

    #[test]
    fn test_usage_serde_roundtrip() {
        let usage = Usage {
            input_tokens: 500,
            output_tokens: 200,
            cache_read_tokens: Some(50),
            cache_write_tokens: Some(10),
        };

        let json = serde_json::to_string(&usage).unwrap();
        let parsed: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, parsed);
    }

    #[test]
    fn test_usage_default() {
        let usage = Usage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.cache_read_tokens, None);
        assert_eq!(usage.cache_write_tokens, None);
    }

    // ── ToolDefinition ────────────────────────────────────────────────

    #[test]
    fn test_tool_definition_serde() {
        let def = ToolDefinition {
            name: "read".into(),
            description: "Read a file from disk".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string"}
                },
                "required": ["file_path"]
            }),
        };
        let json = serde_json::to_string(&def).unwrap();
        let parsed: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(def.name, parsed.name);
        assert_eq!(def.description, parsed.description);
        // serde_json::Value doesn't implement Eq through PartialEq on the outer struct
        assert_eq!(def.parameters.to_string(), parsed.parameters.to_string());
    }

    // ── ToolResult ────────────────────────────────────────────────────

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult {
            success: true,
            result: Some("file contents here".into()),
            images: vec![],
            ui_summary: Some("Read 42 lines".into()),
            ui_details: None,
            ui_details_full: None,
            file_changes: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);
        assert_eq!(parsed.ui_summary, Some("Read 42 lines".into()));
        assert_eq!(parsed.result, Some("file contents here".into()));
    }

    #[test]
    fn test_tool_result_with_file_changes() {
        let result = ToolResult {
            success: true,
            result: None,
            images: vec![],
            ui_summary: Some("Applied edit (+3 / -1)".into()),
            ui_details: None,
            ui_details_full: None,
            file_changes: Some(FileChanges {
                path: "src/main.rs".into(),
                lines_added: 3,
                lines_removed: 1,
                diff: Some("- old\n+ new".into()),
            }),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);
        let fc = parsed.file_changes.unwrap();
        assert_eq!(fc.path, "src/main.rs");
        assert_eq!(fc.lines_added, 3);
        assert_eq!(fc.lines_removed, 1);
    }

    // ── AgentEvent (no serde — channel-only, but we can test construction)

    #[test]
    fn test_agent_event_construction() {
        // AgentEvent doesn't derive Serialize/Deserialize (channel-only type)
        // but we should verify it can be constructed
        let event = AgentEvent::TurnStart { turn: 3 };
        match event {
            AgentEvent::TurnStart { turn } => assert_eq!(turn, 3),
            _ => panic!("wrong variant"),
        }

        let event = AgentEvent::Error {
            error: "something went wrong".into(),
        };
        match event {
            AgentEvent::Error { error } => assert_eq!(error, "something went wrong"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_agent_event_shell_result_construction() {
        let ev = AgentEvent::ShellResult {
            command: "ls".into(),
            result: ToolResult::default(),
            send_to_llm: false,
        };
        match ev {
            AgentEvent::ShellResult {
                command,
                result,
                send_to_llm,
            } => {
                assert_eq!(command, "ls");
                assert!(!result.success);
                assert!(!send_to_llm);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_stream_part_variants() {
        // Verify all StreamPart variants can be constructed
        let td = StreamPart::TextDelta {
            text: "hello".into(),
        };
        assert!(matches!(td, StreamPart::TextDelta { .. }));

        let ts = StreamPart::ToolCallStart {
            id: "c1".into(),
            name: "bash".into(),
        };
        assert!(matches!(ts, StreamPart::ToolCallStart { .. }));
    }
}
