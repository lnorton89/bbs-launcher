use crate::app::App;
use crate::config::{find_config, BbsConfig, BbsItem};
use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

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

pub fn draw_banner(frame: &mut Frame, area: Rect, config: &BbsConfig, app: &App) {
    let banner_lines: Vec<Line> = app
        .banner
        .lines()
        .map(|line| {
            let spans: Vec<Span> = line
                .chars()
                .map(|c| {
                    Span::styled(
                        c.to_string(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                })
                .collect();
            Line::from(spans)
        })
        .collect();

    let banner = Paragraph::new(banner_lines)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(format!(" {} ", config.bbs.title))
                .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .title_alignment(Alignment::Center),
        );

    frame.render_widget(banner, area);
}

pub fn draw_menu(frame: &mut Frame, area: Rect, items: &[BbsItem], state: &mut ListState) {
    let menu_items: Vec<ListItem> = items
        .iter()
        .map(|item| {
            let icon_color = color_from_str(&item.color);
            let key_span = Span::styled(
                format!("[{}] ", item.key),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            );
            let icon_span = Span::styled(
                format!("{} ", item.icon),
                Style::default().fg(icon_color).add_modifier(Modifier::BOLD),
            );
            let label_span = Span::styled(
                format!("{}", item.label),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            );
            let desc_span = Span::styled(
                format!(" - {}", item.desc),
                Style::default().fg(Color::Gray),
            );

            ListItem::new(Line::from(vec![key_span, icon_span, label_span, desc_span]))
        })
        .collect();

    let menu = List::new(menu_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Main Menu ")
                .title_style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD))
                .title_alignment(Alignment::Left),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(menu, area, state);
}

pub fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let spinner = match app.spinner {
        0 => "◐",
        1 => "◓",
        2 => "◑",
        _ => "◒",
    };

    let status_text = if let Some(item) = app.get_selected() {
        format!(
            " {} Ready | {}: {} ({}) | {} ",
            spinner, item.key, item.label, item.desc, app.status_message
        )
    } else {
        format!(" {} Ready | {}", spinner, app.status_message)
    };

    let status = Paragraph::new(status_text)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .style(Style::default().fg(Color::Gray));

    frame.render_widget(status, area);
}

pub fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let footer_text = format!(
        " bbs-launcher v0.1 | config: {} | {} items ",
        find_config().unwrap().display(),
        app.items.len()
    );

    let footer = Paragraph::new(footer_text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));

    frame.render_widget(footer, area);
}
