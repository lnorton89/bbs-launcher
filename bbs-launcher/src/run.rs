use crate::app::App;
use crate::config::BbsItem;
use crate::ui::{draw_banner, draw_footer, draw_menu, draw_status};
use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    Terminal,
};
use std::io;
use std::time::{Duration, Instant};

pub fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> Result<()> {
    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(12),
                    Constraint::Min(3),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(f.area());

            draw_banner(f, chunks[0], &app.config, &app);
            draw_menu(f, chunks[1], &app.items, &mut app.state);
            draw_status(f, chunks[2], &app);
            draw_footer(f, chunks[3], &app);
        })?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            return Ok(());
                        }
                        if app.get_selected().map(|i| i.key.to_lowercase()) == Some("q".to_string()) {
                            return Ok(());
                        }
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        app.next();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.previous();
                    }
                    KeyCode::Char(c) => {
                        if let Some(item) = app.find_by_key(&c.to_string()) {
                            let item = item.clone();
                            let label = item.label.clone();
                            app.status_message = format!("Launching: {}...", label);
                            return launch_command(&item);
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(item) = app.get_selected() {
                            let item = item.clone();
                            let label = item.label.clone();
                            app.status_message = format!("Launching: {}...", label);
                            return launch_command(&item);
                        }
                    }
                    KeyCode::Esc => {
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.update_spinner();
            last_tick = Instant::now();
        }
    }
}

fn launch_command(item: &BbsItem) -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture, Clear(ClearType::All))?;

    if item.wsl.unwrap_or(false) {
        let mut shell = std::process::Command::new("wsl")
            .args(["bash", "-c", &item.cmd])
            .spawn()
            .context("Failed to launch WSL command")?;

        shell.wait()?;
    } else {
        let mut shell = std::process::Command::new("cmd")
            .args(["/C", &item.cmd])
            .spawn()
            .context("Failed to launch command")?;

        shell.wait()?;
    }

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    Ok(())
}
