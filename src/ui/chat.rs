//! Chat log widget — scrollable message display with streaming updates.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::ui::blocks::MessageBlock;
use crate::ui::styles::Styles;

/// The chat log holds all displayed message blocks.
pub struct ChatLog {
    /// All rendered lines in display order.
    lines: Vec<Line<'static>>,
    /// How many rows the user has scrolled UP from the bottom.
    /// 0 = bottom (most recent), N = N rows above bottom.
    scroll: u16,
}

impl ChatLog {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            scroll: 0,
        }
    }

    pub fn add_block(&mut self, block: MessageBlock) {
        self.lines.extend(block.lines);
    }

    pub fn add_line(&mut self, line: String, styles: &Styles) {
        self.lines
            .push(Line::from(Span::styled(line, styles.base())));
    }

    pub fn append_text(&mut self, text: &str, styles: &Styles) {
        if let Some(last) = self.lines.last_mut() {
            let current: String = last.spans.iter().map(|s| s.content.as_ref()).collect();
            let s = last
                .spans
                .last()
                .map(|sp| sp.style)
                .unwrap_or(styles.base());
            *last = Line::from(Span::styled(format!("{current}{text}"), s));
        } else {
            self.add_line(text.to_string(), styles);
        }
    }

    pub fn add_separator(&mut self, styles: &Styles) {
        self.lines
            .push(Line::from(Span::styled("───", styles.dim_text())));
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll = 0;
    }

    /// Truncate lines to the given length (keep first `len` lines).
    pub fn truncate_to(&mut self, len: usize) {
        self.lines.truncate(len);
    }

    /// Current number of lines in the chat.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Scroll up by N visual rows (move away from bottom).
    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_add(amount);
    }

    /// Scroll down by N visual rows (move toward bottom).
    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    /// Jump to the bottom.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll = 0;
    }

    pub fn render(&self, f: &mut Frame, area: Rect, styles: &Styles) {
        if self.lines.is_empty() || area.height == 0 || area.width == 0 {
            return;
        }

        let total = self.lines.len();
        let scroll = (self.scroll as usize).min(total.saturating_sub(1));

        // Take a generous window from the bottom so wrapping overflow is covered.
        let window = (area.height as usize * 3).min(total).max(1);
        let end = total.saturating_sub(scroll);
        let start = end.saturating_sub(window);
        // Ensure we always have at least one line
        if start >= end {
            return;
        }

        let slice: Vec<Line<'static>> = self.lines[start..end].to_vec();

        // Build Paragraph once and query exact wrapped line count of the slice.
        let p = Paragraph::new(slice)
            .style(styles.base())
            .wrap(Wrap { trim: false });
        let slice_visual = p.line_count(area.width) as u16;

        // Scroll the slice to show its bottom portion in the viewport.
        // If the slice is short (e.g. first messages), show from top.
        let offset = slice_visual.saturating_sub(area.height);

        let p = p.scroll((offset, 0u16));
        f.render_widget(p, area);
    }
}

impl Default for ChatLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use ratatui::text::Line;

    #[test]
    fn test_line_count_reasonable() {
        use ratatui::widgets::{Paragraph, Wrap};
        // Simulate a chat with 20 lines of ~60 chars each
        let lines: Vec<Line> = (0..20)
            .map(|i| {
                Line::from(format!(
                    "  This is chat line number {i} with some extra text to make it longer"
                ))
            })
            .collect();
        let p = Paragraph::new(lines).wrap(Wrap { trim: false });
        let count = p.line_count(80);
        // Each line is ~65 chars. At width 80, each is 1 row → 20 rows
        // (or maybe more if wrapping happens at different boundaries)
        println!("line_count at width 80: {count}");
        assert!(count >= 20, "line_count ({count}) should be at least 20");
        assert!(count <= 80, "line_count ({count}) should be reasonable");
    }
}
