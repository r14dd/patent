//! ratatui interface.
//!
//! Header (idea + sources-checked transparency line), verdict panel
//! (🟢/🟡/🔴 + headline + gaps + caveat), and a scrollable/filterable matches
//! table. `↑/↓` scroll, `/` filter, `m` show more, `s` sort, `Enter` details,
//! `o` open URL, `?` help, `q` quit; the mouse wheel scrolls and a click
//! selects a row.

use patent::model::{Match, Saturation, Source, Verdict};
use patent::tui::{App, Mode};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, HighlightSpacing, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Table, TableState, Wrap,
    },
    DefaultTerminal, Frame,
};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};

use std::time::Duration;

/// Top-level TUI phase: search input, loading, or viewing results.
enum Phase {
    /// User is typing a query.
    Search { input: String, cursor: usize },
    /// Search pipeline is running.
    Searching { idea: String, tick: usize },
    /// Viewing results (delegates to App).
    Results,
}

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

/// LLM backend configuration forwarded from the CLI into the interactive TUI.
#[derive(Debug, Clone)]
pub struct TuiCfg {
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub fast: bool,
    pub keyword_only: bool,
    pub sources_include: Option<std::collections::HashSet<patent::Source>>,
    pub sources_exclude: std::collections::HashSet<patent::Source>,
}

fn level_icon(level: Saturation) -> &'static str {
    match level {
        Saturation::Open => "🟢",
        Saturation::Crowded => "🟡",
        Saturation::Saturated => "🔴",
    }
}

fn level_color(level: Saturation) -> Color {
    match level {
        Saturation::Open => Color::Green,
        Saturation::Crowded => Color::Yellow,
        Saturation::Saturated => Color::Red,
    }
}

fn score_color(sim: f32) -> Color {
    if sim >= 0.7 {
        Color::Green
    } else if sim >= 0.4 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn source_color(source: Source) -> Color {
    match source {
        Source::CratesIo => Color::Yellow,
        Source::GitHub => Color::White,
        Source::Npm => Color::Red,
        Source::PyPI => Color::Blue,
        Source::HackerNews => Color::Rgb(255, 102, 0),
        Source::Go => Color::Cyan,
        Source::Maven => Color::Rgb(200, 50, 50),
        Source::RubyGems => Color::Magenta,
        Source::DockerHub => Color::Rgb(30, 144, 255),
        Source::VsCodeMarketplace => Color::Rgb(0, 122, 204),
        Source::NuGet => Color::Rgb(100, 45, 170),
        Source::Homebrew => Color::Rgb(250, 175, 5),
        Source::Packagist => Color::Rgb(119, 123, 180),
        Source::Hex => Color::Rgb(110, 74, 126),
        Source::ArtifactHub => Color::Rgb(65, 117, 152),
        Source::Aur => Color::Rgb(23, 147, 209),
        Source::Hackage => Color::Rgb(94, 80, 134),
        Source::Nixpkgs => Color::Rgb(126, 186, 228),
    }
}

/// The plain text of a styled line (span contents concatenated).
fn line_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Rows `text` occupies when word-wrapped to `width` columns.
///
/// Mirrors ratatui's `Wrap { trim: false }` (greedy word packing, hard-splitting
/// any word longer than the line) closely enough to never *under*-count for
/// normal text — over-counting is harmless (the table just gets a row fewer),
/// but under-counting would clip the integrity-critical caveat.
fn wrapped_rows(text: &str, width: u16) -> u16 {
    let width = width.max(1) as usize;
    let mut rows: u16 = 1;
    let mut col = 0usize;
    let mut first = true;
    for word in text.split(' ') {
        let wlen = word.chars().count();
        let needed = if first { wlen } else { col + 1 + wlen };
        if !first && needed > width {
            rows = rows.saturating_add(1);
            col = 0;
            first = true;
        }
        if first && wlen > width {
            // A single word longer than the line is hard-split across rows.
            rows = rows.saturating_add(((wlen - 1) / width) as u16);
            col = wlen - ((wlen - 1) / width) * width;
        } else if first {
            col = wlen;
        } else {
            col += 1 + wlen;
        }
        first = false;
    }
    rows.max(1)
}

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn draw_search(frame: &mut Frame, input: &str, cursor: usize) {
    let area = frame.area();
    let [_, center, _] = Layout::vertical([
        Constraint::Percentage(35),
        Constraint::Min(7),
        Constraint::Percentage(35),
    ])
    .areas(area);
    let [_, box_area, _] = Layout::horizontal([
        Constraint::Percentage(15),
        Constraint::Min(30),
        Constraint::Percentage(15),
    ])
    .areas(center);

    let [title_area, _, input_area, _, hint_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(box_area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "patent",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  prior-art search for code ideas",
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]))
    .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(title, title_area);

    let display_input = if input.is_empty() {
        Line::from(Span::styled(
            "describe your dev-tool idea...",
            Style::default().fg(MUTED),
        ))
    } else {
        Line::from(Span::raw(input))
    };
    let input_widget = Paragraph::new(display_input).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(ACCENT)),
    );
    frame.render_widget(input_widget, input_area);

    // Place cursor inside the input box.
    let cursor_x = input_area.x + 1 + (cursor as u16).min(input_area.width.saturating_sub(2));
    let cursor_y = input_area.y + 1;
    frame.set_cursor_position((cursor_x, cursor_y));

    let hint = Paragraph::new(Line::from(vec![
        key_span("Enter"),
        label_span(" search  "),
        key_span("Esc"),
        label_span(" quit"),
    ]))
    .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(hint, hint_area);
}

fn draw_searching(frame: &mut Frame, idea: &str, tick: usize) {
    let area = frame.area();
    let [_, center, _] = Layout::vertical([
        Constraint::Percentage(35),
        Constraint::Min(5),
        Constraint::Percentage(35),
    ])
    .areas(area);
    let [_, box_area, _] = Layout::horizontal([
        Constraint::Percentage(15),
        Constraint::Min(30),
        Constraint::Percentage(15),
    ])
    .areas(center);

    let spinner = SPINNER[tick % SPINNER.len()];
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" {spinner} "),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("Searching...", Style::default().fg(Color::White)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::raw("   "),
            Span::styled(idea, Style::default().fg(ACCENT)),
        ]),
        Line::raw(""),
        Line::from(vec![Span::raw("   "), label_span("Esc to cancel")]),
    ];
    let panel = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(ACCENT)),
    );
    frame.render_widget(panel, box_area);
}

fn draw(frame: &mut Frame, app: &App, table_state: &mut TableState) -> Rect {
    let area = frame.area();
    let width = area.width;
    let verdict = app.verdict();

    // Build the verdict lines first so the panel can be sized to fit them. The
    // humble caveat is the last line and MUST always be visible (integrity
    // rule), so the panel is never capped — the table takes whatever remains.
    let color = level_color(verdict.level);
    let mut verdict_lines = vec![
        Line::from(vec![
            Span::raw(" "),
            Span::styled(
                format!("{} {}", level_icon(verdict.level), verdict.level),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" — ", Style::default().add_modifier(Modifier::DIM)),
            Span::styled(
                &verdict.headline,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::raw(""),
    ];
    for gap in &verdict.gaps {
        verdict_lines.push(Line::from(vec![
            Span::styled("  • ", Style::default().fg(Color::Yellow)),
            Span::styled(gap.as_str(), Style::default().fg(Color::White)),
        ]));
    }
    verdict_lines.push(Line::raw(""));
    verdict_lines.push(Line::from(Span::styled(
        format!(" ⚠  {}", verdict.caveat),
        Style::default()
            .add_modifier(Modifier::DIM)
            .add_modifier(Modifier::ITALIC),
    )));

    // Header height: idea + sources (+ optional "not reached") + bottom border.
    let header_content = if verdict.sources_failed.is_empty() {
        2
    } else {
        3
    };
    let header_height = header_content + 1;

    // Verdict height: sum of word-wrapped content rows, plus panel chrome.
    // Never capped — the table takes whatever's left — so the last line (the
    // humble caveat) is always allocated space.
    let verdict_rows: u16 = verdict_lines
        .iter()
        .map(|l| wrapped_rows(&line_text(l), width))
        .sum();
    // +2 for the panel chrome (the `.title(" Verdict ")` row + the bottom
    // border), +1 slack so a unicode-width rounding difference (e.g. the ⚠
    // glyph) can never clip the caveat by a row.
    let verdict_height = verdict_rows + 3;

    let [header_area, verdict_area, table_area, footer_area] = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Length(verdict_height),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    // -- header
    let sources: Vec<Span> = verdict
        .sources_checked
        .iter()
        .enumerate()
        .flat_map(|(i, s)| {
            let mut spans = Vec::new();
            if i > 0 {
                spans.push(Span::styled(
                    " · ",
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            spans.push(Span::styled(
                s.to_string(),
                Style::default().fg(source_color(*s)),
            ));
            spans
        })
        .collect();

    let mut source_line = vec![Span::styled(
        " Sources: ",
        Style::default().add_modifier(Modifier::DIM),
    )];
    source_line.extend(sources);

    let mut header_lines = vec![
        Line::from(vec![
            Span::raw(" "),
            Span::styled(
                app.idea(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(source_line),
    ];
    // Transparency: selected sources that failed are surfaced, not hidden, so a
    // thin result is never mistaken for "nothing out there."
    if !verdict.sources_failed.is_empty() {
        let mut nr = vec![Span::styled(
            " Not reached: ",
            Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
        )];
        for (i, s) in verdict.sources_failed.iter().enumerate() {
            if i > 0 {
                nr.push(Span::styled(
                    " · ",
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            nr.push(Span::styled(s.to_string(), Style::default().fg(MUTED)));
        }
        header_lines.push(Line::from(nr));
    }

    let header = Paragraph::new(header_lines).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(MUTED)),
    );
    frame.render_widget(header, header_area);

    // -- verdict panel
    let verdict_panel = Paragraph::new(verdict_lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(MUTED))
                .title(Span::styled(
                    " Verdict ",
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )),
        );
    frame.render_widget(verdict_panel, verdict_area);

    // -- matches table (stateful so it scrolls to keep the selection visible)
    let displayed = app.displayed_matches();
    let total_visible = app.visible_matches().len();

    let rows: Vec<Row> = displayed
        .iter()
        .map(|m| {
            Row::new(vec![
                Cell::from(format!("{:.2}", m.similarity))
                    .style(Style::default().fg(score_color(m.similarity))),
                Cell::from(m.name.as_str()).style(
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from(m.source.to_string()).style(
                    Style::default()
                        .fg(source_color(m.source))
                        .add_modifier(Modifier::DIM),
                ),
                Cell::from(m.description.as_str())
                    .style(Style::default().add_modifier(Modifier::DIM)),
            ])
        })
        .collect();

    let title = if app.mode() == Mode::Filter {
        format!(
            " Matches [/{}] ({}/{}) ",
            app.filter_text(),
            total_visible,
            app.total_matches()
        )
    } else if !app.filter_text().is_empty() {
        format!(
            " Matches [{}] ({}/{}) ",
            app.filter_text(),
            total_visible,
            app.total_matches()
        )
    } else if app.has_more() {
        format!(
            " Matches ({} of {} — m for all) ",
            displayed.len(),
            total_visible
        )
    } else if app.is_expanded() {
        format!(" Matches (all {}) ", displayed.len())
    } else {
        format!(" Matches ({}) ", displayed.len())
    };
    let title = format!("{title}· sort: {} ", app.sort().label());

    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Fill(2),
        ],
    )
    .header(
        Row::new(vec!["Score", "Name", "Source", "Description"])
            .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .bottom_margin(1),
    )
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .highlight_spacing(HighlightSpacing::Never)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(ACCENT))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
    );

    if displayed.is_empty() {
        table_state.select(None);
    } else {
        table_state.select(Some(app.cursor().min(displayed.len() - 1)));
    }
    frame.render_stateful_widget(table, table_area, table_state);

    // -- scrollbar: only when there's more than fits in the table viewport.
    // Chrome = top border + header row + header bottom_margin + bottom border.
    let viewport = table_area.height.saturating_sub(4);
    if (displayed.len() as u16) > viewport && viewport > 0 {
        let mut sb_state = ScrollbarState::new(displayed.len().saturating_sub(viewport as usize))
            .position(table_state.offset());
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::default().fg(ACCENT)),
            table_area.inner(ratatui::layout::Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut sb_state,
        );
    }

    // -- footer hint bar
    let footer_spans = match app.mode() {
        Mode::Normal => {
            let mut spans = vec![
                key_span(" ↑↓"),
                label_span(" scroll  "),
                key_span("/"),
                label_span(" filter  "),
            ];
            if app.has_more() {
                spans.extend([key_span("m"), label_span(" more  ")]);
            } else if app.is_expanded() {
                spans.extend([key_span("m"), label_span(" less  ")]);
            }
            spans.extend([
                key_span("Enter"),
                label_span(" details  "),
                key_span("o"),
                label_span(" open  "),
                key_span("y"),
                label_span(" copy  "),
                key_span("s"),
                label_span(" sort  "),
                key_span("n"),
                label_span(" new search  "),
                key_span("?"),
                label_span(" help  "),
                key_span("q"),
                label_span(" quit"),
            ]);
            spans
        }
        Mode::Filter => vec![
            label_span(" type to filter  "),
            key_span("Esc"),
            label_span(" cancel  "),
            key_span("Enter"),
            label_span(" confirm"),
        ],
        Mode::Help => vec![
            label_span(" "),
            key_span("?"),
            label_span(" or "),
            key_span("Esc"),
            label_span(" close help"),
        ],
        Mode::Detail => vec![
            key_span(" ↑↓"),
            label_span(" scroll  "),
            key_span("o"),
            label_span(" / "),
            key_span("Enter"),
            label_span(" open  "),
            key_span("y"),
            label_span(" copy  "),
            key_span("Esc"),
            label_span(" close"),
        ],
    };
    let footer_line = if let Some(status) = app.status_message() {
        Line::from(Span::styled(
            format!(" {status}"),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        Line::from(footer_spans)
    };
    let footer = Paragraph::new(footer_line);
    frame.render_widget(footer, footer_area);

    // -- overlays (drawn last so they float above everything)
    if app.mode() == Mode::Help {
        draw_help(frame);
    }
    if app.mode() == Mode::Detail {
        draw_detail(frame, app);
    }

    table_area
}

/// Word-wrap `text` to `width` columns, returning one string per visual line.
fn word_wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current.clone());
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Floating popup with the selected match's full details.
fn draw_detail(frame: &mut Frame, app: &App) {
    let Some(m) = app.selected_match() else {
        return;
    };
    let area = centered_rect(74, 62, frame.area());
    frame.render_widget(Clear, area);

    let score_col = score_color(m.similarity);

    // Inner width for word-wrapping: popup minus borders (2) minus indent (2).
    let inner_w = (area.width as usize).saturating_sub(4);

    let desc_lines = word_wrap(&m.description, inner_w);
    // Fixed chrome: top border + empty + name + meta + empty + empty + url + empty + bottom border
    let chrome: u16 = 9;
    let viewport = (area.height.saturating_sub(chrome)) as usize;
    let max_scroll = desc_lines.len().saturating_sub(viewport.max(1));
    let scroll = app.detail_scroll_offset().min(max_scroll);

    let pop_str = match m.popularity {
        None => "—".to_string(),
        Some(p) if p >= 1_000_000 => format!("{}M ★", p / 1_000_000),
        Some(p) if p >= 1_000 => format!("{}k ★", p / 1_000),
        Some(p) => format!("{p} ★"),
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    lines.push(Line::from(Span::styled(
        format!("  {}", m.name),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            m.source.to_string(),
            Style::default().fg(source_color(m.source)),
        ),
        Span::styled("  ·  ", Style::default().fg(MUTED)),
        Span::styled(
            format!("{:.2}", m.similarity),
            Style::default().fg(score_col).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::default().fg(MUTED)),
        Span::styled(pop_str, Style::default().fg(MUTED)),
    ]));

    lines.push(Line::raw(""));

    let shown: Vec<&str> = desc_lines
        .iter()
        .skip(scroll)
        .take(viewport.max(1))
        .map(|s| s.as_str())
        .collect();

    if shown.is_empty() {
        lines.push(Line::from(Span::styled("  —", Style::default().fg(MUTED))));
    } else {
        for seg in &shown {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(*seg, Style::default().fg(Color::White)),
            ]));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  ↳ ", Style::default().fg(MUTED)),
        Span::styled(
            m.url.as_str(),
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::UNDERLINED),
        ),
    ]));
    lines.push(Line::raw(""));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(score_col))
        .title(Line::from(Span::styled(
            format!(" {} ", m.name),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )))
        .title(
            Line::from(Span::styled(
                format!(" {:.2} ", m.similarity),
                Style::default().fg(score_col).add_modifier(Modifier::BOLD),
            ))
            .right_aligned(),
        );

    frame.render_widget(Paragraph::new(lines).block(block), area);

    if desc_lines.len() > viewport.max(1) {
        let mut sb = ScrollbarState::new(max_scroll).position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::default().fg(score_col)),
            area.inner(ratatui::layout::Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut sb,
        );
    }
}

fn draw_help(frame: &mut Frame) {
    let area = centered_rect(56, 84, frame.area());
    frame.render_widget(Clear, area);

    let lines = vec![
        Line::raw(""),
        help_section("Navigation"),
        help_row("↑ / k", "Scroll up"),
        help_row("↓ / j", "Scroll down"),
        help_row("g / Home", "Jump to top"),
        help_row("G / End", "Jump to bottom"),
        Line::raw(""),
        help_section("Actions"),
        help_row("Enter", "Show match details"),
        help_row("o", "Open in browser"),
        help_row("y", "Copy URL to clipboard"),
        help_row("s", "Cycle sort (similarity/popularity/name)"),
        help_row("/", "Filter matches"),
        help_row("m", "Show more / less"),
        help_row("n", "New search"),
        help_row("?", "Toggle this help"),
        help_row("q", "Quit"),
        Line::raw(""),
        help_section("Mouse"),
        help_row("wheel", "Scroll the list"),
        help_row("click", "Select a row"),
        Line::raw(""),
        help_section("Filter mode"),
        help_row("Esc", "Cancel filter"),
        help_row("Enter", "Confirm filter"),
        help_row("Backspace", "Delete character"),
        Line::raw(""),
    ];

    let help = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(ACCENT))
            .title(Span::styled(
                " Keybindings ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
    );
    frame.render_widget(help, area);
}

fn help_section(title: &str) -> Line<'_> {
    Line::from(Span::styled(
        format!("  {title}"),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ))
}

fn help_row<'a>(key: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("  {key:>12}  "),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc, Style::default().fg(Color::White)),
    ])
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, vert, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);
    let [_, horiz, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(vert);
    horiz
}

fn key_span(text: &str) -> Span<'_> {
    Span::styled(
        text,
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )
}

fn label_span(text: &str) -> Span<'_> {
    Span::styled(text, Style::default().add_modifier(Modifier::DIM))
}

fn handle_result_event(
    app: &mut App,
    phase: &mut Phase,
    table_state: &TableState,
    table_area: Rect,
) -> std::io::Result<()> {
    use crossterm::event::{
        self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    };

    match event::read()? {
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return Ok(());
            }

            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                app.quit();
                return Ok(());
            }

            match app.mode() {
                Mode::Normal => match key.code {
                    KeyCode::Char('q') => app.quit(),
                    KeyCode::Char('n') => {
                        *phase = Phase::Search {
                            input: String::new(),
                            cursor: 0,
                        };
                    }
                    KeyCode::Down | KeyCode::Char('j') => app.scroll_down(),
                    KeyCode::Up | KeyCode::Char('k') => app.scroll_up(),
                    KeyCode::Home | KeyCode::Char('g') => app.scroll_to_top(),
                    KeyCode::End | KeyCode::Char('G') => app.scroll_to_bottom(),
                    KeyCode::Char('/') => app.enter_filter(),
                    KeyCode::Char('m') => app.toggle_expand(),
                    KeyCode::Char('s') => app.cycle_sort(),
                    KeyCode::Char('?') => app.toggle_help(),
                    KeyCode::Char('o') => open_selected(app),
                    KeyCode::Char('y') => app.yank_url(),
                    KeyCode::Enter => app.enter_detail(),
                    _ => {}
                },
                Mode::Filter => match key.code {
                    KeyCode::Esc => app.exit_filter(),
                    KeyCode::Backspace => app.filter_pop(),
                    KeyCode::Enter => app.confirm_filter(),
                    KeyCode::Char(c) => app.filter_push(c),
                    _ => {}
                },
                Mode::Help => match key.code {
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => app.toggle_help(),
                    _ => {}
                },
                Mode::Detail => match key.code {
                    KeyCode::Char('o') | KeyCode::Enter => open_selected(app),
                    KeyCode::Char('y') => app.yank_url(),
                    KeyCode::Down | KeyCode::Char('j') => app.scroll_detail_down(),
                    KeyCode::Up | KeyCode::Char('k') => app.scroll_detail_up(),
                    KeyCode::Esc | KeyCode::Char('q') => app.exit_detail(),
                    _ => {}
                },
            }
        }
        Event::Mouse(mouse) if app.mode() == Mode::Normal => match mouse.kind {
            MouseEventKind::ScrollDown => app.scroll_down(),
            MouseEventKind::ScrollUp => app.scroll_up(),
            MouseEventKind::Down(MouseButton::Left) => {
                let first_row = table_area.y.saturating_add(3);
                let last_row = table_area
                    .y
                    .saturating_add(table_area.height)
                    .saturating_sub(1);
                let within_x = mouse.column >= table_area.x
                    && mouse.column < table_area.x.saturating_add(table_area.width);
                if within_x && mouse.row >= first_row && mouse.row < last_row {
                    let row = (mouse.row - first_row) as usize;
                    app.select_row(table_state.offset() + row);
                }
            }
            _ => {}
        },
        _ => {}
    }

    Ok(())
}

fn is_safe_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// Open the selected match's URL in the default browser, if any.
fn open_selected(app: &App) {
    if let Some(url) = app.selected_url() {
        if is_safe_url(url) {
            let _ = open::that(url);
        }
    }
}

/// Launch the TUI with pre-computed results (CLI provided an idea).
pub async fn run_with_results(
    idea: &str,
    verdict: Verdict,
    matches: Vec<Match>,
    cfg: TuiCfg,
) -> anyhow::Result<()> {
    let app = App::new(idea, verdict, matches);
    run_tui(app, Phase::Results, cfg).await
}

/// Launch the TUI in interactive search mode (no idea on CLI).
pub async fn run_interactive(cfg: TuiCfg) -> anyhow::Result<()> {
    let app = App::new(
        "",
        Verdict {
            level: Saturation::Open,
            headline: String::new(),
            gaps: vec![],
            sources_checked: vec![],
            sources_failed: vec![],
            caveat: String::new(),
        },
        vec![],
    );
    run_tui(
        app,
        Phase::Search {
            input: String::new(),
            cursor: 0,
        },
        cfg,
    )
    .await
}

async fn run_tui(app: App, initial_phase: Phase, cfg: TuiCfg) -> anyhow::Result<()> {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
        ratatui::restore();
        original_hook(info);
    }));

    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);

    let result = run_loop(&mut terminal, app, initial_phase, cfg).await;

    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

/// Result of the search pipeline, sent back from the worker task.
struct SearchResult {
    idea: String,
    verdict: Verdict,
    matches: Vec<Match>,
}

async fn run_loop(
    terminal: &mut DefaultTerminal,
    mut app: App,
    initial_phase: Phase,
    cfg: TuiCfg,
) -> anyhow::Result<()> {
    let mut table_state = TableState::default();
    let mut table_area = Rect::default();
    let mut phase = initial_phase;
    let mut search_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<SearchResult>>> = None;

    loop {
        terminal.draw(|frame| match &phase {
            Phase::Search { input, cursor } => draw_search(frame, input, *cursor),
            Phase::Searching { idea, tick } => draw_searching(frame, idea, *tick),
            Phase::Results => {
                table_area = draw(frame, &app, &mut table_state);
            }
        })?;

        // In Searching phase, poll for results and tick the spinner.
        if let Phase::Searching { tick, idea } = &mut phase {
            if let Some(rx) = &mut search_rx {
                match rx.try_recv() {
                    Ok(Ok(result)) => {
                        app.load_results(result.idea, result.verdict, result.matches);
                        table_state = TableState::default();
                        phase = Phase::Results;
                        search_rx = None;
                        continue;
                    }
                    Ok(Err(e)) => {
                        app.load_results(
                            idea.clone(),
                            Verdict {
                                level: Saturation::Open,
                                headline: format!("Search failed: {e}"),
                                gaps: vec![],
                                sources_checked: vec![],
                                sources_failed: vec![],
                                caveat: patent::verdict::CAVEAT.to_string(),
                            },
                            vec![],
                        );
                        table_state = TableState::default();
                        phase = Phase::Results;
                        search_rx = None;
                        continue;
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
                    Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                        phase = Phase::Search {
                            input: idea.clone(),
                            cursor: idea.len(),
                        };
                        search_rx = None;
                        continue;
                    }
                }
            }
            *tick = tick.wrapping_add(1);
        }

        // Non-blocking event poll.
        if !crossterm::event::poll(Duration::from_millis(80))? {
            continue;
        }

        match &mut phase {
            Phase::Search { input, cursor } => {
                if handle_search_event(input, cursor, &mut app)? {
                    let idea = input.trim().to_string();
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    search_rx = Some(rx);
                    phase = Phase::Searching {
                        idea: idea.clone(),
                        tick: 0,
                    };
                    tokio::spawn(run_search_pipeline(idea, tx, cfg.clone()));
                }
            }
            Phase::Searching { idea, .. } => {
                use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if key.code == KeyCode::Esc
                        || (key.modifiers.contains(KeyModifiers::CONTROL)
                            && key.code == KeyCode::Char('c'))
                    {
                        phase = Phase::Search {
                            input: idea.clone(),
                            cursor: idea.len(),
                        };
                        search_rx = None;
                    }
                }
            }
            Phase::Results => {
                handle_result_event(&mut app, &mut phase, &table_state, table_area)?;
            }
        }

        if app.should_quit() {
            return Ok(());
        }
    }
}

/// Returns true when the user submits a search (Enter on non-empty input).
fn handle_search_event(
    input: &mut String,
    cursor: &mut usize,
    app: &mut App,
) -> std::io::Result<bool> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                app.quit();
                return Ok(false);
            }
            match key.code {
                KeyCode::Enter if !input.trim().is_empty() => return Ok(true),
                KeyCode::Char(c) => {
                    input.insert(*cursor, c);
                    *cursor += c.len_utf8();
                }
                KeyCode::Backspace if *cursor > 0 => {
                    let prev = input[..*cursor].char_indices().last().map_or(0, |(i, _)| i);
                    input.drain(prev..*cursor);
                    *cursor = prev;
                }
                KeyCode::Left if *cursor > 0 => {
                    *cursor = input[..*cursor].char_indices().last().map_or(0, |(i, _)| i);
                }
                KeyCode::Right if *cursor < input.len() => {
                    *cursor += input[*cursor..].chars().next().map_or(0, |c| c.len_utf8());
                }
                KeyCode::Home => *cursor = 0,
                KeyCode::End => *cursor = input.len(),
                KeyCode::Esc => app.quit(),
                _ => {}
            }
        }
        _ => {}
    }
    Ok(false)
}

async fn run_search_pipeline(
    idea: String,
    tx: tokio::sync::oneshot::Sender<anyhow::Result<SearchResult>>,
    cfg: TuiCfg,
) {
    let result = execute_pipeline(idea, cfg).await;
    let _ = tx.send(result);
}

async fn execute_pipeline(idea: String, cfg: TuiCfg) -> anyhow::Result<SearchResult> {
    let result = crate::pipeline::run(
        &idea,
        &crate::pipeline::PipelineCfg {
            api_base: cfg.api_base,
            api_key: cfg.api_key,
            model: cfg.model,
            fast: cfg.fast,
            keyword_only: cfg.keyword_only,
            limit: patent::rank::DEFAULT_LIMIT,
            sources_include: cfg.sources_include,
            sources_exclude: cfg.sources_exclude,
        },
        |_| {},
    )
    .await?;
    Ok(SearchResult {
        idea,
        verdict: result.verdict,
        matches: result.matches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use patent::verdict::CAVEAT;
    use ratatui::{backend::TestBackend, Terminal};

    fn verdict_with(gaps: usize, failed: Vec<Source>) -> Verdict {
        Verdict {
            level: Saturation::Crowded,
            headline: "Several closely-related tools turned up in the sources checked.".into(),
            gaps: (0..gaps)
                .map(|i| format!("a differentiator number {i} the user could pursue"))
                .collect(),
            sources_checked: vec![
                Source::Npm,
                Source::CratesIo,
                Source::GitHub,
                Source::HackerNews,
            ],
            sources_failed: failed,
            caveat: CAVEAT.to_string(),
        }
    }

    fn many_matches(n: usize) -> Vec<Match> {
        (0..n)
            .map(|i| Match {
                name: format!("tool-{i}"),
                source: Source::Npm,
                url: format!("https://example.com/{i}"),
                description: "a tool that does something useful".into(),
                popularity: Some(100),
                similarity: 0.9 - (i as f32 * 0.01),
            })
            .collect()
    }

    fn rendered(width: u16, height: u16, verdict: Verdict, matches: Vec<Match>) -> String {
        let app = App::new(
            "an interactive cli to manage processes on a port",
            verdict,
            matches,
        );
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut table_state = TableState::default();
        terminal
            .draw(|f| {
                draw(f, &app, &mut table_state);
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    fn rendered_with(
        width: u16,
        height: u16,
        verdict: Verdict,
        matches: Vec<Match>,
        setup: impl FnOnce(&mut App),
    ) -> String {
        let mut app = App::new(
            "an interactive cli to manage processes on a port",
            verdict,
            matches,
        );
        setup(&mut app);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut table_state = TableState::default();
        terminal
            .draw(|f| {
                draw(f, &app, &mut table_state);
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn caveat_is_never_clipped_at_common_sizes() {
        // The humble caveat ends with "before committing." and must ALWAYS be
        // visible — this is the non-negotiable integrity guarantee. Guards the
        // regression where the panel title row / word-wrap was under-budgeted.
        for (w, h) in [(80u16, 24u16), (100, 30), (120, 40), (80, 28)] {
            for gaps in [0usize, 2, 4] {
                let v = verdict_with(gaps, vec![]);
                let text = rendered(w, h, v.clone(), many_matches(40));
                assert!(
                    text.contains("committing"),
                    "caveat clipped at {w}x{h} with {gaps} gaps"
                );
            }
        }
    }

    #[test]
    fn not_reached_sources_are_surfaced() {
        let v = verdict_with(2, vec![Source::PyPI, Source::Go]);
        let text = rendered(100, 30, v, many_matches(5));
        assert!(text.contains("Not reached"), "failed sources must be shown");
        assert!(
            text.contains("committing"),
            "caveat still shown with a not-reached line"
        );
    }

    #[test]
    fn renders_without_panic_at_tiny_sizes() {
        // Layout/scrollbar must not panic on degenerate terminal sizes.
        let v = verdict_with(3, vec![Source::PyPI]);
        for (w, h) in [(1u16, 1u16), (10, 3), (40, 5), (80, 2)] {
            let _ = rendered(w, h, v.clone(), many_matches(50));
        }
    }

    #[test]
    fn table_title_shows_active_sort_key() {
        let v = verdict_with(0, vec![]);
        let text = rendered(100, 30, v, many_matches(5));
        assert!(
            text.contains("sort: similarity"),
            "the matches table should surface the active sort key"
        );
    }

    #[test]
    fn detail_popup_shows_full_match_info() {
        let v = verdict_with(1, vec![]);
        let text = rendered_with(100, 30, v, many_matches(5), |app| app.enter_detail());
        assert!(text.contains("tool-0"), "match name shown in popup");
        assert!(text.contains("https://example.com/0"), "full URL shown");
        assert!(
            text.contains("a tool that does something useful"),
            "description shown"
        );
    }

    #[test]
    fn detail_popup_renders_without_panic_at_tiny_sizes() {
        let v = verdict_with(1, vec![]);
        for (w, h) in [(1u16, 1u16), (10, 3), (40, 8)] {
            let _ = rendered_with(w, h, v.clone(), many_matches(5), |app| app.enter_detail());
        }
    }

    #[test]
    fn name_column_not_truncated_at_100_wide() {
        let v = verdict_with(0, vec![]);
        let long_name = "longname-x25-toolname-abc"; // 25 chars — exceeds current Length(24)
        let m = Match {
            name: long_name.to_string(),
            source: Source::Npm,
            url: "https://example.com/long".into(),
            description: "a tool".into(),
            popularity: Some(100),
            similarity: 0.9,
        };
        let text = rendered(100, 30, v, vec![m]);
        assert!(
            text.contains(long_name),
            "name column should show full 25-char name at 100 wide"
        );
    }

    #[test]
    fn scrollbar_does_not_move_within_viewport() {
        // Moving cursor by 1 (still within the visible viewport) must NOT shift
        // the scrollbar thumb — the thumb tracks the viewport offset, not the
        // cursor index.
        let v = verdict_with(0, vec![]);
        let matches = many_matches(50);

        let scrollbar_col = |app: &App| -> Vec<String> {
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            let mut ts = TableState::default();
            terminal
                .draw(|f| {
                    draw(f, app, &mut ts);
                })
                .unwrap();
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 80 == 79)
                .map(|(_, c)| c.symbol().to_string())
                .collect()
        };

        let app_at_0 = App::new(
            "an interactive cli to manage processes on a port",
            v.clone(),
            matches.clone(),
        );
        let mut app_at_5 = App::new(
            "an interactive cli to manage processes on a port",
            v,
            matches,
        );
        for _ in 0..5 {
            app_at_5.scroll_down();
        }

        assert_eq!(
            scrollbar_col(&app_at_0),
            scrollbar_col(&app_at_5),
            "scrollbar must not move when cursor stays within viewport"
        );
    }

    // ── word_wrap edge cases ────────────────────────────────────────────────

    #[test]
    fn word_wrap_empty_and_whitespace() {
        assert_eq!(word_wrap("", 40), vec![""]);
        assert_eq!(word_wrap("   ", 40), vec![""]);
    }

    #[test]
    fn word_wrap_single_short_word() {
        assert_eq!(word_wrap("hello", 40), vec!["hello"]);
    }

    #[test]
    fn word_wrap_splits_at_width() {
        // "hello world" — 11 chars — splits because 5+1+5 = 11 > 8
        let lines = word_wrap("hello world", 8);
        assert_eq!(lines, vec!["hello", "world"]);
    }

    #[test]
    fn word_wrap_fits_on_one_line() {
        let lines = word_wrap("hello world", 20);
        assert_eq!(lines, vec!["hello world"]);
    }

    #[test]
    fn word_wrap_word_longer_than_width_is_not_split() {
        // No hyphenation: long tokens go on their own line unbroken.
        let lines = word_wrap("averylongwordwithoutanyspaces", 10);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "averylongwordwithoutanyspaces");
    }

    #[test]
    fn word_wrap_zero_width() {
        let lines = word_wrap("hello world", 0);
        assert_eq!(lines, vec!["hello world"]);
    }

    // ── detail popup edge cases ─────────────────────────────────────────────

    #[test]
    fn detail_popup_empty_description_does_not_panic() {
        let v = verdict_with(0, vec![]);
        let m = Match {
            name: "no-desc".to_string(),
            source: Source::CratesIo,
            url: "https://example.com/no-desc".into(),
            description: String::new(),
            popularity: None,
            similarity: 0.5,
        };
        let text = rendered_with(100, 30, v, vec![m], |app| app.enter_detail());
        assert!(
            text.contains("no-desc"),
            "name shown with empty description"
        );
    }

    #[test]
    fn detail_popup_no_popularity_shows_dash() {
        let v = verdict_with(0, vec![]);
        let m = Match {
            name: "no-pop".to_string(),
            source: Source::CratesIo,
            url: "https://example.com".into(),
            description: "something".into(),
            popularity: None,
            similarity: 0.6,
        };
        let text = rendered_with(100, 30, v, vec![m], |app| app.enter_detail());
        assert!(text.contains('—'), "dash shown when popularity is None");
    }

    #[test]
    fn detail_popup_long_name_does_not_panic() {
        let v = verdict_with(0, vec![]);
        let m = Match {
            name: "a".repeat(80),
            source: Source::GitHub,
            url: "https://github.com/x/y".into(),
            description: "tool".into(),
            popularity: Some(42),
            similarity: 0.8,
        };
        let _ = rendered_with(80, 24, v, vec![m], |app| app.enter_detail());
    }

    #[test]
    fn detail_popup_very_long_url_does_not_panic() {
        let v = verdict_with(0, vec![]);
        let m = Match {
            name: "long-url".to_string(),
            source: Source::GitHub,
            url: format!("https://github.com/{}", "x".repeat(200)),
            description: "a tool".into(),
            popularity: None,
            similarity: 0.7,
        };
        let _ = rendered_with(80, 24, v, vec![m], |app| app.enter_detail());
    }

    #[test]
    fn detail_popup_scroll_shows_bottom_of_long_description() {
        let v = verdict_with(0, vec![]);
        // Build a description long enough that "lastword" is only visible after scrolling.
        let mut words: Vec<String> = (0..40).map(|i| format!("word{i}")).collect();
        words.push("lastword".to_string());
        let m = Match {
            name: "long-desc".to_string(),
            source: Source::CratesIo,
            url: "https://example.com".into(),
            description: words.join(" "),
            popularity: None,
            similarity: 0.9,
        };
        // At scroll=0, "lastword" is beyond the viewport.
        let t0 = rendered_with(100, 30, v.clone(), vec![m.clone()], |app| {
            app.enter_detail()
        });
        let t_end = rendered_with(100, 30, v, vec![m], |app| {
            app.enter_detail();
            for _ in 0..50 {
                app.scroll_detail_down();
            }
        });
        assert!(
            !t0.contains("lastword") || t_end.contains("lastword"),
            "lastword visible after scrolling to end"
        );
        assert!(t_end.contains("lastword"), "lastword visible at scroll end");
    }

    #[test]
    fn detail_popup_scroll_clamps_visually_when_overscrolled() {
        // Hammering j past the end should not panic and should still show content.
        let v = verdict_with(0, vec![]);
        let m = Match {
            name: "clamp-test".to_string(),
            source: Source::Npm,
            url: "https://example.com".into(),
            description: "short".into(),
            popularity: Some(1),
            similarity: 0.5,
        };
        let text = rendered_with(100, 30, v, vec![m], |app| {
            app.enter_detail();
            for _ in 0..1000 {
                app.scroll_detail_down();
            }
        });
        assert!(
            text.contains("clamp-test"),
            "name still visible after overscroll"
        );
        assert!(
            text.contains("short"),
            "description still visible after overscroll"
        );
    }

    #[test]
    fn detail_scroll_resets_when_reopened() {
        let v = verdict_with(0, vec![]);
        let mut words: Vec<String> = (0..40).map(|i| format!("word{i}")).collect();
        words.push("lastword".to_string());
        let m = Match {
            name: "reset-test".to_string(),
            source: Source::CratesIo,
            url: "https://example.com".into(),
            description: words.join(" "),
            popularity: None,
            similarity: 0.9,
        };
        // Scroll to end, then close and reopen — should be back at top (word0 visible).
        let text = rendered_with(100, 30, v, vec![m], |app| {
            app.enter_detail();
            for _ in 0..50 {
                app.scroll_detail_down();
            }
            app.exit_detail();
            app.enter_detail();
        });
        assert!(text.contains("word0"), "scroll reset to top after reopen");
    }

    #[test]
    fn detail_popup_renders_at_all_common_sizes() {
        let v = verdict_with(2, vec![Source::PyPI]);
        for (w, h) in [(80u16, 24u16), (100, 30), (120, 40), (60, 20)] {
            let _ = rendered_with(w, h, v.clone(), many_matches(5), |app| app.enter_detail());
        }
    }

    #[test]
    fn is_safe_url_allows_https_and_http_only() {
        assert!(is_safe_url("https://crates.io/crates/tokio"));
        assert!(is_safe_url("http://example.com/path"));
        assert!(is_safe_url("HTTPS://example.com"));
        assert!(is_safe_url("HTTP://example.com"));
        assert!(!is_safe_url("file:///etc/passwd"));
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url(""));
        assert!(!is_safe_url("data:text/html,<script>alert(1)</script>"));
    }

    // ── search screen rendering ────────────────────────────────────────────

    fn render_search(width: u16, height: u16, input: &str, cursor: usize) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw_search(f, input, cursor)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    fn render_searching(width: u16, height: u16, idea: &str, tick: usize) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw_searching(f, idea, tick)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn search_screen_shows_placeholder_when_empty() {
        let text = render_search(80, 24, "", 0);
        assert!(
            text.contains("describe your dev-tool idea"),
            "placeholder shown when input is empty"
        );
        assert!(text.contains("patent"), "title shown");
    }

    #[test]
    fn search_screen_shows_typed_input() {
        let input = "cli tool for ports";
        let text = render_search(80, 24, input, input.len());
        assert!(text.contains(input), "typed input appears on screen");
    }

    #[test]
    fn search_screen_shows_keybinding_hints() {
        let text = render_search(80, 24, "", 0);
        assert!(text.contains("Enter"), "Enter hint shown");
        assert!(text.contains("Esc"), "Esc hint shown");
    }

    #[test]
    fn search_screen_does_not_panic_at_tiny_sizes() {
        for (w, h) in [(1u16, 1u16), (10, 3), (20, 5)] {
            let _ = render_search(w, h, "some input text", 5);
        }
    }

    #[test]
    fn searching_screen_shows_spinner_and_idea() {
        let text = render_searching(80, 24, "cli tool for ports", 0);
        assert!(text.contains("Searching"), "searching label shown");
        assert!(
            text.contains("cli tool for ports"),
            "idea shown during search"
        );
    }

    #[test]
    fn searching_screen_spinner_changes_with_tick() {
        let t0 = render_searching(80, 24, "idea", 0);
        let t1 = render_searching(80, 24, "idea", 1);
        assert_ne!(t0, t1, "spinner should animate between ticks");
    }

    #[test]
    fn searching_screen_shows_cancel_hint() {
        let text = render_searching(80, 24, "idea", 0);
        assert!(text.contains("Esc"), "cancel hint shown");
    }

    #[test]
    fn searching_screen_does_not_panic_at_tiny_sizes() {
        for (w, h) in [(1u16, 1u16), (10, 3), (20, 5)] {
            let _ = render_searching(w, h, "idea", 3);
        }
    }

    // ── results footer shows new search hint ───────────────────────────────

    #[test]
    fn results_footer_shows_new_search_key() {
        let v = verdict_with(0, vec![]);
        let text = rendered(100, 30, v, many_matches(5));
        assert!(
            text.contains("new search"),
            "footer should show 'n new search' hint"
        );
    }

    // ── help overlay shows new search ──────────────────────────────────────

    #[test]
    fn help_overlay_lists_new_search() {
        let v = verdict_with(0, vec![]);
        let text = rendered_with(100, 30, v, many_matches(5), |app| app.toggle_help());
        assert!(
            text.contains("New search"),
            "help overlay should list 'n' → 'New search'"
        );
    }
}
