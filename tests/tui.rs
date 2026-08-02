use patent::model::{Match, Saturation, Source, Verdict};
use patent::tui::{App, Mode, SortKey};
use patent::verdict::CAVEAT;

fn verdict() -> Verdict {
    Verdict {
        level: Saturation::Saturated,
        headline: "Lots of prior art found in the sources checked.".into(),
        gaps: vec!["no Windows support".into(), "no async API".into()],
        sources_checked: vec![Source::Npm, Source::CratesIo, Source::GitHub],
        sources_failed: vec![],
        caveat: CAVEAT.to_string(),
    }
}

fn matches() -> Vec<Match> {
    vec![
        Match {
            name: "kill-port".into(),
            source: Source::Npm,
            url: "https://npmjs.com/package/kill-port".into(),
            description: "Kill process on a port".into(),
            popularity: Some(50_000),
            similarity: 0.85,
            last_updated: None,
        },
        Match {
            name: "fkill-cli".into(),
            source: Source::Npm,
            url: "https://npmjs.com/package/fkill-cli".into(),
            description: "Fabulously kill processes".into(),
            popularity: Some(10_000),
            similarity: 0.72,
            last_updated: None,
        },
        Match {
            name: "port-killer".into(),
            source: Source::CratesIo,
            url: "https://crates.io/crates/port-killer".into(),
            description: "Rust port killer utility".into(),
            popularity: Some(500),
            similarity: 0.60,
            last_updated: None,
        },
    ]
}

// -- construction ------------------------------------------------------------

#[test]
fn new_app_starts_in_normal_mode() {
    let v = verdict();
    let m = matches();
    let app = App::new("test idea", v, m);
    assert_eq!(app.mode(), Mode::Normal);
}

#[test]
fn new_app_starts_with_cursor_at_zero() {
    let v = verdict();
    let m = matches();
    let app = App::new("test idea", v, m);
    assert_eq!(app.cursor(), 0);
}

#[test]
fn new_app_has_empty_filter() {
    let v = verdict();
    let m = matches();
    let app = App::new("test idea", v, m);
    assert!(app.filter_text().is_empty());
}

#[test]
fn new_app_stores_idea() {
    let v = verdict();
    let m = matches();
    let app = App::new("kill port tool", v, m);
    assert_eq!(app.idea(), "kill port tool");
}

// -- scrolling ---------------------------------------------------------------

#[test]
fn scroll_down_increments_cursor() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.scroll_down();
    assert_eq!(app.cursor(), 1);
}

#[test]
fn scroll_down_wraps_to_first_item() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.scroll_down();
    app.scroll_down();
    assert_eq!(app.cursor(), 2);
    app.scroll_down();
    assert_eq!(app.cursor(), 0);
}

#[test]
fn scroll_up_decrements_cursor() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.scroll_down();
    app.scroll_down();
    app.scroll_up();
    assert_eq!(app.cursor(), 1);
}

#[test]
fn scroll_up_wraps_to_last_item() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.scroll_up();
    assert_eq!(app.cursor(), 2);
}

#[test]
fn scroll_on_empty_matches_stays_at_zero() {
    let v = verdict();
    let empty: Vec<Match> = vec![];
    let mut app = App::new("x", v, empty);
    app.scroll_down();
    assert_eq!(app.cursor(), 0);
    app.scroll_up();
    assert_eq!(app.cursor(), 0);
}

// -- filtering ---------------------------------------------------------------

#[test]
fn enter_filter_mode_switches_mode() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    assert_eq!(app.mode(), Mode::Filter);
}

#[test]
fn exit_filter_mode_returns_to_normal() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.exit_filter();
    assert_eq!(app.mode(), Mode::Normal);
}

#[test]
fn filter_char_appends_to_filter_text() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('r');
    app.filter_push('u');
    assert_eq!(app.filter_text(), "ru");
}

#[test]
fn filter_backspace_removes_last_char() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('a');
    app.filter_push('b');
    app.filter_pop();
    assert_eq!(app.filter_text(), "a");
}

#[test]
fn filter_backspace_on_empty_is_noop() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_pop();
    assert!(app.filter_text().is_empty());
}

#[test]
fn filtered_matches_returns_all_when_no_filter() {
    let v = verdict();
    let m = matches();
    let app = App::new("x", v, m);
    assert_eq!(app.visible_matches().len(), 3);
}

#[test]
fn filtered_matches_filters_by_name() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('f');
    app.filter_push('k');
    let visible = app.visible_matches();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].name, "fkill-cli");
}

#[test]
fn filtered_matches_filters_by_description() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('R');
    app.filter_push('u');
    app.filter_push('s');
    app.filter_push('t');
    let visible = app.visible_matches();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].name, "port-killer");
}

#[test]
fn filter_is_case_insensitive() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('K');
    app.filter_push('I');
    app.filter_push('L');
    app.filter_push('L');
    let visible = app.visible_matches();
    assert_eq!(visible.len(), 3);
}

#[test]
fn filter_resets_cursor_to_zero() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.scroll_down();
    app.scroll_down();
    app.enter_filter();
    app.filter_push('p');
    assert_eq!(app.cursor(), 0);
}

#[test]
fn cursor_clamps_when_filter_shrinks_visible() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.scroll_down();
    app.scroll_down();
    app.enter_filter();
    app.filter_push('f');
    app.filter_push('k');
    assert!(app.cursor() < app.visible_matches().len());
}

#[test]
fn exit_filter_clears_filter_text() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('x');
    app.exit_filter();
    assert!(app.filter_text().is_empty());
    assert_eq!(app.visible_matches().len(), 3);
}

// -- selected URL ------------------------------------------------------------

#[test]
fn selected_url_returns_current_match_url() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.scroll_down();
    assert_eq!(
        app.selected_url(),
        Some("https://npmjs.com/package/fkill-cli")
    );
}

#[test]
fn selected_url_returns_none_when_no_matches() {
    let v = verdict();
    let empty: Vec<Match> = vec![];
    let app = App::new("x", v, empty);
    assert_eq!(app.selected_url(), None);
}

#[test]
fn selected_url_respects_filter() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('p');
    app.filter_push('o');
    app.filter_push('r');
    app.filter_push('t');
    app.filter_push('-');
    app.filter_push('k');
    assert_eq!(
        app.selected_url(),
        Some("https://crates.io/crates/port-killer")
    );
}

// -- quit flag ---------------------------------------------------------------

#[test]
fn app_starts_running() {
    let v = verdict();
    let m = matches();
    let app = App::new("x", v, m);
    assert!(!app.should_quit());
}

#[test]
fn quit_sets_flag() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.quit();
    assert!(app.should_quit());
}

// -- confirm filter ----------------------------------------------------------

#[test]
fn confirm_filter_keeps_filter_text() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('f');
    app.filter_push('k');
    app.confirm_filter();
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.filter_text(), "fk");
    assert_eq!(app.visible_matches().len(), 1);
}

#[test]
fn confirm_filter_preserves_cursor() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('k'); // all 3 match "kill"
    app.confirm_filter();
    app.scroll_down();
    assert_eq!(app.cursor(), 1);
}

// -- scroll within filtered view ---------------------------------------------

#[test]
fn scroll_within_filtered_view_clamps_correctly() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('p');
    app.filter_push('o');
    app.filter_push('r');
    app.filter_push('t');
    // "kill-port" and "port-killer" match -> 2 visible
    let count = app.visible_matches().len();
    assert_eq!(count, 2);
    app.scroll_down();
    assert_eq!(app.cursor(), 1);
    app.scroll_down();
    assert_eq!(app.cursor(), 0); // wraps to first
}

// -- filter_pop widens visible set -------------------------------------------

#[test]
fn filter_pop_widens_visible_matches() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('f');
    app.filter_push('k');
    assert_eq!(app.visible_matches().len(), 1);
    app.filter_pop(); // back to "f" — "fkill-cli" still matches via "Fabulously"
    assert!(!app.visible_matches().is_empty());
    app.filter_pop(); // back to "" — all visible
    assert_eq!(app.visible_matches().len(), 3);
}

// -- single match boundary ---------------------------------------------------

#[test]
fn single_match_scroll_stays_at_zero() {
    let v = verdict();
    let m = vec![matches().remove(0)];
    let mut app = App::new("x", v, m);
    app.scroll_down();
    assert_eq!(app.cursor(), 0);
    app.scroll_up();
    assert_eq!(app.cursor(), 0);
}

// -- show more / expand ------------------------------------------------------

fn many_matches(n: usize) -> Vec<Match> {
    (0..n)
        .map(|i| Match {
            name: format!("tool-{i}"),
            source: Source::Npm,
            url: format!("https://example.com/{i}"),
            description: format!("Tool number {i}"),
            popularity: Some(1000 - i as u64),
            similarity: 1.0 - (i as f32 * 0.01),
            last_updated: None,
        })
        .collect()
}

#[test]
fn default_page_size_limits_displayed_matches() {
    let v = verdict();
    let m = many_matches(50);
    let app = App::new("x", v, m);
    assert_eq!(app.displayed_matches().len(), patent::tui::DEFAULT_PAGE);
    assert!(!app.is_expanded());
}

#[test]
fn fewer_than_page_size_shows_all() {
    let v = verdict();
    let m = many_matches(5);
    let app = App::new("x", v, m);
    assert_eq!(app.displayed_matches().len(), 5);
}

#[test]
fn toggle_expand_shows_all_matches() {
    let v = verdict();
    let m = many_matches(50);
    let mut app = App::new("x", v, m);
    app.toggle_expand();
    assert!(app.is_expanded());
    assert_eq!(app.displayed_matches().len(), 50);
}

#[test]
fn toggle_expand_twice_collapses_back() {
    let v = verdict();
    let m = many_matches(50);
    let mut app = App::new("x", v, m);
    app.toggle_expand();
    app.toggle_expand();
    assert!(!app.is_expanded());
    assert_eq!(app.displayed_matches().len(), patent::tui::DEFAULT_PAGE);
}

#[test]
fn scroll_wraps_within_displayed_page() {
    let v = verdict();
    let m = many_matches(50);
    let mut app = App::new("x", v, m);
    for _ in 0..patent::tui::DEFAULT_PAGE {
        app.scroll_down();
    }
    assert_eq!(app.cursor(), 0);
}

#[test]
fn expand_then_scroll_wraps_around() {
    let v = verdict();
    let m = many_matches(50);
    let mut app = App::new("x", v, m);
    app.toggle_expand();
    for _ in 0..50 {
        app.scroll_down();
    }
    assert_eq!(app.cursor(), 0);
}

#[test]
fn collapse_clamps_cursor_if_past_page() {
    let v = verdict();
    let m = many_matches(50);
    let mut app = App::new("x", v, m);
    app.toggle_expand();
    for _ in 0..30 {
        app.scroll_down();
    }
    assert_eq!(app.cursor(), 30);
    app.toggle_expand(); // collapse — cursor must clamp
    assert_eq!(app.cursor(), patent::tui::DEFAULT_PAGE - 1);
}

#[test]
fn filter_applies_to_displayed_matches() {
    let v = verdict();
    let m = many_matches(50);
    let mut app = App::new("x", v, m);
    app.enter_filter();
    app.filter_push('0'); // matches tool-0, tool-10, tool-20, tool-30, tool-40
    let displayed = app.displayed_matches();
    assert!(displayed.len() <= patent::tui::DEFAULT_PAGE);
    assert!(displayed.iter().all(|m| m.name.contains('0')));
}

#[test]
fn has_more_is_true_when_truncated() {
    let v = verdict();
    let m = many_matches(50);
    let app = App::new("x", v, m);
    assert!(app.has_more());
}

#[test]
fn has_more_is_false_when_all_fit() {
    let v = verdict();
    let m = many_matches(5);
    let app = App::new("x", v, m);
    assert!(!app.has_more());
}

#[test]
fn has_more_is_false_when_expanded() {
    let v = verdict();
    let m = many_matches(50);
    let mut app = App::new("x", v, m);
    app.toggle_expand();
    assert!(!app.has_more());
}

// -- verdict accessors -------------------------------------------------------

#[test]
fn verdict_accessor_returns_the_verdict() {
    let v = verdict();
    let m = matches();
    let app = App::new("x", v, m);
    assert_eq!(app.verdict().level, Saturation::Saturated);
    assert_eq!(app.verdict().gaps.len(), 2);
    assert_eq!(app.verdict().sources_checked.len(), 3);
    assert_eq!(app.verdict().caveat, CAVEAT);
}

// -- sorting -----------------------------------------------------------------

#[test]
fn default_sort_is_similarity_descending() {
    let v = verdict();
    let m = matches();
    let app = App::new("x", v, m);
    assert_eq!(app.sort(), SortKey::Similarity);
    let names: Vec<&str> = app
        .visible_matches()
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, ["kill-port", "fkill-cli", "port-killer"]);
}

#[test]
fn cycle_sort_advances_through_all_keys() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.cycle_sort();
    assert_eq!(app.sort(), SortKey::Popularity);
    app.cycle_sort();
    assert_eq!(app.sort(), SortKey::Name);
    app.cycle_sort();
    assert_eq!(app.sort(), SortKey::Similarity);
}

#[test]
fn sort_by_name_orders_alphabetically() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.cycle_sort(); // Popularity
    app.cycle_sort(); // Name
    let names: Vec<&str> = app
        .visible_matches()
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, ["fkill-cli", "kill-port", "port-killer"]);
}

#[test]
fn sort_by_popularity_orders_most_first() {
    let v = verdict();
    // Similarity order and popularity order deliberately disagree.
    let m = vec![
        Match {
            name: "niche-but-relevant".into(),
            source: Source::CratesIo,
            url: "u1".into(),
            description: String::new(),
            popularity: Some(10),
            similarity: 0.95,
            last_updated: None,
        },
        Match {
            name: "popular-but-loose".into(),
            source: Source::Npm,
            url: "u2".into(),
            description: String::new(),
            popularity: Some(99_999),
            similarity: 0.50,
            last_updated: None,
        },
    ];
    let mut app = App::new("x", v, m);
    assert_eq!(app.visible_matches()[0].name, "niche-but-relevant"); // similarity
    app.cycle_sort(); // Popularity
    assert_eq!(app.visible_matches()[0].name, "popular-but-loose");
}

#[test]
fn cycle_sort_resets_cursor_to_top() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.scroll_down();
    app.scroll_down();
    app.cycle_sort();
    assert_eq!(app.cursor(), 0);
}

#[test]
fn sort_composes_with_filter() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.cycle_sort(); // Popularity
    app.cycle_sort(); // Name
    app.enter_filter();
    for c in "port".chars() {
        app.filter_push(c);
    }
    // "kill-port" and "port-killer" match "port", returned name-sorted.
    let names: Vec<&str> = app
        .visible_matches()
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, ["kill-port", "port-killer"]);
}

// -- detail view & row selection ---------------------------------------------

#[test]
fn enter_detail_switches_to_detail_mode() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.enter_detail();
    assert_eq!(app.mode(), Mode::Detail);
    app.exit_detail();
    assert_eq!(app.mode(), Mode::Normal);
}

#[test]
fn enter_detail_is_noop_with_no_matches() {
    let v = verdict();
    let empty: Vec<Match> = vec![];
    let mut app = App::new("x", v, empty);
    app.enter_detail();
    assert_eq!(app.mode(), Mode::Normal);
}

#[test]
fn selected_match_tracks_cursor() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    assert_eq!(
        app.selected_match().map(|m| m.name.as_str()),
        Some("kill-port")
    );
    app.scroll_down();
    assert_eq!(
        app.selected_match().map(|m| m.name.as_str()),
        Some("fkill-cli")
    );
}

#[test]
fn select_row_sets_and_clamps_cursor() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("x", v, m);
    app.select_row(1);
    assert_eq!(app.cursor(), 1);
    app.select_row(99); // clamps to the last visible row
    assert_eq!(app.cursor(), 2);
}

#[test]
fn select_row_on_empty_is_noop() {
    let v = verdict();
    let empty: Vec<Match> = vec![];
    let mut app = App::new("x", v, empty);
    app.select_row(3);
    assert_eq!(app.cursor(), 0);
}

// ── interactive search: load_results round-trip ────────────────────────

#[test]
fn load_results_replaces_everything() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("first idea here", v, m);

    assert_eq!(app.displayed_matches().len(), 3);
    assert_eq!(app.idea(), "first idea here");

    let v2 = Verdict {
        level: Saturation::Open,
        headline: "Nothing close.".into(),
        gaps: vec![],
        sources_checked: vec![Source::PyPI],
        sources_failed: vec![],
        caveat: CAVEAT.to_string(),
    };
    let m2 = vec![Match {
        name: "new-tool".into(),
        source: Source::PyPI,
        url: "https://pypi.org/project/new-tool".into(),
        description: "brand new".into(),
        popularity: Some(1),
        similarity: 0.5,
        last_updated: None,
    }];
    app.load_results("second idea here".into(), v2, m2);

    assert_eq!(app.idea(), "second idea here");
    assert_eq!(app.verdict().level, Saturation::Open);
    assert_eq!(app.displayed_matches().len(), 1);
    assert_eq!(app.displayed_matches()[0].name, "new-tool");
    assert_eq!(app.cursor(), 0);
    assert_eq!(app.filter_text(), "");
    assert!(!app.is_expanded());
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.sort(), SortKey::Similarity);
}

#[test]
fn load_results_after_filtering_resets_filter() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("idea one here", v, m);
    app.enter_filter();
    app.filter_push('k');
    app.filter_push('i');
    app.filter_push('l');
    app.confirm_filter();
    assert!(!app.filter_text().is_empty(), "filter should be active");

    let v2 = verdict();
    app.load_results("idea two here".into(), v2, matches());
    assert_eq!(app.filter_text(), "");
    assert_eq!(app.displayed_matches().len(), 3); // all back
}

#[test]
fn load_results_after_scrolling_resets_cursor() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("idea scroll test", v, m);
    app.scroll_down();
    app.scroll_down();
    assert_eq!(app.cursor(), 2);

    let v2 = verdict();
    app.load_results("new idea here".into(), v2, matches());
    assert_eq!(app.cursor(), 0);
}

#[test]
fn load_results_after_detail_returns_to_normal() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("detail test idea", v, m);
    app.enter_detail();
    assert_eq!(app.mode(), Mode::Detail);

    let v2 = verdict();
    app.load_results("back to normal".into(), v2, matches());
    assert_eq!(app.mode(), Mode::Normal);
}

#[test]
fn load_results_with_empty_matches() {
    let v = verdict();
    let m = matches();
    let mut app = App::new("non empty start", v, m);
    assert!(!app.displayed_matches().is_empty());

    let v2 = Verdict {
        level: Saturation::Open,
        headline: "Nothing found.".into(),
        gaps: vec![],
        sources_checked: vec![Source::GitHub],
        sources_failed: vec![Source::Npm],
        caveat: CAVEAT.to_string(),
    };
    app.load_results("empty result idea".into(), v2, vec![]);
    assert!(app.displayed_matches().is_empty());
    assert_eq!(app.cursor(), 0);
    assert_eq!(app.verdict().sources_failed, vec![Source::Npm]);
}

#[test]
fn multiple_load_results_cycles() {
    let v = verdict();
    let mut app = App::new("round one", v, matches());
    assert_eq!(app.idea(), "round one");

    for i in 0..5 {
        let v2 = verdict();
        app.load_results(format!("round {}", i + 2), v2, matches());
        assert_eq!(app.idea(), format!("round {}", i + 2));
        assert_eq!(app.cursor(), 0);
        assert_eq!(app.mode(), Mode::Normal);
    }
}
