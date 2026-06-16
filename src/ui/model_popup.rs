//! Interactive model selector popup — shown when the user types `/model`
//! with no argument.
//!
//! Renders a scrollable, bordered overlay listing all known models grouped
//! by provider. Arrow keys navigate, Enter selects, Esc dismisses.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use std::cmp;

use crate::llm::models::{self, Model, all_providers};
use crate::ui::model_switch::is_provider_implemented;
use crate::ui::styles::Styles;

// ── Data structures ──────────────────────────────────────────────────────

/// A single entry in the popup list — either a provider header or a model.
#[derive(Debug, Clone)]
enum PopupEntry {
    ProviderHeader { name: String, implemented: bool },
    ModelEntry { provider: String, model: Model },
}

/// State for the interactive model-selector popup.
#[derive(Default)]
pub struct ModelPopup {
    /// Whether the popup is currently visible.
    pub active: bool,
    /// All entries (headers + models), in display order.
    entries: Vec<PopupEntry>,
    /// Index of the currently highlighted entry.
    selected: usize,
    /// First visible row (for scrolling).
    scroll_offset: usize,
}

/// Default number of visible rows used for scroll calculations.
const DEFAULT_VISIBLE_ROWS: usize = 20;

impl ModelPopup {
    /// Activate the popup: build the entry list from the model catalog
    /// and show it.
    pub fn activate(&mut self) {
        self.entries = build_entries();
        self.selected = 0;
        self.scroll_offset = 0;
        self.active = true;
    }

    /// Dismiss the popup.
    pub fn dismiss(&mut self) {
        self.active = false;
    }

    /// Move selection up one row.
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = self.entries.len().saturating_sub(1);
        }
        self.adjust_scroll();
    }

    /// Move selection down one row.
    pub fn select_next(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        } else {
            self.selected = 0;
        }
        self.adjust_scroll();
    }

    /// Page up — scroll by half the visible area.
    pub fn page_up(&mut self) {
        let step = (DEFAULT_VISIBLE_ROWS / 2).max(1);
        self.selected = self.selected.saturating_sub(step);
        self.adjust_scroll();
    }

    /// Page down — scroll by half the visible area.
    pub fn page_down(&mut self) {
        let step = (DEFAULT_VISIBLE_ROWS / 2).max(1);
        self.selected = cmp::min(self.selected + step, self.entries.len().saturating_sub(1));
        self.adjust_scroll();
    }

    /// If a model entry is selected, returns `Some((provider, model_id))`.
    /// Returns `None` if a provider header is selected (the user must
    /// navigate to an actual model to select one).
    pub fn accept(&self) -> Option<(String, String)> {
        match self.entries.get(self.selected)? {
            PopupEntry::ModelEntry { provider, model } => {
                Some((provider.clone(), model.id.clone()))
            }
            PopupEntry::ProviderHeader { .. } => None,
        }
    }

    /// Keep `selected` within the visible scroll window.
    fn adjust_scroll(&mut self) {
        let vh = DEFAULT_VISIBLE_ROWS;
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + vh {
            self.scroll_offset = self.selected.saturating_sub(vh.saturating_sub(1));
        }
    }

    // ── Rendering ────────────────────────────────────────────────────

    /// Render the popup overlay on top of the current frame.
    pub fn render(&self, f: &mut Frame, styles: &Styles) {
        if !self.active || self.entries.is_empty() {
            return;
        }

        let area = f.area();

        // Compute popup dimensions — fill most of the screen.
        let popup_w = (area.width.saturating_sub(4)).clamp(40, 90);
        let visible_height = (area.height.saturating_sub(4)).clamp(6, 30);

        let popup_area = Rect::new(
            (area.width.saturating_sub(popup_w)) / 2,
            2,
            popup_w,
            visible_height,
        );

        // Clear the area behind the popup.
        f.render_widget(Clear, popup_area);

        // Build list items for the visible range.
        let vh = visible_height as usize;
        // The list area is the popup minus 2 for border and 1 for footer.
        let list_rows = vh.saturating_sub(3);

        let end = cmp::min(self.scroll_offset + list_rows, self.entries.len());
        let visible_slice = &self.entries[self.scroll_offset..end];

        let items: Vec<ListItem> = visible_slice
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let global_idx = self.scroll_offset + i;
                let is_selected = global_idx == self.selected;
                entry_to_list_item(entry, is_selected, styles)
            })
            .collect();

        // Build footer with key hints and scroll indicator.
        let footer_style = Style::default().fg(styles.dim).bg(styles.bg);
        let scroll_info = if self.entries.len() > list_rows {
            format!(
                " [{}-{}/{}]",
                self.scroll_offset + 1,
                end,
                self.entries.len()
            )
        } else {
            format!(" [1-{}/{}]", self.entries.len(), self.entries.len())
        };
        let footer_text = Line::from(vec![
            Span::styled("↑↓ navigate", footer_style),
            Span::raw("  "),
            Span::styled("↵ select", footer_style),
            Span::raw("  "),
            Span::styled("Esc cancel", footer_style),
            Span::raw("  "),
            Span::styled(scroll_info, footer_style),
        ]);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(styles.accent_text())
            .title(" Select Model ")
            .title_style(Style::default().fg(styles.accent).bold())
            .title_bottom(footer_text);

        let inner_area = block.inner(popup_area);

        // Render the list first.
        let list = List::new(items);
        f.render_widget(block, popup_area);
        f.render_widget(list, inner_area);

        // Render a summary line for the selected model below the list.
        if let Some(PopupEntry::ModelEntry { model, .. }) = self.entries.get(self.selected) {
            let summary = format_model_summary(model);
            // The summary area is 1 row at the bottom of the popup content area.
            let summary_y = inner_area.y + inner_area.height.saturating_sub(1);
            let summary_area = Rect::new(
                inner_area.x + 1,
                summary_y,
                inner_area.width.saturating_sub(2),
                1,
            );
            let para = Paragraph::new(summary)
                .style(Style::default().fg(styles.dim))
                .wrap(Wrap { trim: false });
            f.render_widget(para, summary_area);
        }
    }
}

// ── Entry building ───────────────────────────────────────────────────────

/// Build the flat entry list: one header per provider, then its models.
fn build_entries() -> Vec<PopupEntry> {
    let mut entries = Vec::new();
    for provider in all_providers() {
        let models: Vec<Model> = models::models_for_provider(provider);
        if models.is_empty() {
            continue;
        }
        let implemented = is_provider_implemented(provider);
        entries.push(PopupEntry::ProviderHeader {
            name: provider.to_string(),
            implemented,
        });
        for m in models {
            entries.push(PopupEntry::ModelEntry {
                provider: provider.to_string(),
                model: m,
            });
        }
    }
    entries
}

// ── Rendering helpers ────────────────────────────────────────────────────

/// Format a single popup entry as a ListItem.
fn entry_to_list_item<'a>(
    entry: &'a PopupEntry,
    selected: bool,
    styles: &'a Styles,
) -> ListItem<'a> {
    match entry {
        PopupEntry::ProviderHeader { name, implemented } => {
            let suffix = if *implemented {
                ""
            } else {
                " (not implemented)"
            };
            let text = format!("{name}{suffix}");
            let style = if !implemented {
                styles.dim_text()
            } else if selected {
                Style::default().fg(styles.bg).bg(styles.accent).bold()
            } else {
                Style::default().fg(styles.accent).bold()
            };
            ListItem::new(Line::from(Span::styled(text, style)))
        }
        PopupEntry::ModelEntry { model, provider } => {
            let implemented = is_provider_implemented(provider);
            let marker = if selected { "› " } else { "  " };
            let tokens = format_tokens(model.context_window);
            let vision = if model.supports_vision { "👁" } else { "  " };
            let thinking = if model.supports_thinking && !model.thinking_levels.is_empty() {
                let levels: Vec<&str> = model
                    .thinking_levels
                    .iter()
                    .filter(|l| !matches!(l, crate::llm::base::ThinkingLevel::None))
                    .map(|l| thinking_short(l))
                    .collect();
                if levels.is_empty() {
                    "---".to_string()
                } else {
                    levels.join("/")
                }
            } else {
                "---".to_string()
            };

            let line = format!(
                "{marker}{id:<30} {tokens:>6}  {vision}  {thinking:<12}",
                id = model.id,
            );

            let style = if !implemented {
                styles.dim_text()
            } else if selected {
                Style::default().fg(styles.bg).bg(styles.accent)
            } else {
                Style::default().fg(styles.fg)
            };

            ListItem::new(Line::from(Span::styled(line, style)))
        }
    }
}

/// Format a one-line summary shown at the bottom of the popup for the
/// selected model.
fn format_model_summary(model: &Model) -> String {
    let mut parts = vec![
        format!("{}", model.display_name),
        format!("{} ctx", format_tokens(model.context_window)),
    ];
    if model.supports_vision {
        parts.push("vision".into());
    }
    if model.supports_thinking && !model.thinking_levels.is_empty() {
        let levels: Vec<&str> = model
            .thinking_levels
            .iter()
            .filter(|l| !matches!(l, crate::llm::base::ThinkingLevel::None))
            .map(|l| thinking_short(l))
            .collect();
        if !levels.is_empty() {
            parts.push(format!("thinking: {}", levels.join("/")));
        }
    }
    format!(" {} ", parts.join(" | "))
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.0}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{}K", n / 1_000)
    } else {
        n.to_string()
    }
}

fn thinking_short(level: &crate::llm::base::ThinkingLevel) -> &'static str {
    match level {
        crate::llm::base::ThinkingLevel::None => "off",
        crate::llm::base::ThinkingLevel::Minimal => "min",
        crate::llm::base::ThinkingLevel::Low => "low",
        crate::llm::base::ThinkingLevel::Medium => "med",
        crate::llm::base::ThinkingLevel::High => "high",
        crate::llm::base::ThinkingLevel::XHigh => "xhi",
    }
}
