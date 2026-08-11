use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, Clear, ClearType,
    },
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use serde::Deserialize;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// 6-row-tall "ANSI Shadow" style glyph for one character: solid `█` fill
/// blocks outlined with double-line box-drawing shadows (`╗ ╔ ╝ ╚ ═ ║`).
/// Every row of a glyph has the same display width so letters stack into
/// clean columns. Unknown characters render as a blank cell.
fn glyph_rows(c: char) -> [&'static str; 6] {
    match c.to_ascii_uppercase() {
        'A' => [
            " █████╗ ",
            "██╔══██╗",
            "███████║",
            "██╔══██║",
            "██║  ██║",
            "╚═╝  ╚═╝",
        ],
        'B' => [
            "██████╗ ",
            "██╔══██╗",
            "██████╔╝",
            "██╔══██╗",
            "██████╔╝",
            "╚═════╝ ",
        ],
        'C' => [
            " ██████╗",
            "██╔════╝",
            "██║     ",
            "██║     ",
            "╚██████╗",
            " ╚═════╝",
        ],
        'D' => [
            "██████╗ ",
            "██╔══██╗",
            "██║  ██║",
            "██║  ██║",
            "██████╔╝",
            "╚═════╝ ",
        ],
        'E' => [
            "███████╗",
            "██╔════╝",
            "█████╗  ",
            "██╔══╝  ",
            "███████╗",
            "╚══════╝",
        ],
        'F' => [
            "███████╗",
            "██╔════╝",
            "█████╗  ",
            "██╔══╝  ",
            "██║     ",
            "╚═╝     ",
        ],
        'G' => [
            " ██████╗ ",
            "██╔════╝ ",
            "██║  ███╗",
            "██║   ██║",
            "╚██████╔╝",
            " ╚═════╝ ",
        ],
        'H' => [
            "██╗  ██╗",
            "██║  ██║",
            "███████║",
            "██╔══██║",
            "██║  ██║",
            "╚═╝  ╚═╝",
        ],
        'I' => [
            "██╗",
            "██║",
            "██║",
            "██║",
            "██║",
            "╚═╝",
        ],
        'J' => [
            "     ██╗",
            "     ██║",
            "     ██║",
            "██   ██║",
            "╚█████╔╝",
            " ╚════╝ ",
        ],
        'K' => [
            "██╗  ██╗",
            "██║ ██╔╝",
            "█████╔╝ ",
            "██╔═██╗ ",
            "██║  ██╗",
            "╚═╝  ╚═╝",
        ],
        'L' => [
            "██╗     ",
            "██║     ",
            "██║     ",
            "██║     ",
            "███████╗",
            "╚══════╝",
        ],
        'M' => [
            "███╗   ███╗",
            "████╗ ████║",
            "██╔████╔██║",
            "██║╚██╔╝██║",
            "██║ ╚═╝ ██║",
            "╚═╝     ╚═╝",
        ],
        'N' => [
            "███╗   ██╗",
            "████╗  ██║",
            "██╔██╗ ██║",
            "██║╚██╗██║",
            "██║ ╚████║",
            "╚═╝  ╚═══╝",
        ],
        'O' => [
            " ██████╗ ",
            "██╔═══██╗",
            "██║   ██║",
            "██║   ██║",
            "╚██████╔╝",
            " ╚═════╝ ",
        ],
        'P' => [
            "██████╗ ",
            "██╔══██╗",
            "██████╔╝",
            "██╔═══╝ ",
            "██║     ",
            "╚═╝     ",
        ],
        'Q' => [
            " ██████╗ ",
            "██╔═══██╗",
            "██║   ██║",
            "██║   ██║",
            "╚██████╔╝",
            " ╚════██╗",
        ],
        'R' => [
            "██████╗ ",
            "██╔══██╗",
            "██████╔╝",
            "██╔══██╗",
            "██║  ██║",
            "╚═╝  ╚═╝",
        ],
        'S' => [
            "███████╗",
            "██╔════╝",
            "███████╗",
            "╚════██║",
            "███████║",
            "╚══════╝",
        ],
        'T' => [
            "████████╗",
            "╚══██╔══╝",
            "   ██║   ",
            "   ██║   ",
            "   ██║   ",
            "   ╚═╝   ",
        ],
        'U' => [
            "██╗   ██╗",
            "██║   ██║",
            "██║   ██║",
            "██║   ██║",
            "╚██████╔╝",
            " ╚═════╝ ",
        ],
        'V' => [
            "██╗   ██╗",
            "██║   ██║",
            "██║   ██║",
            "╚██╗ ██╔╝",
            " ╚████╔╝ ",
            "  ╚═══╝  ",
        ],
        'W' => [
            "██╗    ██╗",
            "██║    ██║",
            "██║ █╗ ██║",
            "██║███╗██║",
            "╚███╔███╔╝",
            " ╚══╝╚══╝ ",
        ],
        'X' => [
            "██╗  ██╗",
            "╚██╗██╔╝",
            " ╚███╔╝ ",
            " ██╔██╗ ",
            "██╔╝ ██╗",
            "╚═╝  ╚═╝",
        ],
        'Y' => [
            "██╗   ██╗",
            "╚██╗ ██╔╝",
            " ╚████╔╝ ",
            "  ╚██╔╝  ",
            "   ██║   ",
            "   ╚═╝   ",
        ],
        'Z' => [
            "███████╗",
            "╚══███╔╝",
            "  ███╔╝ ",
            " ███╔╝  ",
            "███████╗",
            "╚══════╝",
        ],
        '0' => [
            " ██████╗ ",
            "██╔═████╗",
            "██║██╔██║",
            "████╔╝██║",
            "╚██████╔╝",
            " ╚═════╝ ",
        ],
        '1' => [
            " ██╗",
            "███║",
            "╚██║",
            " ██║",
            " ██║",
            " ╚═╝",
        ],
        '2' => [
            "██████╗ ",
            "╚════██╗",
            " █████╔╝",
            "██╔═══╝ ",
            "███████╗",
            "╚══════╝",
        ],
        '3' => [
            "██████╗ ",
            "╚════██╗",
            " █████╔╝",
            " ╚═══██╗",
            "██████╔╝",
            "╚═════╝ ",
        ],
        '4' => [
            "██╗  ██╗",
            "██║  ██║",
            "███████║",
            "╚════██║",
            "     ██║",
            "     ╚═╝",
        ],
        '5' => [
            "███████╗",
            "██╔════╝",
            "███████╗",
            "╚════██║",
            "███████║",
            "╚══════╝",
        ],
        '6' => [
            " ██████╗ ",
            "██╔════╝ ",
            "███████╗ ",
            "██╔═══██╗",
            "╚██████╔╝",
            " ╚═════╝ ",
        ],
        '7' => [
            "███████╗",
            "╚════██║",
            "    ██╔╝",
            "   ██╔╝ ",
            "   ██║  ",
            "   ╚═╝  ",
        ],
        '8' => [
            " █████╗ ",
            "██╔══██╗",
            "╚█████╔╝",
            "██╔══██╗",
            "╚█████╔╝",
            " ╚════╝ ",
        ],
        '9' => [
            " █████╗ ",
            "██╔══██╗",
            "╚██████║",
            " ╚═══██║",
            " █████╔╝",
            " ╚════╝ ",
        ],
        '-' => [
            "      ",
            "      ",
            "█████╗",
            "╚════╝",
            "      ",
            "      ",
        ],
        '_' => [
            "        ",
            "        ",
            "        ",
            "        ",
            "███████╗",
            "╚══════╝",
        ],
        '.' => [
            "   ",
            "   ",
            "   ",
            "   ",
            "██╗",
            "╚═╝",
        ],
        _ => [
            "    ",
            "    ",
            "    ",
            "    ",
            "    ",
            "    ",
        ],
    }
}

/// Renders `text` as a 6-row "ANSI Shadow" style banner (solid `█` fill
/// blocks with `╗ ╔ ╝ ╚ ═ ║` double-line shadows), with a single column
/// of space between letters. Input is uppercase-normalized; spaces and
/// unknown characters become blank cells. Returns newline-joined rows
/// for `draw_banner` to consume via `.lines()`.
fn generate_banner_text(text: &str) -> String {
    const HEIGHT: usize = 6;
    let mut lines = vec![String::new(); HEIGHT];
    for (i, c) in text.chars().enumerate() {
        let glyph = glyph_rows(c);
        for (row, line) in lines.iter_mut().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            line.push_str(glyph[row]);
        }
    }
    lines.join("\n")
}

/// Looks up the local machine's hostname, uppercased for banner display.
/// Falls back to a placeholder if the OS doesn't report one.
fn get_hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "UNKNOWN-HOST".to_string())
        .to_uppercase()
}

#[derive(Debug, Deserialize, Clone)]
struct BbsItem {
    key: String,
    label: String,
    cmd: String,
    desc: String,
    icon: String,
    color: String,
    wsl: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
struct BbsConfig {
    bbs: BbsHeader,
    items: Vec<BbsItem>,
}

#[derive(Debug, Deserialize, Clone)]
struct BbsHeader {
    title: String,
}

#[derive(Debug)]
struct App {
    config: BbsConfig,
    items: Vec<BbsItem>,
    state: ListState,
    status_message: String,
    spinner: usize,
    banner: String,
}

impl App {
    fn new(config: BbsConfig) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        let banner = generate_banner_text(&get_hostname());
        Self {
            config,
            items: Vec::new(),
            state,
            status_message: "Navigate: ↑/↓ or j/k  |  Launch: number key or Enter  |  Quit: q".to_string(),
            spinner: 0,
            banner,
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn get_selected(&self) -> Option<&BbsItem> {
        self.state.selected().and_then(|i| self.items.get(i))
    }

    fn find_by_key(&self, key: &str) -> Option<&BbsItem> {
        self.items.iter().find(|item| item.key == key)
    }

    fn update_spinner(&mut self) {
        self.spinner = (self.spinner + 1) % 4;
    }
}

fn color_from_str(s: &str) -> Color {
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

fn load_config() -> Result<BbsConfig> {
    let config_path = find_config()?;
    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config from: {}", config_path.display()))?;
    let config: BbsConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse TOML config: {}", config_path.display()))?;
    Ok(config)
}

fn find_config() -> Result<PathBuf> {
    let mut paths = Vec::new();
    
    if let Ok(exe_dir) = std::env::current_exe() {
        if let Some(parent) = exe_dir.parent() {
            paths.push(parent.join("bbs.toml"));
        }
    }
    
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("bbs.toml"));
    }
    
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".config").join("bbs-launcher").join("bbs.toml"));
    }
    
    for path in &paths {
        if path.exists() {
            return Ok(path.clone());
        }
    }
    
    Ok(paths.into_iter().next().unwrap())
}

fn draw_banner(frame: &mut Frame, area: Rect, config: &BbsConfig, app: &App) {
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

fn draw_menu(frame: &mut Frame, area: Rect, items: &[BbsItem], state: &mut ListState) {
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

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let spinner = match app.spinner {
        0 => "◐",
        1 => "◓",
        2 => "◑",
        _ => "◒",
    };

    let status_text = if let Some(item) = app.get_selected() {
        format!(
            " {} Ready | {}: {} ({}) | {} ",
            spinner,
            item.key,
            item.label,
            item.desc,
            app.status_message
        )
    } else {
        format!(
            " {} Ready | {}",
            spinner,
            app.status_message
        )
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

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
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

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> Result<()> {
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

fn main() -> Result<()> {
    let config = load_config().context("Failed to load bbs.toml config")?;
    let mut app = App::new(config.clone());
    app.items = config.items.clone();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}
