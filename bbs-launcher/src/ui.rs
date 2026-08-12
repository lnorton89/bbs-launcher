use crate::app::{App, Mode, Row, Theme};
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

/// True for the box-drawing glyphs ratatui uses to stroke borders. The
/// chase recolours only these, so titles and content keep their own
/// colours instead of being swept up in the gradient.
fn is_border_glyph(symbol: &str) -> bool {
    symbol
        .chars()
        .next()
        .is_some_and(|c| ('\u{2500}'..='\u{257F}').contains(&c))
}

/// What the travelling border light is made of.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ChaseStyle {
    /// Sweep the full hue wheel around the outline.
    Hue,
    /// Hold one colour and chase a dim band through it.
    DimBand(u8, u8, u8),
}

/// Paints a travelling light around the border of `area`, like an LED
/// strip. Position along the perimeter drives the effect and the whole
/// pattern drifts with time, so it reads as light moving clockwise
/// rather than as cells blinking independently.
///
/// Under [`ChaseStyle::Hue`] one full wheel is spread over the outline,
/// so adjacent cells differ by only a degree or two, with a gentle
/// brightness wave riding along to give the motion something to show
/// even where neighbouring hues are nearly identical. Under
/// [`ChaseStyle::DimBand`] the hue is fixed and a narrow dimmed segment
/// travels through it instead.
///
/// `degrees_per_tick` sets the speed; it comes from the configured lap
/// time (see `chase_lap_secs`).
fn border_chase(
    frame: &mut Frame,
    area: Rect,
    tick: u64,
    animate: bool,
    degrees_per_tick: f32,
    style: ChaseStyle,
) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let (w, h) = (area.width as u32, area.height as u32);
    let perimeter = (2 * (w - 1) + 2 * (h - 1)) as f32;
    let phase_deg = if animate {
        tick as f32 * degrees_per_tick
    } else {
        0.0
    };
    let phase_rad = phase_deg.to_radians();

    let buf = frame.buffer_mut();
    let mut paint = |x: u16, y: u16, pos: u32| {
        let Some(cell) = buf.cell_mut((x, y)) else {
            return;
        };
        if !is_border_glyph(cell.symbol()) {
            return;
        }
        let t = pos as f32 / perimeter;
        let wave = (t * std::f32::consts::TAU - phase_rad).sin();
        let color = match style {
            ChaseStyle::Hue => {
                let hue = (t * 360.0 - phase_deg).rem_euclid(360.0);
                // Stays well clear of 0 so the dim part of the wave
                // still reads as coloured light rather than going muddy.
                let glow = 0.68 + 0.32 * wave;
                let (r, g, b) = hsv_to_rgb(hue, 0.85, glow);
                Color::Rgb(r, g, b)
            }
            ChaseStyle::DimBand(r, g, b) => {
                // Raising the normalised wave to a fractional power
                // pushes most of the lap up near full brightness, so the
                // dark part stays a narrow band travelling through the
                // theme colour instead of an even half-lit/half-dark
                // split. The floor keeps the band visible rather than
                // punching a hole in the border.
                let lit = (0.5 + 0.5 * wave).powf(0.4);
                let level = DIM_FLOOR + (1.0 - DIM_FLOOR) * lit;
                Color::Rgb(
                    (r as f32 * level) as u8,
                    (g as f32 * level) as u8,
                    (b as f32 * level) as u8,
                )
            }
        };
        cell.set_fg(color);
    };

    // Walk the outline clockwise from the top-left so `pos` measures
    // distance travelled along the strip.
    let (x0, y0) = (area.x, area.y);
    let (x1, y1) = (area.x + area.width - 1, area.y + area.height - 1);
    let mut pos = 0;
    for x in x0..=x1 {
        paint(x, y0, pos);
        pos += 1;
    }
    for y in (y0 + 1)..=y1 {
        paint(x1, y, pos);
        pos += 1;
    }
    for x in (x0..x1).rev() {
        paint(x, y1, pos);
        pos += 1;
    }
    for y in ((y0 + 1)..y1).rev() {
        paint(x0, y, pos);
        pos += 1;
    }
}

/// How far the dim band drops below the theme colour at its darkest.
const DIM_FLOOR: f32 = 0.3;

/// Applies the travelling border light to every bordered pane, in
/// whichever form suits the active theme. A no-op when the chase is
/// switched off.
fn apply_chase(frame: &mut Frame, app: &App, areas: &[Rect]) {
    if !app.chase {
        return;
    }
    let style = match app.theme {
        Theme::Rainbow => ChaseStyle::Hue,
        Theme::Solid(color) => {
            let (r, g, b) = theme_rgb(color);
            ChaseStyle::DimBand(r, g, b)
        }
    };
    for area in areas {
        border_chase(
            frame,
            *area,
            app.tick,
            app.animate,
            app.chase_degrees_per_tick,
            style,
        );
    }
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
                    let color = if rainbow {
                        // Spread the wheel across the letterforms so the
                        // banner carries a gradient of its own, instead
                        // of every glyph sharing one shifting tint. The
                        // brightness floor keeps colours vivid where the
                        // wave dips.
                        let hue = (col as f32 * 2.4 + row as f32 * 7.0
                            - phase * 14.0)
                            .rem_euclid(360.0);
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
    apply_chase(frame, app, &[chunks[0], mid[0], mid[1]]);

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

    /// Reads the fg colours of a pane's border cells, walking clockwise
    /// from the top-left corner. Each entry carries its distance along
    /// the perimeter, because a title interrupts the run of border
    /// glyphs and the cells either side of it are not neighbours.
    /// Returns the menu pane's rect alongside its border colours. The
    /// rect comes from what the app actually drew rather than being
    /// hardcoded, so a layout change can't silently make these tests
    /// sample interior cells instead of the border.
    type Rgb = (u8, u8, u8);
    /// A border cell: how far along the perimeter it sits, and the
    /// colour it was rendered in.
    type BorderCell = (u32, Rgb);

    fn border_colors(app: &mut App) -> (Rect, Vec<BorderCell>) {
        let mut terminal = ratatui::Terminal::new(TestBackend::new(110, 32)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let area = app.menu_area.expect("draw records the menu area");
        let buf = terminal.backend().buffer().clone();
        let (x0, y0) = (area.x, area.y);
        let (x1, y1) = (area.x + area.width - 1, area.y + area.height - 1);
        let mut coords: Vec<(u16, u16)> = Vec::new();
        coords.extend((x0..=x1).map(|x| (x, y0)));
        coords.extend(((y0 + 1)..=y1).map(|y| (x1, y)));
        coords.extend((x0..x1).rev().map(|x| (x, y1)));
        coords.extend(((y0 + 1)..y1).rev().map(|y| (x0, y)));
        let colors = coords
            .into_iter()
            .enumerate()
            .filter(|&(_, (x, y))| is_border_glyph(buf[(x, y)].symbol()))
            .filter_map(|(pos, (x, y))| match buf[(x, y)].fg {
                Color::Rgb(r, g, b) => Some((pos as u32, (r, g, b))),
                _ => None,
            })
            .collect();
        (area, colors)
    }

    #[test]
    fn rainbow_chase_is_a_smooth_travelling_gradient() {
        use crate::app::Theme;
        let mut app = test_app();
        app.theme = Theme::Rainbow;
        app.animate = true;

        let (area, colors) = border_colors(&mut app);
        assert!(colors.len() > 50, "expected a full border of cells");

        // Diffused, not banded: cells that really are adjacent stay
        // close in colour, all the way around including the corners.
        let step = |a: Rgb, b: Rgb| {
            (a.0 as i32 - b.0 as i32).abs().max(
                (a.1 as i32 - b.1 as i32)
                    .abs()
                    .max((a.2 as i32 - b.2 as i32).abs()),
            )
        };
        let adjacent: Vec<i32> = colors
            .windows(2)
            .filter(|w| w[1].0 == w[0].0 + 1)
            .map(|w| step(w[0].1, w[1].1))
            .collect();
        assert!(adjacent.len() > 40, "expected long unbroken runs of border");
        let biggest = *adjacent.iter().max().unwrap();
        assert!(
            biggest <= 20,
            "gradient should be gradual, but adjacent cells jumped by {biggest}"
        );

        // Rainbow, not monochrome: the whole wheel is represented.
        let distinct = colors
            .iter()
            .map(|(_, c)| c)
            .collect::<std::collections::HashSet<_>>();
        assert!(
            distinct.len() > 20,
            "expected many hues around the border, got {}",
            distinct.len()
        );

        // It travels: the same cells are lit differently a few ticks on.
        const TICKS: u64 = 12;
        for _ in 0..TICKS {
            app.on_tick();
        }
        let later = border_colors(&mut app).1;
        assert_ne!(colors, later, "the chase should move over time");

        // And it travels as a chase — the whole pattern slides clockwise
        // by a predictable distance rather than every cell recolouring
        // independently. After TICKS, the light at position p is what
        // used to be at p - shift.
        let perimeter = f32::from(2 * (area.width - 1) + 2 * (area.height - 1));
        let shift = (TICKS as f32 * app.chase_degrees_per_tick / 360.0 * perimeter)
            .round() as u32;
        assert!(shift > 0, "the test needs enough ticks to move the pattern");

        let earlier: std::collections::HashMap<u32, Rgb> =
            colors.iter().copied().collect();
        let mut compared = 0;
        let mut worst = 0;
        for (pos, c) in &later {
            let Some(prev) = pos.checked_sub(shift).and_then(|p| earlier.get(&p)) else {
                continue;
            };
            worst = worst.max(step(*c, *prev));
            compared += 1;
        }
        assert!(compared > 30, "expected plenty of overlap to compare");
        assert!(
            worst <= 25,
            "pattern should have slid {shift} cells clockwise, but a cell \
             differed from its predecessor by {worst}"
        );
    }

    #[test]
    fn chase_lap_secs_sets_the_speed_and_rejects_nonsense() {
        use crate::app::{Theme, TICKS_PER_SEC};

        let lap_of = |configured: Option<f32>| {
            let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("bbs.toml");
            let (mut config, path) =
                crate::config::load_config(Some(config_path)).unwrap();
            config.bbs.chase_lap_secs = configured;
            let app = App::new(config, path);
            // Invert the conversion to recover the effective lap time.
            360.0 / (app.chase_degrees_per_tick * TICKS_PER_SEC)
        };

        let approx = |a: f32, b: f32| (a - b).abs() < 0.01;
        assert!(approx(lap_of(Some(4.0)), 4.0), "a plain value is honoured");
        assert!(approx(lap_of(None), 12.0), "unset falls back to the default");
        // Out-of-range and non-finite values clamp or fall back rather
        // than producing a strobe or a frozen border.
        assert!(approx(lap_of(Some(0.0)), 0.5), "too fast clamps up");
        assert!(approx(lap_of(Some(-3.0)), 0.5), "negative clamps up");
        assert!(approx(lap_of(Some(99_999.0)), 600.0), "too slow clamps down");
        assert!(approx(lap_of(Some(f32::NAN)), 12.0), "NaN falls back");
        assert!(approx(lap_of(Some(f32::INFINITY)), 12.0), "inf falls back");

        // A faster lap really does move the pattern further per tick.
        let sample = |lap: f32| {
            let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("bbs.toml");
            let (mut config, path) =
                crate::config::load_config(Some(config_path)).unwrap();
            config.bbs.chase_lap_secs = Some(lap);
            let mut app = App::new(config, path);
            app.theme = Theme::Rainbow;
            app.animate = true;
            let before = border_colors(&mut app).1;
            app.on_tick();
            let after = border_colors(&mut app).1;
            // Total colour movement across the strip after one tick.
            before
                .iter()
                .zip(after.iter())
                .map(|((_, a), (_, b))| {
                    (a.0 as i32 - b.0 as i32).abs()
                        + (a.1 as i32 - b.1 as i32).abs()
                        + (a.2 as i32 - b.2 as i32).abs()
                })
                .sum::<i32>()
        };
        assert!(
            sample(2.0) > sample(60.0),
            "a shorter lap should advance the chase further each tick"
        );
    }

    #[test]
    fn solid_themes_chase_a_dim_band_in_their_own_colour() {
        use crate::app::Theme;
        let mut app = test_app();
        app.theme = Theme::Solid(Color::Cyan);
        app.animate = true;

        let colors = border_colors(&mut app).1;
        assert!(colors.len() > 50, "expected a full border of cells");

        // One hue throughout: every cell is the theme colour at some
        // brightness, so normalising by the brightest channel gives the
        // same chromaticity everywhere.
        let chroma = |(r, g, b): Rgb| {
            let m = r.max(g).max(b).max(1) as f32;
            (r as f32 / m, g as f32 / m, b as f32 / m)
        };
        let first = chroma(colors[0].1);
        for (_, c) in &colors {
            let k = chroma(*c);
            let off = (k.0 - first.0)
                .abs()
                .max((k.1 - first.1).abs())
                .max((k.2 - first.2).abs());
            assert!(off < 0.05, "solid chase must not shift hue, saw {c:?}");
        }

        // But brightness does vary — that is the band.
        let level = |(r, g, b): Rgb| r as u32 + g as u32 + b as u32;
        let dimmest = colors.iter().map(|(_, c)| level(*c)).min().unwrap();
        let brightest = colors.iter().map(|(_, c)| level(*c)).max().unwrap();
        assert!(
            dimmest * 2 < brightest,
            "expected a clearly dim band ({dimmest} vs {brightest})"
        );

        // Mostly lit, with the darkness confined to a travelling band
        // rather than half the border.
        let midpoint = (dimmest + brightest) / 2;
        let lit = colors.iter().filter(|(_, c)| level(*c) > midpoint).count();
        assert!(
            lit * 2 > colors.len(),
            "the band should be narrower than the lit stretch"
        );

        // And it travels.
        let before = colors;
        for _ in 0..12 {
            app.on_tick();
        }
        assert_ne!(before, border_colors(&mut app).1, "the band should move");
    }

    #[test]
    fn border_chase_can_be_switched_off() {
        let mut app = test_app();
        app.theme = crate::app::Theme::Solid(Color::Cyan);
        app.chase = false;
        // With the chase off, borders keep their plain named colour and
        // no cell carries an Rgb fg.
        assert!(
            border_colors(&mut app).1.is_empty(),
            "no cell should be repainted when the chase is disabled"
        );
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
