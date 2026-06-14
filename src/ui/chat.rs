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
        let total_lines = self.lines.len() as u16;
        let visible = area.height.saturating_sub(1);
        if total_lines == 0 || visible == 0 {
            return;
        }

        let start = if total_lines > visible {
            let max_scroll = total_lines - visible;
            let scroll = self.scroll.min(max_scroll);
            (total_lines - visible - scroll) as usize
        } else {
            0
        };

        let end = (start + visible as usize).min(self.lines.len());
        let visible_lines: Vec<Line> = self.lines[start..end].to_vec();
        let p = Paragraph::new(visible_lines).style(styles.base());
        f.render_widget(p, area);
    }
}

impl Default for ChatLog {
    fn default() -> Self {
        Self::new()
    }
}
