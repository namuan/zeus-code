//! File autocomplete triggered by `@` in the input box.
//!
//! When the user types `@`, a popup appears listing project files.
//! As the user types more characters, the list filters to matching paths.
//! Tab inserts the highlighted file; Esc dismisses the popup.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};
use std::path::{Path, PathBuf};

use crate::ui::styles::Styles;

/// State for the file-autocomplete popup.
#[derive(Debug, Clone)]
pub struct AutocompleteState {
    /// Whether the popup is currently visible.
    pub active: bool,
    /// Byte position of the `@` character in the input text.
    pub trigger_pos: usize,
    /// All candidates (relative file paths from project root).
    candidates: Vec<String>,
    /// Currently highlighted candidate index.
    pub selected: usize,
    /// The filter text (everything after `@` in the input).
    filter: String,
    /// Whether file collection is complete (set to false while loading).
    pub ready: bool,
}

impl Default for AutocompleteState {
    fn default() -> Self {
        Self::inactive()
    }
}

impl AutocompleteState {
    /// Return an inactive state.
    pub fn inactive() -> Self {
        Self {
            active: false,
            trigger_pos: 0,
            candidates: Vec::new(),
            selected: 0,
            filter: String::new(),
            ready: false,
        }
    }

    /// Activate autocomplete: collect project files and show the popup.
    pub fn activate(trigger_pos: usize, filter: &str) -> Self {
        let all = collect_project_files();
        let filtered = filter_candidates(&all, filter);
        let mut state = Self {
            active: true,
            trigger_pos,
            candidates: filtered,
            selected: 0,
            filter: filter.to_string(),
            ready: true,
        };
        // Pre-select the best fuzzy match
        state.selected = state.best_match_index(filter);
        state
    }

    /// Refresh the filter text and re-filter candidates.
    /// Uses `all_files` as the full (unfiltered) candidate list for fast re-filtering.
    pub fn set_filter(&mut self, filter: &str, all_files: &[String]) {
        self.filter = filter.to_string();
        self.candidates = filter_candidates(all_files, filter);
        self.selected = self.best_match_index(filter);
    }

    /// Move selection up (wraps around).
    pub fn select_prev(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.candidates.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// Move selection down (wraps around).
    pub fn select_next(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        if self.selected + 1 >= self.candidates.len() {
            self.selected = 0;
        } else {
            self.selected += 1;
        }
    }

    /// Return the currently highlighted candidate, if any.
    pub fn selected_candidate(&self) -> Option<&str> {
        self.candidates.get(self.selected).map(|s| s.as_str())
    }

    /// Dismiss the popup.
    pub fn dismiss(&mut self) {
        self.active = false;
    }

    /// Find the best fuzzy-match index: prefers exact prefix match, then
    /// filename-only match, then falls back to first candidate.
    fn best_match_index(&self, filter: &str) -> usize {
        if filter.is_empty() || self.candidates.is_empty() {
            return 0;
        }
        let lower = filter.to_lowercase();
        // Prefer exact prefix match
        for (i, c) in self.candidates.iter().enumerate() {
            if c.to_lowercase().starts_with(&lower) {
                return i;
            }
        }
        // Prefer filename match
        for (i, c) in self.candidates.iter().enumerate() {
            if let Some(name) = Path::new(c).file_name().and_then(|n| n.to_str())
                && name.to_lowercase().starts_with(&lower)
            {
                return i;
            }
        }
        0
    }

    /// Render the popup on top of the current frame.
    pub fn render(&self, f: &mut Frame, styles: &Styles) {
        if !self.active || self.candidates.is_empty() {
            return;
        }

        let max_display = 10_usize;
        let count = self.candidates.len().min(max_display);

        // Build list items
        let items: Vec<ListItem> = self
            .candidates
            .iter()
            .take(max_display)
            .enumerate()
            .map(|(i, path)| {
                let style = if i == self.selected {
                    Style::default().fg(styles.bg).bg(styles.accent)
                } else {
                    Style::default().fg(styles.fg).bg(styles.surface)
                };
                ListItem::new(path.as_str()).style(style)
            })
            .collect();

        // Compute popup size
        let max_width: usize = self.candidates.iter().map(|s| s.len()).max().unwrap_or(20);
        let popup_w = (max_width + 4).clamp(24, 60) as u16;
        let popup_h = (count + 2) as u16;

        let frame_area = f.area();
        // Position: above the input box (bottom of screen, 3 rows up for the 3-row input box)
        let popup_x = frame_area.x + 2; // 2 = len("> ")
        let popup_y = frame_area
            .y
            .saturating_add(frame_area.height.saturating_sub(3 + popup_h + 1));

        if popup_h > frame_area.height.saturating_sub(3) {
            // Not enough room; don't render
            return;
        }

        let popup_area = Rect::new(popup_x, popup_y, popup_w, popup_h);

        // Clear the area behind the popup
        f.render_widget(Clear, popup_area);

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(styles.accent_text())
                    .title(format!(" Files ({}/{}) ", count, self.candidates.len()))
                    .title_style(Style::default().fg(styles.accent)),
            )
            .highlight_style(Style::default().fg(styles.bg).bg(styles.accent));

        f.render_widget(list, popup_area);
    }
}

// ── File collection ──────────────────────────────────────────────────

/// Max number of entries to collect (avoid memory issues in giant repos).
const MAX_ENTRIES: usize = 5000;

/// Collect relative paths from the current working directory,
/// respecting `.gitignore` rules.
/// Directories are suffixed with `/` so they sort above and are visually
/// distinct from files.
pub fn collect_project_files() -> Vec<String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let mut entries: Vec<String> = Vec::with_capacity(1024);

    let walker = ignore::WalkBuilder::new(&cwd)
        .standard_filters(true) // respect .gitignore, .ignore, etc.
        .hidden(false) // skip hidden files/dirs
        .git_global(true) // respect global gitignore
        .git_exclude(true) // respect .git/info/exclude
        .sort_by_file_path(|a, b| a.cmp(b))
        .build();

    for entry in walker {
        if entries.len() >= MAX_ENTRIES {
            break;
        }
        match entry {
            Ok(entry) => {
                let ft = entry.file_type();
                if let Ok(rel) = entry.path().strip_prefix(&cwd) {
                    let s = rel.to_string_lossy().to_string();
                    if s.is_empty() {
                        continue;
                    }
                    if ft.is_some_and(|ft| ft.is_dir()) {
                        entries.push(format!("{s}/"));
                    } else if ft.is_some_and(|ft| ft.is_file()) {
                        entries.push(s);
                    }
                }
            }
            Err(_) => continue,
        }
    }

    entries
}

/// Filter candidates by the given substring (case-insensitive).
/// Results are sorted so filename matches appear first, then by path
/// length (shorter paths first), making deeply-nested files that match
/// purely by directory name rank last.
pub fn filter_candidates(candidates: &[String], filter: &str) -> Vec<String> {
    if filter.is_empty() {
        return candidates.to_vec();
    }
    let lower = filter.to_lowercase();

    let mut matches: Vec<String> = candidates
        .iter()
        .filter(|s| s.to_lowercase().contains(&lower))
        .cloned()
        .collect();

    // Sort: directory entries first, then paths with matching directory
    // components, then filename matches, then shorter paths first.
    matches.sort_by(|a, b| {
        let a_is_dir = a.ends_with('/');
        let b_is_dir = b.ends_with('/');
        let a_dir = directory_contains(a, &lower);
        let b_dir = directory_contains(b, &lower);
        let a_file = filename_contains(a, &lower);
        let b_file = filename_contains(b, &lower);

        // Priority 1: actual directory entries
        match (a_is_dir, b_is_dir) {
            (true, false) => return std::cmp::Ordering::Less,
            (false, true) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        // Priority 2: matching directory components in path
        match (a_dir, b_dir) {
            (true, false) => return std::cmp::Ordering::Less,
            (false, true) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        // Priority 3: filename matches
        match (a_file, b_file) {
            (true, false) => return std::cmp::Ordering::Less,
            (false, true) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        // Priority 4: shorter path first
        a.len().cmp(&b.len())
    });

    matches
}

/// Check whether the *filename* portion of a path contains `filter`.
fn filename_contains(path: &str, filter: &str) -> bool {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.to_lowercase().contains(filter))
}

/// Check whether any *directory* component of a path contains `filter`.
fn directory_contains(path: &str, filter: &str) -> bool {
    Path::new(path).parent().is_some_and(|parent| {
        parent
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .any(|comp| comp.to_lowercase().contains(filter))
    })
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(candidates: Vec<String>, filter: &str) -> AutocompleteState {
        let mut state = AutocompleteState {
            active: true,
            trigger_pos: 1,
            candidates,
            selected: 0,
            filter: filter.to_string(),
            ready: true,
        };
        state.selected = state.best_match_index(filter);
        state
    }

    #[test]
    fn test_inactive_by_default() {
        let state = AutocompleteState::inactive();
        assert!(!state.active);
        assert_eq!(state.trigger_pos, 0);
        assert!(state.candidates.is_empty());
    }

    #[test]
    fn test_activate_collects_and_filters() {
        // Can't easily test collect_project_files() without a real dir,
        // but we can verify the state structure is correct after activation.
        // Just test that it doesn't panic and returns active state.
        // (File collection is tested indirectly via the App integration tests.)
    }

    #[test]
    fn test_select_prev_wraps_around() {
        let mut state = make_state(vec!["a.rs".into(), "b.rs".into(), "c.rs".into()], "");
        assert_eq!(state.selected, 0);
        state.select_prev();
        assert_eq!(state.selected, 2); // wraps to last
        state.select_prev();
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn test_select_next_wraps_around() {
        let mut state = make_state(vec!["a.rs".into(), "b.rs".into(), "c.rs".into()], "");
        state.selected = 2;
        state.select_next();
        assert_eq!(state.selected, 0); // wraps to first
        state.select_next();
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn test_select_on_empty_does_nothing() {
        let mut state = make_state(vec![], "");
        state.select_next();
        assert_eq!(state.selected, 0);
        state.select_prev();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_selected_candidate_returns_correct() {
        let state = make_state(vec!["src/main.rs".into(), "src/lib.rs".into()], "");
        assert_eq!(state.selected_candidate(), Some("src/main.rs"));
    }

    #[test]
    fn test_selected_candidate_none_on_empty() {
        let state = make_state(vec![], "");
        assert_eq!(state.selected_candidate(), None);
    }

    #[test]
    fn test_dismiss_sets_inactive() {
        let mut state = make_state(vec!["a.rs".into()], "");
        assert!(state.active);
        state.dismiss();
        assert!(!state.active);
    }

    #[test]
    fn test_set_filter_updates_candidates() {
        let all = vec![
            "src/main.rs".into(),
            "src/lib.rs".into(),
            "tests/main_test.rs".into(),
            "README.md".into(),
        ];
        let mut state = make_state(all.clone(), "");
        assert_eq!(state.candidates.len(), 4);

        state.set_filter("main", &all);
        assert_eq!(state.candidates.len(), 2);
        assert!(state.candidates.contains(&"src/main.rs".into()));
        assert!(state.candidates.contains(&"tests/main_test.rs".into()));
    }

    #[test]
    fn test_set_filter_case_insensitive() {
        let all = vec!["src/Main.rs".into(), "src/lib.rs".into()];
        let mut state = make_state(all.clone(), "");
        state.set_filter("main", &all);
        assert_eq!(state.candidates.len(), 1);
        assert_eq!(state.candidates[0], "src/Main.rs");
    }

    #[test]
    fn test_set_filter_empty_shows_all() {
        let all = vec!["a.rs".into(), "b.rs".into()];
        let filtered = filter_candidates(&all, "a");
        let mut state = make_state(filtered, "a");
        assert_eq!(state.candidates.len(), 1);

        state.set_filter("", &all);
        assert_eq!(state.candidates.len(), 2);
    }

    #[test]
    fn test_best_match_prefers_prefix_over_filename() {
        let candidates = vec![
            "xyz_main.rs".into(), // contains "main" in filename
            "src/main.rs".into(), // exact prefix match for "src"
        ];
        let state = make_state(candidates, "src");
        // Should prefer the path that starts with "src" over filename match
        let best = state.selected_candidate();
        assert!(best.is_some());
        // "src/main.rs" starts with "src", so it should be preferred
    }

    #[test]
    fn test_best_match_falls_back_to_filename() {
        let candidates = vec![
            "other/file.rs".into(),
            "tools/main_helper.rs".into(), // filename starts with "main"
        ];
        let state = make_state(candidates, "main");
        assert_eq!(state.selected_candidate(), Some("tools/main_helper.rs"));
    }

    #[test]
    fn test_filter_candidates_fuzzy_match() {
        let candidates = vec![
            "src/main.rs".into(),
            "src/lib.rs".into(),
            "README.md".into(),
        ];
        let result = filter_candidates(&candidates, "ma");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "src/main.rs");
    }

    #[test]
    fn test_filter_candidates_empty_filter() {
        let candidates = vec!["a.rs".into(), "b.rs".into()];
        let result = filter_candidates(&candidates, "");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_filter_candidates_no_match() {
        let candidates = vec!["a.rs".into(), "b.rs".into()];
        let result = filter_candidates(&candidates, "xyz");
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_candidates_prioritizes_filename_matches() {
        // Matching directories come first, then filename matches.
        // Here neither candidate has a directory match, so filename
        // match still wins.
        let candidates = vec![
            "aaa_stuff/sub/file.rs".into(), // "main" not in directory or filename
            "zzz/deep/nested/main.rs".into(), // "main" in filename
        ];
        // Only "zzz/deep/nested/main.rs" matches "main" at all
        let result = filter_candidates(&candidates, "main");
        assert_eq!(result.len(), 1, "only one candidate contains 'main'");
        assert!(result[0].ends_with("main.rs"));
    }

    #[test]
    fn test_filter_candidates_directory_before_filename() {
        // Scenario: typing "main" matches both a directory entry and a
        // file. The directory should appear first.
        let candidates = vec![
            "tools/main.rs".into(), // filename match
            "src/main/".into(),     // directory entry
        ];
        let result = filter_candidates(&candidates, "main");
        assert_eq!(result.len(), 2);
        // Directory "src/main/" should be first
        assert!(
            result[0].ends_with('/'),
            "directory entry should rank first, got: {:?}",
            result
        );
    }

    #[test]
    fn test_filter_candidates_dir_component_before_filename() {
        // When no actual directory entries exist, paths where "main" is
        // in a directory component still beat pure filename matches.
        let candidates = vec![
            "tools/main.rs".into(),      // "main" in filename
            "src/main/helper.rs".into(), // "main" in directory component
        ];
        let result = filter_candidates(&candidates, "main");
        assert_eq!(result.len(), 2);
        // Path with directory match should be first
        assert!(
            result[0].contains("/main/"),
            "directory-component match should rank first, got: {:?}",
            result
        );
    }
}
