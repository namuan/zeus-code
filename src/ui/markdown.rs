//! Markdown to ratatui Text converter.
//!
//! Parses CommonMark/GFM using `comrak` and produces styled
//! `Vec<Line<'static>>` suitable for display in the chat panel.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, ComrakOptions};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::ui::styles::Styles;

/// Convert markdown text to a list of styled ratatui Lines.
pub fn render_markdown(text: &str, indent: &str, styles: &Styles) -> Vec<Line<'static>> {
    let arena = Arena::new();
    let options = ComrakOptions::default();
    let root = comrak::parse_document(&arena, text, &options);

    let mut lines: Vec<Line<'static>> = Vec::new();
    flatten_ast(root, indent, styles, &mut lines);
    lines
}

/// Walk the AST recursively, appending ratatui Lines.
fn flatten_ast<'a>(
    node: &'a AstNode<'a>,
    indent: &str,
    styles: &Styles,
    out: &mut Vec<Line<'static>>,
) {
    for child in node.children() {
        let value = &child.data.borrow().value;
        match value {
            NodeValue::Document => {
                flatten_ast(child, indent, styles, out);
            }

            NodeValue::Paragraph => {
                let mut spans: Vec<Span<'static>> = Vec::new();
                collect_inline_spans(child, styles, styles.base(), &mut spans);
                if !spans.is_empty() {
                    let prefix = Span::styled(indent.to_string(), styles.base());
                    // Merge prefix into the first span's content for cleaner rendering
                    let mut line_spans = vec![prefix];
                    line_spans.extend(spans);
                    out.push(Line::from(line_spans));
                }
            }

            NodeValue::Heading(h) => {
                let level = h.level as usize;
                let prefix = match level {
                    1 => "# ",
                    2 => "## ",
                    3 => "### ",
                    _ => "#### ",
                };
                let heading_style = styles.heading();
                let mut spans: Vec<Span<'static>> = Vec::new();
                collect_inline_spans(child, styles, heading_style, &mut spans);

                let mut line = Line::from(Span::styled(format!("{indent}{prefix}"), heading_style));
                line.extend(spans);
                out.push(line);
                out.push(Line::from(Span::styled("", styles.base())));
            }

            NodeValue::CodeBlock(cb) => {
                let lang = cb.info.trim().to_string();
                let code_style = styles.code_block();
                let accent = styles.accent_text();

                // Language badge line
                if !lang.is_empty() {
                    out.push(Line::from(Span::styled(
                        format!("{indent}  ┌─ {lang}"),
                        accent,
                    )));
                }

                for c_line in cb.literal.lines() {
                    out.push(Line::from(Span::styled(
                        format!("{indent}  │ {c_line}"),
                        code_style,
                    )));
                }

                if !lang.is_empty() {
                    out.push(Line::from(Span::styled(format!("{indent}  └─"), accent)));
                }
            }

            NodeValue::List(_) => {
                flatten_ast(child, indent, styles, out);
            }

            NodeValue::Item(_) => {
                let bullet = "- ";
                let prefix = format!("{indent}{bullet}");

                for item_child in child.children() {
                    match &item_child.data.borrow().value {
                        NodeValue::Paragraph => {
                            let mut spans: Vec<Span<'static>> = Vec::new();
                            collect_inline_spans(item_child, styles, styles.base(), &mut spans);
                            let mut line = Line::from(Span::styled(prefix.clone(), styles.base()));
                            line.extend(spans);
                            out.push(line);
                        }
                        NodeValue::List(_) => {
                            flatten_ast(item_child, &format!("{indent}  "), styles, out);
                        }
                        _ => {
                            flatten_ast(item_child, indent, styles, out);
                        }
                    }
                }
            }

            NodeValue::BlockQuote => {
                flatten_ast(child, &format!("{indent}▎ "), styles, out);
            }

            NodeValue::ThematicBreak => {
                out.push(Line::from(Span::styled(
                    format!("{indent}───"),
                    styles.dim_text(),
                )));
            }

            // Table support: render as plain dim text
            NodeValue::Table(_) | NodeValue::TableRow(_) | NodeValue::TableCell => {
                let text = collect_text(child);
                if !text.trim().is_empty() {
                    out.push(Line::from(Span::styled(
                        format!("{indent}{text}"),
                        styles.dim_text(),
                    )));
                }
            }

            _ => {
                // Fallback: recurse into unknown nodes
                flatten_ast(child, indent, styles, out);
            }
        }
    }
}

/// Collect all text content from an AST node and its descendants.
fn collect_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut buf = String::new();
    collect_text_inner(node, &mut buf);
    buf
}

fn collect_text_inner<'a>(node: &'a AstNode<'a>, buf: &mut String) {
    for child in node.children() {
        match &child.data.borrow().value {
            NodeValue::Text(t) => buf.push_str(t),
            NodeValue::SoftBreak => buf.push(' '),
            NodeValue::LineBreak => buf.push('\n'),
            NodeValue::Code(c) => buf.push_str(&c.literal),
            _ => collect_text_inner(child, buf),
        }
    }
}

/// Collect inline spans from an AST node, applying style modifiers for
/// emphasis, strong, code, links, etc.
fn collect_inline_spans<'a>(
    node: &'a AstNode<'a>,
    styles: &Styles,
    current_style: ratatui::style::Style,
    out: &mut Vec<Span<'static>>,
) {
    for child in node.children() {
        match &child.data.borrow().value {
            NodeValue::Text(t) => {
                out.push(Span::styled(t.clone(), current_style));
            }
            NodeValue::SoftBreak => {
                // Append a space to the last span if possible
                if let Some(last) = out.last_mut() {
                    *last = Span::styled(format!("{} ", last.content.as_ref()), last.style);
                } else {
                    out.push(Span::raw(" "));
                }
            }
            NodeValue::LineBreak => {
                out.push(Span::raw("\n"));
            }
            NodeValue::Code(c) => {
                out.push(Span::styled(c.literal.clone(), styles.inline_code()));
            }
            NodeValue::Emph => {
                let emph_style = current_style.add_modifier(Modifier::ITALIC);
                collect_inline_spans(child, styles, emph_style, out);
            }
            NodeValue::Strong => {
                let strong_style = current_style.add_modifier(Modifier::BOLD);
                collect_inline_spans(child, styles, strong_style, out);
            }
            NodeValue::Link(_) => {
                let link_style = styles.link();
                collect_inline_spans(child, styles, link_style, out);
            }
            NodeValue::Image(_) => {
                out.push(Span::styled("[image]", styles.dim_text()));
            }
            NodeValue::Strikethrough => {
                let s = current_style.add_modifier(Modifier::CROSSED_OUT);
                collect_inline_spans(child, styles, s, out);
            }
            _ => {
                // Descend into unknown inline nodes
                collect_inline_spans(child, styles, current_style, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_styles() -> Styles {
        Styles::default_theme()
    }

    #[test]
    fn test_plain_text() {
        let lines = render_markdown("hello world", "  ", &test_styles());
        assert!(!lines.is_empty());
        let content: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains("hello world"));
    }

    #[test]
    fn test_bold() {
        let lines = render_markdown("**bold** text", "  ", &test_styles());
        assert!(!lines.is_empty());
        let bold_span = lines[0].spans.iter().find(|s| s.content.as_ref() == "bold");
        assert!(bold_span.is_some());
    }

    #[test]
    fn test_code_block() {
        let md = "```rust\nfn main() {}\n```";
        let lines = render_markdown(md, "  ", &test_styles());
        assert!(lines.iter().any(|l| {
            l.spans
                .iter()
                .any(|s| s.content.as_ref().contains("fn main"))
        }));
    }

    #[test]
    fn test_heading() {
        let lines = render_markdown("# Title", "  ", &test_styles());
        let content: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains("Title"));
        assert!(content.contains("# "));
    }

    #[test]
    fn test_inline_code() {
        let lines = render_markdown("use `std::io` here", "  ", &test_styles());
        let has_code = lines[0]
            .spans
            .iter()
            .any(|s| s.content.as_ref() == "std::io");
        assert!(has_code);
    }
}
