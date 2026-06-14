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

    /// Push a line with a specific style (bypasses the default base style).
    pub fn add_styled_line(&mut self, text: &str, style: ratatui::style::Style) {
        self.lines
            .push(Line::from(Span::styled(text.to_string(), style)));
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

    /// Replace all lines from `mark` onward with freshly rendered markdown.
    /// Used during streaming to show partially-formatted text before
    /// the final `TurnEnd` rendering kicks in.
    pub fn replace_streaming_lines(&mut self, mark: usize, text: &str, styles: &Styles) {
        self.lines.truncate(mark);
        let rendered = crate::ui::markdown::render_markdown(text, "  ", styles);
        self.lines.extend(rendered);
    }

    /// Replace the last line (used for progress indicators like
    /// "N KB streaming…" during tool argument generation).
    pub fn set_progress_line(&mut self, text: String, styles: &Styles) {
        self.lines.pop();
        self.add_line(text, styles);
    }

    /// Current number of lines in the chat.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Collect all line content into a single string (for testing).
    pub fn all_text(&self) -> String {
        use ratatui::text::Text;
        Text::from(self.lines.clone()).to_string()
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

        // Build Paragraph and query its EXACT wrapped line count.
        // This uses ratatui's internal wrapping algorithm, so it's always
        // correct — no estimation error at narrow widths.
        let p = Paragraph::new(self.lines.clone())
            .style(styles.base())
            .wrap(Wrap { trim: false });
        let total_visual = p.line_count(area.width);

        // Bottom offset: how many rows to skip to make the last `area.height`
        // rows visible. Scroll = rows scrolled up from bottom.
        let bottom = total_visual.saturating_sub(area.height as usize);
        let offset = bottom.saturating_sub(scroll).min(u16::MAX as usize) as u16;

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
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    fn make_chat(n: usize) -> ChatLog {
        let mut chat = ChatLog::new();
        let styles = crate::ui::styles::Styles::default_theme();
        for i in 0..n {
            chat.add_line(format!("line {i:04}"), &styles);
        }
        chat
    }

    /// Render the chat into a test terminal and return the visible lines.
    fn render_chat(chat: &ChatLog, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, width, height);
                chat.render(f, area, &crate::ui::styles::Styles::default_theme());
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut lines = Vec::new();
        for y in 0..height {
            let row: String = (0..width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect();
            lines.push(row);
        }
        lines
    }

    #[test]
    fn test_scroll_bottom_shows_last_lines() {
        let chat = make_chat(100);
        let lines = render_chat(&chat, 80, 20);
        // At bottom (scroll=0), we should see the last few lines
        let has_last = lines.iter().any(|l| l.contains("0099"));
        assert!(
            has_last,
            "at bottom (scroll=0), should see line 99. Got:\n{}",
            lines.join("\n")
        );
    }

    #[test]
    fn test_scroll_up_shows_earlier_lines() {
        let mut chat = make_chat(100);
        chat.scroll = 50; // scroll up 50 rows from bottom
        let lines = render_chat(&chat, 80, 20);
        // With scroll=50, we should see lines ~30-49 (50 rows above bottom)
        let has_early = lines.iter().any(|l| l.contains("0030"));
        assert!(
            has_early,
            "scroll=50 should show line ~30. Got:\n{}",
            lines.join("\n")
        );
        // Should NOT see the last line (99)
        let has_last = lines.iter().any(|l| l.contains("0099"));
        assert!(
            !has_last,
            "scroll=50 should NOT show last line. Got:\n{}",
            lines.join("\n")
        );
    }

    #[test]
    fn test_scroll_to_top_shows_first_lines() {
        let mut chat = make_chat(50);
        chat.scroll = 80; // scroll past all content
        let lines = render_chat(&chat, 80, 20);
        let has_first = lines.iter().any(|l| l.contains("0000"));
        assert!(
            has_first,
            "at top, should see line 0. Got:\n{}",
            lines.join("\n")
        );
    }

    #[test]
    fn test_never_shows_blank_when_content_exists() {
        for scroll in [0u16, 1, 3, 5, 10, 100] {
            let mut chat = make_chat(5);
            chat.scroll = scroll;
            let lines = render_chat(&chat, 80, 20);
            let has_content = lines.iter().any(|l| !l.trim().is_empty());
            assert!(
                has_content,
                "scroll={scroll}: should show content, not blank.\nGot:\n{}",
                lines.join("\n")
            );
        }
    }

    #[test]
    fn test_wrapping_long_lines() {
        let mut chat = ChatLog::new();
        let styles = crate::ui::styles::Styles::default_theme();
        // One very long line that should wrap to multiple visual rows
        chat.add_line(format!("{}", "=".repeat(200)), &styles);
        chat.add_line("bottom line".to_string(), &styles);

        // Render at width 40 — the long line should wrap to 5 rows (200/40)
        let lines = render_chat(&chat, 40, 8);
        let has_bottom = lines.iter().any(|l| l.contains("bottom"));
        assert!(
            has_bottom,
            "bottom line should be visible. Got:\n{}",
            lines.join("\n")
        );
    }

    #[test]
    fn test_line_count_at_least_line_count() {
        // Paragraph::line_count(width) should be >= number of logical lines
        // (each line takes at least 1 visual row)
        let chat = make_chat(100);
        let p = Paragraph::new(chat.lines.clone()).wrap(Wrap { trim: false });
        let count = p.line_count(80);
        assert!(count >= 100, "line_count {count} should be >= 100");
        // Should not be wildly inflated either
        assert!(count <= 200, "line_count {count} should be reasonable");
    }

    #[test]
    fn test_line_count_at_narrow_width() {
        let chat = make_chat(20);
        let p = Paragraph::new(chat.lines.clone()).wrap(Wrap { trim: false });
        let count_wide = p.line_count(80);
        let count_narrow = p.line_count(20);
        // At narrow width, long lines wrap → more visual rows
        assert!(
            count_narrow >= count_wide,
            "narrow ({count_narrow}) should be >= wide ({count_wide})"
        );
    }
}
