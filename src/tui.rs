//! TUI state machine.
//!
//! Pure logic for scrolling, filtering, and URL selection — no rendering or
//! terminal I/O. The binary's `tui` module handles drawing and events.

use crate::model::{Match, Verdict};

/// How many matches to show before the user presses "show more".
pub const DEFAULT_PAGE: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Filter,
    Help,
    /// Full-screen-ish popup with the selected match's untruncated details.
    Detail,
}

/// How the matches list is ordered. Cycled with `s` in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    /// Cosine similarity to the idea — the ranker's order, and the default.
    Similarity,
    /// Source popularity signal (stars, downloads…), most first.
    Popularity,
    /// Match name, A→Z (case-insensitive).
    Name,
}

impl SortKey {
    /// Short label shown in the matches-table title.
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Similarity => "similarity",
            SortKey::Popularity => "popularity",
            SortKey::Name => "name",
        }
    }

    fn next(self) -> Self {
        match self {
            SortKey::Similarity => SortKey::Popularity,
            SortKey::Popularity => SortKey::Name,
            SortKey::Name => SortKey::Similarity,
        }
    }
}

pub struct App<'a> {
    idea: &'a str,
    verdict: &'a Verdict,
    matches: &'a [Match],
    cursor: usize,
    filter: String,
    visible: Vec<usize>,
    mode: Mode,
    expanded: bool,
    quit: bool,
    sort: SortKey,
    detail_scroll: usize,
    status: Option<(String, std::time::Instant)>,
}

impl<'a> App<'a> {
    pub fn new(idea: &'a str, verdict: &'a Verdict, matches: &'a [Match]) -> Self {
        let visible = (0..matches.len()).collect();
        let mut app = Self {
            idea,
            verdict,
            matches,
            cursor: 0,
            filter: String::new(),
            visible,
            mode: Mode::Normal,
            expanded: false,
            quit: false,
            sort: SortKey::Similarity,
            detail_scroll: 0,
            status: None,
        };
        app.apply_sort();
        app
    }

    pub fn idea(&self) -> &str {
        self.idea
    }

    pub fn verdict(&self) -> &Verdict {
        self.verdict
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn filter_text(&self) -> &str {
        &self.filter
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn visible_matches(&self) -> Vec<&Match> {
        self.visible.iter().map(|&i| &self.matches[i]).collect()
    }

    /// The matches to render — respects page size unless expanded.
    pub fn displayed_matches(&self) -> Vec<&Match> {
        let limit = self.display_limit();
        self.visible
            .iter()
            .take(limit)
            .map(|&i| &self.matches[i])
            .collect()
    }

    /// True when there are more matches beyond the current page.
    pub fn has_more(&self) -> bool {
        !self.expanded && self.visible.len() > DEFAULT_PAGE
    }

    pub fn toggle_expand(&mut self) {
        self.expanded = !self.expanded;
        self.clamp_cursor();
    }

    pub fn scroll_down(&mut self) {
        let max = self.display_limit().saturating_sub(1);
        if self.cursor < max {
            self.cursor += 1;
        } else {
            self.cursor = 0;
        }
    }

    pub fn scroll_up(&mut self) {
        if self.cursor == 0 {
            self.cursor = self.display_limit().saturating_sub(1);
        } else {
            self.cursor -= 1;
        }
    }

    pub fn total_matches(&self) -> usize {
        self.matches.len()
    }

    pub fn scroll_to_top(&mut self) {
        self.cursor = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.cursor = self.display_limit().saturating_sub(1);
    }

    pub fn toggle_help(&mut self) {
        self.mode = if self.mode == Mode::Help {
            Mode::Normal
        } else {
            Mode::Help
        };
    }

    pub fn enter_filter(&mut self) {
        self.mode = Mode::Filter;
        self.cursor = 0;
    }

    pub fn confirm_filter(&mut self) {
        self.mode = Mode::Normal;
    }

    pub fn exit_filter(&mut self) {
        self.mode = Mode::Normal;
        self.filter.clear();
        self.recompute_visible();
        self.cursor = 0;
    }

    pub fn filter_push(&mut self, c: char) {
        self.filter.push(c);
        self.recompute_visible();
        self.clamp_cursor();
    }

    pub fn filter_pop(&mut self) {
        self.filter.pop();
        self.recompute_visible();
        self.clamp_cursor();
    }

    /// The match under the cursor within the displayed page, if any.
    pub fn selected_match(&self) -> Option<&Match> {
        let limit = self.display_limit();
        self.visible
            .iter()
            .take(limit)
            .nth(self.cursor)
            .map(|&i| &self.matches[i])
    }

    /// URL of the selected match, if any.
    pub fn selected_url(&self) -> Option<&str> {
        self.selected_match().map(|m| m.url.as_str())
    }

    /// Select a row by its position within the displayed page (used by mouse
    /// clicks). Clamped to the visible range; a no-op when nothing is shown.
    pub fn select_row(&mut self, row: usize) {
        let limit = self.display_limit();
        if limit == 0 {
            return;
        }
        self.cursor = row.min(limit - 1);
    }

    /// Open the detail popup for the selected match (no-op if none selected).
    pub fn enter_detail(&mut self) {
        if self.selected_match().is_some() {
            self.detail_scroll = 0;
            self.mode = Mode::Detail;
        }
    }

    /// Close the detail popup, returning to the list.
    pub fn exit_detail(&mut self) {
        self.detail_scroll = 0;
        self.mode = Mode::Normal;
    }

    pub fn detail_scroll_offset(&self) -> usize {
        self.detail_scroll
    }

    pub fn scroll_detail_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
    }

    pub fn scroll_detail_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), std::time::Instant::now()));
    }

    pub fn status_message(&self) -> Option<&str> {
        const DISPLAY_DURATION: std::time::Duration = std::time::Duration::from_secs(2);
        self.status
            .as_ref()
            .filter(|(_, t)| t.elapsed() < DISPLAY_DURATION)
            .map(|(msg, _)| msg.as_str())
    }

    /// Copy the selected match's URL to the system clipboard via `arboard`.
    /// Sets a status message indicating success or failure.
    pub fn yank_url(&mut self) {
        let Some(url) = self.selected_url() else {
            return;
        };
        let url = url.to_string();
        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&url)) {
            Ok(()) => self.set_status("Copied to clipboard"),
            Err(_) => self.set_status("Clipboard unavailable"),
        }
    }

    /// The current match sort order.
    pub fn sort(&self) -> SortKey {
        self.sort
    }

    /// Advance to the next sort order, re-sort, and jump back to the top.
    pub fn cycle_sort(&mut self) {
        self.sort = self.sort.next();
        self.apply_sort();
        self.cursor = 0;
    }

    fn display_limit(&self) -> usize {
        if self.expanded {
            self.visible.len()
        } else {
            self.visible.len().min(DEFAULT_PAGE)
        }
    }

    fn recompute_visible(&mut self) {
        if self.filter.is_empty() {
            self.visible = (0..self.matches.len()).collect();
        } else {
            let lower = self.filter.to_lowercase();
            self.visible = self
                .matches
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    m.name.to_lowercase().contains(&lower)
                        || m.description.to_lowercase().contains(&lower)
                })
                .map(|(i, _)| i)
                .collect();
        }
        self.apply_sort();
    }

    /// Order `visible` by the current [`SortKey`]. Stable, so equal keys keep
    /// their prior (similarity-ranked) order.
    fn apply_sort(&mut self) {
        let matches = self.matches;
        match self.sort {
            SortKey::Similarity => self.visible.sort_by(|&a, &b| {
                matches[b]
                    .similarity
                    .partial_cmp(&matches[a].similarity)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortKey::Popularity => self
                .visible
                .sort_by(|&a, &b| matches[b].popularity.cmp(&matches[a].popularity)),
            SortKey::Name => self.visible.sort_by(|&a, &b| {
                matches[a]
                    .name
                    .to_lowercase()
                    .cmp(&matches[b].name.to_lowercase())
            }),
        }
    }

    fn clamp_cursor(&mut self) {
        let limit = self.display_limit();
        if limit == 0 {
            self.cursor = 0;
        } else if self.cursor >= limit {
            self.cursor = limit - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Match, Saturation, Source, Verdict};
    use crate::verdict::CAVEAT;

    fn test_verdict() -> Verdict {
        Verdict {
            level: Saturation::Open,
            headline: "Nothing close found.".into(),
            gaps: vec![],
            sources_checked: vec![Source::GitHub],
            sources_failed: vec![],
            caveat: CAVEAT.to_string(),
        }
    }

    fn test_matches() -> Vec<Match> {
        vec![Match {
            name: "example-tool".into(),
            source: Source::CratesIo,
            url: "https://crates.io/crates/example-tool".into(),
            description: "an example".into(),
            popularity: Some(100),
            similarity: 0.8,
        }]
    }

    #[test]
    fn set_status_is_visible_immediately() {
        let v = test_verdict();
        let m = test_matches();
        let mut app = App::new("test idea for tools", &v, &m);
        app.set_status("hello");
        assert_eq!(app.status_message(), Some("hello"));
    }

    #[test]
    fn status_message_none_by_default() {
        let v = test_verdict();
        let m = test_matches();
        let app = App::new("test idea for tools", &v, &m);
        assert_eq!(app.status_message(), None);
    }

    #[test]
    fn yank_url_sets_status_message() {
        let v = test_verdict();
        let m = test_matches();
        let mut app = App::new("test idea for tools", &v, &m);
        app.yank_url();
        let msg = app.status_message().unwrap();
        assert!(
            msg == "Copied to clipboard" || msg == "Clipboard unavailable",
            "yank must set a status message, got: {msg}"
        );
    }

    #[test]
    fn yank_url_noop_when_no_selection() {
        let v = test_verdict();
        let m: Vec<Match> = vec![];
        let mut app = App::new("test idea for tools", &v, &m);
        app.yank_url();
        assert_eq!(app.status_message(), None);
    }
}
