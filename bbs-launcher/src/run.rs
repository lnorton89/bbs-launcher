use crate::app::{App, Mode, Row};
use crate::config::BbsItem;
use crate::github::{self, Nav};
use crate::ui::draw;
use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{backend::Backend, Terminal};
use std::io;
use std::time::{Duration, Instant};

pub fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> Result<()> {
    let tick_rate = Duration::from_millis(100);
    let mut next_tick = Instant::now();
    // Redraw only when something actually changed. Drawing once per loop
    // iteration instead meant every stray input event — mouse motion in
    // particular, which arrives in long bursts — forced a full repaint.
    let mut dirty = true;
    let mut frame_cost = Duration::ZERO;

    loop {
        if dirty {
            let started = Instant::now();
            terminal.draw(|f| draw(f, &mut app))?;
            frame_cost = started.elapsed();
            dirty = false;
        }

        // Wait until the next scheduled tick. Deriving the timeout from a
        // fixed deadline (rather than from how long the last frame took)
        // keeps a slow frame from producing a zero timeout on every pass,
        // which would spin the loop at full CPU and never recover.
        let timeout = next_tick.saturating_duration_since(Instant::now());

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if handle_key(terminal, &mut app, key)? {
                        return Ok(());
                    }
                    dirty = true;
                }
                // Nothing here reacts to hover or drag, so redrawing for
                // them is pure cost.
                Event::Mouse(m)
                    if matches!(
                        m.kind,
                        MouseEventKind::Moved | MouseEventKind::Drag(_)
                    ) => {}
                Event::Mouse(mouse) => {
                    if handle_mouse(terminal, &mut app, mouse)? {
                        return Ok(());
                    }
                    dirty = true;
                }
                Event::Resize(..) => dirty = true,
                _ => {}
            }
        }

        let now = Instant::now();
        if now >= next_tick {
            app.on_tick();
            dirty = true;
            // Drop missed ticks rather than trying to catch up: after a
            // pause (a launched command, a suspended terminal) a backlog
            // would otherwise burn CPU replaying ticks nobody can see.
            //
            // Never schedule frames faster than the terminal can actually
            // paint one either. On a slow console a 100ms tick would mean
            // drawing back to back at full CPU, which makes input lag —
            // so the animation slows down instead and input stays sharp.
            next_tick = now + tick_rate.max(frame_cost * 2);
        }
    }
}

/// Handles one key press. Returns Ok(true) when the app should quit.
fn handle_key<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    key: KeyEvent,
) -> Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q'))
    {
        return Ok(true);
    }

    match app.mode {
        Mode::Help => {
            app.mode = Mode::Normal;
            Ok(false)
        }
        Mode::Github => match github::handle_key(app, key) {
            Nav::Back => {
                app.mode = Mode::Normal;
                Ok(false)
            }
            Nav::Stay => Ok(false),
        },
        Mode::Search => match key.code {
            KeyCode::Esc => {
                app.query.clear();
                app.apply_filter();
                app.mode = Mode::Normal;
                Ok(false)
            }
            KeyCode::Enter => {
                if let Some(item) = app.selected_item().cloned() {
                    app.mode = Mode::Normal;
                    app.query.clear();
                    app.apply_filter();
                    app.select_label(&item.label);
                    activate_item(terminal, app, &item)
                } else {
                    Ok(false)
                }
            }
            KeyCode::Backspace => {
                app.query.pop();
                app.apply_filter();
                Ok(false)
            }
            KeyCode::Down => {
                app.next();
                Ok(false)
            }
            KeyCode::Up => {
                app.previous();
                Ok(false)
            }
            KeyCode::Char(c) => {
                app.query.push(c);
                app.apply_filter();
                Ok(false)
            }
            _ => Ok(false),
        },
        Mode::Normal => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Ok(true),
            KeyCode::Down | KeyCode::Char('j') => {
                app.next();
                Ok(false)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.previous();
                Ok(false)
            }
            KeyCode::Home | KeyCode::Char('g') => {
                app.select_first();
                Ok(false)
            }
            KeyCode::End | KeyCode::Char('G') => {
                app.select_last();
                Ok(false)
            }
            KeyCode::PageDown => {
                app.jump(5);
                Ok(false)
            }
            KeyCode::PageUp => {
                app.jump(-5);
                Ok(false)
            }
            KeyCode::Left => {
                // Collapse the selected category (or jump from an item
                // up to its category header).
                app.collapse_or_jump();
                Ok(false)
            }
            KeyCode::Right => {
                app.expand_selected();
                Ok(false)
            }
            KeyCode::Char('/') => {
                app.mode = Mode::Search;
                app.query.clear();
                app.apply_filter();
                Ok(false)
            }
            KeyCode::Char('?') => {
                app.mode = Mode::Help;
                Ok(false)
            }
            KeyCode::Enter => {
                // On a category header, Enter folds instead of launching.
                if app.toggle_selected_category() {
                    return Ok(false);
                }
                if let Some(item) = app.selected_item().cloned() {
                    activate_item(terminal, app, &item)
                } else {
                    Ok(false)
                }
            }
            KeyCode::Char(c) => {
                if let Some(item) = app.find_by_key(&c.to_string()).cloned() {
                    app.select_label(&item.label);
                    activate_item(terminal, app, &item)
                } else {
                    Ok(false)
                }
            }
            _ => Ok(false),
        },
    }
}

/// Routes an activated item: built-in screens open in-app, everything
/// else is launched as a command.
fn activate_item<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    item: &BbsItem,
) -> Result<bool> {
    match item.screen.as_deref() {
        Some("github") => {
            app.stats.record(&item.label);
            if let Err(err) = app.stats.save() {
                app.status_message = format!("Couldn't save stats: {}", err);
            }
            app.mode = Mode::Github;
            app.github.open();
            Ok(false)
        }
        Some(other) => {
            app.status_message = format!("Unknown screen type: {}", other);
            Ok(false)
        }
        None => launch(terminal, app, item),
    }
}

/// Handles a mouse event. Returns Ok(true) when the app should quit.
fn handle_mouse<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    mouse: MouseEvent,
) -> Result<bool> {
    if app.mode == Mode::Github {
        return handle_github_mouse(app, mouse);
    }
    match mouse.kind {
        MouseEventKind::ScrollDown => app.next(),
        MouseEventKind::ScrollUp => app.previous(),
        MouseEventKind::Down(MouseButton::Left) => {
            if app.mode == Mode::Help {
                app.mode = Mode::Normal;
                return Ok(false);
            }
            let Some(area) = app.menu_area else {
                return Ok(false);
            };
            // Only rows inside the menu's borders count.
            let inside = mouse.column > area.x
                && mouse.column < area.x + area.width.saturating_sub(1)
                && mouse.row > area.y
                && mouse.row < area.y + area.height.saturating_sub(1);
            if inside {
                let idx = (mouse.row - area.y - 1) as usize + app.state.offset();
                if idx < app.rows.len() {
                    let was_selected = app.state.selected() == Some(idx);
                    let now = Instant::now();
                    let is_double = was_selected
                        && app.last_click.take().is_some_and(|(t, i)| {
                            i == idx && now.duration_since(t) < Duration::from_millis(450)
                        });
                    app.state.select(Some(idx));
                    app.last_click = Some((now, idx));
                    // A single click on a category header folds it; items
                    // still need a double-click to launch.
                    if matches!(app.rows.get(idx), Some(Row::Header { .. })) {
                        app.toggle_category_at(idx);
                    } else if is_double {
                        if let Some(item) = app.selected_item().cloned() {
                            return activate_item(terminal, app, &item);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

/// Mouse handling while the GitHub screen is open: scroll moves the
/// list, a click selects, a double-click opens in the browser.
fn handle_github_mouse(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    match mouse.kind {
        MouseEventKind::ScrollDown => app.github.next(),
        MouseEventKind::ScrollUp => app.github.previous(),
        MouseEventKind::Down(MouseButton::Left) => {
            let Some(area) = app.github.list_area else {
                return Ok(false);
            };
            let inside = mouse.column > area.x
                && mouse.column < area.x + area.width.saturating_sub(1)
                && mouse.row > area.y
                && mouse.row < area.y + area.height.saturating_sub(1);
            if inside {
                let tab = app.github.tab;
                let offset = app.github.states[tab].offset();
                let idx = (mouse.row - area.y - 1) as usize + offset;
                let len = app.github.entries.get(tab).map(|e| e.len()).unwrap_or(0);
                if idx < len {
                    let was_selected = app.github.states[tab].selected() == Some(idx);
                    let now = Instant::now();
                    let is_double = was_selected
                        && app.last_click.take().is_some_and(|(t, i)| {
                            i == idx && now.duration_since(t) < Duration::from_millis(450)
                        });
                    app.github.states[tab].select(Some(idx));
                    app.last_click = Some((now, idx));
                    if is_double {
                        app.github.open_selected();
                    }
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

/// Suspends the TUI, runs the item's command, then restores the menu.
/// Returns Ok(true) only for the built-in "exit"/"quit" pseudo-commands.
fn launch<B: Backend>(terminal: &mut Terminal<B>, app: &mut App, item: &BbsItem) -> Result<bool> {
    let cmd = item.cmd.trim();
    if cmd.eq_ignore_ascii_case("exit") || cmd.eq_ignore_ascii_case("quit") {
        return Ok(true);
    }

    app.stats.record(&item.label);
    if let Err(err) = app.stats.save() {
        app.status_message = format!("Couldn't save stats: {}", err);
    }

    disable_raw_mode()?;
    execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        Clear(ClearType::All)
    )?;

    let mut command = if item.wsl.unwrap_or(false) {
        let mut c = std::process::Command::new("wsl");
        c.args(["bash", "-c", &item.cmd]);
        c
    } else {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", &item.cmd]);
        c
    };
    if let Some(cwd) = &item.cwd {
        command.current_dir(cwd);
    }

    let result = command
        .spawn()
        .context("Failed to launch command")
        .and_then(|mut child| child.wait().context("Command failed while running"));

    app.status_message = match &result {
        Ok(status) if status.success() => format!("{} finished", item.label),
        Ok(status) => format!(
            "{} exited with code {}",
            item.label,
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".to_string())
        ),
        Err(err) => format!("{} failed: {:#}", item.label, err),
    };

    if item.pause.unwrap_or(false) && result.is_ok() {
        println!();
        println!("  Press Enter to return to {}...", app.config.bbs.title);
        let mut buf = String::new();
        let _ = io::stdin().read_line(&mut buf);
    }

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.clear()?;
    Ok(false)
}
