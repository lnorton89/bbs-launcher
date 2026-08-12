mod app;
mod screens;
mod config;
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

/// Frame-buffering writer that releases bytes to the terminal in
/// bounded chunks with a brief pause between them — app-side flow
/// control.
///
/// Terminals built on xterm.js without pty flow control (Tabby on
/// Windows, notably) drop or mangle bytes when a large burst lands on
/// their input pipe faster than the parser drains it, corrupting the
/// screen from the very first full-frame paint. VS Code's terminal is
/// also xterm.js but paces its pty reads, which is why the same output
/// renders fine there. Chunking with micro-pauses keeps every terminal
/// happy at the cost of a few milliseconds per frame.
///
/// Tunable without a rebuild for diagnosis: `BBS_WRITE_CHUNK` (bytes,
/// 0 disables pacing) and `BBS_WRITE_PAUSE_US` (microseconds between
/// chunks).
struct PacedWriter<W: io::Write> {
    inner: W,
    buf: Vec<u8>,
    chunk: usize,
    pause: std::time::Duration,
}

impl<W: io::Write> PacedWriter<W> {
    fn new(inner: W) -> Self {
        // Defaults match the plain 8KB-buffered chunking the app used
        // historically — the write shape with the best track record
        // across terminals. The knobs exist to experiment per-terminal
        // without a rebuild.
        let chunk = std::env::var("BBS_WRITE_CHUNK")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8192);
        let pause_us = std::env::var("BBS_WRITE_PAUSE_US")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        PacedWriter {
            inner,
            buf: Vec::with_capacity(256 * 1024),
            chunk,
            pause: std::time::Duration::from_micros(pause_us),
        }
    }
}

impl<W: io::Write> io::Write for PacedWriter<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.chunk == 0 {
            // Pacing disabled: one write, like a plain BufWriter.
            self.inner.write_all(&self.buf)?;
        } else {
            for piece in self.buf.chunks(self.chunk) {
                self.inner.write_all(piece)?;
                self.inner.flush()?;
                if !self.pause.is_zero() {
                    std::thread::sleep(self.pause);
                }
            }
        }
        self.buf.clear();
        self.inner.flush()
    }
}

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
    // Buffer whole frames, then release them in paced chunks: raw
    // `io::Stdout` would take a lock per style run (hundreds of locked
    // syscalls per frame), while unpaced bursts overflow terminals
    // without pty flow control and corrupt the screen — see
    // [`PacedWriter`].
    let backend = CrosstermBackend::new(PacedWriter::new(stdout));
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
