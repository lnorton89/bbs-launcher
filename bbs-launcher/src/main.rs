mod app;
mod config;
mod github;
mod run;
mod stats;
mod ui;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::PathBuf;

use app::App;
use config::load_config;
use run::run_app;

/// A BBS-style terminal launcher menu.
#[derive(Parser, Debug)]
#[command(name = "bbs-launcher", version, about, long_about = None)]
struct Cli {
    /// Config file to use instead of searching the default locations
    /// (exe directory, current directory, ~/.config/bbs-launcher).
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Print the resolved config path and menu items, then exit.
    #[arg(short, long)]
    list: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let (config, config_path) =
        load_config(cli.config).context("Failed to load bbs.toml config")?;

    if cli.list {
        println!("config: {}", config_path.display());
        println!("title:  {}", config.bbs.title);
        println!("{} items:", config.items.len());
        for item in &config.items {
            let target = match (&item.screen, item.wsl.unwrap_or(false)) {
                (Some(screen), _) => format!("<{screen} screen>"),
                (None, true) => format!("wsl: {}", item.cmd),
                (None, false) => item.cmd.clone(),
            };
            let category = item
                .category
                .as_deref()
                .map(|c| format!("[{c}] "))
                .unwrap_or_default();
            println!("  [{}] {}{:<14} {}", item.key, category, item.label, target);
        }
        return Ok(());
    }

    let app = App::new(config, config_path);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    // Buffer the backend: `io::Stdout` is line-buffered and takes a lock
    // per write, and a frame issues one write per style run — hundreds of
    // locked syscalls each. BufWriter collapses that into one flush per
    // frame. (`execute!` flushes, so escapes stay correctly ordered.)
    let backend = CrosstermBackend::new(io::BufWriter::new(stdout));
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
