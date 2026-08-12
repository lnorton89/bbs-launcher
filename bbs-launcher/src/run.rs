use crate::app::{App, Mode, Row};
use crate::config::BbsItem;
use crate::screens::{bluetti, github, Nav};
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
    // Single source of truth with the animation code, which converts
    // configured durations into per-tick steps using the same rate.
    let tick_rate = Duration::from_secs_f32(1.0 / crate::app::TICKS_PER_SEC);
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
                Event::Resize(w, h) => {
                    // Resize using the event's own dimensions instead of
                    // waiting for the next draw's size query: on Windows
                    // the query can lag the real window (fast or
                    // one-axis resizes), which left every frame painted
                    // at a stale width. Terminal::resize also clears, so
                    // cells the new layout doesn't cover can't linger as
                    // on-screen garbage.
                    terminal.resize(ratatui::layout::Rect::new(0, 0, w, h))?;
                    dirty = true;
                }
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
    // The classic terminal fixer-upper: force a full repaint, in any
    // mode, for when the display has drifted out of sync with reality.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
        terminal.clear()?;
        return Ok(false);
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
        Mode::Bluetti => match bluetti::handle_key(app, key) {
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
            // Cycles the menu order. This shadows `s` as an item hotkey;
            // such items can still be launched with Enter or search.
            KeyCode::Char('s') => {
                app.cycle_menu_sort();
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
        Some("bluetti") => {
            app.stats.record(&item.label);
            if let Err(err) = app.stats.save() {
                app.status_message = format!("Couldn't save stats: {}", err);
            }
            app.mode = Mode::Bluetti;
            app.bluetti.open();
            Ok(false)
        }
        Some(other) => {
            app.status_message = format!("Unknown screen type: {}", other);
            Ok(false)
        }
        None => launch(terminal, app, item),
    }
}

/// Which visible row of a bordered list pane a click landed on, given
/// the list's scroll `offset` and row count. `None` for clicks on the
/// border, outside the pane, or past the last row.
fn clicked_row(
    area: Option<ratatui::layout::Rect>,
    mouse: &MouseEvent,
    offset: usize,
    len: usize,
) -> Option<usize> {
    let area = area?;
    let inside = mouse.column > area.x
        && mouse.column < area.x + area.width.saturating_sub(1)
        && mouse.row > area.y
        && mouse.row < area.y + area.height.saturating_sub(1);
    if !inside {
        return None;
    }
    let idx = (mouse.row - area.y - 1) as usize + offset;
    (idx < len).then_some(idx)
}

/// Handles a mouse event. Returns Ok(true) when the app should quit.
fn handle_mouse<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    mouse: MouseEvent,
) -> Result<bool> {
    match app.mode {
        Mode::Github => {
            match mouse.kind {
                MouseEventKind::ScrollDown => app.github.next(),
                MouseEventKind::ScrollUp => app.github.previous(),
                MouseEventKind::Down(MouseButton::Left) => {
                    let tab = app.github.tab;
                    let len = app.github.entries.get(tab).map(|e| e.len()).unwrap_or(0);
                    let offset = app.github.states[tab].offset();
                    if let Some(idx) =
                        clicked_row(app.github.list_area, &mouse, offset, len)
                    {
                        let was = app.github.states[tab].selected() == Some(idx);
                        app.github.states[tab].select(Some(idx));
                        // A double-click opens the entry in the browser.
                        if app.register_click(idx, was) {
                            app.github.open_selected();
                        }
                    }
                }
                _ => {}
            }
            Ok(false)
        }
        Mode::Bluetti => {
            match mouse.kind {
                MouseEventKind::ScrollDown => app.bluetti.next(),
                MouseEventKind::ScrollUp => app.bluetti.previous(),
                MouseEventKind::Down(MouseButton::Left) => {
                    let len = app.bluetti.sorted_fields().len();
                    let offset = app.bluetti.state.offset();
                    if let Some(idx) =
                        clicked_row(app.bluetti.list_area, &mouse, offset, len)
                    {
                        app.bluetti.state.select(Some(idx));
                    }
                }
                _ => {}
            }
            Ok(false)
        }
        _ => {
            match mouse.kind {
                MouseEventKind::ScrollDown => app.next(),
                MouseEventKind::ScrollUp => app.previous(),
                MouseEventKind::Down(MouseButton::Left) => {
                    if app.mode == Mode::Help {
                        app.mode = Mode::Normal;
                        return Ok(false);
                    }
                    let offset = app.state.offset();
                    if let Some(idx) =
                        clicked_row(app.menu_area, &mouse, offset, app.rows.len())
                    {
                        let was = app.state.selected() == Some(idx);
                        app.state.select(Some(idx));
                        let is_double = app.register_click(idx, was);
                        // A single click on a category header folds it;
                        // items still need a double-click to launch.
                        if matches!(app.rows.get(idx), Some(Row::Header { .. })) {
                            app.toggle_category_at(idx);
                        } else if is_double {
                            if let Some(item) = app.selected_item().cloned() {
                                return activate_item(terminal, app, &item);
                            }
                        }
                    }
                }
                _ => {}
            }
            Ok(false)
        }
    }
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

    // Flush any frame bytes still buffered in the backend before
    // touching the terminal through a second handle — two interleaved
    // write paths are a reliable source of creeping display corruption.
    terminal.backend_mut().flush()?;
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
    // The window may have been resized while the child owned the
    // screen — and the resize events consumed with it. Re-sync from a
    // fresh query; Terminal::resize includes the full clear a bare
    // clear() would have done.
    let (w, h) = crossterm::terminal::size()?;
    terminal.resize(ratatui::layout::Rect::new(0, 0, w, h))?;
    Ok(false)
}
