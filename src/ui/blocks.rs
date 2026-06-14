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

    // Render file diffs inline when present
    if let Some(fc) = &result.file_changes
        && let Some(diff) = fc.diff.clone()
    {
        // Blank line before diff
        lines.push(Line::from(Span::styled("", styles.base())));
        // Diff header
        lines.push(Line::from(Span::styled(
            format!("  ┌─ {} +{}/-{}", fc.path, fc.lines_added, fc.lines_removed),
            styles.accent_text(),
        )));
        // Diff content with +/- coloring
        for dline in diff.lines() {
            let (marker, style) = if dline.starts_with('+') {
                (" ", styles.success_text())
            } else if dline.starts_with('-') {
                (" ", styles.error_text())
            } else {
                (" ", styles.dim_text())
            };
            lines.push(Line::from(Span::styled(
                format!("  │{marker}{dline}"),
                style,
            )));
        }
        // Diff footer
        lines.push(Line::from(Span::styled("  └─", styles.accent_text())));
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::FileChanges;

    fn test_styles() -> Styles {
        Styles::default_theme()
    }

    #[test]
    fn test_render_tool_result_without_diff() {
        let result = ToolResult {
            success: true,
            result: Some("done".into()),
            images: vec![],
            ui_summary: Some("Created file.txt".into()),
            ui_details: None,
            ui_details_full: None,
            file_changes: None,
        };
        let block = render_tool_result("x", "write", &result, &test_styles());
        // Should have just the summary line (no diff)
        assert_eq!(block.lines.len(), 1);
        let content: String = block.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(content.contains("✓"));
        assert!(content.contains("write"));
        assert!(content.contains("Created file.txt"));
    }

    #[test]
    fn test_render_tool_result_with_diff() {
        let result = ToolResult {
            success: true,
            result: Some("done".into()),
            images: vec![],
            ui_summary: Some("Modified src/main.rs (2 lines)".into()),
            ui_details: None,
            ui_details_full: None,
            file_changes: Some(FileChanges {
                path: "src/main.rs".into(),
                lines_added: 2,
                lines_removed: 1,
                diff: Some(
                    "@@ -1,3 +1,4 @@\n fn main() {\n-    old\n+    new\n+    extra\n }\n".into(),
                ),
            }),
        };
        let block = render_tool_result("x", "edit", &result, &test_styles());
        // Should have summary line + blank + header + 4 diff lines + footer = 8
        assert!(
            block.lines.len() >= 6,
            "expected diff lines, got {}",
            block.lines.len()
        );
        // First line is summary
        let summary: String = block.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(summary.contains("✓"), "should have checkmark");
        // Should contain the diff content
        let all: String = block
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("src/main.rs"), "should contain file path");
        assert!(all.contains("+2/-1"), "should contain line stats");
        assert!(all.contains("+    new"), "should contain added line");
        assert!(all.contains("-    old"), "should contain removed line");
        assert!(all.contains("┌─"), "should have header border");
        assert!(all.contains("└─"), "should have footer border");
    }
}
