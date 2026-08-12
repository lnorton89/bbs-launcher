//! Drawing for the Bluetti power-station monitor screen.

use super::effects::apply_chase;
use super::pane_block;
use crate::app::App;
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

    // ── header: title + connection status ──
    let status_color = if app.bluetti.connected {
        Color::Green
    } else {
        Color::Red
    };
    let mut head = vec![
        Span::styled(
            " Bluetti ",
            Style::default()
                .bg(accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(app.bluetti.status.clone(), Style::default().fg(status_color)),
    ];
    if app.bluetti.msg_count > 0 {
        head.push(Span::styled(
            format!("   {} updates ", app.bluetti.msg_count),
            Style::default().fg(Color::DarkGray),
        ));
    }
    let header = Paragraph::new(Line::from(head)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(accent))
            .title(" Bluetti Monitor ")
            .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
            .title_alignment(Alignment::Center),
    );
    frame.render_widget(header, chunks[0]);

    // ── device tab bar ──
    let mut tab_line: Vec<Span> = Vec::new();
    if app.bluetti.devices.is_empty() {
        tab_line.push(Span::styled(
            " waiting for device data… ",
            Style::default().fg(Color::DarkGray),
        ));
    }
    for (i, device) in app.bluetti.devices.iter().enumerate() {
        if i > 0 {
            tab_line.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        }
        if i == app.bluetti.tab {
            tab_line.push(Span::styled(
                format!(" {} ", device),
                Style::default()
                    .bg(accent)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            tab_line.push(Span::styled(
                format!(" {} ", device),
                Style::default().fg(Color::Gray),
            ));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(tab_line)).alignment(Alignment::Center),
        chunks[1],
    );

    // ── body: field list + summary pane ──
    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(chunks[2]);
    draw_bluetti_list(frame, mid[0], app);
    draw_bluetti_summary(frame, mid[1], app);
    apply_chase(frame, app, &[chunks[0], mid[0], mid[1]]);

    // ── footer ──
    frame.render_widget(
        Paragraph::new(" ↑/↓ j/k move · ←/→ h/l device · t toggle switch · r reconnect · q back ")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray)),
        chunks[3],
    );
}

fn draw_bluetti_list(frame: &mut Frame, area: Rect, app: &mut App) {
    let accent = app.accent();
    app.bluetti.list_area = Some(area);

    let fields = app.bluetti.sorted_fields();
    // The label column is sized to the longest label on screen, so a
    // bridge that publishes long field names ("Internal AC frequency")
    // can never collide with its values. The +2 keeps a visible gutter.
    let label_width = fields
        .iter()
        .map(|(name, _)| crate::screens::bluetti::field_label(name).chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    let rows: Vec<ListItem> = fields
        .into_iter()
        .map(|(name, field)| {
            let stale = field.updated.elapsed() > crate::screens::bluetti::STALE_AFTER;
            let value_style = if stale {
                Style::default().fg(Color::DarkGray)
            } else {
                match field.value.as_str() {
                    "ON" => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    "OFF" => Style::default().fg(Color::DarkGray),
                    _ => Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                }
            };
            // Units only make sense on numeric values — `cell_voltages`
            // is a JSON list, switches are ON/OFF.
            let unit = if field.value.parse::<f64>().is_ok() {
                crate::screens::bluetti::field_unit(name)
            } else {
                ""
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(
                        " {:<width$}",
                        crate::screens::bluetti::field_label(name),
                        width = label_width
                    ),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(format!("{}{}", field.value, unit), value_style),
            ]))
        })
        .collect();

    let (title, border) = if rows.is_empty() {
        (" Live State  (no data yet) ".to_string(), Color::DarkGray)
    } else {
        (format!(" Live State ({}) ", rows.len()), accent)
    };
    let list = List::new(rows)
        .block(pane_block(&title, border))
        .highlight_style(
            Style::default()
                .bg(accent)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    frame.render_stateful_widget(list, area, &mut app.bluetti.state);
}

/// Renders the last `width` samples as a one-line block sparkline.
/// Flat data draws mid-height so a steady value still reads as a line.
pub(super) fn sparkline(values: &[f64], width: usize) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let tail: Vec<f64> = values
        .iter()
        .copied()
        .skip(values.len().saturating_sub(width))
        .collect();
    if tail.is_empty() {
        return String::new();
    }
    let (min, max) = tail
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    let span = max - min;
    tail.iter()
        .map(|&v| {
            if span <= f64::EPSILON {
                BARS[3]
            } else {
                let t = (v - min) / span;
                BARS[((t * 7.0).round() as usize).min(7)]
            }
        })
        .collect()
}

/// Renders `pct` as a `[█████░░░] 33%` gauge.
fn battery_bar(pct: u64, width: usize) -> String {
    let filled = (pct.min(100) as usize * width) / 100;
    let mut bar = String::from("[");
    for i in 0..width {
        bar.push(if i < filled { '█' } else { '░' });
    }
    bar.push_str(&format!("] {pct}%"));
    bar
}

fn draw_bluetti_summary(frame: &mut Frame, area: Rect, app: &App) {
    let accent = app.accent();
    let view = &app.bluetti;
    let mut lines: Vec<Line> = Vec::new();

    let device_fields = view.current_device().and_then(|d| view.fields.get(d));
    let value_of = |name: &str| {
        device_fields
            .and_then(|f| f.get(name))
            .map(|f| f.value.clone())
    };
    let watts_of = |name: &str| {
        value_of(name)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
    };

    if let Some(device) = view.current_device() {
        lines.push(Line::from(Span::styled(
            format!(" {}", device),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::default());

        if let Some(pct) = value_of("total_battery_percent").and_then(|v| v.parse::<u64>().ok()) {
            let color = match pct {
                0..=19 => Color::Red,
                20..=49 => Color::Yellow,
                _ => Color::Green,
            };
            lines.push(Line::from(vec![
                Span::styled(" Battery   ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    battery_bar(pct, 16),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        let power_in = watts_of("ac_input_power") + watts_of("dc_input_power");
        let power_out = watts_of("ac_output_power") + watts_of("dc_output_power");
        let net = power_in - power_out;
        lines.push(Line::from(vec![
            Span::styled(" In / Out  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{power_in:.0} W in"),
                Style::default().fg(Color::Green),
            ),
            Span::styled("  ·  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{power_out:.0} W out"),
                Style::default().fg(Color::Yellow),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Net       ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}{:.0} W", if net >= 0.0 { "+" } else { "" }, net),
                Style::default()
                    .fg(if net >= 0.0 { Color::Green } else { Color::Yellow })
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        // Rolling trends (one sample every 5s, up to five minutes),
        // shown once there is enough history to mean anything. The
        // range annotation says what each sparkline spans, so a flat
        // line at "61…61" reads as steady rather than broken.
        if let Some(history) = view.history.get(device).filter(|h| h.len() >= 4) {
            let (batteries, nets): (Vec<f64>, Vec<f64>) = history.iter().copied().unzip();
            let width = (area.width as usize).saturating_sub(28).clamp(10, 30);
            let span = |v: &[f64]| {
                v.iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
                        (lo.min(x), hi.max(x))
                    })
            };
            let (nlo, nhi) = span(&nets);
            let (blo, bhi) = span(&batteries);
            let minutes = ((history.len() * 5).div_ceil(60)).max(1);
            lines.push(Line::from(Span::styled(
                format!(" Trend, last {minutes} min"),
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(vec![
                Span::styled("   net W   ", Style::default().fg(Color::DarkGray)),
                Span::styled(sparkline(&nets, width), Style::default().fg(accent)),
                Span::styled(
                    format!("  {nlo:+.0}…{nhi:+.0}"),
                    Style::default().fg(Color::Gray),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("   batt %  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    sparkline(&batteries, width),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("  {blo:.0}…{bhi:.0}"),
                    Style::default().fg(Color::Gray),
                ),
            ]));
        }
        lines.push(Line::default());
    }

    lines.push(Line::from(vec![
        Span::styled(" Broker    ", Style::default().fg(Color::DarkGray)),
        Span::styled(view.broker.clone(), Style::default().fg(Color::White)),
    ]));
    if let Some(filter) = &view.device_filter {
        lines.push(Line::from(vec![
            Span::styled(" Filter    ", Style::default().fg(Color::DarkGray)),
            Span::styled(filter.clone(), Style::default().fg(Color::White)),
        ]));
    }
    if let Some(last) = view.last_msg {
        let secs = last.elapsed().as_secs();
        let age = if secs == 0 {
            "just now".to_string()
        } else {
            format!("{secs}s ago")
        };
        lines.push(Line::from(vec![
            Span::styled(" Last data ", Style::default().fg(Color::DarkGray)),
            Span::styled(age, Style::default().fg(Color::White)),
        ]));
    }
    if view.devices.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            " Start bluetti-mqtt-node and data will appear here.",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
    }

    let summary = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(pane_block(" Summary ", Color::DarkGray));
    frame.render_widget(summary, area);
}
