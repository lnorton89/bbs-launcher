//! The main launcher surface: banner, motd ticker, menu list, details
//! pane, status line, footer, and the help overlay.

use super::effects::{apply_chase, color_from_str, hsv_to_rgb, quant, theme_rgb};
use super::SPINNER_FRAMES;
use crate::app::{App, Mode, Row, Theme};
use crate::stats::time_ago;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

/// Draws the whole main screen (plus the help overlay when open).
pub(super) fn draw(frame: &mut Frame, app: &mut App) {
    // The ticker row only exists when a motd is configured.
    let ticker_rows = if app.motd.is_some() { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(ticker_rows),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_banner(frame, chunks[0], app);
    if ticker_rows > 0 {
        draw_ticker(frame, chunks[1], app);
    }

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(chunks[2]);

    draw_menu(frame, mid[0], app);
    draw_info(frame, mid[1], app);
    draw_status(frame, chunks[3], app);
    draw_footer(frame, chunks[4], app);

    // After the panes are drawn, so the chase overwrites their border
    // colours rather than being overwritten by them.
    apply_chase(frame, app, &[chunks[0], mid[0], mid[1]]);

    if app.mode == Mode::Help {
        draw_help(frame, app);
    }
}

fn draw_banner(frame: &mut Frame, area: Rect, app: &App) {
    let (br, bg, bb) = theme_rgb(app.accent());
    let rainbow = app.theme == Theme::Rainbow;
    // `app.animate` rather than re-reading the config: one source of
    // truth, so the banner and the border chase can never disagree
    // about whether animation is on.
    let phase = if app.animate {
        app.tick as f32 * 0.12
    } else {
        0.0
    };

    let banner_lines: Vec<Line> = app
        .banner
        .lines()
        .enumerate()
        .map(|(row, line)| {
            let spans: Vec<Span> = line
                .chars()
                .enumerate()
                .map(|(col, c)| {
                    if c == ' ' {
                        return Span::raw(" ");
                    }
                    // Diagonal brightness wave across the letterforms,
                    // quantized so a glyph only repaints when its level
                    // actually steps (see effects::quant).
                    let f = quant(
                        0.55 + 0.45 * (col as f32 * 0.06 + row as f32 * 0.4 - phase).sin(),
                        1.0 / 24.0,
                    );
                    let color = if rainbow {
                        // Spread the wheel across the letterforms so the
                        // banner carries a gradient of its own, instead
                        // of every glyph sharing one shifting tint. The
                        // brightness floor keeps colours vivid where the
                        // wave dips.
                        let hue = quant(
                            (col as f32 * 2.4 + row as f32 * 7.0 - phase * 14.0)
                                .rem_euclid(360.0),
                            4.0,
                        );
                        let (r, g, b) = hsv_to_rgb(hue, 0.85, f.clamp(0.5, 1.0));
                        Color::Rgb(r, g, b)
                    } else {
                        Color::Rgb(
                            (br as f32 * f) as u8,
                            (bg as f32 * f) as u8,
                            (bb as f32 * f) as u8,
                        )
                    };
                    Span::styled(
                        c.to_string(),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    )
                })
                .collect();
            Line::from(spans)
        })
        .collect();

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.accent()))
        .title(format!(" {} ", app.config.bbs.title))
        .title_style(Style::default().fg(app.accent()).add_modifier(Modifier::BOLD))
        .title_alignment(Alignment::Center);
    if let Some(subtitle) = &app.config.bbs.subtitle {
        block = block.title_bottom(
            Line::from(format!(" {} ", subtitle))
                .style(Style::default().fg(Color::DarkGray))
                .centered(),
        );
    }

    let banner = Paragraph::new(banner_lines)
        .alignment(Alignment::Center)
        .block(block);

    frame.render_widget(banner, area);
}

/// One frame of a scrolling marquee: a `width`-wide window into `text`
/// repeated endlessly, starting `offset` characters in.
pub fn marquee(text: &str, width: usize, offset: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || width == 0 {
        return String::new();
    }
    let start = offset % chars.len();
    chars.iter().cycle().skip(start).take(width).collect()
}

fn draw_ticker(frame: &mut Frame, area: Rect, app: &App) {
    let Some(text) = app.motd.as_deref() else {
        return;
    };
    // Two ticks per character keeps the scroll readable at a 100ms tick.
    let offset = if app.animate { (app.tick / 2) as usize } else { 0 };
    let visible = marquee(text, area.width as usize, offset);

    let ticker = Paragraph::new(Line::from(vec![Span::styled(
        visible,
        Style::default().fg(app.accent()).add_modifier(Modifier::BOLD),
    )]));
    frame.render_widget(ticker, area);
}

/// Splits `text` into styled runs, marking the chars whose position in
/// the search haystack (`offset` + index within `text`) the fuzzy query
/// matched. Matched runs keep their base colour scheme readable by
/// switching to underlined yellow — and the underline survives even on
/// the selected row, where the highlight bar repaints all foregrounds.
fn highlight_runs(
    text: &str,
    positions: &[usize],
    offset: usize,
    base: Style,
) -> Vec<Span<'static>> {
    let matched_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let mut spans = Vec::new();
    let mut run = String::new();
    let mut run_matched = false;
    for (i, c) in text.chars().enumerate() {
        // `positions` is built in ascending order, so binary search works.
        let matched = positions.binary_search(&(offset + i)).is_ok();
        if matched != run_matched && !run.is_empty() {
            let style = if run_matched { matched_style } else { base };
            spans.push(Span::styled(std::mem::take(&mut run), style));
        }
        run_matched = matched;
        run.push(c);
    }
    if !run.is_empty() {
        let style = if run_matched { matched_style } else { base };
        spans.push(Span::styled(run, style));
    }
    spans
}

fn draw_menu(frame: &mut Frame, area: Rect, app: &mut App) {
    let accent = app.accent();
    // Rendered from `rows`, not `filtered`, so the row the selection
    // state points at is always the row drawn highlighted.
    let menu_items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|row| match row {
            Row::Header { name, count } => {
                let arrow = if app.collapsed.contains(name) { "▸" } else { "▾" };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} {}", arrow, name.to_uppercase()),
                        Style::default().fg(accent).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  ({})", count),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            }
            Row::Item(idx) => {
                let item = &app.items[*idx];
                let icon_color = color_from_str(&item.color);
                // Indent items that sit under a category header.
                let indent = if item.category.is_some() { "  " } else { "" };
                let label_style =
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
                let desc_style = Style::default().fg(Color::Gray);
                let mut spans = vec![
                    Span::styled(
                        format!("{}[{}] ", indent, item.key),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{} ", item.icon),
                        Style::default().fg(icon_color).add_modifier(Modifier::BOLD),
                    ),
                ];
                // While searching, underline the chars the query hit.
                // The positions index into "label desc cmd", so the desc
                // starts one char (the joining space) past the label.
                match app.match_positions.get(idx).filter(|_| !app.query.is_empty()) {
                    Some(positions) => {
                        spans.extend(highlight_runs(&item.label, positions, 0, label_style));
                        spans.push(Span::styled(" - ".to_string(), desc_style));
                        let desc_offset = item.label.chars().count() + 1;
                        spans.extend(highlight_runs(
                            &item.desc,
                            positions,
                            desc_offset,
                            desc_style,
                        ));
                    }
                    None => {
                        spans.push(Span::styled(item.label.clone(), label_style));
                        spans.push(Span::styled(format!(" - {}", item.desc), desc_style));
                    }
                }
                if let Some(st) = app.stats.get(&item.label) {
                    if st.count > 0 {
                        spans.push(Span::styled(
                            format!("  {}×", st.count),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                }
                ListItem::new(Line::from(spans))
            }
        })
        .collect();

    let (title, border_color) = match app.mode {
        Mode::Search => (
            format!(" /{}█  ({}/{}) ", app.query, app.filtered.len(), app.items.len()),
            accent,
        ),
        _ => {
            // Show the active order when it isn't plain config order.
            let sort_note = match app.menu_sort {
                crate::app::MenuSort::Config => String::new(),
                other => format!(" · by {}", other.label()),
            };
            (
                format!(" Main Menu ({}{}) ", app.items.len(), sort_note),
                Color::DarkGray,
            )
        }
    };

    let menu = List::new(menu_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(title)
                .title_style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD))
                .title_alignment(Alignment::Left),
        )
        .highlight_style(
            Style::default()
                .bg(app.accent())
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    app.menu_area = Some(area);
    frame.render_stateful_widget(menu, area, &mut app.state);
}

fn kv(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {:<10} ", key), Style::default().fg(Color::DarkGray)),
        Span::styled(value.to_string(), Style::default().fg(Color::White)),
    ])
}

fn draw_info(frame: &mut Frame, area: Rect, app: &App) {
    let lines: Vec<Line> = if let Some(item) = app.selected_item() {
        let mut lines = vec![
            Line::from(Span::styled(
                format!(" {}", item.label),
                Style::default().fg(app.accent()).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!(" {}", item.desc),
                Style::default().fg(Color::Gray),
            )),
            Line::default(),
            kv("Key", &item.key),
            kv("Command", &item.cmd),
            kv(
                "Shell",
                if item.wsl.unwrap_or(false) {
                    "WSL (bash)"
                } else {
                    "Windows (cmd)"
                },
            ),
        ];
        if let Some(cwd) = &item.cwd {
            lines.push(kv("Directory", cwd));
        }
        let st = app.stats.get(&item.label);
        let count = st.map(|s| s.count).unwrap_or(0);
        let count_str = if count == 0 {
            "never".to_string()
        } else {
            count.to_string()
        };
        lines.push(kv("Launches", &count_str));
        if let Some(t) = st.and_then(|s| s.last_launched) {
            lines.push(kv("Last run", &time_ago(t)));
        }
        if item.pause.unwrap_or(false) {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                " pauses before returning to menu",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )));
        }
        lines
    } else if let Some(Row::Header { name, count }) = app.selected_row() {
        let collapsed = app.collapsed.contains(name);
        let launches: u64 = app
            .items
            .iter()
            .filter(|i| i.category.as_deref() == Some(name.as_str()))
            .filter_map(|i| app.stats.get(&i.label))
            .map(|s| s.count)
            .sum();
        vec![
            Line::from(Span::styled(
                format!(" {}", name.to_uppercase()),
                Style::default().fg(app.accent()).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                " category",
                Style::default().fg(Color::Gray),
            )),
            Line::default(),
            kv("Items", &count.to_string()),
            kv("State", if collapsed { "collapsed" } else { "expanded" }),
            kv("Launches", &launches.to_string()),
            Line::default(),
            Line::from(Span::styled(
                if collapsed {
                    " Enter or → to expand"
                } else {
                    " Enter or ← to collapse"
                },
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )),
        ]
    } else {
        vec![
            Line::default(),
            Line::from(Span::styled(
                " No matches",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )),
        ]
    };

    let info = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Details ")
            .title_style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD)),
    );

    frame.render_widget(info, area);
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let spinner = SPINNER_FRAMES[app.spinner % SPINNER_FRAMES.len()];
    let hint = match app.mode {
        Mode::Search => "type to filter · Enter launch · Esc cancel",
        Mode::Help => "press any key to close help",
        Mode::Normal => "j/k move · Enter launch · / search · s sort · ? help · q quit",
        // Built-in screens draw their own footers; never rendered.
        Mode::Github | Mode::Bluetti => "",
    };

    let line = Line::from(vec![
        Span::styled(format!(" {} ", spinner), Style::default().fg(app.accent())),
        Span::styled(app.status_message.clone(), Style::default().fg(Color::Gray)),
        Span::styled(format!("  │  {}", hint), Style::default().fg(Color::DarkGray)),
    ]);

    let status = Paragraph::new(line).alignment(Alignment::Center);
    frame.render_widget(status, area);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let elapsed = app.session_start.elapsed().as_secs();
    let uptime = format!("{:02}:{:02}:{:02}", elapsed / 3600, elapsed / 60 % 60, elapsed % 60);
    let clock = chrono::Local::now().format("%a %d %b %H:%M:%S");
    let items = app.items.len();

    // Progressively shorter forms, so a narrow terminal drops the config
    // path rather than truncating the clock off the end.
    let short_path = std::path::Path::new(&app.config_path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| app.config_path.clone());
    let ver = env!("CARGO_PKG_VERSION");
    let candidates = [
        format!(
            " bbs-launcher v{ver} │ {} │ {items} items │ session {uptime} │ {clock} ",
            app.config_path
        ),
        format!(" bbs-launcher v{ver} │ {short_path} │ {items} items │ session {uptime} │ {clock} "),
        format!(" v{ver} │ {items} items │ {uptime} │ {clock} "),
        format!(" {items} items │ {clock} "),
    ];
    let width = area.width as usize;
    let footer_text = candidates
        .iter()
        .find(|s| s.chars().count() <= width)
        .cloned()
        .unwrap_or_else(|| clock.to_string());

    let footer = Paragraph::new(footer_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));

    frame.render_widget(footer, area);
}
fn draw_help(frame: &mut Frame, app: &App) {
    // Sized to the longest entry line and the full row count so nothing
    // is clipped; centered_rect clamps this to the terminal.
    let area = centered_rect(68, 28, frame.area());
    frame.render_widget(Clear, area);

    let section = |t: &str| {
        Line::from(Span::styled(
            format!(" {}", t),
            Style::default().fg(app.accent()).add_modifier(Modifier::BOLD),
        ))
    };
    let entry = |k: &str, d: &str| {
        Line::from(vec![
            Span::styled(format!("   {:<14}", k), Style::default().fg(Color::Yellow)),
            Span::styled(d.to_string(), Style::default().fg(Color::Gray)),
        ])
    };

    let lines = vec![
        Line::default(),
        section("Navigation"),
        entry("↑/↓  j/k", "move selection"),
        entry("g / G", "first / last item"),
        entry("PgUp / PgDn", "jump 5 items"),
        entry("← / →", "collapse / expand category"),
        entry("mouse", "scroll · click · double-click launches"),
        Line::default(),
        section("Actions"),
        entry("Enter", "launch item · fold category header"),
        entry("1-9 …", "launch by hotkey (works when collapsed)"),
        entry("/", "fuzzy search (label, desc, command)"),
        entry("s", "cycle sort: config · most launched · recent"),
        entry("?", "toggle this help"),
        entry("q / Esc", "quit"),
        entry("Ctrl+L", "force a full repaint"),
        Line::default(),
        section("Screens"),
        entry("screen items", "GitHub dashboard · Bluetti power monitor"),
        entry("←/→ h/l", "switch screen tab"),
        entry("o · m · r · s", "open · mark read · refresh/reconnect · sort"),
        Line::default(),
        section("Config"),
        entry("bbs.toml", "theme · motd · items — live-reloads on save"),
        entry("stats.toml", "launch counts (~/.config/bbs-launcher)"),
        entry("--config FILE", "use a different config · --list to dump it"),
    ];

    let help = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.accent()))
            .title(" Help ")
            .title_style(Style::default().fg(app.accent()).add_modifier(Modifier::BOLD))
            .title_alignment(Alignment::Center),
    );

    frame.render_widget(help, area);
    apply_chase(frame, app, &[area]);
}

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let w = width.min(r.width);
    let h = height.min(r.height);
    Rect {
        x: r.x + (r.width - w) / 2,
        y: r.y + (r.height - h) / 2,
        width: w,
        height: h,
    }
}
