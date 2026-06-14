//! Theme colors → ratatui Style objects.

use ratatui::style::{Color, Modifier, Style};

/// Style constants used throughout the TUI.
pub struct Styles {
    pub bg: Color,
    pub fg: Color,
    pub dim: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub surface: Color,
    pub border: Color,
}

impl Styles {
    pub fn default_theme() -> Self {
        Self {
            bg: Color::Rgb(40, 40, 40),
            fg: Color::Rgb(235, 219, 178),
            dim: Color::Rgb(102, 92, 84),
            accent: Color::Rgb(131, 165, 152),
            success: Color::Rgb(184, 187, 38),
            warning: Color::Rgb(250, 189, 47),
            error: Color::Rgb(251, 73, 52),
            surface: Color::Rgb(60, 56, 54),
            border: Color::Rgb(80, 73, 69),
        }
    }

    pub fn base(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }

    pub fn dim_text(&self) -> Style {
        Style::default().fg(self.dim).bg(self.bg)
    }

    pub fn accent_text(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .bg(self.bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn thinking_text(&self) -> Style {
        Style::default()
            .fg(self.dim)
            .bg(self.surface)
            .add_modifier(Modifier::ITALIC)
    }

    pub fn user_badge(&self) -> Style {
        Style::default()
            .fg(self.bg)
            .bg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn assistant_badge(&self) -> Style {
        Style::default()
            .fg(self.bg)
            .bg(Color::Rgb(69, 133, 136))
            .add_modifier(Modifier::BOLD)
    }

    pub fn tool_badge(&self) -> Style {
        Style::default()
            .fg(self.bg)
            .bg(Color::Rgb(214, 93, 14))
            .add_modifier(Modifier::BOLD)
    }

    pub fn success_text(&self) -> Style {
        Style::default().fg(self.success)
    }

    pub fn error_text(&self) -> Style {
        Style::default().fg(self.error)
    }

    pub fn status_bar(&self) -> Style {
        Style::default().fg(self.fg).bg(self.surface)
    }

    pub fn input_style(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }

    pub fn cursor_style(&self) -> Style {
        Style::default()
            .fg(self.bg)
            .bg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn border_style(&self) -> Style {
        Style::default().fg(self.border)
    }

    // ── Markdown styles ──────────────────────────────────────────────

    pub fn heading(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .bg(self.bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn code_block(&self) -> Style {
        Style::default().fg(self.fg).bg(self.surface)
    }

    pub fn inline_code(&self) -> Style {
        Style::default().fg(self.accent).bg(self.surface)
    }

    pub fn link(&self) -> Style {
        Style::default()
            .fg(Color::Rgb(69, 133, 136))
            .bg(self.bg)
            .add_modifier(Modifier::UNDERLINED)
    }

    pub fn block_quote(&self) -> Style {
        Style::default()
            .fg(self.dim)
            .bg(self.bg)
            .add_modifier(Modifier::ITALIC)
    }
}
