//! Status widgets: InfoBar, StatusLine.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::styles::Styles;

/// Return a spinning character based on elapsed time (cycles every ~200ms).
fn spinner_frame(elapsed: Duration) -> char {
    const SPINNERS: &[char] = &['|', '/', '-', '\\'];
    let ms = elapsed.as_millis() as usize;
    SPINNERS[(ms / 200) % SPINNERS.len()]
}

/// Render the status bar at the bottom of the screen.
pub fn render_status_line(
    f: &mut Frame,
    area: Rect,
    working: bool,
    working_since: Option<Duration>,
    turn: u64,
    styles: &Styles,
) {
    let text = if working {
        let spinner = working_since.map_or('|', spinner_frame);
        Line::from(vec![
            Span::styled(format!(" {spinner} Working…"), styles.accent_text()),
            Span::styled(format!("  turn {turn}"), styles.dim_text()),
            Span::styled("  [Esc to cancel]", styles.dim_text()),
        ])
    } else {
        Line::from(vec![Span::styled(
            " Ready  [Ctrl+C to quit]",
            styles.dim_text(),
        )])
    };

    let p = Paragraph::new(text).style(styles.status_bar());
    f.render_widget(p, area);
}

/// Render the info bar showing cwd, model, etc.
pub fn render_info_bar(
    f: &mut Frame,
    area: Rect,
    cwd: &str,
    model: &str,
    thinking: &str,
    tokens: u64,
    styles: &Styles,
) {
    let text = Line::from(vec![
        Span::styled(format!(" {cwd}  "), styles.dim_text()),
        Span::styled(format!("{model}  "), styles.accent_text()),
        Span::styled(format!("🧠{thinking}  "), styles.dim_text()),
        Span::styled(format!("{tokens}tok"), styles.dim_text()),
    ]);

    let p = Paragraph::new(text).style(styles.status_bar());
    f.render_widget(p, area);
}
