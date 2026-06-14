//! Input box widget with autocomplete and history.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::ui::styles::Styles;

/// The input box state.
pub struct InputBox {
    /// Current text in the input.
    pub text: String,
    /// Cursor position (byte offset).
    pub cursor: usize,
    /// History of previous inputs.
    history: Vec<String>,
    /// Current position in history navigation.
    history_index: Option<usize>,
    /// Draft saved when navigating history.
    draft: String,
}

impl InputBox {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            draft: String::new(),
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let before = self.cursor - 1;
            if self.text.is_char_boundary(before) {
                self.text.remove(before);
                self.cursor = before;
            }
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
        }
    }

    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            while self.cursor > 0 && !self.text.is_char_boundary(self.cursor) {
                self.cursor -= 1;
            }
        }
    }

    pub fn cursor_right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor += 1;
            while self.cursor < self.text.len() && !self.text.is_char_boundary(self.cursor) {
                self.cursor += 1;
            }
        }
    }

    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    pub fn cursor_end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn submit(&mut self) -> String {
        let text = std::mem::take(&mut self.text);
        self.cursor = 0;
        self.history_index = None;
        if !text.trim().is_empty() {
            self.history.push(text.clone());
        }
        text
    }

    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        if self.history_index.is_none() {
            self.draft = std::mem::take(&mut self.text);
            self.history_index = Some(self.history.len() - 1);
        } else if let Some(idx) = self.history_index
            && idx > 0
        {
            self.history_index = Some(idx - 1);
        }
        if let Some(idx) = self.history_index {
            self.text = self.history[idx].clone();
            self.cursor = self.text.len();
        }
    }

    pub fn history_next(&mut self) {
        match self.history_index {
            None => {}
            Some(idx) if idx + 1 < self.history.len() => {
                self.history_index = Some(idx + 1);
                self.text = self.history[self.history_index.unwrap()].clone();
                self.cursor = self.text.len();
            }
            _ => {
                self.history_index = None;
                self.text = std::mem::take(&mut self.draft);
                self.cursor = self.text.len();
            }
        }
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.text.len();
    }

    pub fn render(&self, f: &mut Frame, area: Rect, styles: &Styles) {
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(styles.border_style())
            .style(styles.input_style());

        let prefix = "> ";
        let display = format!("{prefix}{}", self.text);
        let p = Paragraph::new(display)
            .block(block)
            .style(styles.input_style());
        f.render_widget(p, area);

        let cursor_x = (prefix.len() + self.cursor) as u16;
        if cursor_x < area.width.saturating_sub(1) && area.height > 0 {
            f.set_cursor_position((area.x + cursor_x, area.y + area.height.saturating_sub(1)));
        }
    }
}

impl Default for InputBox {
    fn default() -> Self {
        Self::new()
    }
}
