//! All ratatui drawing, split by surface:
//!
//! - [`menu`] — the main launcher screen (banner, ticker, menu list,
//!   details, status, footer, help overlay)
//! - [`github`] — the GitHub dashboard screen
//! - [`bluetti`] — the Bluetti power-station monitor screen
//! - [`effects`] — theme colours and the travelling border chase, shared
//!   by every surface

mod bluetti;
mod effects;
mod github;
mod menu;
#[cfg(test)]
mod tests;

pub use effects::{color_from_str, hsv_to_rgb, quant};


use crate::app::{App, Mode};
use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders},
    Frame,
};

pub(crate) const SPINNER_FRAMES: [&str; 10] =
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(frame: &mut Frame, app: &mut App) {
    match app.mode {
        Mode::Github => github::draw(frame, app),
        Mode::Bluetti => bluetti::draw(frame, app),
        Mode::Normal | Mode::Search | Mode::Help => menu::draw(frame, app),
    }
}

/// The standard bordered pane used by the full-screen views.
pub(crate) fn pane_block<'a>(title: &'a str, border: Color) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(title)
        .title_style(Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD))
        .title_alignment(Alignment::Left)
}
