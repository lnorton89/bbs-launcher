//! Drawing for the GitHub dashboard screen.

use super::effects::apply_chase;
use super::{pane_block, SPINNER_FRAMES};
use crate::app::App;
use crate::screens::github::SectionKind;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub(super) fn draw(frame: &mut Frame, app: &mut App) {
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
    let hints = match app.github.sections[app.github.tab] {
        SectionKind::Notifications => {
            " ←/→ h/l tabs · ↑/↓ j/k move · r refresh · o open in browser · m mark read · q back "
        }
        SectionKind::Repos => {
            " ←/→ h/l tabs · ↑/↓ j/k move · s sort · r refresh · o open in browser · q back "
        }
        _ => " ←/→ h/l tabs · ↑/↓ j/k move · r refresh · o open in browser · q back ",
    };
    frame.render_widget(
        Paragraph::new(hints)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray)),
        chunks[3],
    );
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
            .block(pane_block(kind.label(), Color::Red))
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
    } else if kind == SectionKind::Repos {
        // The repos list is sortable; show which order is active.
        (
            format!(
                " {} ({}) · by {} ",
                kind.label(),
                items.len(),
                app.github.repo_sort.label()
            ),
            accent,
        )
    } else {
        (format!(" {} ({}) ", kind.label(), items.len()), accent)
    };

    let list = List::new(items)
        .block(pane_block(&title, border))
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
            if app.github.sections[tab] == SectionKind::Repos {
                actions.push(format!(
                    "  s — cycle sort (now: {})",
                    app.github.repo_sort.label()
                ));
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
        .block(pane_block(" Details ", Color::DarkGray));
    frame.render_widget(details, area);
}
