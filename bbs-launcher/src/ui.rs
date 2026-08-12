use crate::app::{App, Mode, Row};
use crate::github::SectionKind;
use crate::stats::time_ago;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Convert an HSV triple (h: 0-360, s/v: 0-1) to an RGB tuple.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match ((h % 360.0) as i32).max(0) {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

pub fn color_from_str(s: &str) -> Color {
    match s.to_lowercase().as_str() {
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        _ => Color::White,
    }
}

/// RGB base used for the banner shimmer gradient of each theme color.
fn theme_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Red | Color::LightRed => (255, 85, 85),
        Color::Green | Color::LightGreen => (80, 250, 123),
        Color::Yellow | Color::LightYellow => (241, 250, 140),
        Color::Blue | Color::LightBlue => (98, 114, 250),
        Color::Magenta | Color::LightMagenta => (255, 121, 198),
        Color::White | Color::Gray => (245, 245, 245),
        // Rainbow theme: use the current animated hue directly.
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 220, 255),
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    if app.mode == Mode::Github {
        draw_github(frame, app);
        return;
    }
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

    if app.mode == Mode::Help {
        draw_help(frame, app);
    }
}

fn draw_banner(frame: &mut Frame, area: Rect, app: &App) {
    let (br, bg, bb) = theme_rgb(app.accent());
    let animate = app.config.bbs.banner_animation.unwrap_or(true);
    let phase = if animate { app.tick as f32 * 0.12 } else { 0.0 };

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
                    // Diagonal brightness wave across the letterforms.
                    let f = 0.55
                        + 0.45 * (col as f32 * 0.06 + row as f32 * 0.4 - phase).sin();
                    Span::styled(
                        c.to_string(),
                        Style::default()
                            .fg(Color::Rgb(
                                (br as f32 * f) as u8,
                                (bg as f32 * f) as u8,
                                (bb as f32 * f) as u8,
                            ))
                            .add_modifier(Modifier::BOLD),
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
                let mut spans = vec![
                    Span::styled(
                        format!("{}[{}] ", indent, item.key),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{} ", item.icon),
                        Style::default().fg(icon_color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        item.label.clone(),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" - {}", item.desc),
                        Style::default().fg(Color::Gray),
                    ),
                ];
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
        _ => (format!(" Main Menu ({}) ", app.items.len()), Color::DarkGray),
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
        Mode::Normal => "j/k move · Enter launch · / search · ? help · q quit",
        // The GitHub screen draws its own footer; this is never rendered.
        Mode::Github => "",
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
    let candidates = [
        format!(
            " bbs-launcher v0.2 │ {} │ {items} items │ session {uptime} │ {clock} ",
            app.config_path
        ),
        format!(" bbs-launcher v0.2 │ {short_path} │ {items} items │ session {uptime} │ {clock} "),
        format!(" v0.2 │ {items} items │ {uptime} │ {clock} "),
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

// ─────────────────────────── GitHub screen ───────────────────────────

fn draw_github(frame: &mut Frame, app: &mut App) {
    let accent = app.accent();
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    // ── header: title, sync spinner, connection status ──
    let busy = app.github.loading.iter().any(|l| *l);
    let mut head = vec![
        Span::styled(
            " GitHub ",
            Style::default()
                .bg(accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];
    if busy {
        head.push(Span::styled(
            format!(
                "{} syncing ",
                SPINNER_FRAMES[app.spinner % SPINNER_FRAMES.len()]
            ),
            Style::default().fg(Color::Yellow),
        ));
    }
    let status_color = if app.github.owner.is_some() {
        Color::Green
    } else if app.github.status.contains("authenticated") {
        Color::Red
    } else {
        Color::Gray
    };
    head.push(Span::styled(
        app.github.status.clone(),
        Style::default().fg(status_color),
    ));
    head.push(Span::styled(
        format!(
            "   {} sections · {} per page ",
            app.github.sections.len(),
            app.github.per_page
        ),
        Style::default().fg(Color::DarkGray),
    ));

    let header = Paragraph::new(Line::from(head)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(accent))
            .title(" GitHub Dashboard ")
            .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
            .title_alignment(Alignment::Center),
    );
    frame.render_widget(header, chunks[0]);

    // ── tab bar ──
    let mut tab_line: Vec<Span> = Vec::new();
    for (i, section) in app.github.sections.iter().enumerate() {
        if i > 0 {
            tab_line.push(Span::styled(
                " │ ",
                Style::default().fg(Color::DarkGray),
            ));
        }
        if i == app.github.tab {
            tab_line.push(Span::styled(
                format!(" {} ", section.label()),
                Style::default()
                    .bg(accent)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            tab_line.push(Span::styled(
                format!(" {} ", section.label()),
                Style::default().fg(Color::Gray),
            ));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(tab_line)).alignment(Alignment::Center),
        chunks[1],
    );

    // ── body: list + details panes ──
    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(chunks[2]);
    draw_github_list(frame, mid[0], app);
    draw_github_details(frame, mid[1], app);

    // ── footer: key hints ──
    let hints = if app.github.sections[app.github.tab] == SectionKind::Notifications {
        " ←/→ h/l tabs · ↑/↓ j/k move · r refresh · o open in browser · m mark read · q back "
    } else {
        " ←/→ h/l tabs · ↑/↓ j/k move · r refresh · o open in browser · q back "
    };
    frame.render_widget(
        Paragraph::new(hints)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray)),
        chunks[3],
    );
}

fn github_block<'a>(title: &'a str, border: Color) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(title)
        .title_style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD))
        .title_alignment(Alignment::Left)
}

fn draw_github_list(frame: &mut Frame, area: Rect, app: &mut App) {
    let accent = app.accent();
    let tab = app.github.tab;
    let kind = app.github.sections[tab];
    app.github.list_area = Some(area);

    // A failed fetch with no data gets an error pane instead of a list.
    if let Some(err) = &app.github.errors[tab] {
        if app.github.entries[tab].is_empty() {
            let msg = Paragraph::new(Line::from(vec![
                Span::styled(
                    " ✗ ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(err.clone(), Style::default().fg(Color::Gray)),
            ]))
            .block(github_block(kind.label(), Color::Red))
            .wrap(Wrap { trim: true });
            frame.render_widget(msg, area);
            return;
        }
    }

    let items: Vec<ListItem> = app.github.entries[tab]
        .iter()
        .map(|e| {
            let bullet = if kind == SectionKind::Notifications {
                " ● "
            } else {
                "   "
            };
            ListItem::new(Line::from(vec![
                Span::styled(bullet, Style::default().fg(Color::Yellow)),
                Span::styled(
                    e.title.clone(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {}", e.subtitle),
                    Style::default().fg(Color::Gray),
                ),
            ]))
        })
        .collect();

    let (title, border) = if app.github.loading[tab] {
        (format!(" {}  loading… ", kind.label()), Color::Yellow)
    } else if app.github.errors[tab].is_some() {
        (format!(" {}  (failed — r to retry) ", kind.label()), Color::Red)
    } else {
        (format!(" {} ({}) ", kind.label(), items.len()), accent)
    };

    let list = List::new(items)
        .block(github_block(&title, border))
        .highlight_style(
            Style::default()
                .bg(accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, area, &mut app.github.states[tab]);
}

fn draw_github_details(frame: &mut Frame, area: Rect, app: &App) {
    let accent = app.accent();
    let tab = app.github.tab;
    let selected = app.github.states[tab].selected().unwrap_or(0);
    let entry = app.github.entries[tab].get(selected);

    let mut lines: Vec<Line> = Vec::new();
    match entry {
        Some(e) => {
            lines.push(Line::from(Span::styled(
                e.title.clone(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            )));
            if !e.subtitle.is_empty() {
                lines.push(Line::from(Span::styled(
                    e.subtitle.clone(),
                    Style::default().fg(Color::Gray),
                )));
            }
            lines.push(Line::default());
            for (k, v) in &e.detail {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {:<12} ", k),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(v.clone(), Style::default().fg(Color::White)),
                ]));
            }
            lines.push(Line::default());
            let mut actions = vec!["  o — open in browser".to_string()];
            if app.github.sections[tab] == SectionKind::Notifications {
                actions.push("  m — mark as read".to_string());
            }
            for a in actions {
                lines.push(Line::from(Span::styled(
                    a,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )));
            }
        }
        None => {
            if app.github.loading[tab] {
                lines.push(Line::from(Span::styled(
                    " fetching…",
                    Style::default().fg(Color::Gray),
                )));
            } else if app.github.entries[tab].is_empty() {
                lines.push(Line::from(Span::styled(
                    " nothing here yet",
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                )));
            }
        }
    }

    let details = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(github_block(" Details ", Color::DarkGray));
    frame.render_widget(details, area);
}

fn draw_help(frame: &mut Frame, app: &App) {
    // Sized to the longest entry line and the full row count so nothing
    // is clipped; centered_rect clamps this to the terminal.
    let area = centered_rect(68, 27, frame.area());
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
        entry("?", "toggle this help"),
        entry("q / Esc", "quit"),
        Line::default(),
        section("Screens"),
        entry("GitHub item", "opens the GitHub dashboard (screen = \"github\")"),
        entry("←/→ h/l", "switch dashboard tab"),
        entry("o · m · r", "open · mark read · refresh"),
        Line::default(),
        section("Config"),
        entry("bbs.toml", "theme · banner · motd · categories · items"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Mode, Row};
    use crate::github::Entry;
    use ratatui::backend::TestBackend;

    fn test_app() -> App {
        // Point at the workspace bbs.toml (tests run with cwd inside
        // target/, where find_config would miss it).
        let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("bbs.toml");
        let (config, path) = crate::config::load_config(Some(config_path)).unwrap();
        App::new(config, path)
    }

    fn buffer_text(app: &mut App) -> String {
        let mut terminal = ratatui::Terminal::new(TestBackend::new(110, 32)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    #[ignore = "visual check; run with --ignored --nocapture"]
    fn snapshot() {
        let mut app = test_app();
        let mut terminal = ratatui::Terminal::new(TestBackend::new(110, 32)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        for y in 0..buf.area.height {
            let row: String = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect();
            println!("{row}");
        }
    }

    #[test]
    fn main_menu_renders() {
        let text = buffer_text(&mut test_app());
        assert!(text.contains("Main Menu"));
        assert!(text.contains("GitHub"));
        assert!(text.contains("Details"));
    }

    #[test]
    fn category_headers_render_and_fold() {
        let mut app = test_app();
        // The sample config groups items under headers.
        let text = buffer_text(&mut app);
        assert!(text.contains("DEVELOP"), "category header should render");
        assert!(text.contains("Lazygit"), "expanded items should be visible");

        // Fold every category: headers stay, member items disappear.
        let headers: Vec<String> = app
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Header { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert!(!headers.is_empty(), "sample config should have categories");
        for name in &headers {
            let pos = app
                .rows
                .iter()
                .position(|r| matches!(r, Row::Header { name: n, .. } if n == name))
                .unwrap();
            assert!(app.toggle_category_at(pos));
        }
        let folded = buffer_text(&mut app);
        assert!(folded.contains("DEVELOP"), "headers survive folding");
        assert!(!folded.contains("Lazygit"), "members hidden when folded");
        // Only headers and uncategorized items remain.
        assert!(app
            .rows
            .iter()
            .all(|r| matches!(r, Row::Header { .. } | Row::Item(_))));
        assert_eq!(
            app.rows.iter().filter(|r| matches!(r, Row::Header { .. })).count(),
            headers.len()
        );
    }

    #[test]
    fn selection_follows_the_row_under_it() {
        // Regression guard: the list is built from `rows`, so the index
        // the selection state holds must address the same row that gets
        // drawn — headers included.
        let mut app = test_app();
        for i in 0..app.rows.len() {
            app.state.select(Some(i));
            match &app.rows[i] {
                Row::Header { .. } => assert!(
                    app.selected_item().is_none(),
                    "row {i} is a header but resolved to an item"
                ),
                Row::Item(idx) => assert_eq!(
                    app.selected_item().map(|it| it.label.as_str()),
                    Some(app.items[*idx].label.as_str()),
                    "row {i} resolved to the wrong item"
                ),
            }
        }
    }

    #[test]
    fn search_flattens_categories_and_matches_fuzzily() {
        let mut app = test_app();
        app.mode = Mode::Search;
        app.query = "lzg".into();
        app.apply_filter();
        // No headers while searching, and the subsequence hit is found.
        assert!(app.rows.iter().all(|r| matches!(r, Row::Item(_))));
        assert_eq!(
            app.selected_item().map(|i| i.label.clone()),
            Some("Lazygit".into())
        );
        let text = buffer_text(&mut app);
        assert!(text.contains("/lzg"), "query echoes in the menu title");
    }

    #[test]
    fn footer_shrinks_instead_of_truncating() {
        let mut app = test_app();
        // Wide: full config path fits.
        for (width, expect_path) in [(160u16, true), (110, false), (40, false)] {
            let mut terminal =
                ratatui::Terminal::new(TestBackend::new(width, 32)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            let buf = terminal.backend().buffer().clone();
            let last = buf.area.height - 1;
            let row: String = (0..buf.area.width)
                .map(|x| buf[(x, last)].symbol())
                .collect();
            assert!(
                row.trim().chars().count() <= width as usize,
                "footer overflows at width {width}"
            );
            assert_eq!(
                row.contains(&app.config_path),
                expect_path,
                "full path presence wrong at width {width}"
            );
            // The clock's seconds field must never be cut off the end.
            assert!(
                row.trim_end().ends_with(|c: char| c.is_ascii_digit()),
                "clock truncated at width {width}: {:?}",
                row.trim_end()
            );
        }
    }

    #[test]
    fn marquee_wraps_around_and_handles_edges() {
        assert_eq!(marquee("abcd", 4, 0), "abcd");
        assert_eq!(marquee("abcd", 4, 1), "bcda");
        // Offsets past the end wrap instead of running out of text.
        assert_eq!(marquee("abcd", 4, 5), "bcda");
        // A window wider than the text repeats it.
        assert_eq!(marquee("ab", 5, 0), "ababa");
        assert_eq!(marquee("", 4, 0), "");
        assert_eq!(marquee("abcd", 0, 0), "");
    }

    #[test]
    fn ticker_renders_and_is_hidden_without_motd() {
        let mut app = test_app();
        assert!(app.motd.is_some(), "sample config sets a motd");
        assert!(buffer_text(&mut app).contains("Welcome back"));

        // No motd -> no ticker row, and the menu still draws.
        app.motd = None;
        let text = buffer_text(&mut app);
        assert!(!text.contains("Welcome back"));
        assert!(text.contains("Main Menu"));
    }

    #[test]
    fn github_screen_renders_with_entries() {
        let mut app = test_app();
        app.mode = Mode::Github;
        app.github.owner = Some("lnorton89".into());
        app.github.status = "connected as @lnorton89".into();
        let tab = app.github.tab;
        app.github.entries[tab].push(Entry {
            title: "Fix the bug".into(),
            subtitle: "octo/app · @octocat".into(),
            id: "#12".into(),
            url: Some("https://github.com/octo/app/pull/12".into()),
            detail: vec![("Repository".into(), "octo/app".into())],
        });
        app.github.states[tab].select(Some(0));

        let text = buffer_text(&mut app);
        assert!(text.contains("GitHub Dashboard"));
        assert!(text.contains("Notifications"));
        assert!(text.contains("Fix the bug"));
        assert!(text.contains("Repository"));
    }

    #[test]
    fn rainbow_theme_parses_and_cycles() {
        use crate::app::Theme;
        assert_eq!(Theme::parse("rainbow"), Theme::Rainbow);
        assert_eq!(Theme::parse("PRIDE"), Theme::Rainbow);
        assert_eq!(Theme::parse("cyan"), Theme::Solid(Color::Cyan));

        let mut app = test_app();
        app.theme = Theme::Rainbow;
        app.animate = true;
        let first = app.accent();
        for _ in 0..10 {
            app.on_tick();
        }
        let second = app.accent();
        assert_ne!(first, second, "hue should move with ticks");

        app.animate = false;
        let fixed = app.accent();
        assert_eq!(fixed, app.accent(), "static hue when animation off");

        // The full screen renders fine under a rainbow accent.
        let text = buffer_text(&mut app);
        assert!(text.contains("Main Menu"));
    }

    #[test]
    fn github_screen_renders_error_state() {
        let mut app = test_app();
        app.mode = Mode::Github;
        app.github.errors[0] = Some("GitHub CLI (gh) not available".into());

        let text = buffer_text(&mut app);
        assert!(text.contains("GitHub CLI"));
    }
}
