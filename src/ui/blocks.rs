//! Message block rendering.
//!
//! Renders user messages, thinking blocks, assistant responses,
//! tool calls, and tool results as ratatui Text.

use ratatui::text::{Line, Span};

use crate::core::types::{ContentBlock, ToolResult};
use crate::ui::markdown;
use crate::ui::styles::Styles;

/// Rendered message block ready for display.
pub struct MessageBlock {
    pub lines: Vec<Line<'static>>,
    pub height: u16,
}

/// Render a user message.
pub fn render_user_message(text: &str, skill: Option<&str>, styles: &Styles) -> MessageBlock {
    let mut lines = Vec::new();

    // Badge line
    let badge = if let Some(s) = skill {
        format!("  You (skill: {s})  ")
    } else {
        "  You  ".into()
    };
    lines.push(Line::from(vec![Span::styled(badge, styles.user_badge())]));

    // Content
    for line in text.lines() {
        lines.push(Line::from(Span::styled(format!("  {line}"), styles.base())));
    }

    MessageBlock {
        height: lines.len() as u16 + 1,
        lines,
    }
}

/// Render an assistant message.
pub fn render_assistant_message(content: &[ContentBlock], styles: &Styles) -> Vec<MessageBlock> {
    let mut blocks = Vec::new();

    for block in content {
        match block {
            ContentBlock::Text { text } => {
                blocks.push(render_text_block(text, styles));
            }
            ContentBlock::Thinking { thinking, .. } => {
                blocks.push(render_thinking_block(thinking, styles));
            }
            ContentBlock::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                blocks.push(render_tool_request(id, name, arguments, styles));
            }
            ContentBlock::Image(_) => {
                blocks.push(MessageBlock {
                    lines: vec![Line::from(Span::styled("  [image]", styles.dim_text()))],
                    height: 1,
                });
            }
        }
    }

    blocks
}

/// Render a text block from the assistant (markdown-aware).
fn render_text_block(text: &str, styles: &Styles) -> MessageBlock {
    let lines = markdown::render_markdown(text, "  ", styles);
    let h = lines.len() as u16;
    MessageBlock { lines, height: h }
}

/// Render a thinking block.
fn render_thinking_block(thinking: &str, styles: &Styles) -> MessageBlock {
    let mut lines = vec![Line::from(vec![Span::styled(
        "  💭 Thinking…",
        styles.dim_text(),
    )])];
    for line in thinking.lines().take(3) {
        lines.push(Line::from(Span::styled(
            format!("    {line}"),
            styles.thinking_text(),
        )));
    }
    if thinking.lines().count() > 3 {
        lines.push(Line::from(Span::styled("    …", styles.dim_text())));
    }
    MessageBlock {
        height: lines.len() as u16,
        lines,
    }
}

/// Render a tool call request.
fn render_tool_request(_id: &str, name: &str, arguments: &str, styles: &Styles) -> MessageBlock {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(format!("  {name}"), styles.tool_badge()),
        Span::styled(" ", styles.base()),
        Span::styled(truncate_args(arguments, 60), styles.dim_text()),
    ]));
    MessageBlock {
        height: lines.len() as u16,
        lines,
    }
}

/// Render a tool result.
pub fn render_tool_result(
    _id: &str,
    name: &str,
    result: &ToolResult,
    styles: &Styles,
) -> MessageBlock {
    let mut lines = Vec::new();
    let icon = if result.success { "✓" } else { "✗" };
    let summary_style = if result.success {
        styles.success_text()
    } else {
        styles.error_text()
    };

    lines.push(Line::from(vec![
        Span::styled(format!("  {icon} {name}"), styles.tool_badge()),
        Span::styled(
            format!("  {}", result.ui_summary.as_deref().unwrap_or("done")),
            summary_style,
        ),
    ]));

    let h = lines.len() as u16;
    MessageBlock { lines, height: h }
}

/// Render a status message (errors, info).
pub fn render_status(text: &str, styles: &Styles, is_error: bool) -> MessageBlock {
    let s = if is_error {
        styles.error_text()
    } else {
        styles.dim_text()
    };
    MessageBlock {
        lines: vec![Line::from(Span::styled(format!("  {text}"), s))],
        height: 1,
    }
}

fn truncate_args(args: &str, max_len: usize) -> String {
    let one_line = args.replace('\n', " ");
    if one_line.len() <= max_len {
        one_line
    } else {
        format!("{}…", &one_line[..max_len - 1])
    }
}
