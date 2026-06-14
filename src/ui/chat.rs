//! Chat log widget — scrollable message display with streaming updates.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::blocks::MessageBlock;
use crate::ui::styles::Styles;

/// The chat log holds all displayed message blocks.
pub struct ChatLog {
    /// All rendered lines in display order.
    lines: Vec<Line<'static>>,
    /// Vertical scroll offset.
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

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_add(amount);
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll = 0;
    }

    pub fn render(&self, f: &mut Frame, area: Rect, styles: &Styles) {
        use ratatui::widgets::Wrap;

        if self.lines.is_empty() || area.height == 0 {
            return;
        }

        // Pass all lines so the Paragraph can reflow with wrapping.
        // Scroll offset is in visual rows (after wrapping), not logical lines.
        let scroll_offset = self.scroll;
        let p = Paragraph::new(self.lines.clone())
            .style(styles.base())
            .wrap(Wrap { trim: false })
            .scroll((scroll_offset, 0u16));
        f.render_widget(p, area);
    }
}

impl Default for ChatLog {
    fn default() -> Self {
        Self::new()
    }
}
